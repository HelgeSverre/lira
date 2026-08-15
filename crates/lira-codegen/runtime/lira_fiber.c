/*
 * Stackful green threads for natively compiled Lira.
 *
 * The bytecode VM can suspend a fiber by saving an instruction pointer, because
 * its call frames live in a heap vector. Native code has no such luxury: call
 * frames live on the machine stack and are addressed through SP. So each fiber
 * gets its own mmap'd stack with a guard page, and suspending one is a
 * callee-saved-register swap plus an SP swap (see lira_ctx.S).
 *
 * Everything here is single-threaded and cooperative: fibers only ever move
 * between states at an explicit switch point (yield, channel send/recv,
 * completion), so no locking is required.
 */
#include "lira_rt.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#error "the Lira native backend does not support Windows yet"
#else
#include <sys/mman.h>
#include <unistd.h>
#endif

void lira_rt_panic(const char *message);

#define LIRA_FIBER_STACK_SIZE (256 * 1024)

enum LiraFiberState {
    LIRA_FIBER_READY = 0,
    LIRA_FIBER_RUNNING = 1,
    LIRA_FIBER_BLOCKED = 2,
    LIRA_FIBER_DONE = 3
};

typedef struct LiraFiber {
    void *sp;         /* saved stack pointer while suspended */
    void *stack_map;  /* base of the mmap'd region, including the guard page */
    size_t map_size;
    LiraFiberEntry entry;
    void *env;
    int state;
    int64_t id;
    int64_t xfer;         /* value handed over by a channel rendezvous */
    int8_t xfer_closed;   /* set when the fiber was woken by close() */
    struct LiraFiber *next;
} LiraFiber;

/* ------------------------------------------------------------------ */
/* Scheduler state                                                     */
/* ------------------------------------------------------------------ */

static LiraFiber *g_run_head = NULL;
static LiraFiber *g_run_tail = NULL;
static LiraFiber *g_current = NULL;
static void *g_sched_sp = NULL;
static int64_t g_next_id = 0;
static int64_t g_live = 0; /* fibers created but not yet finished */

static void lira_runq_push(LiraFiber *f) {
    f->next = NULL;
    if (g_run_tail == NULL) {
        g_run_head = g_run_tail = f;
    } else {
        g_run_tail->next = f;
        g_run_tail = f;
    }
}

static LiraFiber *lira_runq_pop(void) {
    LiraFiber *f = g_run_head;
    if (f == NULL) {
        return NULL;
    }
    g_run_head = f->next;
    if (g_run_head == NULL) {
        g_run_tail = NULL;
    }
    f->next = NULL;
    return f;
}

/* ------------------------------------------------------------------ */
/* Stack setup                                                         */
/* ------------------------------------------------------------------ */

void lira_fiber_trampoline(void);

/* Called from the trampoline on the fiber's own stack. Never returns. */
void lira_fiber_start(LiraFiber *f) {
    f->entry(f->env);
    f->state = LIRA_FIBER_DONE;
    g_live--;
    /* Hand control back to the scheduler for the last time. The scheduler
     * reclaims the stack we are standing on, so this switch must not return. */
    lira_ctx_switch(&f->sp, g_sched_sp);
    lira_rt_panic("resumed a finished fiber");
}

static void lira_fiber_init_stack(LiraFiber *f) {
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0) {
        page = 4096;
    }
    size_t guard = (size_t)page;
    size_t usable = LIRA_FIBER_STACK_SIZE;
    f->map_size = guard + usable;

    void *map = mmap(NULL, f->map_size, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (map == MAP_FAILED) {
        lira_rt_panic("failed to allocate fiber stack");
    }
    /* Stacks grow down, so the guard page goes at the low end: an overflow
     * faults instead of silently trampling another fiber's heap data. */
    if (mprotect(map, guard, PROT_NONE) != 0) {
        lira_rt_panic("failed to guard fiber stack");
    }
    f->stack_map = map;

    uintptr_t top = (uintptr_t)map + f->map_size;
    top &= ~(uintptr_t)15; /* both ABIs want a 16-byte aligned stack top */

#if defined(__x86_64__)
    /* Frame the SysV `ret`-based switch expects, low to high:
     *   r15, r14, r13, r12, rbx, rbp, return address
     * r15 carries the fiber pointer into the trampoline. Placing the return
     * slot at top-8 leaves RSP 16-byte aligned at the trampoline's first
     * instruction, which is what its `call` requires. */
    uintptr_t sp = top - 56;
    uint64_t *slots = (uint64_t *)sp;
    slots[0] = (uint64_t)(uintptr_t)f; /* r15 */
    slots[1] = 0;                      /* r14 */
    slots[2] = 0;                      /* r13 */
    slots[3] = 0;                      /* r12 */
    slots[4] = 0;                      /* rbx */
    slots[5] = 0;                      /* rbp */
    slots[6] = (uint64_t)(uintptr_t)lira_fiber_trampoline;
    f->sp = (void *)sp;
#elif defined(__aarch64__)
    /* AAPCS64 frame written by lira_ctx_switch: x19..x28, x29, x30, d8..d15.
     * x19 carries the fiber pointer, x30 is where the switch's `ret` lands. */
    uintptr_t sp = top - 160;
    uint64_t *slots = (uint64_t *)sp;
    memset(slots, 0, 160);
    slots[0] = (uint64_t)(uintptr_t)f;                       /* x19 */
    slots[11] = (uint64_t)(uintptr_t)lira_fiber_trampoline;  /* x30 */
    f->sp = (void *)sp;
#else
#error "the Lira native backend supports x86_64 and aarch64 fibers only"
#endif
}

static LiraFiber *lira_fiber_new(LiraFiberEntry entry, void *env) {
    LiraFiber *f = (LiraFiber *)calloc(1, sizeof(LiraFiber));
    if (f == NULL) {
        lira_rt_panic("out of memory");
    }
    f->entry = entry;
    f->env = env;
    f->state = LIRA_FIBER_READY;
    f->id = g_next_id++;
    lira_fiber_init_stack(f);
    g_live++;
    return f;
}

static void lira_fiber_free(LiraFiber *f) {
    if (f->stack_map != NULL) {
        munmap(f->stack_map, f->map_size);
        f->stack_map = NULL;
    }
    free(f);
}

/* Suspend the running fiber and return to the scheduler loop. */
static void lira_switch_to_scheduler(void) {
    LiraFiber *f = g_current;
    if (f == NULL) {
        lira_rt_panic("fiber operation outside of a fiber");
    }
    lira_ctx_switch(&f->sp, g_sched_sp);
}

/* ------------------------------------------------------------------ */
/* Public fiber API                                                    */
/* ------------------------------------------------------------------ */

int64_t lira_rt_spawn(LiraFiberEntry entry, void *env) {
    LiraFiber *f = lira_fiber_new(entry, env);
    lira_runq_push(f);
    return f->id;
}

void lira_rt_yield(void) {
    LiraFiber *f = g_current;
    if (f == NULL) {
        return;
    }
    f->state = LIRA_FIBER_READY;
    lira_runq_push(f);
    lira_switch_to_scheduler();
}

int64_t lira_rt_fiber_id(void) { return g_current ? g_current->id : 0; }

int32_t lira_rt_boot(LiraFiberEntry entry, void *env) {
    LiraFiber *root = lira_fiber_new(entry, env);
    lira_runq_push(root);

    for (;;) {
        LiraFiber *f = lira_runq_pop();
        if (f == NULL) {
            break;
        }
        g_current = f;
        f->state = LIRA_FIBER_RUNNING;
        lira_ctx_switch(&g_sched_sp, f->sp);
        g_current = NULL;
        if (f->state == LIRA_FIBER_DONE) {
            lira_fiber_free(f);
        }
    }

    fflush(stdout);
    if (g_live > 0) {
        fprintf(stderr, "lira: fatal error: deadlock - all %" PRId64
                        " remaining fiber(s) are blocked\n",
                g_live);
        fflush(stderr);
        return 1;
    }
    return 0;
}

/* ------------------------------------------------------------------ */
/* Channels                                                            */
/* ------------------------------------------------------------------ */

typedef struct LiraChan {
    LiraHeader hdr;
    int64_t *buf;
    int64_t cap;
    int64_t len;
    int64_t head;
    int8_t closed;
    LiraFiber *send_head;
    LiraFiber *send_tail;
    LiraFiber *recv_head;
    LiraFiber *recv_tail;
} LiraChan;

static void lira_wait_push(LiraFiber **head, LiraFiber **tail, LiraFiber *f) {
    f->next = NULL;
    if (*tail == NULL) {
        *head = *tail = f;
    } else {
        (*tail)->next = f;
        *tail = f;
    }
}

static LiraFiber *lira_wait_pop(LiraFiber **head, LiraFiber **tail) {
    LiraFiber *f = *head;
    if (f == NULL) {
        return NULL;
    }
    *head = f->next;
    if (*head == NULL) {
        *tail = NULL;
    }
    f->next = NULL;
    return f;
}

static void lira_wake(LiraFiber *f) {
    f->state = LIRA_FIBER_READY;
    lira_runq_push(f);
}

static void lira_buf_push(LiraChan *c, int64_t v) {
    c->buf[(c->head + c->len) % c->cap] = v;
    c->len++;
}

static int64_t lira_buf_pop(LiraChan *c) {
    int64_t v = c->buf[c->head];
    c->head = (c->head + 1) % c->cap;
    c->len--;
    return v;
}

void *lira_rt_chan_new(int64_t capacity) {
    if (capacity < 0) {
        capacity = 0;
    }
    LiraChan *c = (LiraChan *)lira_rt_alloc((int64_t)sizeof(LiraChan), LIRA_KIND_CHANNEL);
    c->cap = capacity;
    c->len = 0;
    c->head = 0;
    c->closed = 0;
    if (capacity > 0) {
        c->buf = (int64_t *)calloc((size_t)capacity, sizeof(int64_t));
        if (c->buf == NULL) {
            lira_rt_panic("out of memory");
        }
    }
    return c;
}

void lira_rt_chan_send(void *chan, int64_t value) {
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL) {
        lira_rt_panic("send on null channel");
    }
    if (c->closed) {
        lira_rt_panic("send on closed channel");
    }

    /* A waiting receiver takes the value directly — this is also what makes
     * unbuffered channels a true rendezvous. */
    LiraFiber *r = lira_wait_pop(&c->recv_head, &c->recv_tail);
    if (r != NULL) {
        r->xfer = value;
        r->xfer_closed = 0;
        lira_wake(r);
        return;
    }

    if (c->len < c->cap) {
        lira_buf_push(c, value);
        return;
    }

    LiraFiber *self = g_current;
    if (self == NULL) {
        lira_rt_panic("blocking channel send outside of a fiber");
    }
    self->xfer = value;
    self->state = LIRA_FIBER_BLOCKED;
    lira_wait_push(&c->send_head, &c->send_tail, self);
    lira_switch_to_scheduler();
}

int64_t lira_rt_chan_recv(void *chan) {
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL) {
        lira_rt_panic("recv on null channel");
    }

    if (c->len > 0) {
        int64_t v = lira_buf_pop(c);
        /* A blocked sender can now move its value into the freed slot. */
        LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
        if (s != NULL) {
            lira_buf_push(c, s->xfer);
            lira_wake(s);
        }
        return v;
    }

    LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
    if (s != NULL) {
        int64_t v = s->xfer;
        lira_wake(s);
        return v;
    }

    if (c->closed) {
        return 0;
    }

    LiraFiber *self = g_current;
    if (self == NULL) {
        lira_rt_panic("blocking channel recv outside of a fiber");
    }
    self->state = LIRA_FIBER_BLOCKED;
    self->xfer = 0;
    self->xfer_closed = 0;
    lira_wait_push(&c->recv_head, &c->recv_tail, self);
    lira_switch_to_scheduler();
    return self->xfer;
}

int8_t lira_rt_chan_try_recv(void *chan, int64_t *out) {
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL) {
        return 0;
    }
    if (c->len > 0) {
        *out = lira_buf_pop(c);
        LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
        if (s != NULL) {
            lira_buf_push(c, s->xfer);
            lira_wake(s);
        }
        return 1;
    }
    LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
    if (s != NULL) {
        *out = s->xfer;
        lira_wake(s);
        return 1;
    }
    return 0;
}

int8_t lira_rt_chan_try_send(void *chan, int64_t value) {
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL || c->closed) {
        return 0;
    }
    LiraFiber *r = lira_wait_pop(&c->recv_head, &c->recv_tail);
    if (r != NULL) {
        r->xfer = value;
        r->xfer_closed = 0;
        lira_wake(r);
        return 1;
    }
    if (c->len < c->cap) {
        lira_buf_push(c, value);
        return 1;
    }
    return 0;
}

void lira_rt_chan_close(void *chan) {
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL || c->closed) {
        return;
    }
    c->closed = 1;
    /* Receivers waiting on a closed channel observe the zero value. */
    LiraFiber *f;
    while ((f = lira_wait_pop(&c->recv_head, &c->recv_tail)) != NULL) {
        f->xfer = 0;
        f->xfer_closed = 1;
        lira_wake(f);
    }
    while ((f = lira_wait_pop(&c->send_head, &c->send_tail)) != NULL) {
        lira_wake(f);
    }
}

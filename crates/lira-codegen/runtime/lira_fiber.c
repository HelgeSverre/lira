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
#include <errno.h>
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(_WIN32)
#error "the Lira native backend does not support Windows yet"
#else
#include <sys/mman.h>
#include <unistd.h>
#endif

void lira_rt_panic(const char *message);

#define LIRA_FIBER_STACK_SIZE (256 * 1024)
#define LIRA_NATIVE_MAX_FIBERS_DEFAULT 512u

enum LiraFiberState {
    LIRA_FIBER_READY = 0,
    LIRA_FIBER_RUNNING = 1,
    LIRA_FIBER_BLOCKED = 2,
    LIRA_FIBER_DONE = 3,
    LIRA_FIBER_BLOCKED_IO = 4
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
    int8_t xfer_failed;   /* set when a blocked send was woken by close() */
    int64_t select_progress; /* g_channel_progress when this select last spun */
    int64_t select_spins;
    uint64_t io_generation;
    uint64_t io_request;
    struct LiraFiber *next;
    struct LiraFiber *all_next;
} LiraFiber;

/* ------------------------------------------------------------------ */
/* Scheduler state                                                     */
/* ------------------------------------------------------------------ */

/* Bumped by anything that could make a blocked `select` ready. A select with no
 * default spins through the run queue; if a full sweep goes by with no channel
 * activity at all, nothing can ever make it ready and it is a deadlock. */
static int64_t g_channel_progress = 0;

static LiraFiber *g_run_head = NULL;
static LiraFiber *g_run_tail = NULL;
static LiraFiber *g_current = NULL;
static LiraFiber *g_all_fibers = NULL;
static struct LiraChan *g_all_channels = NULL;
static void *g_sched_sp = NULL;
static int64_t g_next_id = 0;
static int64_t g_live = 0; /* fibers created but not yet finished */
static size_t g_fiber_limit;
static int g_fiber_limit_initialized;
static int8_t g_runtime_failed = 0;
static uint64_t g_next_channel_id = 1;
static uint64_t g_select_rng = UINT64_C(0x6a09e667f3bcc909);

#define LIRA_SELECT_DEFAULT_SEED UINT64_C(0x6a09e667f3bcc909)
#define LIRA_SELECT_INCREMENT UINT64_C(0x9e3779b97f4a7c15)
#define LIRA_SELECT_CHANNEL_SALT UINT64_C(0xd6e8feb86659fd93)
#define LIRA_SELECT_DIRECTION_SALT UINT64_C(0xa5a356341f125d3b)
#define LIRA_SELECT_ORDINAL_SALT UINT64_C(0x8cb92d3b5e4f1a77)

static void lira_fiber_cleanup(void);
static void lira_runq_push(LiraFiber *f);

/* Fiber stacks are mmap'd rather than GC objects, so bound their count
 * independently. The environment is a fail-closed lower-only override. */
static size_t lira_fiber_limit(void) {
    if (g_fiber_limit_initialized) {
        return g_fiber_limit;
    }
    g_fiber_limit_initialized = 1;
    g_fiber_limit = LIRA_NATIVE_MAX_FIBERS_DEFAULT;
    const char *text = getenv("LIRA_NATIVE_MAX_FIBERS");
    if (text == NULL) {
        return g_fiber_limit;
    }
    if (*text == '\0') {
        lira_rt_panic("native fiber limit is invalid");
        return 0;
    }
    size_t value = 0;
    for (const unsigned char *cursor = (const unsigned char *)text; *cursor != '\0';
         ++cursor) {
        if (*cursor < '0' || *cursor > '9') {
            lira_rt_panic("native fiber limit is invalid");
            return 0;
        }
        size_t digit = (size_t)(*cursor - '0');
        if (value > (SIZE_MAX - digit) / 10) {
            lira_rt_panic("native fiber limit is invalid");
            return 0;
        }
        value = value * 10 + digit;
    }
    if (value == 0 || value > LIRA_NATIVE_MAX_FIBERS_DEFAULT) {
        lira_rt_panic("native fiber limit is invalid");
        return 0;
    }
    g_fiber_limit = value;
    return g_fiber_limit;
}

/* Completion callbacks are invoked only by lira_io_drain on this scheduler
 * thread.  A worker may retain the fiber pointer as opaque owner data, but it
 * never dereferences it.  Generation checks make a late completion harmless. */
static void lira_fiber_io_complete(void *owner, uint64_t generation, void *result,
                                   int status, void *failure_arg) {
    (void)result;
    (void)failure_arg;
    lira_rt_io_wake(owner, generation, status);
}

int8_t lira_rt_io_wake(void *owner, uint64_t generation, int status) {
    LiraFiber *f = (LiraFiber *)owner;
    (void)status;
    if (f == NULL || f->state != LIRA_FIBER_BLOCKED_IO ||
        f->io_request != generation) {
        return 0;
    }
    f->io_request = 0;
    f->state = LIRA_FIBER_READY;
    g_channel_progress++;
    lira_runq_push(f);
    return 1;
}

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
    g_channel_progress++;
    /* Hand control back to the scheduler for the last time. The scheduler
     * reclaims the stack we are standing on, so this switch must not return. */
    lira_ctx_switch(&f->sp, g_sched_sp);
    lira_rt_panic("resumed a finished fiber");
}

static int lira_fiber_init_stack(LiraFiber *f) {
    long page = sysconf(_SC_PAGESIZE);
    if (page <= 0) {
        page = 4096;
    }
    size_t guard = (size_t)page;
    size_t usable = LIRA_FIBER_STACK_SIZE;
    if (guard > SIZE_MAX - usable) {
        lira_gc_note_allocation_failure("fiber stack size overflow");
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }
    f->map_size = guard + usable;

    /* mmap'd stacks are outside the hidden-header allocator, but must still
     * participate in the same process-wide budget. Reserve the entire mapping
     * before asking the kernel for it, and commit only once both the mapping
     * and its guard page are ready. */
    if (!lira_gc_try_reserve_external(f->map_size)) {
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }

    void *map = mmap(NULL, f->map_size, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (map == MAP_FAILED) {
        lira_gc_release_external_reservation(f->map_size);
        lira_gc_note_allocation_failure("failed to allocate fiber stack");
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }
    /* Stacks grow down, so the guard page goes at the low end: an overflow
     * faults instead of silently trampling another fiber's heap data. */
    if (mprotect(map, guard, PROT_NONE) != 0) {
        munmap(map, f->map_size);
        lira_gc_release_external_reservation(f->map_size);
        lira_gc_note_allocation_failure("failed to guard fiber stack");
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }
    lira_gc_commit_external_alloc(f->map_size);
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
    return 1;
}

static LiraFiber *lira_fiber_new(LiraFiberEntry entry, void *env) {
    if (g_live < 0 || (uint64_t)g_live >= lira_fiber_limit()) {
        lira_rt_panic("native fiber limit exceeded");
        return NULL;
    }
    LiraFiber *f = (LiraFiber *)lira_rt_mem_try_alloc(sizeof(LiraFiber), 1);
    if (f == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
        return NULL;
    }
    f->entry = entry;
    f->env = env;
    f->state = LIRA_FIBER_READY;
    f->id = g_next_id++;
    if (!lira_fiber_init_stack(f)) {
        lira_rt_mem_free(f);
        return NULL;
    }
    f->all_next = g_all_fibers;
    g_all_fibers = f;
    g_live++;
    return f;
}

static void lira_fiber_free(LiraFiber *f) {
    LiraFiber **link = &g_all_fibers;
    while (*link != NULL && *link != f) {
        link = &(*link)->all_next;
    }
    if (*link == f) {
        *link = f->all_next;
    }
    if (f->stack_map != NULL) {
        munmap(f->stack_map, f->map_size);
        lira_gc_account_external_free(f->map_size);
        f->stack_map = NULL;
    }
    lira_rt_mem_free(f);
}

/* The collector is called from generated code while a fiber is running. A
 * local marker is below the generated caller's frames on downward-growing
 * stacks, so scanning from it to the stack top includes all live native
 * locals. Suspended fibers retain their saved SP and are scanned from there. */
void lira_fiber_gc_scan_roots(void) {
    uintptr_t marker = 0;
    jmp_buf registers;
    (void)setjmp(registers);
    lira_gc_mark_range(&registers, (const char *)&registers + sizeof(registers));
    for (LiraFiber *f = g_all_fibers; f != NULL; f = f->all_next) {
        uintptr_t begin;
        if (f == g_current) {
            begin = (uintptr_t)&marker;
        } else if (f->sp != NULL) {
            begin = (uintptr_t)f->sp;
        } else {
            begin = (uintptr_t)f->stack_map;
        }
        uintptr_t end = (uintptr_t)f->stack_map + f->map_size;
        if (begin < end) {
            lira_gc_mark_range((const void *)begin, (const void *)end);
        }
        lira_gc_mark_ptr(f->env);
        lira_gc_mark_ptr((const void *)(uintptr_t)f->xfer);
    }
}

/* Suspend the running fiber and return to the scheduler loop. */
static void lira_switch_to_scheduler(void) {
    LiraFiber *f = g_current;
    if (f == NULL) {
        lira_rt_panic("fiber operation outside of a fiber");
    }
    lira_ctx_switch(&f->sp, g_sched_sp);
}

/* Sleep work owns only the copied duration and never enters the Lira runtime. */
static int lira_sleep_work(void *arg, void **result) {
    int64_t millis = *(int64_t *)arg;
    while (millis > 0 && !lira_io_cancelled()) {
        int64_t slice = millis > 10 ? 10 : millis;
        struct timespec ts = {
            .tv_sec = 0,
            .tv_nsec = (long)(slice * 1000000),
        };
        while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
        }
        millis -= slice;
    }
    *result = NULL;
    return lira_io_cancelled() ? 1 : 0;
}

int8_t lira_rt_fail_in_fiber(const char *message) {
    (void)message;
    LiraFiber *f = g_current;
    if (f == NULL || g_sched_sp == NULL || f->state != LIRA_FIBER_RUNNING) {
        return 0;
    }
    g_runtime_failed = 1;
    f->state = LIRA_FIBER_DONE;
    f->io_request = 0;
    if (g_live > 0) {
        g_live--;
    }
    /* The scheduler owns cleanup of this fiber.  The generated call stack is
     * abandoned without unwinding through Rust FFI frames. */
    lira_ctx_switch(&f->sp, g_sched_sp);
    return 1;
}

int8_t lira_rt_io_submit_current(LiraIoWorkFn work, void *arg,
                                  LiraIoDestroyFn destroy_arg,
                                  LiraIoCompleteFn complete,
                                  LiraIoDestroyFn destroy_result) {
    LiraFiber *f = g_current;
    if (f == NULL) {
        return 0;
    }
    uint64_t generation = ++f->io_generation;
    f->io_request = generation;
    if (lira_io_submit(work, arg, destroy_arg, complete, destroy_result, f,
                       generation) != 0) {
        f->io_request = 0;
        return -1;
    }
    f->state = LIRA_FIBER_BLOCKED_IO;
    lira_switch_to_scheduler();
    return 1;
}

int8_t lira_rt_io_sleep(int64_t millis) {
    if (millis <= 0) {
        return 0;
    }
    if (g_current == NULL) {
        return 0;
    }
    int64_t *arg = (int64_t *)lira_rt_mem_try_alloc(sizeof(int64_t), 0);
    if (arg == NULL) {
        return -1;
    }
    *arg = millis;
    int8_t parked = lira_rt_io_submit_current(lira_sleep_work, arg,
                                               lira_rt_mem_free,
                                               lira_fiber_io_complete, NULL);
    if (parked != 1) {
        lira_rt_mem_free(arg);
    }
    return parked;
}

/* ------------------------------------------------------------------ */
/* Public fiber API                                                    */
/* ------------------------------------------------------------------ */

int64_t lira_rt_spawn(LiraFiberEntry entry, void *env) {
    LiraFiber *f = lira_fiber_new(entry, env);
    if (f == NULL) {
        return -1;
    }
    lira_runq_push(f);
    return f->id;
}

/* Park a `select` that found no ready arm. Returns after yielding, so the
 * caller loops and tries its arms again. */
void lira_rt_select_block(void) {
    LiraFiber *f = g_current;
    if (f == NULL) {
        lira_rt_panic("`select` outside of a fiber");
    }
    if (f->select_progress != g_channel_progress) {
        f->select_progress = g_channel_progress;
        f->select_spins = 0;
    } else {
        f->select_spins++;
    }
    /* One sweep of the run queue plus slack: if every other fiber has had a
     * turn and no channel moved, no arm can ever become ready. */
    /* A pending worker completion can wake a fiber and make progress without
     * changing channel counters. Do not classify this as a channel deadlock
     * while the scheduler still has external work outstanding. */
    if (lira_io_pending() == 0 && f->select_spins > g_live + 4) {
        lira_rt_panic("deadlock - `select` has no arm that can become ready");
        return;
    }
    lira_rt_yield();
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
    /* A JIT module may invoke the runtime repeatedly in one host process. Do
     * not let a previous deadlock or completed run leak scheduler identity or
     * saved stacks into the next program. */
    lira_io_shutdown();
    if (lira_io_reap_orphans()) {
        lira_rt_tcp_reap_orphans();
        lira_rt_file_reap_orphans();
    }
    /* Handle tables are process-global for ABI compatibility; close anything
     * left by a prior JIT run after its workers have joined. */
    lira_rt_tcp_cancel_all();
    lira_rt_file_cancel_all();
    lira_fiber_cleanup();
    /* Parse the lower-only memory policy on the scheduler thread before any
     * I/O worker can attempt a raw allocation.  This preserves the exact
     * diagnostic and prevents a malformed policy from looking like an
     * unrelated worker-pool startup failure. */
    if (!lira_gc_initialize_memory_limit()) {
        fflush(stdout);
        fprintf(stderr, "lira: runtime error: native memory limit is invalid\n");
        fflush(stderr);
        return 1;
    }
    if (lira_io_start() != 0) {
        fflush(stdout);
        fprintf(stderr, "lira: runtime error: %s\n", lira_gc_last_allocation_error());
        fflush(stderr);
        return 1;
    }
    g_next_id = 0;
    g_next_channel_id = 1;
    g_channel_progress = 0;
    g_runtime_failed = 0;
    g_select_rng = LIRA_SELECT_DEFAULT_SEED;
    const char *seed_text = getenv("LIRA_SELECT_SEED");
    if (seed_text != NULL && *seed_text != '\0') {
        uint64_t parsed = 0;
        int valid = 1;
        for (const unsigned char *p = (const unsigned char *)seed_text; *p != '\0';
             ++p) {
            if (*p < '0' || *p > '9') {
                valid = 0;
                break;
            }
            uint64_t digit = (uint64_t)(*p - '0');
            if (parsed > (UINT64_MAX - digit) / UINT64_C(10)) {
                valid = 0;
                break;
            }
            parsed = parsed * UINT64_C(10) + digit;
        }
        if (valid && parsed != 0) {
            g_select_rng = parsed;
        }
    }
    LiraFiber *root = lira_fiber_new(entry, env);
    lira_runq_push(root);

    for (;;) {
        /* Completion handlers only run here, never on workers.  They turn
         * blocked fibers back into ordinary ready fibers and may enqueue more
         * work, so drain before selecting the next fiber. */
        lira_io_drain();
        LiraFiber *f = lira_runq_pop();
        if (f == NULL) {
            if (lira_io_pending() != 0) {
                lira_io_wait();
                continue;
            }
            break;
        }
        g_current = f;
        f->state = LIRA_FIBER_RUNNING;
        lira_ctx_switch(&g_sched_sp, f->sp);
        g_current = NULL;
        if (f->state == LIRA_FIBER_DONE) {
            lira_fiber_free(f);
        }
        if (g_runtime_failed) {
            break;
        }
    }

    fflush(stdout);
    if (g_runtime_failed) {
        lira_rt_tcp_cancel_all();
        lira_io_abort();
        lira_rt_file_cancel_all();
        lira_fiber_cleanup();
        return 1;
    }
    if (g_live > 0) {
        fprintf(stderr, "lira: fatal error: deadlock - all %" PRId64
                        " remaining fiber(s) are blocked\n",
                g_live);
        fflush(stderr);
        lira_rt_tcp_cancel_all();
        lira_io_shutdown();
        lira_rt_file_cancel_all();
        lira_fiber_cleanup();
        return 1;
    }
    lira_io_shutdown();
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
    uint64_t select_id;
    LiraFiber *send_head;
    LiraFiber *send_tail;
    LiraFiber *recv_head;
    LiraFiber *recv_tail;
    struct LiraChan *all_next;
} LiraChan;

static void lira_fiber_cleanup(void) {
    /* Wait queues are intrusive links into LiraFiber. Clear every channel's
     * queues before releasing the fibers, otherwise a later GC scan could
     * follow a dangling waiter after a deadlocked JIT run. */
    for (LiraChan *c = (LiraChan *)g_all_channels; c != NULL; c = c->all_next) {
        c->send_head = NULL;
        c->send_tail = NULL;
        c->recv_head = NULL;
        c->recv_tail = NULL;
    }
    while (g_all_fibers != NULL) {
        lira_fiber_free(g_all_fibers);
    }
    g_run_head = NULL;
    g_run_tail = NULL;
    g_current = NULL;
    g_sched_sp = NULL;
    g_live = 0;
}

void lira_fiber_gc_scan_channel(const void *handle) {
    const LiraChan *c = (const LiraChan *)handle;
    if (c == NULL) {
        return;
    }
    if (c->buf != NULL && c->cap > 0 && (uint64_t)c->cap <= SIZE_MAX / sizeof(int64_t)) {
        lira_gc_mark_range(c->buf, c->buf + c->cap);
    }
    for (LiraFiber *f = c->send_head; f != NULL; f = f->next) {
        lira_gc_mark_ptr(f->env);
        lira_gc_mark_ptr((const void *)(uintptr_t)f->xfer);
    }
    for (LiraFiber *f = c->recv_head; f != NULL; f = f->next) {
        lira_gc_mark_ptr(f->env);
        lira_gc_mark_ptr((const void *)(uintptr_t)f->xfer);
    }
}

void lira_fiber_gc_destroy_channel(void *handle) {
    LiraChan *c = (LiraChan *)handle;
    if (c == NULL) {
        return;
    }
    LiraChan **link = (LiraChan **)&g_all_channels;
    while (*link != NULL && *link != c) {
        link = &(*link)->all_next;
    }
    if (*link == c) {
        *link = c->all_next;
    }
    for (LiraFiber *f = c->send_head; f != NULL;) {
        LiraFiber *next = f->next;
        f->next = NULL;
        f = next;
    }
    for (LiraFiber *f = c->recv_head; f != NULL;) {
        LiraFiber *next = f->next;
        f->next = NULL;
        f = next;
    }
    c->send_head = NULL;
    c->send_tail = NULL;
    c->recv_head = NULL;
    c->recv_tail = NULL;
    if (c->buf == NULL) {
        return;
    }
    lira_rt_mem_free(c->buf);
    c->buf = NULL;
}

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
    /* Avoid `head + len`: both operands are bounded by cap, but their sum can
     * overflow signed int64 for a malformed or very large ABI capacity. */
    if (c->cap <= 0 || c->head < 0 || c->head >= c->cap || c->len < 0 ||
        c->len >= c->cap || c->buf == NULL) {
        lira_rt_panic("channel buffer metadata is invalid");
    }
    int64_t offset = c->len;
    int64_t remaining = c->cap - c->head;
    int64_t index = offset < remaining ? c->head + offset : offset - remaining;
    c->buf[index] = v;
    c->len++;
}

static int64_t lira_buf_pop(LiraChan *c) {
    if (c->cap <= 0 || c->head < 0 || c->head >= c->cap || c->len <= 0 ||
        c->len > c->cap || c->buf == NULL) {
        lira_rt_panic("channel buffer metadata is invalid");
    }
    int64_t v = c->buf[c->head];
    c->head = (c->head + 1) % c->cap;
    c->len--;
    return v;
}

void *lira_rt_chan_new(int64_t capacity) {
    if (capacity < 0) {
        lira_rt_panic("channel capacity is negative");
    }
    if ((uint64_t)capacity > SIZE_MAX / sizeof(int64_t)) {
        lira_rt_panic("channel capacity is too large");
    }
    LiraChan *c = (LiraChan *)lira_rt_alloc((int64_t)sizeof(LiraChan), LIRA_KIND_CHANNEL);
    c->cap = capacity;
    c->len = 0;
    c->head = 0;
    c->closed = 0;
    c->select_id = g_next_channel_id++;
    c->all_next = (LiraChan *)g_all_channels;
    g_all_channels = c;
    if (capacity > 0) {
        size_t bytes = (size_t)capacity * sizeof(int64_t);
        c->buf = (int64_t *)lira_rt_mem_try_alloc(bytes, 1);
        if (c->buf == NULL) {
            lira_rt_panic(lira_gc_last_allocation_error());
            return c;
        }
    }
    return c;
}

void lira_rt_chan_send(void *chan, int64_t value) {
    g_channel_progress++;
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
        r->xfer_failed = 0;
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
    self->xfer_failed = 0;
    self->state = LIRA_FIBER_BLOCKED;
    lira_wait_push(&c->send_head, &c->send_tail, self);
    lira_switch_to_scheduler();
    if (self->xfer_failed) {
        lira_rt_panic("send on closed channel");
    }
}

int64_t lira_rt_chan_recv(void *chan) {
    g_channel_progress++;
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
            s->xfer_failed = 0;
            lira_wake(s);
        }
        return v;
    }

    LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
    if (s != NULL) {
        int64_t v = s->xfer;
        s->xfer_failed = 0;
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
    self->xfer_failed = 0;
    lira_wait_push(&c->recv_head, &c->recv_tail, self);
    lira_switch_to_scheduler();
    return self->xfer;
}

int8_t lira_rt_chan_try_recv(void *chan, int64_t *out) {
    /* Only a *successful* try counts as progress: a `select` that keeps polling
     * an empty channel must not look like the program is getting somewhere, or
     * the deadlock check below can never fire. */
    LiraChan *c = (LiraChan *)chan;
    if (c == NULL || out == NULL) {
        return 0;
    }
    if (c->len > 0) {
        *out = lira_buf_pop(c);
        LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
        if (s != NULL) {
            lira_buf_push(c, s->xfer);
            lira_wake(s);
        }
        g_channel_progress++;
        return 1;
    }
    LiraFiber *s = lira_wait_pop(&c->send_head, &c->send_tail);
    if (s != NULL) {
        *out = s->xfer;
        s->xfer_failed = 0;
        lira_wake(s);
        g_channel_progress++;
        return 1;
    }
    if (c->closed) {
        *out = 0;
        g_channel_progress++;
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
        r->xfer_failed = 0;
        lira_wake(r);
        g_channel_progress++;
        return 1;
    }
    if (c->len < c->cap) {
        lira_buf_push(c, value);
        g_channel_progress++;
        return 1;
    }
    return 0;
}

static uint64_t lira_select_mix(uint64_t mixed) {
    mixed = (mixed ^ (mixed >> 30)) * UINT64_C(0xbf58476d1ce4e5b9);
    mixed = (mixed ^ (mixed >> 27)) * UINT64_C(0x94d049bb133111eb);
    return mixed ^ (mixed >> 31);
}

static uint64_t lira_select_next_seed(void) {
    g_select_rng += LIRA_SELECT_INCREMENT;
    return lira_select_mix(g_select_rng);
}

static uint64_t lira_select_score(uint64_t round_seed, const LiraSelectArm *arm,
                                  int is_send) {
    LiraChan *channel = (LiraChan *)arm->channel;
    uint64_t direction = (uint64_t)(is_send != 0);
    return lira_select_mix(round_seed ^ channel->select_id * LIRA_SELECT_CHANNEL_SALT ^
                           direction * LIRA_SELECT_DIRECTION_SALT ^
                           arm->ordinal * LIRA_SELECT_ORDINAL_SALT);
}

static int lira_select_recv_ready(const LiraChan *channel) {
    return channel != NULL &&
           (channel->len > 0 || channel->send_head != NULL || channel->closed);
}

static int lira_select_send_ready(const LiraChan *channel) {
    return channel != NULL && !channel->closed &&
           (channel->len < channel->cap || channel->recv_head != NULL);
}

int64_t lira_rt_select(const LiraSelectArm *arms, int64_t count, int64_t *recv_out) {
    if (arms == NULL || recv_out == NULL || count <= 0) {
        return -1;
    }

    int64_t selected = -1;
    uint64_t selected_score = 0;
    uint64_t round_seed = 0;
    int have_ready = 0;

    /* The first pass only observes channel state.  Cooperative fibers cannot
     * switch during this call, so the second pass can commit the winner
     * without a reservation or an allocation. */
    for (int64_t index = 0; index < count; ++index) {
        const LiraSelectArm *arm = &arms[index];
        LiraChan *channel = (LiraChan *)arm->channel;
        int ready = arm->operation == 0 ? lira_select_recv_ready(channel)
                                        : arm->operation == 1 &&
                                              lira_select_send_ready(channel);
        if (!ready) {
            continue;
        }
        if (!have_ready) {
            round_seed = lira_select_next_seed();
            have_ready = 1;
        }
        uint64_t score = lira_select_score(round_seed, arm, arm->operation == 1);
        /* Rust's max_by_key retains the later arm on an exact tie. */
        if (selected < 0 || score >= selected_score) {
            selected = index;
            selected_score = score;
        }
    }

    if (selected < 0) {
        return -1;
    }

    const LiraSelectArm *arm = &arms[selected];
    if (arm->operation == 0) {
        if (!lira_rt_chan_try_recv(arm->channel, recv_out)) {
            return -1;
        }
    } else if (arm->operation == 1) {
        if (!lira_rt_chan_try_send(arm->channel, arm->value)) {
            return -1;
        }
    } else {
        return -1;
    }
    return selected;
}

void lira_rt_chan_close(void *chan) {
    g_channel_progress++;
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
        f->xfer_failed = 0;
        lira_wake(f);
    }
    int blocked_send_failed = 0;
    while ((f = lira_wait_pop(&c->send_head, &c->send_tail)) != NULL) {
        f->xfer_failed = 1;
        lira_wake(f);
        blocked_send_failed = 1;
    }
    /* A send that was already in progress fails at the close operation. Stop
     * the current program before code after `close` can observe a state in
     * which that child failure has been deferred. This matches the bytecode
     * scheduler's immediate child-failure propagation. */
    if (blocked_send_failed) {
        lira_rt_panic("send on closed channel");
    }
}

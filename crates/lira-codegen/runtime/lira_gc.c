/*
 * Conservative tracing collector for the native backend.
 *
 * Native code does not carry enough type metadata to make a moving or fully
 * precise collector possible: generated structs and closures are ordinary
 * header-prefixed allocations.  Objects therefore stay at stable addresses
 * and are kept in a side table.  Roots are the live portions of every fiber
 * stack and each fiber environment; aggregate kinds with known layouts are
 * traversed precisely and unknown aggregates are scanned conservatively.
 */
#include "lira_rt.h"

#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct LiraGcObject {
    void *ptr;
    size_t size;
    uint32_t kind;
    uint8_t marked;
    struct LiraGcObject *next;
} LiraGcObject;

typedef struct LiraGcRootSlot {
    void *slot;
    struct LiraGcRootSlot *next;
} LiraGcRootSlot;

static LiraGcObject *g_objects;
static LiraGcObject **g_index;
static size_t g_index_cap;
static size_t g_index_len;
static LiraGcRootSlot *g_root_slots;
static size_t g_live_bytes;
static size_t g_live_objects;
/* Bytes promised to an in-flight calloc/realloc but not committed yet.  Keep
 * these separate from live bytes so failed allocations never inflate the
 * accounting, while the limit still covers the peak allocation. */
static size_t g_reserved_bytes;
static size_t g_since_collect;
static int g_collecting;
static LiraGcObject **g_worklist;
static size_t g_work_len;
static size_t g_work_cap;
/* Accounting is shared with I/O workers. Object graph state remains owned by
 * the scheduler; only these counters are protected by this mutex. Never call
 * lira_rt_panic or collect while holding it. */
static pthread_mutex_t g_accounting_lock = PTHREAD_MUTEX_INITIALIZER;
/* Once an internal underflow/overflow is observed, future reservations fail
 * closed.  Try-only worker allocation paths must not enter the fiber scheduler
 * merely to report an accounting invariant failure. */
static int g_accounting_invalid;
static _Thread_local const char *g_last_allocation_error;

/* Keep generated programs bounded without making small examples pay for a
 * collection on every temporary string.  Explicit `collect()` remains
 * available for deterministic tests and resource-sensitive programs. */
#define LIRA_GC_INITIAL_THRESHOLD (8u * 1024u * 1024u)
#define LIRA_GC_MAX_THRESHOLD (64u * 1024u * 1024u)
#define LIRA_NATIVE_MEMORY_LIMIT_DEFAULT (256u * 1024u * 1024u)
static size_t g_threshold = LIRA_GC_INITIAL_THRESHOLD;
static size_t g_memory_limit;
static int g_memory_limit_initialized;
static int g_memory_limit_invalid;

/* A runtime panic can abandon a fiber without unwinding its C frames. Reset
 * the re-entrancy guard before that control transfer so a later fiber or JIT
 * run is still able to collect. The next collection rebuilds mark state and
 * starts the worklist from length zero. */
void lira_gc_abort_collection(void) {
    g_collecting = 0;
}

/* Any support is optional for programs that never use dynamic values. A weak
 * fallback keeps the collector archive linkable in the host compiler; when a
 * generated program references Any, lira_any.c supplies the strong cleanup
 * implementation and overrides this no-op. */
#if defined(__GNUC__) || defined(__clang__)
__attribute__((weak)) void lira_rt_any_forget(const void *value) {
    (void)value;
}
#else
void lira_rt_any_forget(const void *value) {
    (void)value;
}
#endif

static int lira_gc_add_overflow(size_t a, size_t b) {
    return b > SIZE_MAX - a;
}

void lira_gc_note_allocation_failure(const char *message) {
    g_last_allocation_error = message;
}

const char *lira_gc_last_allocation_error(void) {
    return g_last_allocation_error != NULL ? g_last_allocation_error : "out of memory";
}

/* Decimal environment values are deliberately strict: malformed, zero, and
 * overflowed values fail closed instead of disabling the safety limit.  The
 * default is an upper bound; LIRA_NATIVE_MEMORY_LIMIT_BYTES may only lower it
 * for an untrusted or automated run. */
static int lira_gc_parse_memory_limit_locked(void) {
    if (g_memory_limit_initialized) {
        return !g_memory_limit_invalid;
    }
    g_memory_limit_initialized = 1;
    g_memory_limit = LIRA_NATIVE_MEMORY_LIMIT_DEFAULT;
    const char *text = getenv("LIRA_NATIVE_MEMORY_LIMIT_BYTES");
    if (text == NULL) {
        return 1;
    }
    if (*text == '\0') {
        g_memory_limit_invalid = 1;
        return 0;
    }
    size_t value = 0;
    for (const unsigned char *cursor = (const unsigned char *)text; *cursor != '\0';
         ++cursor) {
        if (*cursor < '0' || *cursor > '9') {
            g_memory_limit_invalid = 1;
            return 0;
        }
        size_t digit = (size_t)(*cursor - '0');
        if (value > (SIZE_MAX - digit) / 10) {
            g_memory_limit_invalid = 1;
            return 0;
        }
        value = value * 10 + digit;
    }
    if (value == 0 || value > LIRA_NATIVE_MEMORY_LIMIT_DEFAULT) {
        g_memory_limit_invalid = 1;
        return 0;
    }
    g_memory_limit = value;
    return 1;
}

int lira_gc_initialize_memory_limit(void) {
    pthread_mutex_lock(&g_accounting_lock);
    int valid = lira_gc_parse_memory_limit_locked();
    pthread_mutex_unlock(&g_accounting_lock);
    return valid;
}

static int lira_gc_contains(const LiraGcObject *object, uintptr_t candidate) {
    uintptr_t start = (uintptr_t)object->ptr;
    if (candidate < start) {
        return 0;
    }
    /* Do not let malformed sizes wrap the end address. */
    return candidate - start < object->size;
}

static size_t lira_gc_hash(uintptr_t value) {
    value ^= value >> 33;
    value *= UINT64_C(0xff51afd7ed558ccd);
    value ^= value >> 33;
    return (size_t)value;
}

static int lira_gc_index_rebuild(size_t requested) {
    if (requested == 0 && g_objects == NULL) {
        lira_rt_mem_free(g_index);
        g_index = NULL;
        g_index_cap = 0;
        g_index_len = 0;
        return 1;
    }
    size_t cap = 256;
    size_t minimum = requested > (SIZE_MAX - 1) / 2 ? SIZE_MAX : requested * 2 + 1;
    while (cap < requested || cap < minimum) {
        if (cap > SIZE_MAX / 2) {
            lira_gc_note_allocation_failure("native collector index overflow");
            return 0;
        }
        cap *= 2;
    }
    if (cap > SIZE_MAX / sizeof(*g_index)) {
        lira_gc_note_allocation_failure("native collector index overflow");
        return 0;
    }
    LiraGcObject **index = (LiraGcObject **)lira_rt_mem_try_alloc(
        cap * sizeof(*index), 1);
    if (index == NULL) {
        return 0;
    }
    size_t count = 0;
    for (LiraGcObject *object = g_objects; object != NULL; object = object->next) {
        size_t pos = lira_gc_hash((uintptr_t)object->ptr) & (cap - 1);
        while (index[pos] != NULL) {
            pos = (pos + 1) & (cap - 1);
        }
        index[pos] = object;
        count++;
    }
    lira_rt_mem_free(g_index);
    g_index = index;
    g_index_cap = cap;
    g_index_len = count;
    return 1;
}

static LiraGcObject *lira_gc_index_exact(uintptr_t candidate);

static int lira_gc_index_insert(LiraGcObject *object) {
    if (g_index_cap == 0 ||
        g_index_len >= (g_index_cap - 1) / 2) {
        if (!lira_gc_index_rebuild(g_index_len + 1)) {
            return 0;
        }
        if (lira_gc_index_exact((uintptr_t)object->ptr) == object) {
            return 1;
        }
    }
    size_t pos = lira_gc_hash((uintptr_t)object->ptr) & (g_index_cap - 1);
    while (g_index[pos] != NULL) {
        pos = (pos + 1) & (g_index_cap - 1);
    }
    g_index[pos] = object;
    g_index_len++;
    return 1;
}

static LiraGcObject *lira_gc_index_exact(uintptr_t candidate) {
    if (g_index_cap == 0) {
        return NULL;
    }
    size_t pos = lira_gc_hash(candidate) & (g_index_cap - 1);
    for (;;) {
        LiraGcObject *object = g_index[pos];
        if (object == NULL) {
            return NULL;
        }
        if ((uintptr_t)object->ptr == candidate) {
            return object;
        }
        pos = (pos + 1) & (g_index_cap - 1);
    }
}

static LiraGcObject *lira_gc_find(uintptr_t candidate) {
    /* Heap objects are returned as base pointers in generated code.  Accepting
     * an interior pointer also keeps conservative scanning sound for C helper
     * frames that temporarily point at a field. */
    LiraGcObject *exact = lira_gc_index_exact(candidate);
    if (exact != NULL) {
        return exact;
    }
    for (LiraGcObject *object = g_objects; object != NULL; object = object->next) {
        if (lira_gc_contains(object, candidate)) {
            return object;
        }
    }
    return NULL;
}

void lira_gc_mark_ptr(const void *candidate) {
    uintptr_t value = (uintptr_t)candidate;
    if (value == 0 || (value & (sizeof(uintptr_t) - 1)) != 0) {
        return;
    }
    LiraGcObject *object = lira_gc_find(value);
    if (object == NULL || object->marked) {
        return;
    }
    object->marked = 1;
    if (g_work_len == g_work_cap) {
        size_t next_cap = g_work_cap == 0 ? 256 : g_work_cap * 2;
        if (next_cap < g_work_cap || next_cap > SIZE_MAX / sizeof(*g_worklist)) {
            lira_rt_panic("native collector worklist overflow");
            return;
        }
        LiraGcObject **next = (LiraGcObject **)lira_rt_mem_try_realloc(
            g_worklist, next_cap * sizeof(*g_worklist));
        if (next == NULL) {
            lira_rt_panic(lira_gc_last_allocation_error());
            return;
        }
        g_worklist = next;
        g_work_cap = next_cap;
    }
    g_worklist[g_work_len++] = object;
}

static void lira_gc_scan_object(const LiraGcObject *object) {
    switch (object->kind) {
        case LIRA_KIND_STRING:
            /* Strings contain bytes, never object pointers. */
            break;
            case LIRA_KIND_ARRAY: {
                const LiraArray *array = (const LiraArray *)object->ptr;
                if (array->data != NULL && array->len > 0 && array->cap >= array->len &&
                    (uint64_t)array->len <= SIZE_MAX / sizeof(int64_t)) {
                    lira_gc_mark_range(array->data, array->data + array->len);
                }
            break;
        }
        case LIRA_KIND_MAP:
            lira_map_gc_scan(object->ptr);
            break;
        case LIRA_KIND_CHANNEL:
            lira_fiber_gc_scan_channel(object->ptr);
            break;
        case LIRA_KIND_INTERFACE: {
            const LiraInterface *value = (const LiraInterface *)object->ptr;
            const LiraInterfaceSpec *spec = lira_rt_interface_spec(value);
            if (spec == NULL) {
                break;
            }
            /* The immutable spec/witness are never heap edges. */
            if (value->witness->payload_kind == LIRA_INTERFACE_PAYLOAD_REF) {
                lira_gc_mark_ptr((const void *)(uintptr_t)value->payload);
            }
            break;
        }
        case LIRA_KIND_ANY: {
            const LiraAny *value = (const LiraAny *)object->ptr;
            switch (value->tag) {
                case LIRA_ANY_STRING:
                case LIRA_ANY_ARRAY:
                case LIRA_ANY_OBJECT:
                case LIRA_ANY_REF:
                case LIRA_ANY_FUNCTION:
                case LIRA_ANY_CHANNEL:
                case LIRA_ANY_INTERFACE:
                    lira_gc_mark_ptr((const void *)(uintptr_t)value->payload);
                    break;
                default:
                    break;
            }
            break;
        }
        default:
            /* Structs, enums and closures have a header but no runtime type
             * descriptor. Scan aligned payload words conservatively. */
            if (object->size > sizeof(LiraHeader)) {
                const uintptr_t *begin = (const uintptr_t *)((const char *)object->ptr +
                                                               sizeof(LiraHeader));
                const uintptr_t *end = (const uintptr_t *)((const char *)object->ptr +
                                                             object->size);
                lira_gc_mark_range(begin, end);
            }
            break;
    }
}

void lira_gc_mark_range(const void *begin, const void *end) {
    uintptr_t first = (uintptr_t)begin;
    uintptr_t last = (uintptr_t)end;
    if (first > last) {
        uintptr_t temp = first;
        first = last;
        last = temp;
    }
    first &= ~(uintptr_t)(sizeof(uintptr_t) - 1);
    for (uintptr_t cursor = first; cursor < last; cursor += sizeof(uintptr_t)) {
        uintptr_t candidate = *(const uintptr_t *)cursor;
        lira_gc_mark_ptr((const void *)candidate);
        if (cursor > UINTPTR_MAX - sizeof(uintptr_t)) {
            break;
        }
    }
}

int lira_gc_register(void *ptr, size_t size, uint32_t kind) {
    g_last_allocation_error = NULL;
    LiraGcObject *object = (LiraGcObject *)lira_rt_mem_try_alloc(
        sizeof(LiraGcObject), 1);
    if (object == NULL) {
        return 0;
    }
    object->ptr = ptr;
    object->size = size;
    object->kind = kind;
    object->next = g_objects;
    g_objects = object;
    if (!lira_gc_index_insert(object)) {
        g_objects = object->next;
        lira_rt_mem_free(object);
        return 0;
    }
    pthread_mutex_lock(&g_accounting_lock);
    int object_count_overflow = g_live_objects == SIZE_MAX;
    pthread_mutex_unlock(&g_accounting_lock);
    if (object_count_overflow) {
        /* Roll back the side-table insertion; the caller still owns the
         * pending payload reservation and will release it on failure. */
        g_objects = object->next;
        g_index_len = 0;
        lira_rt_mem_free(object);
        (void)lira_gc_index_rebuild(0);
        lira_gc_note_allocation_failure("native heap object count overflow");
        return 0;
    }
    /* lira_rt_alloc reserved the object payload before calloc. */
    lira_gc_commit_external_alloc(size);
    pthread_mutex_lock(&g_accounting_lock);
    g_live_objects++;
    pthread_mutex_unlock(&g_accounting_lock);
    return 1;
}

void lira_gc_register_root_slot(void *slot) {
    if (slot == NULL) {
        return;
    }
    for (LiraGcRootSlot *root = g_root_slots; root != NULL; root = root->next) {
        if (root->slot == slot) {
            return;
        }
    }
    LiraGcRootSlot *root = (LiraGcRootSlot *)lira_rt_mem_try_alloc(
        sizeof(LiraGcRootSlot), 1);
    if (root == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
        return;
    }
    root->slot = slot;
    root->next = g_root_slots;
    g_root_slots = root;
}

/* JIT data sections belong to a temporary JITModule. Clear their addresses
 * before that module is dropped; otherwise a later JIT run could scan a stale
 * global cell. AOT processes keep their root set for the process lifetime. */
void lira_gc_unregister_all_root_slots(void) {
    while (g_root_slots != NULL) {
        LiraGcRootSlot *root = g_root_slots;
        g_root_slots = root->next;
        lira_rt_mem_free(root);
    }
}

/* Remove a previously registered root slot once the value it kept alive has
 * been published into a durable (fiber- or heap-reachable) location. Keeps the
 * transient C-helper build loops from leaking one slot each. */
void lira_gc_unregister_root_slot(void *slot) {
    if (slot == NULL) {
        return;
    }
    LiraGcRootSlot **cursor = &g_root_slots;
    while (*cursor != NULL) {
        LiraGcRootSlot *root = *cursor;
        if (root->slot == slot) {
            *cursor = root->next;
            lira_rt_mem_free(root);
            return;
        }
        cursor = &root->next;
    }
}

void lira_gc_reserve_external(size_t bytes) {
    if (bytes == 0) {
        return;
    }
    lira_gc_maybe_collect();
    pthread_mutex_lock(&g_accounting_lock);
    int valid = lira_gc_parse_memory_limit_locked();
    size_t limit = g_memory_limit;
    int accounting_valid = !g_accounting_invalid;
    int available = valid && accounting_valid &&
                    !lira_gc_add_overflow(g_reserved_bytes, bytes) &&
                    !lira_gc_add_overflow(g_live_bytes, g_reserved_bytes) &&
                    g_live_bytes <= limit && g_reserved_bytes <= limit - g_live_bytes &&
                    bytes <= limit - g_live_bytes - g_reserved_bytes;
    if (available) {
        g_reserved_bytes += bytes;
    }
    pthread_mutex_unlock(&g_accounting_lock);
    if (!valid) {
        lira_rt_panic("native memory limit is invalid");
        return;
    }
    if (!accounting_valid) {
        lira_rt_panic("native heap accounting is invalid");
        return;
    }
    if (!available) {
        lira_rt_panic("native memory limit exceeded");
        return;
    }
}

int lira_gc_try_reserve_external(size_t bytes) {
    if (bytes == 0) {
        g_last_allocation_error = NULL;
        return 1;
    }
    /* Worker-safe path: no collection, panic, or scheduler interaction. */
    pthread_mutex_lock(&g_accounting_lock);
    int valid = lira_gc_parse_memory_limit_locked();
    int accounting_valid = !g_accounting_invalid;
    size_t limit = g_memory_limit;
    int available = valid && accounting_valid &&
                    !lira_gc_add_overflow(g_reserved_bytes, bytes) &&
                    !lira_gc_add_overflow(g_live_bytes, g_reserved_bytes) &&
                    g_live_bytes <= limit && g_reserved_bytes <= limit - g_live_bytes &&
                    bytes <= limit - g_live_bytes - g_reserved_bytes;
    if (available) {
        g_reserved_bytes += bytes;
    }
    pthread_mutex_unlock(&g_accounting_lock);
    if (available) {
        g_last_allocation_error = NULL;
    } else if (!valid) {
        lira_gc_note_allocation_failure("native memory limit is invalid");
    } else if (!accounting_valid) {
        lira_gc_note_allocation_failure("native heap accounting is invalid");
    } else {
        lira_gc_note_allocation_failure("native memory limit exceeded");
    }
    return available;
}

void lira_gc_release_external_reservation(size_t bytes) {
    pthread_mutex_lock(&g_accounting_lock);
    int underflow = bytes > g_reserved_bytes;
    if (!underflow) {
        g_reserved_bytes -= bytes;
    } else {
        g_accounting_invalid = 1;
    }
    pthread_mutex_unlock(&g_accounting_lock);
}

void lira_gc_commit_external_alloc(size_t bytes) {
    pthread_mutex_lock(&g_accounting_lock);
    int failed = g_accounting_invalid || bytes > g_reserved_bytes ||
                 lira_gc_add_overflow(g_live_bytes, bytes);
    if (!failed) {
        g_reserved_bytes -= bytes;
        g_live_bytes += bytes;
        if (!lira_gc_add_overflow(g_since_collect, bytes)) {
            g_since_collect += bytes;
        } else {
            g_since_collect = SIZE_MAX;
        }
    } else {
        g_accounting_invalid = 1;
    }
    pthread_mutex_unlock(&g_accounting_lock);
    if (failed) {
        lira_rt_panic("native heap accounting overflow");
        return;
    }
}

int lira_gc_try_commit_external_alloc(size_t bytes) {
    pthread_mutex_lock(&g_accounting_lock);
    int success = !g_accounting_invalid && bytes <= g_reserved_bytes &&
                  !lira_gc_add_overflow(g_live_bytes, bytes);
    if (success) {
        g_reserved_bytes -= bytes;
        g_live_bytes += bytes;
        if (!lira_gc_add_overflow(g_since_collect, bytes)) {
            g_since_collect += bytes;
        } else {
            g_since_collect = SIZE_MAX;
        }
    } else {
        g_accounting_invalid = 1;
    }
    pthread_mutex_unlock(&g_accounting_lock);
    if (!success) {
        lira_gc_note_allocation_failure("native heap accounting is invalid");
    }
    return success;
}

void lira_gc_account_external_free(size_t bytes) {
    pthread_mutex_lock(&g_accounting_lock);
    if (bytes > g_live_bytes) {
        g_accounting_invalid = 1;
    } else {
        g_live_bytes -= bytes;
    }
    pthread_mutex_unlock(&g_accounting_lock);
}

int lira_gc_validate_no_reservations(void) {
    pthread_mutex_lock(&g_accounting_lock);
    int accounting_valid = !g_accounting_invalid;
    int reservations_empty = g_reserved_bytes == 0;
    pthread_mutex_unlock(&g_accounting_lock);
    if (!accounting_valid) {
        lira_gc_note_allocation_failure("native heap accounting is invalid");
    } else if (!reservations_empty) {
        lira_gc_note_allocation_failure("native heap has outstanding allocation reservations");
    }
    return accounting_valid && reservations_empty;
}

static void lira_gc_sweep(void) {
    LiraGcObject **link = &g_objects;
    while (*link != NULL) {
        LiraGcObject *object = *link;
        if (object->marked) {
            object->marked = 0;
            link = &object->next;
            continue;
        }
        *link = object->next;
        switch (object->kind) {
            case LIRA_KIND_ARRAY: {
                LiraArray *array = (LiraArray *)object->ptr;
                if (array->data != NULL) {
                    lira_rt_mem_free(array->data);
                }
                break;
            }
            case LIRA_KIND_MAP:
                lira_map_gc_destroy(object->ptr);
                break;
            case LIRA_KIND_CHANNEL:
                lira_fiber_gc_destroy_channel(object->ptr);
                break;
            case LIRA_KIND_ANY:
                lira_rt_any_forget(object->ptr);
                break;
            default:
                break;
        }
        lira_gc_account_external_free(object->size);
        pthread_mutex_lock(&g_accounting_lock);
        if (g_live_objects > 0) {
            g_live_objects--;
        }
        pthread_mutex_unlock(&g_accounting_lock);
        free(object->ptr);
        lira_rt_mem_free(object);
    }
    /* Drop the old index before rebuilding: after sweeping it contains stale
     * pointers, and retaining it would charge both old and new tables at the
     * rebuild peak. Linear lookup remains correct if a bounded rebuild fails. */
    lira_rt_mem_free(g_index);
    g_index = NULL;
    g_index_cap = 0;
    g_index_len = 0;
    (void)lira_gc_index_rebuild(0);
}

void lira_rt_collect(void) {
    if (g_collecting) {
        return;
    }
    g_collecting = 1;
    g_work_len = 0;
    for (LiraGcObject *object = g_objects; object != NULL; object = object->next) {
        object->marked = 0;
    }
    for (LiraGcRootSlot *root = g_root_slots; root != NULL; root = root->next) {
        lira_gc_mark_range(root->slot, (const char *)root->slot + sizeof(uintptr_t));
    }
    lira_fiber_gc_scan_roots();
    while (g_work_len > 0) {
        lira_gc_scan_object(g_worklist[--g_work_len]);
    }
    lira_rt_mem_free(g_worklist);
    g_worklist = NULL;
    g_work_cap = 0;
    lira_gc_sweep();
    pthread_mutex_lock(&g_accounting_lock);
    g_since_collect = 0;
    size_t live_bytes = g_live_bytes;
    pthread_mutex_unlock(&g_accounting_lock);
    size_t target = live_bytes > SIZE_MAX / 2 ? SIZE_MAX : live_bytes * 2;
    if (target < LIRA_GC_INITIAL_THRESHOLD) {
        target = LIRA_GC_INITIAL_THRESHOLD;
    }
    pthread_mutex_lock(&g_accounting_lock);
    g_threshold = target > LIRA_GC_MAX_THRESHOLD ? LIRA_GC_MAX_THRESHOLD : target;
    pthread_mutex_unlock(&g_accounting_lock);
    g_collecting = 0;
}

int64_t lira_rt_gc_live_bytes(void) {
    pthread_mutex_lock(&g_accounting_lock);
    size_t live_bytes = g_live_bytes;
    pthread_mutex_unlock(&g_accounting_lock);
    return live_bytes > INT64_MAX ? INT64_MAX : (int64_t)live_bytes;
}

int64_t lira_rt_gc_live_objects(void) {
    pthread_mutex_lock(&g_accounting_lock);
    size_t live_objects = g_live_objects;
    pthread_mutex_unlock(&g_accounting_lock);
    return live_objects > INT64_MAX ? INT64_MAX : (int64_t)live_objects;
}

/* Called by lira_rt_alloc after the new object is visible to the collector.
 * Kept out of the public ABI so allocation policy cannot be changed by source
 * programs. */
void lira_gc_maybe_collect(void) {
    pthread_mutex_lock(&g_accounting_lock);
    size_t since_collect = g_since_collect;
    size_t threshold = g_threshold;
    pthread_mutex_unlock(&g_accounting_lock);
    if (!g_collecting && since_collect >= threshold) {
        lira_rt_collect();
    }
}

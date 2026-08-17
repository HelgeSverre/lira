/*
 * liblira_rt - native runtime for Lira programs compiled by lira-codegen.
 *
 * The Cranelift backend emits fully unboxed code: an `int` lives in an i64
 * register, a `float` in an f64 register, a `bool` in an i8. Only heap values
 * (strings, arrays, structs, enums, channels) are pointers, and every one of
 * them starts with the same `LiraHeader` so the runtime can print, compare and
 * (eventually) reclaim them generically.
 *
 * Memory: heap objects are registered with the native tracing collector. The
 * collector is conservative at machine-code boundaries (generated structs and
 * closures have no runtime type descriptor), but uses precise layouts for
 * strings, arrays, maps, channels, and Any values. The `rc` field remains for
 * ABI compatibility with the bytecode representation.
 */
#ifndef LIRA_RT_H
#define LIRA_RT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------ */
/* Object model                                                        */
/* ------------------------------------------------------------------ */

enum LiraKind {
    LIRA_KIND_STRING = 1,
    LIRA_KIND_ARRAY = 2,
    LIRA_KIND_STRUCT = 3,
    LIRA_KIND_ENUM = 4,
    LIRA_KIND_CHANNEL = 5,
    LIRA_KIND_MAP = 6,
    LIRA_KIND_ANY = 7,
    LIRA_KIND_INTERFACE = 8
};

/* 16 bytes, keeping every payload that follows 8-byte aligned. */
typedef struct LiraHeader {
    uint32_t kind;
    uint32_t flags;
    int64_t rc;
} LiraHeader;

typedef struct LiraStr {
    LiraHeader hdr;
    int64_t len;   /* byte length, excluding the NUL terminator */
    char data[1];  /* NUL-terminated so the runtime can hand it to libc */
} LiraStr;

/* Native interface metadata is immutable compiler-emitted data.  The fixed
 * two-word layouts are intentional: both structures can be addressed by
 * static relocations without a runtime-owned descriptor allocation.  Names
 * and signatures are canonical LiraStr objects; `methods` has exactly
 * `method_count` entries. */
typedef struct LiraInterfaceMethod {
    const LiraStr *name;
    const LiraStr *signature;
} LiraInterfaceMethod;

typedef struct LiraInterfaceSpec {
    uint64_t method_count;
    const LiraInterfaceMethod *methods;
} LiraInterfaceSpec;

enum LiraInterfacePayloadKind {
    LIRA_INTERFACE_REF = 0,
    LIRA_INTERFACE_I64 = 1,
    LIRA_INTERFACE_F64 = 2,
    LIRA_INTERFACE_I8 = 3,
    LIRA_INTERFACE_PAYLOAD_REF = LIRA_INTERFACE_REF,
    LIRA_INTERFACE_PAYLOAD_I64 = LIRA_INTERFACE_I64,
    LIRA_INTERFACE_PAYLOAD_F64 = LIRA_INTERFACE_F64,
    LIRA_INTERFACE_PAYLOAD_I8 = LIRA_INTERFACE_I8
};

/* The trailing slots are function pointers with an intentionally erased C
 * calling convention. Rust lowering owns the typed dispatch ABI; this layer
 * only validates slot presence and retains the immutable table. */
typedef struct LiraInterfaceWitness {
    const LiraInterfaceSpec *spec;
    uint32_t payload_kind;
    uint32_t method_count;
    void (*method_slots[])(void);
} LiraInterfaceWitness;

typedef struct LiraInterface {
    LiraHeader hdr;
    uint64_t payload;
    const LiraInterfaceWitness *witness;
} LiraInterface;

#define LIRA_INTERFACE_MAX_METHODS UINT32_C(1024)

_Static_assert(sizeof(LiraInterfaceMethod) == 16, "interface method ABI size");
_Static_assert(offsetof(LiraInterfaceMethod, name) == 0, "interface method name ABI");
_Static_assert(offsetof(LiraInterfaceMethod, signature) == 8,
               "interface method signature ABI");
_Static_assert(sizeof(LiraInterfaceSpec) == 16, "interface spec ABI size");
_Static_assert(offsetof(LiraInterfaceSpec, method_count) == 0,
               "interface spec count ABI");
_Static_assert(offsetof(LiraInterfaceSpec, methods) == 8, "interface spec methods ABI");
_Static_assert(offsetof(LiraInterfaceWitness, spec) == 0, "interface witness spec ABI");
_Static_assert(offsetof(LiraInterfaceWitness, payload_kind) == 8,
               "interface witness kind ABI");
_Static_assert(offsetof(LiraInterfaceWitness, method_count) == 12,
               "interface witness count ABI");
_Static_assert(offsetof(LiraInterfaceWitness, method_slots) == 16,
               "interface witness slots ABI");
_Static_assert(offsetof(LiraInterface, payload) == 16, "interface payload ABI");
_Static_assert(offsetof(LiraInterface, witness) == 24, "interface witness pointer ABI");
_Static_assert(sizeof(LiraInterface) == 32, "interface ABI size");

typedef struct LiraArray {
    LiraHeader hdr;
    int64_t len;
    int64_t cap;
    int64_t *data; /* uniform 8-byte slots; floats are bit-cast, refs are pointers */
} LiraArray;

/* One native select arm.  Cranelift writes these fields at offsets 0, 8, 16,
 * and 24 respectively; the explicit padding keeps the C ABI size at 32 bytes
 * on every 64-bit target supported by the native backend. */
typedef struct LiraSelectArm {
    void *channel;
    int64_t value;
    uint64_t ordinal;
    uint8_t operation; /* 0 = receive, 1 = send */
    uint8_t reserved[7];
} LiraSelectArm;

_Static_assert(offsetof(LiraSelectArm, channel) == 0, "select channel ABI");
_Static_assert(offsetof(LiraSelectArm, value) == 8, "select value ABI");
_Static_assert(offsetof(LiraSelectArm, ordinal) == 16, "select ordinal ABI");
_Static_assert(offsetof(LiraSelectArm, operation) == 24, "select operation ABI");
_Static_assert(sizeof(LiraSelectArm) == 32, "select arm ABI size");

/* A dynamically typed value.  The payload is either a scalar encoded in its
 * native representation or a pointer-sized heap handle.  Any values are
 * themselves heap objects and are never represented by a null pointer; null
 * is the immortal value returned by lira_rt_any_null().
 *
 * `type_data`/`type_len` describe the element representation of an erased
 * array, tuple, or map. For LIRA_ANY_INTERFACE they instead hold the
 * immutable LiraInterfaceSpec pointer and bounded method count. They point at
 * immutable compiler data (or are zero for an already-dynamic aggregate), so
 * boxing never clones the aggregate.
 * The first four fields are the stable public prefix consumed by the Rust
 * JSON bridge; the metadata is deliberately trailing for ABI compatibility.
 */
typedef struct LiraAny {
    LiraHeader hdr;
    int64_t tag;
    uint64_t payload;
    uint64_t type_data;
    uint64_t type_len;
} LiraAny;

enum LiraAnyTag {
    LIRA_ANY_NULL = 0,
    LIRA_ANY_BOOL = 1,
    LIRA_ANY_INT = 2,
    LIRA_ANY_FLOAT = 3,
    LIRA_ANY_STRING = 4,
    LIRA_ANY_ARRAY = 5,
    LIRA_ANY_OBJECT = 6,
    LIRA_ANY_MAP = 6,
    LIRA_ANY_REF = 7,
    LIRA_ANY_FUNCTION = 8,
    LIRA_ANY_CHANNEL = 9,
    LIRA_ANY_FIBER = 10,
    LIRA_ANY_INTERFACE = 11
};

/* Structs are `LiraHeader` followed by C-layout fields (see layout.rs). */
/* Enums are `LiraHeader`, an i64 tag, then 8-byte payload slots. */

/* Offsets the code generator hard-codes; asserted in lira_rt.c. */
#define LIRA_HEADER_SIZE 16
#define LIRA_STR_LEN_OFFSET 16
#define LIRA_STR_DATA_OFFSET 24
#define LIRA_ARRAY_LEN_OFFSET 16
#define LIRA_ARRAY_CAP_OFFSET 24
#define LIRA_ARRAY_DATA_OFFSET 32
#define LIRA_ENUM_TAG_OFFSET 16
#define LIRA_ENUM_PAYLOAD_OFFSET 24

/* A double in positional notation runs to about 310 integer digits before the
 * point, plus a sign, a point and the fraction. */
#define LIRA_FLOAT_BUFFER 400

/* ------------------------------------------------------------------ */
/* Allocation                                                          */
/* ------------------------------------------------------------------ */

void *lira_rt_alloc(int64_t size, int32_t kind);
/* Worker-safe raw storage. These functions never collect or panic; NULL is
 * returned for a zero/overflowed/over-limit request or libc allocation
 * failure. The matching free operation recovers the hidden allocation header,
 * so callbacks may release storage without knowing its charged size. */
void *lira_rt_mem_try_alloc(size_t size, int zero);
void *lira_rt_mem_try_realloc(void *ptr, size_t size);
void lira_rt_mem_free(void *ptr);
char *lira_rt_mem_try_strdup(const char *text);
void lira_rt_collect(void);
int64_t lira_rt_gc_live_bytes(void);
int64_t lira_rt_gc_live_objects(void);
void lira_rt_panic(const char *message);
/* Returns non-zero after transferring an active fiber to the scheduler.  It
 * returns zero for calls made outside lira_rt_boot, where panic remains a
 * process-fatal error. */
int8_t lira_rt_fail_in_fiber(const char *message);
void lira_rt_abort(const LiraStr *message);

/* Memoizing contexts used by generated value-struct copy helpers. Context and
 * entries are ordinary managed struct objects, so the conservative collector
 * keeps all source/destination edges alive while a copy is in progress. */
void *lira_rt_copy_ctx_new(void);
void lira_rt_copy_ctx_free(void *ctx);
void *lira_rt_copy_ctx_lookup(void *ctx, const void *source);
void lira_rt_copy_ctx_insert(void *ctx, const void *source, void *destination);

/* ------------------------------------------------------------------ */
/* Strings                                                             */
/* ------------------------------------------------------------------ */

LiraStr *lira_rt_str_new(const char *bytes, int64_t len);
LiraStr *lira_rt_str_concat(const LiraStr *a, const LiraStr *b);
int64_t lira_rt_str_len(const LiraStr *s);
int64_t lira_rt_str_char_code(const LiraStr *s, int64_t index);
LiraStr *lira_rt_str_index(const LiraStr *s, int64_t index);
int8_t lira_rt_str_eq(const LiraStr *a, const LiraStr *b);
int64_t lira_rt_str_cmp(const LiraStr *a, const LiraStr *b);

LiraStr *lira_rt_int_to_str(int64_t v);
int64_t lira_rt_str_to_int(const LiraStr *s);
LiraStr *lira_rt_float_to_str(double v);
LiraStr *lira_rt_bool_to_str(int8_t v);

/* Managed interface values. The constructor validates all bounded metadata
 * and allocates the 32-byte object through the tracing collector. Membership
 * is structural: every target method name/signature pair must occur in the
 * value's witness, independent of any nominal interface name. */
LiraInterface *lira_rt_interface_new(uint64_t payload,
                                      const LiraInterfaceWitness *witness);
int8_t lira_rt_interface_is(const LiraInterface *value,
                            const LiraInterfaceSpec *target_spec);
const LiraInterfaceSpec *lira_rt_interface_spec(const LiraInterface *value);
uint64_t lira_rt_interface_payload(const LiraInterface *value,
                                   uint32_t expected_payload_kind);
void (*lira_rt_interface_method_slot(const LiraInterface *value,
                                     uint32_t index))(void);

/* ------------------------------------------------------------------ */
/* Printing                                                            */
/* ------------------------------------------------------------------ */

void lira_rt_print_str(const LiraStr *s);
void lira_rt_println_str(const LiraStr *s);
void lira_rt_print_int(int64_t v);
void lira_rt_println_int(int64_t v);
void lira_rt_print_float(double v);
void lira_rt_println_float(double v);
void lira_rt_print_bool(int8_t v);
void lira_rt_println_bool(int8_t v);

/* ------------------------------------------------------------------ */
/* Arrays                                                              */
/* ------------------------------------------------------------------ */

LiraArray *lira_rt_array_new(int64_t cap);
void lira_rt_array_push(LiraArray *a, int64_t value);
int64_t lira_rt_array_pop(LiraArray *a);
int64_t lira_rt_array_get(const LiraArray *a, int64_t index);
void lira_rt_array_set(LiraArray *a, int64_t index, int64_t value);
int64_t lira_rt_array_len(const LiraArray *a);

/* ------------------------------------------------------------------ */
/* Maps                                                                */
/* ------------------------------------------------------------------ */

void *lira_rt_map_new(void);
void lira_rt_map_set(void *map, LiraStr *key, int64_t value);
int64_t lira_rt_map_get(void *map, const LiraStr *key);
int8_t lira_rt_map_has(void *map, const LiraStr *key);
int64_t lira_rt_map_len(void *map);
LiraArray *lira_rt_map_keys(void *map);

/* ------------------------------------------------------------------ */
/* Dynamic Any values                                                  */
/* ------------------------------------------------------------------ */

LiraAny *lira_rt_any_null(void);
LiraAny *lira_rt_any_box_bool(int8_t value);
LiraAny *lira_rt_any_box_int(int64_t value);
LiraAny *lira_rt_any_box_float(double value);
LiraAny *lira_rt_any_box_string(LiraStr *value);
LiraAny *lira_rt_any_box_array(LiraArray *value);
LiraAny *lira_rt_any_box_array_typed(LiraArray *value, const LiraStr *type);
LiraAny *lira_rt_any_box_map(void *value);
LiraAny *lira_rt_any_box_map_typed(void *value, const LiraStr *type);
LiraAny *lira_rt_any_box_object(void *value);
LiraAny *lira_rt_any_box_object_typed(void *value, const LiraStr *type);
LiraAny *lira_rt_any_box_function(void *value);
LiraAny *lira_rt_any_box_function_typed(void *value, const LiraStr *type);
LiraAny *lira_rt_any_box_channel(void *value);
LiraAny *lira_rt_any_box_channel_typed(void *value, const LiraStr *type);
LiraAny *lira_rt_any_box_fiber(int64_t value);
LiraAny *lira_rt_any_box_ref(void *value);
LiraAny *lira_rt_any_box_interface(LiraInterface *value);
LiraAny *lira_rt_any_box_optional(void *value, const LiraStr *type);
LiraAny *lira_rt_any_from_slot(int64_t raw);
/* Copy an erased value at a semantic value boundary. Struct payloads are
 * cloned from their authoritative descriptor; reference-like payloads keep
 * their identity while receiving a fresh Any wrapper. */
LiraAny *lira_rt_any_copy(const LiraAny *value);

int8_t lira_rt_any_unbox_bool(const LiraAny *value);
int64_t lira_rt_any_unbox_int(const LiraAny *value);
double lira_rt_any_unbox_float(const LiraAny *value);
LiraStr *lira_rt_any_unbox_string(const LiraAny *value);
LiraArray *lira_rt_any_unbox_array(const LiraAny *value);
void *lira_rt_any_unbox_map(const LiraAny *value, const LiraStr *type);
void *lira_rt_any_unbox_ref(const LiraAny *value);
LiraInterface *lira_rt_any_unbox_interface(const LiraAny *value,
                                           const LiraInterfaceSpec *target_spec);
void *lira_rt_any_unbox_function(const LiraAny *value);
void *lira_rt_any_unbox_function_typed(const LiraAny *value, const LiraStr *type);
void *lira_rt_any_unbox_channel(const LiraAny *value);
void *lira_rt_any_unbox_channel_typed(const LiraAny *value, const LiraStr *type);
void *lira_rt_any_unbox_object_typed(const LiraAny *value, const LiraStr *type);
void *lira_rt_any_unbox_optional(const LiraAny *value, const LiraStr *type);
int64_t lira_rt_any_cast_int(const LiraAny *value);
double lira_rt_any_cast_float(const LiraAny *value);
int8_t lira_rt_any_cast_bool(const LiraAny *value);
int8_t lira_rt_any_truthy(const LiraAny *value);
LiraStr *lira_rt_any_to_string(const LiraAny *value);
int64_t lira_rt_any_len(const LiraAny *value);
int64_t lira_rt_any_object_len(const LiraAny *value);
LiraStr *lira_rt_any_object_key_at(const LiraAny *value, int64_t index);
LiraAny *lira_rt_any_index(const LiraAny *object, const LiraAny *key);
LiraAny *lira_rt_any_array_at(const LiraAny *object, int64_t index);
LiraAny *lira_rt_any_object_at(const LiraAny *object, const LiraStr *key);
void lira_rt_any_set(const LiraAny *object, const LiraAny *key, const LiraAny *value);
void lira_rt_any_push(const LiraAny *object, const LiraAny *value);
LiraAny *lira_rt_any_pop(const LiraAny *object);
LiraAny *lira_rt_any_binary(int64_t op, const LiraAny *left, const LiraAny *right);
LiraAny *lira_rt_any_neg(const LiraAny *value);
LiraAny *lira_rt_any_bit_not(const LiraAny *value);
int8_t lira_rt_any_compare(int64_t op, const LiraAny *left, const LiraAny *right);
int8_t lira_rt_any_is(const LiraAny *value, int64_t runtime_kind);
int8_t lira_rt_any_is_typed(const LiraAny *value, const LiraStr *type);

/* ------------------------------------------------------------------ */
/* Regular expressions                                                 */
/* ------------------------------------------------------------------ */

int8_t lira_rt_regex_match(const LiraStr *pattern, const LiraStr *text);
LiraStr *lira_rt_regex_find(const LiraStr *pattern, const LiraStr *text);
LiraArray *lira_rt_regex_find_all(const LiraStr *pattern, const LiraStr *text);
LiraStr *lira_rt_regex_replace(const LiraStr *pattern, const LiraStr *text,
                               const LiraStr *replacement);
LiraStr *lira_rt_regex_replace_all(const LiraStr *pattern, const LiraStr *text,
                                   const LiraStr *replacement);
LiraArray *lira_rt_regex_split(const LiraStr *pattern, const LiraStr *text);
LiraArray *lira_rt_regex_captures(const LiraStr *pattern, const LiraStr *text);
int8_t lira_rt_regex_is_valid(const LiraStr *pattern);

/* Implemented by the embedded Rust runtime (serde_json and ureq). */
LiraAny *lira_rt_json_parse(const LiraStr *value);
LiraStr *lira_rt_json_stringify(const LiraAny *value);
LiraStr *lira_rt_json_stringify_pretty(const LiraAny *value);
LiraArray *lira_rt_http_get(const LiraStr *url);
LiraArray *lira_rt_http_post(const LiraStr *url, const LiraStr *body,
                             const LiraStr *content_type);
LiraArray *lira_rt_http_request(const LiraStr *method, const LiraStr *url,
                                const LiraStr *headers, const LiraStr *body);

/* ------------------------------------------------------------------ */
/* Arithmetic helpers                                                  */
/* ------------------------------------------------------------------ */

int64_t lira_rt_idiv(int64_t a, int64_t b);
int64_t lira_rt_imod(int64_t a, int64_t b);
int64_t lira_rt_ipow(int64_t base, int64_t exp);
double lira_rt_math_fmod(double left, double right);

/* ------------------------------------------------------------------ */
/* Fibers and channels                                                 */
/* ------------------------------------------------------------------ */

typedef void (*LiraFiberEntry)(void *env);

/* Worker jobs use only owned plain data.  Work and destruction run on a
 * worker; completion runs on the scheduler thread after lira_io_drain(). */
typedef int (*LiraIoWorkFn)(void *arg, void **result);
typedef void (*LiraIoCompleteFn)(void *owner, uint64_t generation, void *result,
                                 int status, void *failure_arg);
typedef void (*LiraIoDestroyFn)(void *value);

int lira_io_start(void);
int lira_io_submit(LiraIoWorkFn work, void *arg, LiraIoDestroyFn destroy_arg,
                   LiraIoCompleteFn complete, LiraIoDestroyFn destroy_result,
                   void *owner, uint64_t generation);
int lira_io_submit_sleep(int64_t millis, void *owner, uint64_t generation,
                         LiraIoCompleteFn complete);
size_t lira_io_pending(void);
void lira_io_wait(void);
size_t lira_io_drain(void);
void lira_io_shutdown(void);
void lira_io_abort(void);
int lira_io_reap_orphans(void);
int lira_io_cancelled(void);
int lira_io_orphaned(void);
int lira_io_test_fail_result_alloc(const char *name);

/* Submit a worker job for the currently running fiber. Return 1 after the
 * fiber has parked, 0 when called outside a fiber, and -1 when submission
 * fails. Completion callbacks receive the opaque fiber owner and must call
 * lira_rt_io_wake(owner, generation, 0) after materialising their result.
 * `failure_arg` is non-NULL only when worker result allocation failed; it is
 * retained until the callback returns so busy-handle state can be repaired. */
int8_t lira_rt_io_submit_current(LiraIoWorkFn work, void *arg,
                                  LiraIoDestroyFn destroy_arg,
                                  LiraIoCompleteFn complete,
                                  LiraIoDestroyFn destroy_result);
int8_t lira_rt_io_wake(void *owner, uint64_t generation, int status);
void lira_rt_tcp_cancel_all(void);
void lira_rt_file_cancel_all(void);
void lira_rt_tcp_reap_orphans(void);
void lira_rt_file_reap_orphans(void);

/* Called by lira_rt_sleep to park only the current fiber on a worker job. */
int8_t lira_rt_io_sleep(int64_t millis);

/* Boot the scheduler with `entry` as fiber 0 and run until every fiber has
 * finished. Returns the process exit code. */
int32_t lira_rt_boot(LiraFiberEntry entry, void *env);

/* Record the process arguments so `env_args` can report them. */
void lira_rt_set_args(int argc, char **argv);

int64_t lira_rt_spawn(LiraFiberEntry entry, void *env);
void lira_rt_yield(void);
/* Yield from a `select` with no ready arm; reports a deadlock if none can be. */
void lira_rt_select_block(void);
/* Probe all arms and atomically commit exactly one ready communication arm.
 * Returns its descriptor index, or -1 when no communication arm is ready. */
int64_t lira_rt_select(const LiraSelectArm *arms, int64_t count,
                       int64_t *recv_out);
int64_t lira_rt_fiber_id(void);

void *lira_rt_chan_new(int64_t capacity);
void lira_rt_chan_send(void *chan, int64_t value);
int64_t lira_rt_chan_recv(void *chan);
void lira_rt_chan_close(void *chan);
int8_t lira_rt_chan_try_recv(void *chan, int64_t *out);
int8_t lira_rt_chan_try_send(void *chan, int64_t value);

/* Internal collector hooks implemented by the runtime translation units. */
void lira_gc_mark_range(const void *begin, const void *end);
void lira_gc_mark_ptr(const void *candidate);
void lira_gc_abort_collection(void);
int lira_gc_register(void *ptr, size_t size, uint32_t kind);
int lira_gc_initialize_memory_limit(void);
void lira_gc_note_allocation_failure(const char *message);
const char *lira_gc_last_allocation_error(void);
void lira_gc_register_root_slot(void *slot);
void lira_gc_unregister_root_slot(void *slot);
void lira_gc_unregister_all_root_slots(void);
int lira_gc_validate_no_reservations(void);
void lira_gc_maybe_collect(void);
/* Reserve backing storage before calling calloc/realloc.  A successful
 * allocation must be paired with commit; a failed one must release. */
void lira_gc_reserve_external(size_t bytes);
/* Worker-safe counterpart: no collection, panic, or scheduler interaction. */
int lira_gc_try_reserve_external(size_t bytes);
void lira_gc_release_external_reservation(size_t bytes);
void lira_gc_commit_external_alloc(size_t bytes);
int lira_gc_try_commit_external_alloc(size_t bytes);
void lira_gc_account_external_free(size_t bytes);
void lira_fiber_gc_scan_roots(void);
void lira_fiber_gc_scan_channel(const void *channel);
void lira_fiber_gc_destroy_channel(void *channel);
void lira_map_gc_scan(const void *map);
void lira_map_gc_destroy(void *map);
void lira_rt_any_forget(const void *value);

/* Implemented in lira_ctx.S. Saves callee-saved state on the current stack,
 * stores the resulting stack pointer through `save_sp`, then resumes `new_sp`. */
void lira_ctx_switch(void **save_sp, void *new_sp);

#ifdef __cplusplus
}
#endif

#endif /* LIRA_RT_H */

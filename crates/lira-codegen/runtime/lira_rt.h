/*
 * liblira_rt - native runtime for Lira programs compiled by lira-codegen.
 *
 * The Cranelift backend emits fully unboxed code: an `int` lives in an i64
 * register, a `float` in an f64 register, a `bool` in an i8. Only heap values
 * (strings, arrays, structs, enums, channels) are pointers, and every one of
 * them starts with the same `LiraHeader` so the runtime can print, compare and
 * (eventually) reclaim them generically.
 *
 * Memory: allocations come from malloc and are currently never reclaimed. The
 * `rc` field in the header is the hook for the ARC scheme the bytecode VM uses;
 * the native backend does not yet emit retain/release pairs. See
 * docs/60-native-backend.md.
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
    LIRA_KIND_MAP = 6
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

typedef struct LiraArray {
    LiraHeader hdr;
    int64_t len;
    int64_t cap;
    int64_t *data; /* uniform 8-byte slots; floats are bit-cast, refs are pointers */
} LiraArray;

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
void lira_rt_abort(const LiraStr *message);

/* ------------------------------------------------------------------ */
/* Strings                                                             */
/* ------------------------------------------------------------------ */

LiraStr *lira_rt_str_new(const char *bytes, int64_t len);
LiraStr *lira_rt_str_concat(const LiraStr *a, const LiraStr *b);
int64_t lira_rt_str_len(const LiraStr *s);
int8_t lira_rt_str_eq(const LiraStr *a, const LiraStr *b);
int64_t lira_rt_str_cmp(const LiraStr *a, const LiraStr *b);

LiraStr *lira_rt_int_to_str(int64_t v);
LiraStr *lira_rt_float_to_str(double v);
LiraStr *lira_rt_bool_to_str(int8_t v);

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
/* Arithmetic helpers                                                  */
/* ------------------------------------------------------------------ */

int64_t lira_rt_idiv(int64_t a, int64_t b);
int64_t lira_rt_imod(int64_t a, int64_t b);
int64_t lira_rt_ipow(int64_t base, int64_t exp);

/* ------------------------------------------------------------------ */
/* Fibers and channels                                                 */
/* ------------------------------------------------------------------ */

typedef void (*LiraFiberEntry)(void *env);

/* Boot the scheduler with `entry` as fiber 0 and run until every fiber has
 * finished. Returns the process exit code. */
int32_t lira_rt_boot(LiraFiberEntry entry, void *env);

/* Record the process arguments so `env_args` can report them. */
void lira_rt_set_args(int argc, char **argv);

int64_t lira_rt_spawn(LiraFiberEntry entry, void *env);
void lira_rt_yield(void);
/* Yield from a `select` with no ready arm; reports a deadlock if none can be. */
void lira_rt_select_block(void);
int64_t lira_rt_fiber_id(void);

void *lira_rt_chan_new(int64_t capacity);
void lira_rt_chan_send(void *chan, int64_t value);
int64_t lira_rt_chan_recv(void *chan);
void lira_rt_chan_close(void *chan);
int8_t lira_rt_chan_try_recv(void *chan, int64_t *out);
int8_t lira_rt_chan_try_send(void *chan, int64_t value);

/* Implemented in lira_ctx.S. Saves callee-saved state on the current stack,
 * stores the resulting stack pointer through `save_sp`, then resumes `new_sp`. */
void lira_ctx_switch(void **save_sp, void *new_sp);

#ifdef __cplusplus
}
#endif

#endif /* LIRA_RT_H */

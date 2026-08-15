#include "lira_rt.h"

#include <inttypes.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The code generator computes field offsets from these constants, so a layout
 * change here has to be a deliberate, matched change in layout.rs. */
#define LIRA_STATIC_ASSERT(cond, name) typedef char lira_static_assert_##name[(cond) ? 1 : -1]

LIRA_STATIC_ASSERT(sizeof(LiraHeader) == LIRA_HEADER_SIZE, header_size);
LIRA_STATIC_ASSERT(offsetof(LiraStr, len) == LIRA_STR_LEN_OFFSET, str_len);
LIRA_STATIC_ASSERT(offsetof(LiraStr, data) == LIRA_STR_DATA_OFFSET, str_data);
LIRA_STATIC_ASSERT(offsetof(LiraArray, len) == LIRA_ARRAY_LEN_OFFSET, array_len);
LIRA_STATIC_ASSERT(offsetof(LiraArray, cap) == LIRA_ARRAY_CAP_OFFSET, array_cap);
LIRA_STATIC_ASSERT(offsetof(LiraArray, data) == LIRA_ARRAY_DATA_OFFSET, array_data);

/* ------------------------------------------------------------------ */
/* Allocation                                                          */
/* ------------------------------------------------------------------ */

void lira_rt_panic(const char *message) {
    fflush(stdout);
    fprintf(stderr, "lira: runtime error: %s\n", message);
    fflush(stderr);
    exit(1);
}

void *lira_rt_alloc(int64_t size, int32_t kind) {
    if (size < (int64_t)sizeof(LiraHeader)) {
        size = (int64_t)sizeof(LiraHeader);
    }
    void *raw = calloc(1, (size_t)size);
    if (raw == NULL) {
        lira_rt_panic("out of memory");
    }
    LiraHeader *hdr = (LiraHeader *)raw;
    hdr->kind = (uint32_t)kind;
    hdr->flags = 0;
    hdr->rc = 1;
    return raw;
}

void lira_rt_abort(const LiraStr *message) {
    lira_rt_panic(message == NULL ? "aborted" : message->data);
}

/* ------------------------------------------------------------------ */
/* Strings                                                             */
/* ------------------------------------------------------------------ */

LiraStr *lira_rt_str_new(const char *bytes, int64_t len) {
    if (len < 0) {
        len = 0;
    }
    /* `data[1]` in the struct already covers the NUL terminator. */
    LiraStr *s = (LiraStr *)lira_rt_alloc((int64_t)sizeof(LiraStr) + len, LIRA_KIND_STRING);
    s->len = len;
    if (len > 0 && bytes != NULL) {
        memcpy(s->data, bytes, (size_t)len);
    }
    s->data[len] = '\0';
    return s;
}

LiraStr *lira_rt_str_concat(const LiraStr *a, const LiraStr *b) {
    int64_t alen = a ? a->len : 0;
    int64_t blen = b ? b->len : 0;
    LiraStr *s = (LiraStr *)lira_rt_alloc((int64_t)sizeof(LiraStr) + alen + blen, LIRA_KIND_STRING);
    s->len = alen + blen;
    if (alen > 0) {
        memcpy(s->data, a->data, (size_t)alen);
    }
    if (blen > 0) {
        memcpy(s->data + alen, b->data, (size_t)blen);
    }
    s->data[alen + blen] = '\0';
    return s;
}

int64_t lira_rt_str_len(const LiraStr *s) { return s ? s->len : 0; }

int8_t lira_rt_str_eq(const LiraStr *a, const LiraStr *b) {
    if (a == b) {
        return 1;
    }
    if (a == NULL || b == NULL || a->len != b->len) {
        return 0;
    }
    return memcmp(a->data, b->data, (size_t)a->len) == 0 ? 1 : 0;
}

int64_t lira_rt_str_cmp(const LiraStr *a, const LiraStr *b) {
    int64_t alen = a ? a->len : 0;
    int64_t blen = b ? b->len : 0;
    int64_t min = alen < blen ? alen : blen;
    int order = min > 0 ? memcmp(a->data, b->data, (size_t)min) : 0;
    if (order != 0) {
        return order < 0 ? -1 : 1;
    }
    if (alen == blen) {
        return 0;
    }
    return alen < blen ? -1 : 1;
}

LiraStr *lira_rt_int_to_str(int64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof(buf), "%" PRId64, v);
    return lira_rt_str_new(buf, n);
}

/* Matches the bytecode VM, which formats floats with Rust's `{}`: the shortest
 * decimal form that round-trips, with no forced ".0" on integral values, and
 * "inf" / "-inf" / "NaN" for the specials. */
static int lira_format_float(char *buf, size_t cap, double v) {
    if (isnan(v)) {
        return snprintf(buf, cap, "NaN");
    }
    if (isinf(v)) {
        return snprintf(buf, cap, v < 0 ? "-inf" : "inf");
    }
    for (int precision = 1; precision <= 17; precision++) {
        int n = snprintf(buf, cap, "%.*g", precision, v);
        if (n > 0 && (size_t)n < cap && strtod(buf, NULL) == v) {
            break;
        }
        if (precision == 17) {
            snprintf(buf, cap, "%.17g", v);
        }
    }
    return (int)strlen(buf);
}

LiraStr *lira_rt_float_to_str(double v) {
    char buf[64];
    int n = lira_format_float(buf, sizeof(buf), v);
    return lira_rt_str_new(buf, n);
}

LiraStr *lira_rt_bool_to_str(int8_t v) {
    return v ? lira_rt_str_new("true", 4) : lira_rt_str_new("false", 5);
}

/* ------------------------------------------------------------------ */
/* Printing                                                            */
/* ------------------------------------------------------------------ */

void lira_rt_print_str(const LiraStr *s) {
    if (s != NULL && s->len > 0) {
        fwrite(s->data, 1, (size_t)s->len, stdout);
    } else if (s == NULL) {
        fputs("null", stdout);
    }
}

void lira_rt_println_str(const LiraStr *s) {
    lira_rt_print_str(s);
    fputc('\n', stdout);
}

void lira_rt_print_int(int64_t v) { printf("%" PRId64, v); }
void lira_rt_println_int(int64_t v) { printf("%" PRId64 "\n", v); }

void lira_rt_print_float(double v) {
    char buf[64];
    lira_format_float(buf, sizeof(buf), v);
    fputs(buf, stdout);
}

void lira_rt_println_float(double v) {
    lira_rt_print_float(v);
    fputc('\n', stdout);
}

void lira_rt_print_bool(int8_t v) { fputs(v ? "true" : "false", stdout); }
void lira_rt_println_bool(int8_t v) { puts(v ? "true" : "false"); }

/* ------------------------------------------------------------------ */
/* Arrays                                                              */
/* ------------------------------------------------------------------ */

LiraArray *lira_rt_array_new(int64_t cap) {
    LiraArray *a = (LiraArray *)lira_rt_alloc((int64_t)sizeof(LiraArray), LIRA_KIND_ARRAY);
    a->len = 0;
    a->cap = cap > 0 ? cap : 0;
    a->data = a->cap > 0 ? (int64_t *)calloc((size_t)a->cap, sizeof(int64_t)) : NULL;
    if (a->cap > 0 && a->data == NULL) {
        lira_rt_panic("out of memory");
    }
    return a;
}

static void lira_array_reserve(LiraArray *a, int64_t needed) {
    if (needed <= a->cap) {
        return;
    }
    int64_t cap = a->cap > 0 ? a->cap * 2 : 8;
    while (cap < needed) {
        cap *= 2;
    }
    int64_t *data = (int64_t *)realloc(a->data, (size_t)cap * sizeof(int64_t));
    if (data == NULL) {
        lira_rt_panic("out of memory");
    }
    memset(data + a->cap, 0, (size_t)(cap - a->cap) * sizeof(int64_t));
    a->data = data;
    a->cap = cap;
}

void lira_rt_array_push(LiraArray *a, int64_t value) {
    if (a == NULL) {
        lira_rt_panic("push on null array");
    }
    lira_array_reserve(a, a->len + 1);
    a->data[a->len++] = value;
}

int64_t lira_rt_array_pop(LiraArray *a) {
    if (a == NULL || a->len == 0) {
        lira_rt_panic("pop from empty array");
    }
    return a->data[--a->len];
}

int64_t lira_rt_array_get(const LiraArray *a, int64_t index) {
    if (a == NULL) {
        lira_rt_panic("index into null array");
    }
    if (index < 0 || index >= a->len) {
        char buf[128];
        snprintf(buf, sizeof(buf), "index %" PRId64 " out of bounds for array of length %" PRId64,
                 index, a->len);
        lira_rt_panic(buf);
    }
    return a->data[index];
}

void lira_rt_array_set(LiraArray *a, int64_t index, int64_t value) {
    if (a == NULL) {
        lira_rt_panic("index into null array");
    }
    if (index < 0 || index >= a->len) {
        char buf[128];
        snprintf(buf, sizeof(buf), "index %" PRId64 " out of bounds for array of length %" PRId64,
                 index, a->len);
        lira_rt_panic(buf);
    }
    a->data[index] = value;
}

int64_t lira_rt_array_len(const LiraArray *a) { return a ? a->len : 0; }

/* ------------------------------------------------------------------ */
/* Arithmetic helpers                                                  */
/* ------------------------------------------------------------------ */

int64_t lira_rt_idiv(int64_t a, int64_t b) {
    if (b == 0) {
        lira_rt_panic("division by zero");
    }
    /* INT64_MIN / -1 traps on x86; the VM wraps, so match it. */
    if (b == -1) {
        return (int64_t)(0 - (uint64_t)a);
    }
    return a / b;
}

int64_t lira_rt_imod(int64_t a, int64_t b) {
    if (b == 0) {
        lira_rt_panic("modulo by zero");
    }
    if (b == -1) {
        return 0;
    }
    return a % b;
}

int64_t lira_rt_ipow(int64_t base, int64_t exp) {
    if (exp < 0) {
        return 0;
    }
    int64_t result = 1;
    while (exp > 0) {
        if (exp & 1) {
            result = (int64_t)((uint64_t)result * (uint64_t)base);
        }
        base = (int64_t)((uint64_t)base * (uint64_t)base);
        exp >>= 1;
    }
    return result;
}

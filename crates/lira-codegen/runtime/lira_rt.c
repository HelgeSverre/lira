#include "lira_rt.h"

#include <ctype.h>
#include <errno.h>
#include <inttypes.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void lira_rt_copy_ctx_abort_all(void);

/* The code generator computes field offsets from these constants, so a layout
 * change here has to be a deliberate, matched change in layout.rs. */
#define LIRA_STATIC_ASSERT(cond, name) typedef char lira_static_assert_##name[(cond) ? 1 : -1]

LIRA_STATIC_ASSERT(sizeof(LiraHeader) == LIRA_HEADER_SIZE, header_size);
LIRA_STATIC_ASSERT(offsetof(LiraStr, len) == LIRA_STR_LEN_OFFSET, str_len);
LIRA_STATIC_ASSERT(offsetof(LiraStr, data) == LIRA_STR_DATA_OFFSET, str_data);
LIRA_STATIC_ASSERT(offsetof(LiraArray, len) == LIRA_ARRAY_LEN_OFFSET, array_len);
LIRA_STATIC_ASSERT(offsetof(LiraArray, cap) == LIRA_ARRAY_CAP_OFFSET, array_cap);
LIRA_STATIC_ASSERT(offsetof(LiraArray, data) == LIRA_ARRAY_DATA_OFFSET, array_data);
_Static_assert(sizeof(LiraInterface) == 32, "interface size");
_Static_assert(offsetof(LiraInterface, payload) == 16, "interface payload offset");
_Static_assert(offsetof(LiraInterface, witness) == 24, "interface witness offset");

static int lira_rt_interface_string_valid(const LiraStr *value) {
    return value != NULL && value->hdr.kind == LIRA_KIND_STRING && value->len >= 0 &&
           (uint64_t)value->len <= (uint64_t)UINT32_MAX - sizeof(LiraStr);
}

static int lira_rt_interface_pair_equal(const LiraInterfaceMethod *left,
                                        const LiraInterfaceMethod *right) {
    if (!lira_rt_interface_string_valid(left->name) ||
        !lira_rt_interface_string_valid(left->signature) ||
        !lira_rt_interface_string_valid(right->name) ||
        !lira_rt_interface_string_valid(right->signature)) {
        return 0;
    }
    return left->name->len == right->name->len &&
           memcmp(left->name->data, right->name->data, (size_t)left->name->len) == 0 &&
           left->signature->len == right->signature->len &&
           memcmp(left->signature->data, right->signature->data,
                  (size_t)left->signature->len) == 0;
}

/* Metadata is bounded before any pairwise structural checks.  Rejecting
 * duplicate pairs also makes malformed target tables deterministic rather
 * than treating one entry as satisfying an arbitrary number of duplicates. */
static int lira_rt_interface_spec_valid(const LiraInterfaceSpec *spec) {
    if (spec == NULL || spec->method_count > LIRA_INTERFACE_MAX_METHODS ||
        (spec->method_count != 0 && spec->methods == NULL)) {
        return 0;
    }
    for (uint64_t index = 0; index < spec->method_count; index++) {
        const LiraInterfaceMethod *method = &spec->methods[index];
        if (!lira_rt_interface_string_valid(method->name) ||
            !lira_rt_interface_string_valid(method->signature)) {
            return 0;
        }
        for (uint64_t previous = 0; previous < index; previous++) {
            if (lira_rt_interface_pair_equal(method, &spec->methods[previous])) {
                return 0;
            }
        }
    }
    return 1;
}

static int lira_rt_interface_witness_valid(const LiraInterfaceWitness *witness) {
    if (witness == NULL || witness->payload_kind > LIRA_INTERFACE_PAYLOAD_I8 ||
        witness->method_count > LIRA_INTERFACE_MAX_METHODS ||
        !lira_rt_interface_spec_valid(witness->spec) ||
        witness->method_count != witness->spec->method_count) {
        return 0;
    }
    for (uint32_t index = 0; index < witness->method_count; index++) {
        if (witness->method_slots[index] == NULL) {
            return 0;
        }
    }
    return 1;
}

static int lira_rt_interface_valid(const LiraInterface *value) {
    return value != NULL && value->hdr.kind == LIRA_KIND_INTERFACE && value->hdr.rc > 0 &&
           lira_rt_interface_witness_valid(value->witness);
}

LiraInterface *lira_rt_interface_new(uint64_t payload,
                                      const LiraInterfaceWitness *witness) {
    if (!lira_rt_interface_witness_valid(witness)) {
        lira_rt_panic("invalid native interface witness");
        return NULL;
    }
    LiraInterface *value = (LiraInterface *)lira_rt_alloc(
        (int64_t)sizeof(LiraInterface), LIRA_KIND_INTERFACE);
    value->payload = payload;
    value->witness = witness;
    return value;
}

int8_t lira_rt_interface_is(const LiraInterface *value,
                            const LiraInterfaceSpec *target_spec) {
    if (!lira_rt_interface_valid(value) || !lira_rt_interface_spec_valid(target_spec) ||
        target_spec->method_count > value->witness->method_count) {
        return 0;
    }
    const LiraInterfaceSpec *actual = value->witness->spec;
    for (uint64_t target_index = 0; target_index < target_spec->method_count;
         target_index++) {
        int found = 0;
        for (uint64_t actual_index = 0; actual_index < actual->method_count; actual_index++) {
            if (lira_rt_interface_pair_equal(&target_spec->methods[target_index],
                                             &actual->methods[actual_index])) {
                found = 1;
                break;
            }
        }
        if (!found) {
            return 0;
        }
    }
    return 1;
}

const LiraInterfaceSpec *lira_rt_interface_spec(const LiraInterface *value) {
    return lira_rt_interface_valid(value) ? value->witness->spec : NULL;
}

uint64_t lira_rt_interface_payload(const LiraInterface *value,
                                   uint32_t expected_payload_kind) {
    if (expected_payload_kind > LIRA_INTERFACE_PAYLOAD_I8 ||
        !lira_rt_interface_valid(value) ||
        value->witness->payload_kind != expected_payload_kind) {
        lira_rt_panic("native interface payload kind mismatch");
        return 0;
    }
    return value->payload;
}

void (*lira_rt_interface_method_slot(const LiraInterface *value,
                                    uint32_t index))(void) {
    if (!lira_rt_interface_valid(value) || index >= value->witness->method_count) {
        lira_rt_panic("native interface method index is out of bounds");
        return NULL;
    }
    return value->witness->method_slots[index];
}

static int lira_rt_valid_string(const LiraStr *s) {
    return s != NULL && s->len >= 0 &&
           (uint64_t)s->len <= (uint64_t)UINT32_MAX - sizeof(LiraStr);
}

/* ------------------------------------------------------------------ */
/* Allocation                                                          */
/* ------------------------------------------------------------------ */

/* The collector's scheduler-facing reserve path may collect and report a
 * fatal language error.  Worker callbacks cannot do either, so all raw
 * callback storage goes through this header-prefixed, try-only allocator. */
typedef struct LiraRawMemHeader {
    _Alignas(max_align_t) uint64_t magic;
    size_t total_size;
    size_t payload_size;
} LiraRawMemHeader;

#define LIRA_RAW_MEM_MAGIC UINT64_C(0x4c6972615261774d)

static int lira_rt_mem_total_size(size_t payload, size_t *total) {
    if (payload == 0 || payload > SIZE_MAX - sizeof(LiraRawMemHeader)) {
        return 0;
    }
    *total = sizeof(LiraRawMemHeader) + payload;
    return 1;
}

static void lira_rt_raw_memory_fatal(void) {
    static const char message[] =
        "lira: runtime error: native raw allocation header is invalid\n";
    (void)fwrite(message, 1, sizeof(message) - 1, stderr);
    (void)fflush(stderr);
    _Exit(1);
}

static LiraRawMemHeader *lira_rt_mem_header(void *ptr) {
    if ((uintptr_t)ptr % _Alignof(max_align_t) != 0) {
        lira_rt_raw_memory_fatal();
    }
    LiraRawMemHeader *header = ((LiraRawMemHeader *)ptr) - 1;
    size_t expected = 0;
    if (header->magic != LIRA_RAW_MEM_MAGIC ||
        !lira_rt_mem_total_size(header->payload_size, &expected) ||
        header->total_size != expected) {
        lira_rt_raw_memory_fatal();
    }
    return header;
}

void *lira_rt_mem_try_alloc(size_t size, int zero) {
    size_t total = 0;
    if (!lira_rt_mem_total_size(size, &total)) {
        lira_gc_note_allocation_failure("native allocation is too large");
        return NULL;
    }
    if (!lira_gc_try_reserve_external(total)) {
        return NULL;
    }
    void *raw = zero ? calloc(1, total) : malloc(total);
    if (raw == NULL) {
        lira_gc_release_external_reservation(total);
        lira_gc_note_allocation_failure("out of memory");
        return NULL;
    }
    if (!lira_gc_try_commit_external_alloc(total)) {
        free(raw);
        lira_gc_release_external_reservation(total);
        return NULL;
    }
    LiraRawMemHeader *header = (LiraRawMemHeader *)raw;
    header->magic = LIRA_RAW_MEM_MAGIC;
    header->total_size = total;
    header->payload_size = size;
    return (void *)(header + 1);
}

void *lira_rt_mem_try_realloc(void *ptr, size_t size) {
    if (ptr == NULL) {
        return lira_rt_mem_try_alloc(size, 0);
    }
    if (size == 0) {
        lira_rt_mem_free(ptr);
        return NULL;
    }
    LiraRawMemHeader *old_header = lira_rt_mem_header(ptr);
    size_t old_size = old_header->payload_size;
    void *replacement = lira_rt_mem_try_alloc(size, 0);
    if (replacement == NULL) {
        return NULL;
    }
    size_t copied = old_size < size ? old_size : size;
    memcpy(replacement, ptr, copied);
    lira_rt_mem_free(ptr);
    return replacement;
}

void lira_rt_mem_free(void *ptr) {
    if (ptr == NULL) {
        return;
    }
    LiraRawMemHeader *header = lira_rt_mem_header(ptr);
    size_t total = header->total_size;
    header->magic = 0;
    free(header);
    lira_gc_account_external_free(total);
}

char *lira_rt_mem_try_strdup(const char *text) {
    if (text == NULL) {
        return NULL;
    }
    size_t length = strlen(text);
    if (length == SIZE_MAX) {
        lira_gc_note_allocation_failure("native allocation is too large");
        return NULL;
    }
    char *copy = (char *)lira_rt_mem_try_alloc(length + 1, 0);
    if (copy != NULL) {
        memcpy(copy, text, length + 1);
    }
    return copy;
}

void lira_rt_panic(const char *message) {
    /* A fiber failure abandons generated and runtime frames without C stack
     * unwinding. Restore process-global runtime guards before yielding to the
     * scheduler so subsequent JIT runs cannot inherit poisoned state. */
    lira_gc_abort_collection();
    lira_rt_copy_ctx_abort_all();
    fflush(stdout);
    fprintf(stderr, "lira: runtime error: %s\n", message);
    fflush(stderr);
    if (lira_rt_fail_in_fiber(message)) {
        return;
    }
    exit(1);
}

void *lira_rt_alloc(int64_t size, int32_t kind) {
    /* Collect before allocating: the fresh object has not yet been published
     * into a caller slot, so collecting after registration could reclaim it
     * during a compiler-generated call sequence whose return value is still in
     * a register. */
    lira_gc_maybe_collect();
    if (size < 0) {
        lira_rt_panic("native allocation size is negative");
    }
    if (size < (int64_t)sizeof(LiraHeader)) {
        size = (int64_t)sizeof(LiraHeader);
    }
    if ((uint64_t)size > (uint64_t)UINT32_MAX) {
        lira_rt_panic("native allocation is too large");
    }
    lira_gc_reserve_external((size_t)size);
    void *raw = calloc(1, (size_t)size);
    if (raw == NULL) {
        lira_gc_release_external_reservation((size_t)size);
        lira_rt_panic("out of memory");
    }
    LiraHeader *hdr = (LiraHeader *)raw;
    hdr->kind = (uint32_t)kind;
    hdr->flags = 0;
    hdr->rc = 1;
    if (!lira_gc_register(raw, (size_t)size, hdr->kind)) {
        /* The payload reservation still belongs to this allocation until
         * registration commits it. Never return a payload absent from the
         * collector's object table. */
        lira_gc_release_external_reservation((size_t)size);
        free(raw);
        lira_rt_panic(lira_gc_last_allocation_error());
        return NULL;
    }
    return raw;
}

void lira_rt_abort(const LiraStr *message) {
    if (message == NULL) {
        lira_rt_panic("aborted");
    }
    if (!lira_rt_valid_string(message)) {
        lira_rt_panic("abort message string is invalid");
    }
    lira_rt_panic(message->data);
}

/* ------------------------------------------------------------------ */
/* Struct value-copy contexts                                          */
/* ------------------------------------------------------------------ */

typedef struct LiraCopyEntry {
    LiraHeader hdr;
    const void *source;
    void *destination;
    struct LiraCopyEntry *next;
} LiraCopyEntry;

typedef struct LiraCopyContext {
    LiraHeader hdr;
    LiraCopyEntry *head;
    struct LiraCopyContext *previous;
} LiraCopyContext;

/* The native collector has an explicit root-slot API. Keep the active context
 * stack rooted while generated helpers allocate child objects. Copying an
 * `Any` field can open a nested context, so every context retains the previous
 * one until the nested copy finishes. The collector conservatively scans the
 * context payload and therefore follows both that link and every entry. */
static LiraCopyContext *g_copy_context;

static void lira_rt_copy_ctx_abort_all(void) {
    g_copy_context = NULL;
}

void *lira_rt_copy_ctx_new(void) {
    LiraCopyContext *ctx =
        (LiraCopyContext *)lira_rt_alloc((int64_t)sizeof(LiraCopyContext), LIRA_KIND_STRUCT);
    ctx->previous = g_copy_context;
    g_copy_context = ctx;
    /* Root-slot registration is idempotent while a runtime is alive. It must
     * happen for every context because JIT teardown unregisters all slots. */
    lira_gc_register_root_slot(&g_copy_context);
    return ctx;
}

void lira_rt_copy_ctx_free(void *raw_ctx) {
    if (raw_ctx == NULL || raw_ctx != (void *)g_copy_context) {
        lira_rt_panic("copy contexts must be released in stack order");
        return;
    }
    g_copy_context = g_copy_context->previous;
}

void *lira_rt_copy_ctx_lookup(void *raw_ctx, const void *source) {
    if (raw_ctx == NULL || source == NULL) {
        return NULL;
    }
    const LiraCopyContext *ctx = (const LiraCopyContext *)raw_ctx;
    for (const LiraCopyEntry *entry = ctx->head; entry != NULL; entry = entry->next) {
        if (entry->source == source) {
            return entry->destination;
        }
    }
    return NULL;
}

void lira_rt_copy_ctx_insert(void *raw_ctx, const void *source, void *destination) {
    if (raw_ctx == NULL || source == NULL || destination == NULL) {
        lira_rt_panic("invalid struct copy context entry");
    }
    LiraCopyContext *ctx = (LiraCopyContext *)raw_ctx;
    LiraCopyEntry *entry =
        (LiraCopyEntry *)lira_rt_alloc((int64_t)sizeof(LiraCopyEntry), LIRA_KIND_STRUCT);
    entry->source = source;
    entry->destination = destination;
    entry->next = ctx->head;
    ctx->head = entry;
}

/* ------------------------------------------------------------------ */
/* Strings                                                             */
/* ------------------------------------------------------------------ */

LiraStr *lira_rt_str_new(const char *bytes, int64_t len) {
    if (len < 0) {
        lira_rt_panic("string length is negative");
    }
    if ((uint64_t)len > (uint64_t)UINT32_MAX - sizeof(LiraStr)) {
        lira_rt_panic("string length is too large");
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

/* A null operand renders as "null", the same as printing one does, so
 * `"x: " + maybe` and `println(maybe)` agree. */
static const char LIRA_NULL_TEXT[] = "null";
#define LIRA_NULL_TEXT_LEN 4

LiraStr *lira_rt_str_concat(const LiraStr *a, const LiraStr *b) {
    const char *adata = a ? a->data : LIRA_NULL_TEXT;
    int64_t alen = a ? a->len : LIRA_NULL_TEXT_LEN;
    const char *bdata = b ? b->data : LIRA_NULL_TEXT;
    int64_t blen = b ? b->len : LIRA_NULL_TEXT_LEN;

    if (a != NULL && (alen < 0 || (uint64_t)alen > (uint64_t)UINT32_MAX - sizeof(LiraStr))) {
        lira_rt_panic("string length is invalid");
    }
    if (b != NULL && (blen < 0 || (uint64_t)blen > (uint64_t)UINT32_MAX - sizeof(LiraStr))) {
        lira_rt_panic("string length is invalid");
    }
    uint64_t total = (uint64_t)alen + (uint64_t)blen;
    if (total > (uint64_t)UINT32_MAX - sizeof(LiraStr)) {
        lira_rt_panic("concatenated string is too large");
    }

    LiraStr *s = (LiraStr *)lira_rt_alloc((int64_t)sizeof(LiraStr) + (int64_t)total,
                                          LIRA_KIND_STRING);
    s->len = (int64_t)total;
    if (alen > 0) {
        memcpy(s->data, adata, (size_t)alen);
    }
    if (blen > 0) {
        memcpy(s->data + alen, bdata, (size_t)blen);
    }
    s->data[total] = '\0';
    return s;
}

int64_t lira_rt_str_len(const LiraStr *s) {
    if (s != NULL && !lira_rt_valid_string(s)) {
        lira_rt_panic("string length is invalid");
    }
    return s ? s->len : 0;
}

int8_t lira_rt_str_eq(const LiraStr *a, const LiraStr *b) {
    if (a == b) {
        if (a != NULL && !lira_rt_valid_string(a)) {
            lira_rt_panic("string length is invalid");
        }
        return 1;
    }
    if (a == NULL || b == NULL) {
        return 0;
    }
    if (!lira_rt_valid_string(a) || !lira_rt_valid_string(b)) {
        lira_rt_panic("string length is invalid");
    }
    if (a->len != b->len) {
        return 0;
    }
    return memcmp(a->data, b->data, (size_t)a->len) == 0 ? 1 : 0;
}

int64_t lira_rt_str_cmp(const LiraStr *a, const LiraStr *b) {
    int64_t alen = a ? a->len : 0;
    int64_t blen = b ? b->len : 0;
    if ((a != NULL && !lira_rt_valid_string(a)) ||
        (b != NULL && !lira_rt_valid_string(b))) {
        lira_rt_panic("string length is invalid");
    }
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

int64_t lira_rt_str_to_int(const LiraStr *s) {
    if (s == NULL) {
        return 0;
    }
    if (s->len < 0 || (uint64_t)s->len > (uint64_t)UINT32_MAX - sizeof(LiraStr)) {
        lira_rt_panic("string length is invalid");
    }
    if (s->len == 0 || isspace((unsigned char)s->data[0])) {
        return 0;
    }
    errno = 0;
    char *end = NULL;
    long long value = strtoll(s->data, &end, 10);
    if (errno == ERANGE || end == s->data || end != s->data + s->len) {
        return 0;
    }
    return (int64_t)value;
}

/* Matches the bytecode VM, which formats floats with Rust's `{}`: the shortest
 * decimal form that round-trips, always in positional notation — Rust's Display
 * never switches to an exponent — with "inf" / "-inf" / "NaN" for the specials
 * and no forced ".0" on an integral value.
 *
 * `%g` alone will not do: it prints 10.0 as "1e+01". So the shortest
 * round-tripping digit string is found with `%e`, then re-rendered positionally.
 */
static int lira_format_float(char *buf, size_t cap, double v) {
    if (isnan(v)) {
        return snprintf(buf, cap, "NaN");
    }
    if (isinf(v)) {
        return snprintf(buf, cap, v < 0 ? "-inf" : "inf");
    }
    if (v == 0.0) {
        return snprintf(buf, cap, signbit(v) ? "-0" : "0");
    }

    char scientific[64];
    int significant = 17;
    for (int digits = 1; digits <= 17; digits++) {
        snprintf(scientific, sizeof(scientific), "%.*e", digits - 1, v);
        if (strtod(scientific, NULL) == v) {
            significant = digits;
            break;
        }
    }

    /* Split "-d.dddde+XX" into its sign, digit string and exponent. */
    const char *cursor = scientific;
    int negative = 0;
    if (*cursor == '-') {
        negative = 1;
        cursor++;
    }
    char digits[24];
    int count = 0;
    while (*cursor != 'e' && *cursor != 'E' && *cursor != '\0' && count < (int)sizeof(digits) - 1) {
        if (*cursor != '.') {
            digits[count++] = *cursor;
        }
        cursor++;
    }
    digits[count] = '\0';
    int exponent = (*cursor == 'e' || *cursor == 'E') ? atoi(cursor + 1) : 0;
    (void)significant;

    /* Trailing zeros carry no information in positional form. */
    while (count > 1 && digits[count - 1] == '0') {
        digits[--count] = '\0';
    }

    size_t written = 0;
    if (negative && written + 1 < cap) {
        buf[written++] = '-';
    }
    if (exponent >= count - 1) {
        /* An integer: every digit, then the remaining magnitude as zeros. */
        for (int i = 0; i < count && written + 1 < cap; i++) {
            buf[written++] = digits[i];
        }
        for (int i = 0; i < exponent - (count - 1) && written + 1 < cap; i++) {
            buf[written++] = '0';
        }
    } else if (exponent >= 0) {
        /* The point falls inside the digits. */
        for (int i = 0; i < count && written + 1 < cap; i++) {
            if (i == exponent + 1) {
                buf[written++] = '.';
            }
            if (written + 1 < cap) {
                buf[written++] = digits[i];
            }
        }
    } else {
        /* Smaller than one: "0." then the leading zeros the exponent implies. */
        if (written + 2 < cap) {
            buf[written++] = '0';
            buf[written++] = '.';
        }
        for (int i = 0; i < -exponent - 1 && written + 1 < cap; i++) {
            buf[written++] = '0';
        }
        for (int i = 0; i < count && written + 1 < cap; i++) {
            buf[written++] = digits[i];
        }
    }
    buf[written] = '\0';
    return (int)written;
}

LiraStr *lira_rt_float_to_str(double v) {
    char buf[LIRA_FLOAT_BUFFER];
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
    if (s != NULL && !lira_rt_valid_string(s)) {
        lira_rt_panic("string length is invalid");
    }
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
    char buf[LIRA_FLOAT_BUFFER];
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
    if (cap < 0 || (uint64_t)cap > SIZE_MAX / sizeof(int64_t)) {
        lira_rt_panic("array capacity is too large");
    }
    LiraArray *a = (LiraArray *)lira_rt_alloc((int64_t)sizeof(LiraArray), LIRA_KIND_ARRAY);
    a->len = 0;
    a->cap = cap > 0 ? cap : 0;
    size_t bytes = a->cap > 0 ? (size_t)a->cap * sizeof(int64_t) : 0;
    a->data = a->cap > 0 ? (int64_t *)lira_rt_mem_try_alloc(bytes, 1) : NULL;
    if (a->cap > 0 && a->data == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
    }
    return a;
}

static void lira_validate_array(const LiraArray *a) {
    if (a == NULL) {
        lira_rt_panic("operation on null array");
    }
    if (a->len < 0 || a->cap < 0 || a->len > a->cap ||
        (uint64_t)a->cap > SIZE_MAX / sizeof(int64_t) ||
        (a->cap > 0 && a->data == NULL)) {
        lira_rt_panic("array metadata is invalid");
    }
}

static void lira_array_reserve(LiraArray *a, int64_t needed) {
    lira_validate_array(a);
    if (needed <= a->cap) {
        return;
    }
    if (needed < 0 || (uint64_t)needed > SIZE_MAX / sizeof(int64_t)) {
        lira_rt_panic("array capacity is too large");
    }
    int64_t cap = a->cap > 0 ? a->cap : 8;
    while (cap < needed) {
        if (cap > INT64_MAX / 2) {
            lira_rt_panic("array capacity is too large");
        }
        cap *= 2;
    }
    if ((uint64_t)cap > SIZE_MAX / sizeof(int64_t)) {
        lira_rt_panic("array capacity is too large");
    }
    size_t new_bytes = (size_t)cap * sizeof(int64_t);
    int64_t *data = (int64_t *)lira_rt_mem_try_realloc(a->data, new_bytes);
    if (data == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
    }
    memset(data + a->cap, 0, (size_t)(cap - a->cap) * sizeof(int64_t));
    a->data = data;
    a->cap = cap;
}

void lira_rt_array_push(LiraArray *a, int64_t value) {
    lira_validate_array(a);
    if (a->len == INT64_MAX) {
        lira_rt_panic("array length overflow");
    }
    lira_array_reserve(a, a->len + 1);
    a->data[a->len++] = value;
}

int64_t lira_rt_array_pop(LiraArray *a) {
    lira_validate_array(a);
    if (a->len == 0) {
        lira_rt_panic("pop from empty array");
    }
    return a->data[--a->len];
}

int64_t lira_rt_array_get(const LiraArray *a, int64_t index) {
    lira_validate_array(a);
    if (index < 0 || index >= a->len) {
        char buf[128];
        snprintf(buf, sizeof(buf), "index %" PRId64 " out of bounds for array of length %" PRId64,
                 index, a->len);
        lira_rt_panic(buf);
    }
    return a->data[index];
}

void lira_rt_array_set(LiraArray *a, int64_t index, int64_t value) {
    lira_validate_array(a);
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
        lira_rt_panic("Negative exponent not supported for integers");
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

/* Keep float remainder in the native runtime ABI.  C's fmod follows the
 * language VM's IEEE behavior: finite values with a zero divisor produce NaN
 * (and set the floating-point status), rather than taking the integer
 * divide-by-zero panic path. */
double lira_rt_math_fmod(double left, double right) { return fmod(left, right); }

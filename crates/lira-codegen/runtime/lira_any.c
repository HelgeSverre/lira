/*
 * Dynamic values for the Cranelift backend.
 *
 * An Any is deliberately a real heap object rather than a tagged machine
 * word.  This keeps the ABI uniform on 32- and 64-bit targets and means that
 * scalar values never get mistaken for pointers.  Arrays and maps containing
 * dynamic values store pointers to these objects in their existing 8-byte
 * slots; the small allocation registry below validates those slots before
 * dereferencing them.
 */
#include "lira_rt.h"

#include <ctype.h>
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Keep native Any rendering observationally identical to Value::Display in
 * the bytecode VM. The bound includes the outer aggregate: a child reached
 * at depth eight is rendered as an ellipsis rather than descended into. */
#define LIRA_ANY_RENDER_LIMIT 8
#define LIRA_ANY_RENDER_MAX_BYTES (8u * 1024u * 1024u)
#define LIRA_ANY_OPTIONAL_SLOT_OFFSET 16
#define LIRA_ANY_CLASS_VTABLE_OFFSET 16
#define LIRA_ANY_STATIC_ASSERT(cond, name) \
    typedef char lira_any_static_assert_##name[(cond) ? 1 : -1]

LIRA_ANY_STATIC_ASSERT(sizeof(LiraAny) == 48, size);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraAny, tag) == 16, tag_offset);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraAny, payload) == 24, payload_offset);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraAny, type_data) == 32, type_data_offset);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraAny, type_len) == 40, type_len_offset);
LIRA_ANY_STATIC_ASSERT(sizeof(LiraInterface) == 32, interface_size);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraInterface, payload) == 16, interface_payload_offset);
LIRA_ANY_STATIC_ASSERT(offsetof(LiraInterface, witness) == 24, interface_witness_offset);

/* Any values are validated at every erased boundary. Open addressing gives
 * amortized O(1) lookup and deletion; the collector calls lira_rt_any_forget
 * before freeing an Any object so dead entries are removed from this bounded
 * table. */
#define LIRA_ANY_REGISTRY_INITIAL_CAP 256u
#define LIRA_ANY_REGISTRY_LOAD_NUM 2u
#define LIRA_ANY_REGISTRY_LOAD_DEN 3u
#define LIRA_ANY_REGISTRY_TOMBSTONE ((LiraAny *)(uintptr_t)1)

static LiraAny **lira_any_registry;
static size_t lira_any_registry_cap;
static size_t lira_any_registry_len;
static size_t lira_any_registry_tombstones;

/* Keep the load-factor arithmetic in the size_t domain.  The old expression
 * `(entries * DEN) >= (capacity * NUM)` overflowed before it could decide to
 * grow, which could leave a full table with an unbounded probe. */
static int lira_any_registry_overloaded(size_t capacity, size_t entries) {
    if (capacity == 0) {
        return 1;
    }
    /* For the fixed 2/3 threshold this is ceil(capacity * 2 / 3), expressed
     * without multiplication.  All capacities are powers of two, but this
     * remains correct for any positive capacity passed by a future caller. */
    size_t threshold = capacity - capacity / LIRA_ANY_REGISTRY_LOAD_DEN;
    return entries >= threshold;
}

/* The singleton is never released.  A non-null Any pointer is therefore also
 * a useful invariant for generated code: null is a tag, not an ABI sentinel. */
static LiraAny lira_any_null_value = {
    {LIRA_KIND_ANY, 1, INT64_MAX},
    LIRA_ANY_NULL,
    0,
    0,
    0
};

/* Untyped map constructors still have a concrete native key ABI (string) and
 * store boxed Any values. Keeping that layout explicit prevents a later cast
 * from treating the same slots as another primitive representation. */
static const char lira_any_dynamic_map_descriptor[] = "m(s,y)";

static uint64_t lira_any_ptr_payload(const void *value) {
    return (uint64_t)(uintptr_t)value;
}

static void *lira_any_payload_ptr(uint64_t payload) {
    return (void *)(uintptr_t)payload;
}

static size_t lira_any_hash(const LiraAny *value) {
    uintptr_t key = (uintptr_t)value;
    key ^= key >> 33;
    key *= (uintptr_t)UINT64_C(0xff51afd7ed558ccd);
    key ^= key >> 33;
    return (size_t)key;
}

static int lira_any_registry_rebuild(size_t requested) {
    size_t cap = requested < LIRA_ANY_REGISTRY_INITIAL_CAP
                     ? LIRA_ANY_REGISTRY_INITIAL_CAP
                     : requested;
    while (cap < LIRA_ANY_REGISTRY_INITIAL_CAP ||
           cap < requested || lira_any_registry_overloaded(cap, lira_any_registry_len)) {
        if (cap > SIZE_MAX / 2) {
            lira_rt_panic("Any registry capacity overflow");
            return 0;
        }
        cap *= 2;
    }
    if (cap > SIZE_MAX / sizeof(*lira_any_registry)) {
        lira_rt_panic("Any registry allocation overflow");
        return 0;
    }
    LiraAny **table = (LiraAny **)lira_rt_mem_try_alloc(cap * sizeof(*table), 1);
    if (table == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }
    if (lira_any_registry != NULL) {
        for (size_t i = 0; i < lira_any_registry_cap; i++) {
            LiraAny *value = lira_any_registry[i];
            if (value == NULL || value == LIRA_ANY_REGISTRY_TOMBSTONE) {
                continue;
            }
            size_t pos = lira_any_hash(value) & (cap - 1);
            while (table[pos] != NULL) {
                pos = (pos + 1) & (cap - 1);
            }
            table[pos] = value;
        }
    }
    lira_rt_mem_free(lira_any_registry);
    lira_any_registry = table;
    lira_any_registry_cap = cap;
    lira_any_registry_tombstones = 0;
    return 1;
}

static size_t lira_any_registry_slot(const LiraAny *value, int *found) {
    if (lira_any_registry_cap == 0) {
        *found = 0;
        return 0;
    }
    size_t pos = lira_any_hash(value) & (lira_any_registry_cap - 1);
    size_t tombstone = SIZE_MAX;
    for (size_t probes = 0; probes < lira_any_registry_cap; probes++) {
        LiraAny *entry = lira_any_registry[pos];
        if (entry == NULL) {
            *found = 0;
            return tombstone == SIZE_MAX ? pos : tombstone;
        }
        if (entry == value) {
            *found = 1;
            return pos;
        }
        if (entry == LIRA_ANY_REGISTRY_TOMBSTONE && tombstone == SIZE_MAX) {
            tombstone = pos;
        }
        pos = (pos + 1) & (lira_any_registry_cap - 1);
    }
    lira_rt_panic("Any registry probe overflow");
    *found = 0;
    return 0;
}

static int lira_any_registered(const LiraAny *value) {
    if (value == &lira_any_null_value) {
        return 1;
    }
    if (value == NULL || lira_any_registry_cap == 0) {
        return 0;
    }
    int found = 0;
    (void)lira_any_registry_slot(value, &found);
    return found;
}

static LiraAny *lira_any_checked(const LiraAny *value) {
    if (!lira_any_registered(value)) {
        lira_rt_panic("invalid null or Any value");
    }
    return (LiraAny *)value;
}

static int lira_any_register(LiraAny *value) {
    if (lira_any_registry_len > SIZE_MAX - lira_any_registry_tombstones - 1) {
        lira_rt_panic("Any registry length overflow");
        return 0;
    }
    size_t occupied_after_insert =
        lira_any_registry_len + lira_any_registry_tombstones + 1;
    if (lira_any_registry_cap == 0 ||
        lira_any_registry_overloaded(lira_any_registry_cap, occupied_after_insert)) {
        size_t requested = LIRA_ANY_REGISTRY_INITIAL_CAP;
        if (lira_any_registry_cap != 0) {
            if (lira_any_registry_cap > SIZE_MAX / 2) {
                lira_rt_panic("Any registry capacity overflow");
                return 0;
            }
            requested = lira_any_registry_cap * 2;
        }
        if (!lira_any_registry_rebuild(requested)) {
            return 0;
        }
    }
    int found = 0;
    size_t pos = lira_any_registry_slot(value, &found);
    if (found) {
        return 1;
    }
    if (lira_any_registry[pos] == LIRA_ANY_REGISTRY_TOMBSTONE) {
        lira_any_registry_tombstones--;
    }
    lira_any_registry[pos] = value;
    lira_any_registry_len++;
    return 1;
}

/* Called by the native collector before reclaiming an Any object. The table is
 * bookkeeping, not a program root. */
void lira_rt_any_forget(const void *pointer) {
    if (pointer == NULL || pointer == &lira_any_null_value || lira_any_registry_cap == 0) {
        return;
    }
    int found = 0;
    size_t pos = lira_any_registry_slot((const LiraAny *)pointer, &found);
    if (!found) {
        return;
    }
    lira_any_registry[pos] = LIRA_ANY_REGISTRY_TOMBSTONE;
    lira_any_registry_len--;
    lira_any_registry_tombstones++;
    if (lira_any_registry_tombstones > lira_any_registry_cap / 4) {
        (void)lira_any_registry_rebuild(lira_any_registry_cap);
    }
    /* A long-lived JIT process may create and collect many batches of Any
     * values. Once the live set is small, release the peak-sized index rather
     * than retaining that allocation forever. The hysteresis (one eighth)
     * keeps ordinary delete/insert traffic from repeatedly resizing. */
    while (lira_any_registry_cap > LIRA_ANY_REGISTRY_INITIAL_CAP &&
           lira_any_registry_len <= lira_any_registry_cap / 8) {
        if (!lira_any_registry_rebuild(lira_any_registry_cap / 2)) {
            break;
        }
    }
}

typedef struct LiraAnyTypeView {
    const char *data;
    size_t len;
} LiraAnyTypeView;

static char lira_any_desc_kind(LiraAnyTypeView view);
static void lira_any_require_descriptor_view(const LiraAny *value,
                                             LiraAnyTypeView expected);
static LiraAny *lira_any_box_object_typed_view(void *value, LiraAnyTypeView type);
static LiraAnyTypeView lira_any_type_view(const LiraAny *value);
static LiraAnyTypeView lira_any_desc_tuple_element(LiraAnyTypeView view, int64_t index);
static LiraAny *lira_any_copy_recursive(const LiraAny *value, void *copy_ctx);

static LiraAny *lira_any_new(int64_t tag, uint64_t payload, LiraAnyTypeView type) {
    LiraAny *value = (LiraAny *)lira_rt_alloc((int64_t)sizeof(LiraAny), LIRA_KIND_ANY);
    value->tag = tag;
    value->payload = payload;
    value->type_data = (uint64_t)(uintptr_t)type.data;
    value->type_len = (uint64_t)type.len;
    if (!lira_any_register(value)) {
        return &lira_any_null_value;
    }
    return value;
}

static LiraAny *lira_any_new_dynamic(int64_t tag, uint64_t payload) {
    LiraAnyTypeView type = {NULL, 0};
    return lira_any_new(tag, payload, type);
}

/* For an interface Any, type_data is the immutable canonical spec pointer and
 * type_len is its bounded method count. It is not a byte-string descriptor;
 * interface paths must use lira_any_interface_spec instead of the aggregate
 * descriptor parser. */
static const LiraInterfaceSpec *lira_any_interface_spec(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag != LIRA_ANY_INTERFACE || checked->payload == 0) {
        lira_rt_panic("expected interface Any value");
        return NULL;
    }
    LiraInterface *interface_value =
        (LiraInterface *)lira_any_payload_ptr(checked->payload);
    const LiraInterfaceSpec *spec = lira_rt_interface_spec(interface_value);
    if (spec == NULL || checked->type_data != (uint64_t)(uintptr_t)spec ||
        checked->type_len != spec->method_count) {
        lira_rt_panic("invalid Any interface descriptor");
        return NULL;
    }
    return spec;
}

static const char *lira_any_tag_name(int64_t tag) {
    switch (tag) {
        case LIRA_ANY_NULL:
            return "null";
        case LIRA_ANY_BOOL:
            return "bool";
        case LIRA_ANY_INT:
            return "int";
        case LIRA_ANY_FLOAT:
            return "float";
        case LIRA_ANY_STRING:
            return "string";
        case LIRA_ANY_ARRAY:
            return "array";
        case LIRA_ANY_OBJECT:
            return "object";
        case LIRA_ANY_REF:
            return "ref";
        case LIRA_ANY_FUNCTION:
            return "function";
        case LIRA_ANY_CHANNEL:
            return "channel";
        case LIRA_ANY_FIBER:
            return "fiber";
        case LIRA_ANY_INTERFACE:
            return "interface";
        default:
            return "invalid";
    }
}

static void lira_any_require_tag(const LiraAny *value, int64_t tag) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag != tag) {
        char message[128];
        snprintf(message, sizeof(message), "expected %s, got %s", lira_any_tag_name(tag),
                 lira_any_tag_name(checked->tag));
        lira_rt_panic(message);
    }
}

static LiraStr *lira_any_string_payload(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->payload == 0) {
        lira_rt_panic("Any string has a null payload");
    }
    return (LiraStr *)lira_any_payload_ptr(checked->payload);
}

static int lira_any_is_numeric(const LiraAny *value) {
    int64_t tag = lira_any_checked(value)->tag;
    return tag == LIRA_ANY_INT || tag == LIRA_ANY_FLOAT;
}

static double lira_any_as_float(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag == LIRA_ANY_FLOAT) {
        double result;
        memcpy(&result, &checked->payload, sizeof(result));
        return result;
    }
    return (double)(int64_t)checked->payload;
}

static int64_t lira_any_as_int(const LiraAny *value) {
    return (int64_t)lira_any_checked(value)->payload;
}

/* ------------------------------------------------------------------ */
/* Construction and checked access                                     */
/* ------------------------------------------------------------------ */

LiraAny *lira_rt_any_null(void) { return &lira_any_null_value; }

LiraAny *lira_rt_any_box_bool(int8_t value) {
    return lira_any_new_dynamic(LIRA_ANY_BOOL, value != 0 ? 1U : 0U);
}

LiraAny *lira_rt_any_box_int(int64_t value) {
    return lira_any_new_dynamic(LIRA_ANY_INT, (uint64_t)value);
}

LiraAny *lira_rt_any_box_float(double value) {
    uint64_t payload;
    memcpy(&payload, &value, sizeof(payload));
    return lira_any_new_dynamic(LIRA_ANY_FLOAT, payload);
}

LiraAny *lira_rt_any_box_string(LiraStr *value) {
    return value == NULL ? lira_rt_any_null()
                         : lira_any_new_dynamic(LIRA_ANY_STRING, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_array(LiraArray *value) {
    return value == NULL ? lira_rt_any_null()
                         : lira_any_new_dynamic(LIRA_ANY_ARRAY, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_map(void *value) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    LiraAnyTypeView type = {lira_any_dynamic_map_descriptor,
                            sizeof(lira_any_dynamic_map_descriptor) - 1};
    return lira_any_new(LIRA_ANY_OBJECT, lira_any_ptr_payload(value), type);
}

static LiraAnyTypeView lira_any_descriptor(const LiraStr *type) {
    if (type == NULL) {
        LiraAnyTypeView empty = {NULL, 0};
        return empty;
    }
    if (type->hdr.kind != LIRA_KIND_STRING || type->len < 0 ||
        (uint64_t)type->len > SIZE_MAX) {
        lira_rt_panic("invalid Any type descriptor");
    }
    LiraAnyTypeView view = {type->data, (size_t)type->len};
    if (view.len == 0) {
        lira_rt_panic("empty Any type descriptor");
    }
    return view;
}

/* A class instance stores the pointer to its function slots, not the start of
 * the vtable blob. The preceding slot is an immutable LiraStr* naming the
 * concrete class, which lets an object statically typed as an ancestor retain
 * its runtime identity after erasure. Non-class aggregates never take this
 * path, so their descriptors remain exactly the ones supplied by lowering. */
static LiraAnyTypeView lira_any_concrete_class_descriptor(void *value,
                                                          LiraAnyTypeView fallback) {
    if (value == NULL) {
        return fallback;
    }
    void *vtable = *(void **)((unsigned char *)value + LIRA_ANY_CLASS_VTABLE_OFFSET);
    if (vtable == NULL) {
        return fallback;
    }
    LiraStr *descriptor = (LiraStr *)((void **)vtable)[-1];
    if (descriptor == NULL) {
        return fallback;
    }
    LiraAnyTypeView concrete = lira_any_descriptor(descriptor);
    if (lira_any_desc_kind(concrete) != 'C') {
        lira_rt_panic("invalid concrete class descriptor");
    }
    return concrete;
}

static LiraAny *lira_any_box_object_typed_view(void *value, LiraAnyTypeView type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    if (lira_any_desc_kind(type) == 'C') {
        type = lira_any_concrete_class_descriptor(value, type);
    }
    return lira_any_new(LIRA_ANY_OBJECT, lira_any_ptr_payload(value), type);
}

LiraAny *lira_rt_any_box_array_typed(LiraArray *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    return lira_any_new(LIRA_ANY_ARRAY, lira_any_ptr_payload(value),
                        lira_any_descriptor(type));
}

LiraAny *lira_rt_any_box_map_typed(void *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    return lira_any_new(LIRA_ANY_OBJECT, lira_any_ptr_payload(value),
                        lira_any_descriptor(type));
}

/* These constructors intentionally never inspect the erased pointer.  A
 * closure's payload is a heap object, but a function representation can also
 * contain an executable address; a channel's tag is likewise known to the
 * caller.  Type-directed lowering supplies the tag without probing arbitrary
 * memory. */
LiraAny *lira_rt_any_box_object(void *value) {
    return value == NULL ? lira_rt_any_null()
                         : lira_any_new_dynamic(LIRA_ANY_OBJECT, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_object_typed(void *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    return lira_any_box_object_typed_view(value, lira_any_descriptor(type));
}

LiraAny *lira_rt_any_box_function(void *value) {
    return value == NULL ? lira_rt_any_null()
                         : lira_any_new_dynamic(LIRA_ANY_FUNCTION, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_function_typed(void *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    LiraAnyTypeView descriptor = lira_any_descriptor(type);
    if (lira_any_desc_kind(descriptor) != 'F') {
        lira_rt_panic("invalid Any function descriptor");
    }
    return lira_any_new(LIRA_ANY_FUNCTION, lira_any_ptr_payload(value), descriptor);
}

LiraAny *lira_rt_any_box_channel(void *value) {
    return value == NULL ? lira_rt_any_null()
                         : lira_any_new_dynamic(LIRA_ANY_CHANNEL, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_channel_typed(void *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    LiraAnyTypeView descriptor = lira_any_descriptor(type);
    if (lira_any_desc_kind(descriptor) != 'c') {
        lira_rt_panic("invalid Any channel descriptor");
    }
    return lira_any_new(LIRA_ANY_CHANNEL, lira_any_ptr_payload(value), descriptor);
}

LiraAny *lira_rt_any_box_fiber(int64_t value) {
    /* Fiber handles are scheduler ids, not pointers. Keep the payload scalar
     * so the conservative collector never treats an id as a heap edge. */
    return lira_any_new_dynamic(LIRA_ANY_FIBER, (uint64_t)value);
}

LiraAny *lira_rt_any_box_ref(void *value) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    /* Passing an Any through an erased call should not add a second box. */
    if (lira_any_registered((const LiraAny *)value)) {
        return (LiraAny *)value;
    }

    /* This is deliberately opaque: do not dereference an arbitrary pointer to
     * guess its header kind. Typed callers use the tag-preserving constructors
     * above; foreign callers get an opaque reference. */
    return lira_any_new_dynamic(LIRA_ANY_REF, lira_any_ptr_payload(value));
}

LiraAny *lira_rt_any_box_interface(LiraInterface *value) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    const LiraInterfaceSpec *spec = lira_rt_interface_spec(value);
    if (spec == NULL) {
        lira_rt_panic("invalid native interface value");
    }
    LiraAny *boxed = lira_any_new(LIRA_ANY_INTERFACE, lira_any_ptr_payload(value),
                                  (LiraAnyTypeView){(const char *)(uintptr_t)spec,
                                                    (size_t)spec->method_count});
    return boxed;
}

LiraAny *lira_rt_any_from_slot(int64_t raw) {
    if (raw == 0) {
        return lira_rt_any_null();
    }
    return lira_any_checked((const LiraAny *)(uintptr_t)raw);
}

int8_t lira_rt_any_unbox_bool(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_BOOL);
    return lira_any_checked(value)->payload != 0 ? 1 : 0;
}

int64_t lira_rt_any_unbox_int(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_INT);
    return lira_any_as_int(value);
}

double lira_rt_any_unbox_float(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_FLOAT);
    return lira_any_as_float(value);
}

LiraStr *lira_rt_any_unbox_string(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_STRING);
    return lira_any_string_payload(value);
}

LiraArray *lira_rt_any_unbox_array(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_ARRAY);
    return (LiraArray *)lira_rt_any_unbox_ref(value);
}

void *lira_rt_any_unbox_ref(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_STRING:
        case LIRA_ANY_ARRAY:
        case LIRA_ANY_OBJECT:
        case LIRA_ANY_REF:
        case LIRA_ANY_FUNCTION:
        case LIRA_ANY_CHANNEL:
        case LIRA_ANY_INTERFACE:
            if (checked->payload == 0) {
                lira_rt_panic("Any reference has a null payload");
            }
            return lira_any_payload_ptr(checked->payload);
        default:
            lira_rt_panic("expected reference value");
    }
    return NULL;
}

LiraInterface *lira_rt_any_unbox_interface(const LiraAny *value,
                                           const LiraInterfaceSpec *target_spec) {
    lira_any_require_tag(value, LIRA_ANY_INTERFACE);
    LiraAny *checked = lira_any_checked(value);
    const LiraInterfaceSpec *actual_spec = lira_any_interface_spec(value);
    /* A structural cast that changes the exposed interface needs a generated
     * witness adapter so method slots and ABI conversions follow the target
     * declaration. The lowering handles those finite, checker-approved source
     * specs before reaching this exact fallback. Returning a wider box merely
     * because its method set matches would make target slot indices unsound. */
    if (target_spec != NULL && actual_spec != target_spec) {
        lira_rt_panic("Any interface does not implement the requested interface");
    }
    return (LiraInterface *)(uintptr_t)checked->payload;
}

void *lira_rt_any_unbox_function(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_FUNCTION);
    return lira_rt_any_unbox_ref(value);
}

void *lira_rt_any_unbox_function_typed(const LiraAny *value, const LiraStr *type) {
    LiraAnyTypeView expected = lira_any_descriptor(type);
    if (lira_any_desc_kind(expected) != 'F') {
        lira_rt_panic("invalid Any function descriptor");
    }
    lira_any_require_tag(value, LIRA_ANY_FUNCTION);
    lira_any_require_descriptor_view(value, expected);
    return lira_rt_any_unbox_ref(value);
}

void *lira_rt_any_unbox_channel(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_CHANNEL);
    return lira_rt_any_unbox_ref(value);
}

void *lira_rt_any_unbox_channel_typed(const LiraAny *value, const LiraStr *type) {
    LiraAnyTypeView expected = lira_any_descriptor(type);
    if (lira_any_desc_kind(expected) != 'c') {
        lira_rt_panic("invalid Any channel descriptor");
    }
    lira_any_require_tag(value, LIRA_ANY_CHANNEL);
    lira_any_require_descriptor_view(value, expected);
    return lira_rt_any_unbox_ref(value);
}

/* ------------------------------------------------------------------ */
/* Erased aggregate descriptors                                        */
/* ------------------------------------------------------------------ */

#define LIRA_ANY_TYPE_DEPTH_LIMIT 64

/* Descriptors are emitted as a tiny prefix grammar:
 *
 *   b bool, i integer, f float, s string, y already-dynamic Any,
 *   o object/aggregate, S/C nominal struct/class, r opaque reference,
 *   x finite recursive nominal boundary,
 *   F(param,param;return) function, c(element) channel, I(name) interface,
 *   a(child) array, m(key,value) map, t(child,child,...) tuple, and
 *   S(name|field@offset/width:child,...) / C(...) nominal typed fields. A bare `o`
 *   remains the dynamic map/object representation.
 *
 * The parser returns spans into immutable compiler data. No descriptor is
 * allocated per element, which keeps boxing alias-preserving and bounded. */
static int lira_any_desc_parse_at(const char *data, size_t len, size_t pos,
                                  size_t depth, size_t *end) {
    if (data == NULL || pos >= len || depth > LIRA_ANY_TYPE_DEPTH_LIMIT) {
        return 0;
    }
    char kind = data[pos];
    switch (kind) {
        case 'b':
        case 'i':
        case 'f':
        case 's':
        case 'y':
        case 'r':
        case 'x':
        case 'c':
            /* A bare `c` is retained for legacy dynamic values. Typed
             * channels use `c(element)` so nested descriptors can be parsed
             * and compared without treating the element as optional text. */
            if (pos + 1 >= len || data[pos + 1] != '(') {
                *end = pos + 1;
                return 1;
            }
            {
                size_t child_end;
                if (!lira_any_desc_parse_at(data, len, pos + 2, depth + 1, &child_end) ||
                    child_end >= len || data[child_end] != ')') {
                    return 0;
                }
                *end = child_end + 1;
                return 1;
            }
        case 'F':
            /* Function descriptors are `F(param,param;return)`. The
             * semicolon separates the ordered parameter list from the
             * return descriptor, including for zero-argument functions. A
             * bare `F` remains accepted for old untyped dynamic wrappers. */
            if (pos + 1 >= len || data[pos + 1] != '(') {
                *end = pos + 1;
                return 1;
            }
            {
                size_t cursor = pos + 2;
                if (cursor >= len) {
                    return 0;
                }
                if (data[cursor] == ';') {
                    cursor++;
                } else {
                    for (;;) {
                        size_t param_end;
                        if (!lira_any_desc_parse_at(data, len, cursor, depth + 1, &param_end)) {
                            return 0;
                        }
                        cursor = param_end;
                        if (cursor >= len) {
                            return 0;
                        }
                        if (data[cursor] == ';') {
                            cursor++;
                            break;
                        }
                        if (data[cursor] != ',') {
                            return 0;
                        }
                        cursor++;
                    }
                }
                size_t return_end;
                if (!lira_any_desc_parse_at(data, len, cursor, depth + 1, &return_end) ||
                    return_end >= len || data[return_end] != ')') {
                    return 0;
                }
                *end = return_end + 1;
                return 1;
            }
        case 'I': {
            /* Interface slots are reference-semantic. The nominal spelling is
             * opaque here; generated witness adaptation validates membership
             * before a value enters typed storage. */
            if (pos + 3 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t cursor = pos + 2;
            size_t name_start = cursor;
            while (cursor < len && data[cursor] != ')') {
                cursor++;
            }
            if (cursor == name_start || cursor >= len) {
                return 0;
            }
            *end = cursor + 1;
            return 1;
        }
        case 'u':
            *end = pos + 1;
            return 1;
        case 'o':
        case 'S':
        case 'C':
            if (pos + 1 >= len || data[pos + 1] != '(') {
                *end = pos + 1;
                return 1;
            }
            {
                size_t cursor = pos + 2;
                if (kind == 'S' || kind == 'C') {
                    while (cursor < len && data[cursor] != '|') {
                        cursor++;
                    }
                    if (cursor >= len) {
                        return 0;
                    }
                    cursor++;
                }
                if (cursor < len && data[cursor] == ')') {
                    *end = cursor + 1;
                    return 1;
                }
                for (;;) {
                    size_t name_start = cursor;
                    while (cursor < len && data[cursor] != '@') {
                        /* Lira identifiers are UTF-8. Field names are opaque
                         * bytes here; `@` is the descriptor delimiter and is
                         * not legal in a source identifier. */
                        cursor++;
                    }
                    if (cursor == name_start || cursor >= len) {
                        return 0;
                    }
                    cursor++;
                    size_t offset_start = cursor;
                    while (cursor < len && isdigit((unsigned char)data[cursor])) {
                        cursor++;
                    }
                    if (cursor == offset_start || cursor >= len || data[cursor] != '/') {
                        return 0;
                    }
                    cursor++;
                    size_t width_start = cursor;
                    while (cursor < len && isdigit((unsigned char)data[cursor])) {
                        cursor++;
                    }
                    if (cursor == width_start || cursor >= len || data[cursor] != ':') {
                        return 0;
                    }
                    cursor++;
                    size_t child_end;
                    if (!lira_any_desc_parse_at(data, len, cursor, depth + 1, &child_end)) {
                        return 0;
                    }
                    cursor = child_end;
                    if (cursor >= len) {
                        return 0;
                    }
                    if (data[cursor] == ')') {
                        *end = cursor + 1;
                        return 1;
                    }
                    if (data[cursor] != ',') {
                        return 0;
                    }
                    cursor++;
                }
            }
        case 'e': {
            if (pos + 1 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t cursor = pos + 2;
            while (cursor < len && data[cursor] != ';' && data[cursor] != ')') {
                cursor++;
            }
            if (cursor >= len || data[cursor] == ')') {
                return data[cursor] == ')' ? (*end = cursor + 1, 1) : 0;
            }
            cursor++;
            if (cursor < len && data[cursor] == ')') {
                *end = cursor + 1;
                return 1;
            }
            for (;;) {
                size_t name_start = cursor;
                while (cursor < len && data[cursor] != '@' && data[cursor] != ')') {
                    cursor++;
                }
                if (cursor == name_start || cursor >= len || data[cursor] != '@') {
                    return 0;
                }
                cursor++;
                size_t tag_start = cursor;
                while (cursor < len && isdigit((unsigned char)data[cursor])) {
                    cursor++;
                }
                if (cursor == tag_start || cursor >= len || data[cursor] != '(') {
                    return 0;
                }
                cursor++;
                if (cursor < len && data[cursor] == ')') {
                    cursor++;
                } else {
                    for (;;) {
                        size_t child_end;
                        if (!lira_any_desc_parse_at(data, len, cursor, depth + 1, &child_end)) {
                            return 0;
                        }
                        cursor = child_end;
                        if (cursor >= len) {
                            return 0;
                        }
                        if (data[cursor] == ')') {
                            cursor++;
                            break;
                        }
                        if (data[cursor] != ',') {
                            return 0;
                        }
                        cursor++;
                    }
                }
                if (cursor >= len) {
                    return 0;
                }
                if (data[cursor] == ')') {
                    *end = cursor + 1;
                    return 1;
                }
                if (data[cursor] != ';') {
                    return 0;
                }
                cursor++;
            }
        }
        case 'R': {
            if (pos + 1 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t first_end;
            if (!lira_any_desc_parse_at(data, len, pos + 2, depth + 1, &first_end) ||
                first_end >= len || data[first_end] != ',') {
                return 0;
            }
            size_t second_end;
            if (!lira_any_desc_parse_at(data, len, first_end + 1, depth + 1, &second_end) ||
                second_end >= len || data[second_end] != ')') {
                return 0;
            }
            *end = second_end + 1;
            return 1;
        }
        case 'a':
        case 'q': {
            if (pos + 1 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t child_end;
            if (!lira_any_desc_parse_at(data, len, pos + 2, depth + 1, &child_end) ||
                child_end >= len || data[child_end] != ')') {
                return 0;
            }
            *end = child_end + 1;
            return 1;
        }
        case 'm': {
            if (pos + 1 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t key_end;
            if (!lira_any_desc_parse_at(data, len, pos + 2, depth + 1, &key_end) ||
                key_end >= len || data[key_end] != ',') {
                return 0;
            }
            size_t value_end;
            if (!lira_any_desc_parse_at(data, len, key_end + 1, depth + 1, &value_end) ||
                value_end >= len || data[value_end] != ')') {
                return 0;
            }
            *end = value_end + 1;
            return 1;
        }
        case 't': {
            if (pos + 1 >= len || data[pos + 1] != '(') {
                return 0;
            }
            size_t cursor = pos + 2;
            if (cursor < len && data[cursor] == ')') {
                *end = cursor + 1;
                return 1;
            }
            for (;;) {
                size_t child_end;
                if (!lira_any_desc_parse_at(data, len, cursor, depth + 1, &child_end)) {
                    return 0;
                }
                cursor = child_end;
                if (cursor >= len) {
                    return 0;
                }
                if (data[cursor] == ')') {
                    *end = cursor + 1;
                    return 1;
                }
                if (data[cursor] != ',') {
                    return 0;
                }
                cursor++;
            }
        }
        default:
            return 0;
    }
}

static char lira_any_desc_kind(LiraAnyTypeView view) {
    if (view.data == NULL || view.len == 0) {
        return 'y';
    }
    size_t end;
    if (!lira_any_desc_parse_at(view.data, view.len, 0, 0, &end) || end != view.len) {
        lira_rt_panic("invalid Any type descriptor");
    }
    return view.data[0];
}

static LiraAnyTypeView lira_any_desc_child(LiraAnyTypeView view) {
    if (view.data == NULL || view.len == 0) {
        LiraAnyTypeView dynamic = {NULL, 0};
        return dynamic;
    }
    char kind = lira_any_desc_kind(view);
    if (kind != 'a' && kind != 'q') {
        lira_rt_panic("Any aggregate descriptor has no element type");
    }
    size_t child_end;
    if (!lira_any_desc_parse_at(view.data, view.len, 2, 1, &child_end) ||
        child_end + 1 != view.len) {
        lira_rt_panic("invalid Any aggregate descriptor");
    }
    LiraAnyTypeView child = {view.data + 2, child_end - 2};
    return child;
}

static LiraAnyTypeView lira_any_desc_map_child(LiraAnyTypeView view, int key_side) {
    if (view.data == NULL || view.len < 6 || lira_any_desc_kind(view) != 'm') {
        lira_rt_panic("invalid Any map descriptor");
    }
    size_t key_end;
    if (!lira_any_desc_parse_at(view.data, view.len, 2, 1, &key_end) ||
        key_end >= view.len || view.data[key_end] != ',') {
        lira_rt_panic("invalid Any map descriptor");
    }
    size_t value_end;
    if (!lira_any_desc_parse_at(view.data, view.len, key_end + 1, 1, &value_end) ||
        value_end + 1 != view.len || view.data[value_end] != ')') {
        lira_rt_panic("invalid Any map descriptor");
    }
    size_t start = key_side ? 2 : key_end + 1;
    size_t end = key_side ? key_end : value_end;
    LiraAnyTypeView child = {view.data + start, end - start};
    return child;
}

typedef struct LiraAnyObjectField {
    const char *name;
    size_t name_len;
    size_t offset;
    size_t width;
    LiraAnyTypeView type;
} LiraAnyObjectField;

static int lira_any_parse_decimal(LiraAnyTypeView view, size_t *cursor, size_t *result) {
    size_t value = 0;
    size_t start = *cursor;
    while (*cursor < view.len && isdigit((unsigned char)view.data[*cursor])) {
        size_t digit = (size_t)(view.data[*cursor] - '0');
        if (value > (SIZE_MAX - digit) / 10) {
            return 0;
        }
        value = value * 10 + digit;
        (*cursor)++;
    }
    if (*cursor == start) {
        return 0;
    }
    *result = value;
    return 1;
}

static int lira_any_desc_object_field_at(LiraAnyTypeView view, size_t wanted,
                                         LiraAnyObjectField *field) {
    if (view.data == NULL || view.len < 3 ||
        (view.data[0] != 'o' && view.data[0] != 'S' && view.data[0] != 'C') ||
        view.data[1] != '(') {
        return 0;
    }
    size_t cursor = 2;
    if (view.data[0] == 'S' || view.data[0] == 'C') {
        while (cursor < view.len && view.data[cursor] != '|') {
            cursor++;
        }
        if (cursor >= view.len) {
            return 0;
        }
        cursor++;
    }
    size_t ordinal = 0;
    while (cursor < view.len && view.data[cursor] != ')') {
        size_t name_start = cursor;
        while (cursor < view.len && view.data[cursor] != '@') {
            cursor++;
        }
        if (cursor == name_start || cursor >= view.len) {
            return 0;
        }
        size_t name_len = cursor - name_start;
        cursor++;
        size_t offset;
        if (!lira_any_parse_decimal(view, &cursor, &offset) || cursor >= view.len ||
            view.data[cursor] != '/') {
            return 0;
        }
        cursor++;
        size_t width;
        if (!lira_any_parse_decimal(view, &cursor, &width) || width == 0 ||
            cursor >= view.len || view.data[cursor] != ':') {
            return 0;
        }
        cursor++;
        size_t child_end;
        if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
            return 0;
        }
        if (ordinal == wanted) {
            field->name = view.data + name_start;
            field->name_len = name_len;
            field->offset = offset;
            field->width = width;
            field->type.data = view.data + cursor;
            field->type.len = child_end - cursor;
            return 1;
        }
        ordinal++;
        cursor = child_end;
        if (cursor >= view.len) {
            return 0;
        }
        if (view.data[cursor] == ',') {
            cursor++;
        } else if (view.data[cursor] != ')') {
            return 0;
        }
    }
    return 0;
}

static int lira_any_desc_object_field(LiraAnyTypeView view, const LiraStr *key,
                                      LiraAnyObjectField *field) {
    if (key == NULL || key->len < 0 || (uint64_t)key->len > SIZE_MAX) {
        return 0;
    }
    for (size_t ordinal = 0;; ordinal++) {
        LiraAnyObjectField candidate;
        if (!lira_any_desc_object_field_at(view, ordinal, &candidate)) {
            return 0;
        }
        if (candidate.name_len == (size_t)key->len &&
            memcmp(candidate.name, key->data, candidate.name_len) == 0) {
            *field = candidate;
            return 1;
        }
    }
}

static size_t lira_any_desc_object_field_count(LiraAnyTypeView view) {
    if (view.data == NULL || view.len < 3 ||
        (view.data[0] != 'o' && view.data[0] != 'S' && view.data[0] != 'C') ||
        view.data[1] != '(') {
        return 0;
    }
    size_t count = 0;
    LiraAnyObjectField field;
    while (lira_any_desc_object_field_at(view, count, &field)) {
        count++;
    }
    return count;
}

/* Clone only value-struct payloads. The descriptor is the authority for both
 * the object kind and field layout; a raw pointer is never classified by
 * probing its header alone. The copy context publishes each destination
 * before descending, so recursive value graphs terminate and retain their
 * topology. */
static size_t lira_any_struct_payload_size(LiraAnyTypeView type) {
    size_t size = sizeof(LiraHeader);
    for (size_t ordinal = 0;; ordinal++) {
        LiraAnyObjectField field;
        if (!lira_any_desc_object_field_at(type, ordinal, &field)) {
            break;
        }
        if (field.offset < sizeof(LiraHeader)) {
            lira_rt_panic("Any struct field overlaps its header");
        }
        if (field.offset > SIZE_MAX - field.width) {
            lira_rt_panic("Any struct layout overflows");
        }
        size_t end = field.offset + field.width;
        if (end > size) {
            size = end;
        }
    }
    if (size < sizeof(LiraHeader)) {
        lira_rt_panic("Any struct layout is empty");
    }
    return size;
}

static void *lira_any_copy_struct(void *source, LiraAnyTypeView type, void *copy_ctx);
static void *lira_any_copy_tuple(void *source, LiraAnyTypeView type, void *copy_ctx);

static size_t lira_any_desc_tuple_count(LiraAnyTypeView view) {
    if (view.data == NULL || view.len < 3 || lira_any_desc_kind(view) != 't') {
        lira_rt_panic("invalid Any tuple descriptor");
    }
    size_t cursor = 2;
    size_t count = 0;
    while (cursor < view.len && view.data[cursor] != ')') {
        size_t child_end;
        if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
            lira_rt_panic("invalid Any tuple descriptor");
        }
        if (count == SIZE_MAX) {
            lira_rt_panic("Any tuple length overflow");
        }
        count++;
        cursor = child_end;
        if (cursor < view.len && view.data[cursor] == ',') {
            cursor++;
        }
    }
    if (cursor + 1 != view.len || view.data[cursor] != ')') {
        lira_rt_panic("invalid Any tuple descriptor");
    }
    return count;
}

/* Copy one typed slot according to its descriptor.  Reference-semantic
 * children are deliberately left as raw slots; only value structs, tuples,
 * and dynamic Any wrappers recurse through the shared memoizing context. */
static uint64_t lira_any_copy_slot(uint64_t raw, LiraAnyTypeView type, void *copy_ctx) {
    char kind = lira_any_desc_kind(type);
    if (kind == 'q') {
        if (raw == 0) {
            return 0;
        }
        return lira_any_copy_slot(raw, lira_any_desc_child(type), copy_ctx);
    }
    switch (kind) {
        case 'S':
            if (raw == 0) {
                return 0;
            }
            return (uint64_t)(uintptr_t)lira_any_copy_struct(
                (void *)(uintptr_t)raw, type, copy_ctx);
        case 't':
            if (raw == 0) {
                return 0;
            }
            return (uint64_t)(uintptr_t)lira_any_copy_tuple(
                (void *)(uintptr_t)raw, type, copy_ctx);
        case 'y':
            if (raw == 0) {
                return 0;
            }
            return (uint64_t)(uintptr_t)lira_any_copy_recursive(
                (const LiraAny *)(uintptr_t)raw, copy_ctx);
        default:
            return raw;
    }
}

static void *lira_any_copy_struct(void *source, LiraAnyTypeView type, void *copy_ctx) {
    if (source == NULL) {
        return NULL;
    }
    void *existing = lira_rt_copy_ctx_lookup(copy_ctx, source);
    if (existing != NULL) {
        return existing;
    }
    LiraHeader *source_header = (LiraHeader *)source;
    if (source_header->kind != LIRA_KIND_STRUCT || source_header->rc <= 0) {
        lira_rt_panic("invalid Any struct payload");
    }
    size_t size = lira_any_struct_payload_size(type);
    if (size > INT64_MAX) {
        lira_rt_panic("Any struct payload is too large");
    }
    void *copy = lira_rt_alloc((int64_t)size, LIRA_KIND_STRUCT);
    memcpy((unsigned char *)copy + sizeof(LiraHeader),
           (const unsigned char *)source + sizeof(LiraHeader), size - sizeof(LiraHeader));
    lira_rt_copy_ctx_insert(copy_ctx, source, copy);

    for (size_t ordinal = 0;; ordinal++) {
        LiraAnyObjectField field;
        if (!lira_any_desc_object_field_at(type, ordinal, &field)) {
            break;
        }
        if (field.offset < sizeof(LiraHeader) ||
            field.offset > SIZE_MAX - field.width ||
            field.offset + field.width > size) {
            lira_rt_panic("Any struct field lies outside its payload");
        }
        LiraAnyTypeView semantic_type = field.type;
        char kind = lira_any_desc_kind(semantic_type);
        while (kind == 'q') {
            semantic_type = lira_any_desc_child(semantic_type);
            kind = lira_any_desc_kind(semantic_type);
        }
        if (kind != 'S' && kind != 't' && kind != 'y') {
            continue;
        }
        if (field.width != sizeof(uint64_t)) {
            lira_rt_panic("Any struct reference field has invalid width");
        }
        uint64_t raw = 0;
        memcpy(&raw, (const unsigned char *)source + field.offset, sizeof(raw));
        uint64_t nested = lira_any_copy_slot(raw, field.type, copy_ctx);
        memcpy((unsigned char *)copy + field.offset, &nested, sizeof(nested));
    }
    return copy;
}

static void *lira_any_copy_tuple(void *source, LiraAnyTypeView type, void *copy_ctx) {
    if (source == NULL) {
        return NULL;
    }
    void *existing = lira_rt_copy_ctx_lookup(copy_ctx, source);
    if (existing != NULL) {
        return existing;
    }
    LiraHeader *source_header = (LiraHeader *)source;
    if (source_header->kind != LIRA_KIND_ARRAY || source_header->rc <= 0) {
        lira_rt_panic("invalid Any tuple payload");
    }
    size_t count = lira_any_desc_tuple_count(type);
    if (count > (size_t)INT64_MAX) {
        lira_rt_panic("Any tuple length exceeds native limit");
    }
    LiraArray *source_array = (LiraArray *)source;
    int64_t source_len = lira_rt_array_len(source_array);
    if (source_len < 0 || (uint64_t)source_len != (uint64_t)count) {
        lira_rt_panic("Any tuple payload length does not match descriptor");
    }
    LiraArray *copy = lira_rt_array_new((int64_t)count);
    lira_rt_copy_ctx_insert(copy_ctx, source, copy);
    for (size_t index = 0; index < count; index++) {
        LiraAnyTypeView child = lira_any_desc_tuple_element(type, (int64_t)index);
        uint64_t raw = (uint64_t)lira_rt_array_get(source_array, (int64_t)index);
        uint64_t nested = lira_any_copy_slot(raw, child, copy_ctx);
        lira_rt_array_push(copy, (int64_t)nested);
    }
    return copy;
}

static LiraAnyTypeView lira_any_desc_tuple_element(LiraAnyTypeView view, int64_t index) {
    if (index < 0 || view.data == NULL || view.len < 3 || lira_any_desc_kind(view) != 't') {
        lira_rt_panic("invalid Any tuple index");
    }
    size_t cursor = 2;
    int64_t current = 0;
    while (cursor < view.len && view.data[cursor] != ')') {
        size_t child_end;
        if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
            lira_rt_panic("invalid Any tuple descriptor");
        }
        if (current == index) {
            LiraAnyTypeView child = {view.data + cursor, child_end - cursor};
            return child;
        }
        current++;
        cursor = child_end;
        if (cursor < view.len && view.data[cursor] == ',') {
            cursor++;
        }
    }
    lira_rt_panic("Any tuple index out of bounds");
    LiraAnyTypeView missing = {NULL, 0};
    return missing;
}

static LiraAnyTypeView lira_any_type_view(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->type_len > SIZE_MAX) {
        lira_rt_panic("Any type descriptor length overflow");
    }
    LiraAnyTypeView type = {(const char *)(uintptr_t)checked->type_data,
                            (size_t)checked->type_len};
    return type;
}

static int lira_any_is_typed_object(LiraAnyTypeView type) {
    return type.data != NULL && type.len > 1 &&
           (type.data[0] == 'o' || type.data[0] == 'S' || type.data[0] == 'C') &&
           type.data[1] == '(';
}

static int lira_any_is_enum_descriptor(LiraAnyTypeView type) {
    return type.data != NULL && type.len > 1 && type.data[0] == 'e' && type.data[1] == '(';
}

static int lira_any_is_result_descriptor(LiraAnyTypeView type) {
    return type.data != NULL && type.len > 1 && type.data[0] == 'R' && type.data[1] == '(';
}

static void lira_any_require_descriptor_view(const LiraAny *value,
                                             LiraAnyTypeView expected) {
    LiraAnyTypeView actual = lira_any_type_view(value);
    if (expected.data == NULL || actual.data == NULL || expected.len != actual.len ||
        memcmp(expected.data, actual.data, expected.len) != 0) {
        lira_rt_panic("Any aggregate type does not match the requested type");
    }
}

typedef struct LiraAnyEnumVariant {
    const char *name;
    size_t name_len;
    int64_t tag;
    size_t payload_start;
    size_t payload_count;
} LiraAnyEnumVariant;

static int lira_any_desc_enum_name(LiraAnyTypeView view, const char **name, size_t *name_len) {
    if (!lira_any_is_enum_descriptor(view)) {
        return 0;
    }
    size_t cursor = 2;
    while (cursor < view.len && view.data[cursor] != ';' && view.data[cursor] != ')') {
        cursor++;
    }
    if (cursor == 2 || cursor >= view.len || view.data[cursor] != ';') {
        return 0;
    }
    *name = view.data + 2;
    *name_len = cursor - 2;
    return 1;
}

static int lira_any_desc_enum_variant_at(LiraAnyTypeView view, size_t wanted,
                                         LiraAnyEnumVariant *result) {
    if (!lira_any_is_enum_descriptor(view)) {
        return 0;
    }
    size_t cursor = 2;
    while (cursor < view.len && view.data[cursor] != ';' && view.data[cursor] != ')') {
        cursor++;
    }
    if (cursor >= view.len || view.data[cursor] != ';') {
        return 0;
    }
    cursor++;
    if (cursor < view.len && view.data[cursor] == ')') {
        return 0;
    }
    size_t ordinal = 0;
    while (cursor < view.len && view.data[cursor] != ')') {
        size_t name_start = cursor;
        while (cursor < view.len && view.data[cursor] != '@') {
            cursor++;
        }
        if (cursor == name_start || cursor >= view.len) {
            return 0;
        }
        size_t name_len = cursor - name_start;
        cursor++;
        size_t tag_start = cursor;
        size_t tag_value = 0;
        while (cursor < view.len && isdigit((unsigned char)view.data[cursor])) {
            size_t digit = (size_t)(view.data[cursor] - '0');
            if (tag_value > (SIZE_MAX - digit) / 10) {
                return 0;
            }
            tag_value = tag_value * 10 + digit;
            cursor++;
        }
        if (cursor == tag_start || cursor >= view.len || view.data[cursor] != '(') {
            return 0;
        }
        cursor++;
        size_t payload_start = cursor;
        size_t payload_count = 0;
        if (cursor < view.len && view.data[cursor] == ')') {
            cursor++;
        } else {
            for (;;) {
                size_t child_end;
                if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
                    return 0;
                }
                payload_count++;
                cursor = child_end;
                if (cursor >= view.len) {
                    return 0;
                }
                if (view.data[cursor] == ')') {
                    cursor++;
                    break;
                }
                if (view.data[cursor] != ',') {
                    return 0;
                }
                cursor++;
            }
        }
        if (ordinal == wanted) {
            result->name = view.data + name_start;
            result->name_len = name_len;
            result->tag = (int64_t)tag_value;
            result->payload_start = payload_start;
            result->payload_count = payload_count;
            return 1;
        }
        ordinal++;
        if (cursor >= view.len) {
            return 0;
        }
        if (view.data[cursor] == ';') {
            cursor++;
        } else if (view.data[cursor] != ')') {
            return 0;
        }
    }
    return 0;
}

static int lira_any_desc_enum_variant_tag(LiraAnyTypeView view, int64_t tag,
                                          LiraAnyEnumVariant *result) {
    for (size_t ordinal = 0;; ordinal++) {
        LiraAnyEnumVariant candidate;
        if (!lira_any_desc_enum_variant_at(view, ordinal, &candidate)) {
            return 0;
        }
        if (candidate.tag == tag) {
            *result = candidate;
            return 1;
        }
    }
}

static LiraAnyTypeView lira_any_desc_enum_payload_at(LiraAnyTypeView view,
                                                      const LiraAnyEnumVariant *variant,
                                                      size_t index) {
    if (index >= variant->payload_count) {
        lira_rt_panic("Any enum payload index out of bounds");
    }
    size_t cursor = variant->payload_start;
    for (size_t current = 0; current < index; current++) {
        size_t child_end;
        if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
            lira_rt_panic("invalid Any enum descriptor");
        }
        cursor = child_end + 1;
    }
    size_t child_end;
    if (!lira_any_desc_parse_at(view.data, view.len, cursor, 1, &child_end)) {
        lira_rt_panic("invalid Any enum descriptor");
    }
    LiraAnyTypeView child = {view.data + cursor, child_end - cursor};
    return child;
}

static LiraAnyTypeView lira_any_desc_result_child(LiraAnyTypeView view, int error_side) {
    if (!lira_any_is_result_descriptor(view)) {
        lira_rt_panic("invalid Any result descriptor");
    }
    size_t first_end;
    if (!lira_any_desc_parse_at(view.data, view.len, 2, 1, &first_end) ||
        first_end >= view.len || view.data[first_end] != ',') {
        lira_rt_panic("invalid Any result descriptor");
    }
    size_t second_end;
    if (!lira_any_desc_parse_at(view.data, view.len, first_end + 1, 1, &second_end) ||
        second_end + 1 != view.len) {
        lira_rt_panic("invalid Any result descriptor");
    }
    size_t start = error_side ? first_end + 1 : 2;
    size_t end = error_side ? second_end : first_end;
    LiraAnyTypeView child = {view.data + start, end - start};
    return child;
}

static void *lira_any_unbox_map_view(const LiraAny *value,
                                     LiraAnyTypeView expected) {
    size_t expected_end;
    if (lira_any_desc_kind(expected) != 'm' ||
        !lira_any_desc_parse_at(expected.data, expected.len, 0, 1, &expected_end) ||
        expected_end != expected.len) {
        lira_rt_panic("invalid Any map descriptor");
    }
    lira_any_require_tag(value, LIRA_ANY_OBJECT);
    lira_any_require_descriptor_view(value, expected);
    return lira_rt_any_unbox_ref(value);
}

void *lira_rt_any_unbox_map(const LiraAny *value, const LiraStr *type) {
    return lira_any_unbox_map_view(value, lira_any_descriptor(type));
}

void *lira_rt_any_unbox_object_typed(const LiraAny *value, const LiraStr *type) {
    LiraAnyTypeView expected = lira_any_descriptor(type);
    LiraAny *checked = lira_any_checked(value);
    /* Arrays and tuples use the same two-pointer typed-aggregate ABI as
     * objects. Keeping the validation here lets native lowering reuse the
     * existing registration without ever treating an arbitrary array pointer
     * as a requested element representation. */
    if (checked->tag == LIRA_ANY_ARRAY &&
        (lira_any_desc_kind(expected) == 'a' || lira_any_desc_kind(expected) == 't')) {
        lira_any_require_descriptor_view(value, expected);
        return lira_rt_any_unbox_ref(value);
    }
    lira_any_require_tag(value, LIRA_ANY_OBJECT);
    lira_any_require_descriptor_view(value, expected);
    return lira_rt_any_unbox_ref(value);
}

static LiraAnyTypeView lira_any_aggregate_element(const LiraAny *value, int64_t index) {
    LiraAnyTypeView type = lira_any_type_view(value);
    char kind = lira_any_desc_kind(type);
    if (kind == 't') {
        return lira_any_desc_tuple_element(type, index);
    }
    if (kind == 'm') {
        return lira_any_desc_map_child(type, 0);
    }
    return lira_any_desc_child(type);
}

static LiraAny *lira_any_box_array_view(LiraArray *value, LiraAnyTypeView type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    return lira_any_new(LIRA_ANY_ARRAY, lira_any_ptr_payload(value), type);
}

static LiraAny *lira_any_box_map_view(void *value, LiraAnyTypeView type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    return lira_any_new(LIRA_ANY_OBJECT, lira_any_ptr_payload(value), type);
}

static LiraAny *lira_any_from_typed_slot(int64_t raw, LiraAnyTypeView type) {
    char kind = lira_any_desc_kind(type);
    if (raw == 0 && (kind == 'y' || kind == 's' || kind == 'a' || kind == 'm' ||
                     kind == 't' || kind == 'o' || kind == 'S' || kind == 'C' || kind == 'r' ||
                     kind == 'x' || kind == 'F' || kind == 'c' || kind == 'I' ||
                     kind == 'q' || kind == 'e' || kind == 'R')) {
        return lira_rt_any_null();
    }
    switch (kind) {
        case 'y':
            return lira_rt_any_from_slot(raw);
        case 'b':
            return lira_rt_any_box_bool(raw != 0 ? 1 : 0);
        case 'i':
            return lira_rt_any_box_int(raw);
        case 'f': {
            double value;
            uint64_t bits = (uint64_t)raw;
            memcpy(&value, &bits, sizeof(value));
            return lira_rt_any_box_float(value);
        }
        case 's':
            return lira_rt_any_box_string((LiraStr *)(uintptr_t)raw);
        case 'a':
            /* The erased element is itself an aggregate. Keep the complete
             * descriptor (`a(...)`) on the nested wrapper so a later index
             * operation can recover its child type. */
            return lira_any_box_array_view((LiraArray *)(uintptr_t)raw, type);
        case 'm':
            return lira_any_box_map_view((void *)(uintptr_t)raw, type);
        case 't':
            return lira_any_box_array_view((LiraArray *)(uintptr_t)raw, type);
        case 'q': {
            LiraAnyTypeView child = lira_any_desc_child(type);
            char child_kind = lira_any_desc_kind(child);
            if (child_kind == 'b' || child_kind == 'i' || child_kind == 'u' || child_kind == 'f') {
                LiraHeader *optional = (LiraHeader *)(uintptr_t)raw;
                if (optional->kind != LIRA_KIND_STRUCT) {
                    lira_rt_panic("invalid boxed optional payload");
                }
                if (optional->rc == 0) {
                    lira_rt_panic("invalid boxed optional header");
                }
                int64_t payload;
                memcpy(&payload, (const unsigned char *)optional + 16, sizeof(payload));
                return lira_any_from_typed_slot(payload, child);
            }
            return lira_any_from_typed_slot(raw, child);
        }
        case 'o':
        case 'S':
        case 'C':
            if (kind == 'C') {
                return lira_any_box_object_typed_view((void *)(uintptr_t)raw, type);
            }
            if (lira_any_is_typed_object(type)) {
                return lira_any_new(LIRA_ANY_OBJECT, (uint64_t)(uintptr_t)raw, type);
            }
            return lira_rt_any_box_object((void *)(uintptr_t)raw);
        case 'e':
        case 'R':
            /* Enum and Result payloads are tagged objects, not maps. Keep
             * their complete descriptor so dynamic reflection can validate
             * the discriminant and decode payload slots without guessing a
             * header or interpreting the object as a map. */
            return lira_any_new(LIRA_ANY_OBJECT, (uint64_t)(uintptr_t)raw, type);
        case 'u':
            return lira_rt_any_box_int(raw);
        case 'F':
            if (type.data != NULL && type.len > 1 && type.data[1] == '(') {
                return lira_any_new(LIRA_ANY_FUNCTION, (uint64_t)(uintptr_t)raw, type);
            }
            return lira_rt_any_box_function((void *)(uintptr_t)raw);
        case 'c':
            if (type.data != NULL && type.len > 1 && type.data[1] == '(') {
                return lira_any_new(LIRA_ANY_CHANNEL, (uint64_t)(uintptr_t)raw, type);
            }
            return lira_rt_any_box_channel((void *)(uintptr_t)raw);
        case 'I':
            return lira_rt_any_box_interface((LiraInterface *)(uintptr_t)raw);
        case 'r':
            return lira_rt_any_box_ref((void *)(uintptr_t)raw);
        case 'x':
            return lira_any_new(LIRA_ANY_REF, (uint64_t)raw, type);
        default:
            lira_rt_panic("invalid Any element descriptor");
    }
    return lira_rt_any_null();
}

/* Copy a dynamic value while sharing the same memo table as every nested
 * struct and tuple. Publishing the wrapper before descending is necessary for
 * a value struct that reaches itself through an erased `y` field. */
static LiraAny *lira_any_copy_recursive(const LiraAny *value, void *copy_ctx) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag == LIRA_ANY_NULL) {
        return lira_rt_any_null();
    }
    if (checked->tag == LIRA_ANY_INTERFACE) {
        (void)lira_any_interface_spec(checked);
        return lira_rt_any_box_interface(
            (LiraInterface *)(uintptr_t)checked->payload);
    }
    LiraAnyTypeView type = lira_any_type_view(checked);
    char kind = lira_any_desc_kind(type);
    int copy_struct = checked->tag == LIRA_ANY_OBJECT && kind == 'S';
    int copy_tuple = checked->tag == LIRA_ANY_ARRAY && kind == 't';
    if (!copy_struct && !copy_tuple) {
        return lira_any_new(checked->tag, checked->payload, type);
    }
    void *existing = lira_rt_copy_ctx_lookup(copy_ctx, checked);
    if (existing != NULL) {
        return (LiraAny *)existing;
    }
    LiraAny *copy = lira_any_new(checked->tag, 0, type);
    lira_rt_copy_ctx_insert(copy_ctx, checked, copy);
    if (copy_struct) {
        copy->payload = (uint64_t)(uintptr_t)lira_any_copy_struct(
            (void *)(uintptr_t)checked->payload, type, copy_ctx);
    } else {
        copy->payload = (uint64_t)(uintptr_t)lira_any_copy_tuple(
            (void *)(uintptr_t)checked->payload, type, copy_ctx);
    }
    return copy;
}

LiraAny *lira_rt_any_copy(const LiraAny *value) {
    void *copy_ctx = lira_rt_copy_ctx_new();
    LiraAny *copy = lira_any_copy_recursive(value, copy_ctx);
    lira_rt_copy_ctx_free(copy_ctx);
    return copy;
}

LiraAny *lira_rt_any_box_optional(void *value, const LiraStr *type) {
    if (value == NULL) {
        return lira_rt_any_null();
    }
    LiraAnyTypeView descriptor = lira_any_descriptor(type);
    if (lira_any_desc_kind(descriptor) != 'q') {
        lira_rt_panic("invalid Any optional descriptor");
    }
    LiraAnyTypeView child = lira_any_desc_child(descriptor);
    char child_kind = lira_any_desc_kind(child);
    int64_t raw = (int64_t)(uintptr_t)value;
    if (child_kind == 'b' || child_kind == 'i' || child_kind == 'u' || child_kind == 'f') {
        LiraHeader *optional = (LiraHeader *)value;
        if (optional->kind != LIRA_KIND_STRUCT || optional->rc <= 0) {
            lira_rt_panic("invalid boxed optional value");
        }
        memcpy(&raw, (const unsigned char *)optional + LIRA_ANY_OPTIONAL_SLOT_OFFSET,
               sizeof(raw));
    }
    return lira_any_from_typed_slot(raw, child);
}

static int64_t lira_any_enum_tag(const LiraAny *value) {
    void *object = lira_rt_any_unbox_ref(value);
    LiraHeader *header = (LiraHeader *)object;
    if (header->kind != LIRA_KIND_ENUM) {
        lira_rt_panic("invalid Any enum payload");
    }
    int64_t tag;
    memcpy(&tag, (const unsigned char *)object + 16, sizeof(tag));
    return tag;
}

static int lira_any_key_equals(const LiraStr *key, const char *literal) {
    size_t length = strlen(literal);
    return key != NULL && key->len >= 0 && (size_t)key->len == length &&
           memcmp(key->data, literal, length) == 0;
}

static LiraAny *lira_any_enum_payload(const LiraAny *value, LiraAnyTypeView type,
                                      const LiraAnyEnumVariant *variant) {
    if (variant->payload_count == 0) {
        return lira_rt_any_null();
    }
    void *object = lira_rt_any_unbox_ref(value);
    if (variant->payload_count == 1) {
        LiraAnyTypeView child = lira_any_desc_enum_payload_at(type, variant, 0);
        int64_t raw;
        memcpy(&raw, (const unsigned char *)object + 24, sizeof(raw));
        return lira_any_from_typed_slot(raw, child);
    }
    LiraArray *payloads = lira_rt_array_new((int64_t)variant->payload_count);
    for (size_t index = 0; index < variant->payload_count; index++) {
        LiraAnyTypeView child = lira_any_desc_enum_payload_at(type, variant, index);
        int64_t raw;
        memcpy(&raw, (const unsigned char *)object + 24 + index * sizeof(raw), sizeof(raw));
        LiraAny *element = lira_any_from_typed_slot(raw, child);
        lira_rt_array_push(payloads, (int64_t)(uintptr_t)element);
    }
    return lira_rt_any_box_array(payloads);
}

static LiraAny *lira_any_result_payload(const LiraAny *value, LiraAnyTypeView type,
                                        int error_side) {
    LiraAnyTypeView child = lira_any_desc_result_child(type, error_side);
    int64_t raw;
    memcpy(&raw, (const unsigned char *)lira_rt_any_unbox_ref(value) + 24, sizeof(raw));
    return lira_any_from_typed_slot(raw, child);
}

static LiraAny *lira_any_enum_field(const LiraAny *value, const LiraStr *key) {
    LiraAnyTypeView type = lira_any_type_view(value);
    int64_t tag = lira_any_enum_tag(value);
    if (lira_any_is_enum_descriptor(type)) {
        const char *enum_name;
        size_t enum_name_len;
        if (!lira_any_desc_enum_name(type, &enum_name, &enum_name_len)) {
            lira_rt_panic("invalid Any enum descriptor");
        }
        if (lira_any_key_equals(key, "__enum")) {
            return lira_rt_any_box_string(lira_rt_str_new(enum_name, (int64_t)enum_name_len));
        }
        LiraAnyEnumVariant variant;
        if (!lira_any_desc_enum_variant_tag(type, tag, &variant)) {
            lira_rt_panic("invalid Any enum discriminant");
        }
        if (lira_any_key_equals(key, "__variant")) {
            return lira_rt_any_box_string(
                lira_rt_str_new(variant.name, (int64_t)variant.name_len));
        }
        if (lira_any_key_equals(key, "__data")) {
            return lira_any_enum_payload(value, type, &variant);
        }
    } else if (lira_any_is_result_descriptor(type)) {
        if (lira_any_key_equals(key, "__enum")) {
            return lira_rt_any_box_string(lira_rt_str_new("Result", 6));
        }
        int error_side = tag == 1;
        if (tag != 0 && tag != 1) {
            lira_rt_panic("invalid Any result discriminant");
        }
        if (lira_any_key_equals(key, "__variant")) {
            return lira_rt_any_box_string(
                lira_rt_str_new(error_side ? "Err" : "Ok", error_side ? 3 : 2));
        }
        if (lira_any_key_equals(key, "__data")) {
            return lira_any_result_payload(value, type, error_side);
        }
    }
    lira_rt_panic("object field does not exist");
    return lira_rt_any_null();
}

static size_t lira_any_enum_field_count(const LiraAny *value, LiraAnyTypeView type) {
    if (lira_any_is_result_descriptor(type)) {
        return 3;
    }
    LiraAnyEnumVariant variant;
    if (!lira_any_desc_enum_variant_tag(type, lira_any_enum_tag(value), &variant)) {
        lira_rt_panic("invalid Any enum discriminant");
    }
    return variant.payload_count == 0 ? 2 : 3;
}

static LiraStr *lira_any_enum_key_at(const LiraAny *value, LiraAnyTypeView type,
                                     int64_t index) {
    size_t count = lira_any_enum_field_count(value, type);
    if (index < 0 || (uint64_t)index >= count) {
        lira_rt_panic("object key index out of bounds");
    }
    const char *name;
    if (count == 3 && index == 0) {
        name = "__data";
    } else if ((count == 3 && index == 1) || (count == 2 && index == 0)) {
        name = "__enum";
    } else {
        name = "__variant";
    }
    return lira_rt_str_new(name, (int64_t)strlen(name));
}

static int64_t lira_any_read_object_field(const void *object,
                                          const LiraAnyObjectField *field) {
    if (field->width > sizeof(uint64_t) || field->offset > SIZE_MAX - field->width) {
        lira_rt_panic("invalid Any object field layout");
    }
    uint64_t bits = 0;
    memcpy(&bits, (const unsigned char *)object + field->offset, field->width);
    switch (lira_any_desc_kind(field->type)) {
        case 'b':
            return (int64_t)(bits != 0 ? 1 : 0);
        case 'i':
            switch (field->width) {
                case 1:
                    return (int64_t)(int8_t)bits;
                case 2:
                    return (int64_t)(int16_t)bits;
                case 4:
                    return (int64_t)(int32_t)bits;
                case 8:
                    return (int64_t)bits;
                default:
                    lira_rt_panic("invalid Any signed field width");
            }
            break;
        case 'u':
            return (int64_t)bits;
        default:
            return (int64_t)bits;
    }
    return 0;
}

static void lira_any_write_object_field(void *object, const LiraAnyObjectField *field,
                                        int64_t raw) {
    if (field->width > sizeof(uint64_t) || field->offset > SIZE_MAX - field->width) {
        lira_rt_panic("invalid Any object field layout");
    }
    uint64_t bits = (uint64_t)raw;
    memcpy((unsigned char *)object + field->offset, &bits, field->width);
}

/* ------------------------------------------------------------------ */
/* Truthiness, formatting, length and indexing                         */
/* ------------------------------------------------------------------ */

int8_t lira_rt_any_truthy(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_NULL:
            return 0;
        case LIRA_ANY_BOOL:
            return checked->payload != 0 ? 1 : 0;
        case LIRA_ANY_INT:
            return lira_any_as_int(value) != 0 ? 1 : 0;
        case LIRA_ANY_FLOAT:
            return lira_any_as_float(value) != 0.0 ? 1 : 0;
        case LIRA_ANY_STRING:
            return lira_any_string_payload(value)->len != 0 ? 1 : 0;
        case LIRA_ANY_ARRAY:
            return lira_rt_array_len((const LiraArray *)lira_rt_any_unbox_ref(value)) != 0 ? 1 : 0;
        case LIRA_ANY_OBJECT:
            return 1;
        case LIRA_ANY_REF:
        case LIRA_ANY_FUNCTION:
        case LIRA_ANY_CHANNEL:
        case LIRA_ANY_FIBER:
        case LIRA_ANY_INTERFACE:
            return 1;
        default:
            lira_rt_panic("invalid Any tag");
    }
    return 0;
}

typedef struct LiraAnyRenderFrame {
    int64_t tag;
    uint64_t payload;
} LiraAnyRenderFrame;

typedef struct LiraAnyRenderState {
    char *data;
    size_t len;
    size_t cap;
    int truncated;
} LiraAnyRenderState;

static void lira_any_render_append(LiraAnyRenderState *state, const char *data,
                                   size_t len) {
    if (state->truncated || len == 0) {
        return;
    }
    /* Rendering over the exact VM limit is an error, not a partial value.
     * `state->len` cannot exceed the limit because every append checks it. */
    if (len > LIRA_ANY_RENDER_MAX_BYTES - state->len) {
        state->truncated = 1;
        return;
    }
    size_t required = state->len + len;
    if (required > state->cap) {
        size_t cap = state->cap == 0 ? 128 : state->cap;
        while (cap < required) {
            if (cap > LIRA_ANY_RENDER_MAX_BYTES / 2) {
                cap = LIRA_ANY_RENDER_MAX_BYTES;
                break;
            }
            cap *= 2;
        }
        char *grown = (char *)lira_rt_mem_try_realloc(state->data, cap);
        if (grown == NULL) {
            lira_rt_panic(lira_gc_last_allocation_error());
            state->truncated = 1;
            return;
        }
        state->data = grown;
        state->cap = cap;
    }
    memcpy(state->data + state->len, data, len);
    state->len = required;
}

static void lira_any_render_append_cstr(LiraAnyRenderState *state, const char *text) {
    lira_any_render_append(state, text, strlen(text));
}

static int lira_any_in_render_stack(const LiraAny *value, const LiraAnyRenderFrame *stack,
                                    size_t depth) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag != LIRA_ANY_ARRAY && checked->tag != LIRA_ANY_OBJECT) {
        return 0;
    }
    for (size_t i = 0; i < depth; i++) {
        if (stack[i].tag == checked->tag && stack[i].payload == checked->payload) {
            return 1;
        }
    }
    return 0;
}

static void lira_any_render(const LiraAny *value, LiraAnyRenderFrame *stack, size_t depth,
                            LiraAnyRenderState *state);

static int lira_any_render_key_compare(const void *left_raw, const void *right_raw) {
    LiraStr *left = (LiraStr *)(uintptr_t)*(const int64_t *)left_raw;
    LiraStr *right = (LiraStr *)(uintptr_t)*(const int64_t *)right_raw;
    if (left == NULL || right == NULL || left->len < 0 || right->len < 0) {
        lira_rt_panic("invalid map key while rendering");
        return 0;
    }
    size_t left_len = (size_t)left->len;
    size_t right_len = (size_t)right->len;
    size_t shared = left_len < right_len ? left_len : right_len;
    int ordering = memcmp(left->data, right->data, shared);
    if (ordering != 0) {
        return ordering;
    }
    return left_len < right_len ? -1 : left_len > right_len ? 1 : 0;
}

static int lira_any_render_object_field_compare(const void *left_raw,
                                                const void *right_raw) {
    const LiraAnyObjectField *left = (const LiraAnyObjectField *)left_raw;
    const LiraAnyObjectField *right = (const LiraAnyObjectField *)right_raw;
    size_t shared = left->name_len < right->name_len ? left->name_len : right->name_len;
    int ordering = memcmp(left->name, right->name, shared);
    if (ordering != 0) {
        return ordering;
    }
    return left->name_len < right->name_len ? -1 : left->name_len > right->name_len ? 1 : 0;
}

static void lira_any_render_array(const LiraAny *value, LiraAnyRenderFrame *stack,
                                  size_t depth, LiraAnyRenderState *state) {
    LiraArray *array = (LiraArray *)lira_rt_any_unbox_ref(value);
    int is_tuple = lira_any_desc_kind(lira_any_type_view(value)) == 't';
    lira_any_render_append_cstr(state, is_tuple ? "(" : "[");
    if (depth >= LIRA_ANY_RENDER_LIMIT || lira_any_in_render_stack(value, stack, depth)) {
        lira_any_render_append_cstr(state, "...");
        lira_any_render_append_cstr(state, is_tuple ? ")" : "]");
        return;
    }
    LiraAny *checked = lira_any_checked(value);
    stack[depth].tag = checked->tag;
    stack[depth].payload = checked->payload;
    for (int64_t i = 0; i < array->len; i++) {
        if (state->truncated) {
            return;
        }
        if (i != 0) {
            lira_any_render_append_cstr(state, ", ");
        }
        int64_t slot = lira_rt_array_get(array, i);
        LiraAny *element = lira_any_from_typed_slot(slot, lira_any_aggregate_element(value, i));
        lira_any_render(element, stack, depth + 1, state);
    }
    if (is_tuple && array->len == 1) {
        lira_any_render_append_cstr(state, ",");
    }
    lira_any_render_append_cstr(state, is_tuple ? ")" : "]");
}

static void lira_any_render_map(const LiraAny *value, LiraAnyRenderFrame *stack, size_t depth,
                                LiraAnyRenderState *state) {
    void *map = lira_rt_any_unbox_ref(value);
    LiraArray *keys = lira_rt_map_keys(map);
    if (keys->len > 1) {
        qsort(keys->data, (size_t)keys->len, sizeof(*keys->data),
              lira_any_render_key_compare);
    }
    lira_any_render_append_cstr(state, "{");
    if (depth >= LIRA_ANY_RENDER_LIMIT || lira_any_in_render_stack(value, stack, depth)) {
        lira_any_render_append_cstr(state, "...");
        lira_any_render_append_cstr(state, "}");
        return;
    }
    LiraAny *checked = lira_any_checked(value);
    stack[depth].tag = checked->tag;
    stack[depth].payload = checked->payload;
    for (int64_t i = 0; i < keys->len; i++) {
        if (state->truncated) {
            return;
        }
        if (i != 0) {
            lira_any_render_append_cstr(state, ", ");
        }
        LiraStr *key = (LiraStr *)(uintptr_t)lira_rt_array_get(keys, i);
        lira_any_render_append(state, key->data, (size_t)key->len);
        lira_any_render_append_cstr(state, ": ");
        int64_t raw = lira_rt_map_get(map, key);
        LiraAny *element = lira_any_from_typed_slot(raw, lira_any_aggregate_element(value, i));
        lira_any_render(element, stack, depth + 1, state);
    }
    lira_any_render_append_cstr(state, "}");
}

static void lira_any_render_object(const LiraAny *value, LiraAnyRenderFrame *stack,
                                   size_t depth, LiraAnyRenderState *state) {
    LiraAnyTypeView type = lira_any_type_view(value);
    if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
        lira_any_render_append_cstr(state, "{");
        if (depth >= LIRA_ANY_RENDER_LIMIT || lira_any_in_render_stack(value, stack, depth)) {
            lira_any_render_append_cstr(state, "...}");
            return;
        }
        LiraAny *checked = lira_any_checked(value);
        stack[depth].tag = checked->tag;
        stack[depth].payload = checked->payload;
        size_t count = lira_any_enum_field_count(value, type);
        for (size_t index = 0; index < count; index++) {
            if (state->truncated) {
                return;
            }
            LiraStr *key = lira_any_enum_key_at(value, type, (int64_t)index);
            if (index != 0) {
                lira_any_render_append_cstr(state, ", ");
            }
            lira_any_render_append(state, key->data, (size_t)key->len);
            lira_any_render_append_cstr(state, ": ");
            LiraAny *field = lira_any_enum_field(value, key);
            lira_any_render(field, stack, depth + 1, state);
        }
        lira_any_render_append_cstr(state, "}");
        return;
    }
    if (!lira_any_is_typed_object(type)) {
        lira_any_render_map(value, stack, depth, state);
        return;
    }
    void *object = lira_rt_any_unbox_ref(value);
    lira_any_render_append_cstr(state, "{");
    if (depth >= LIRA_ANY_RENDER_LIMIT || lira_any_in_render_stack(value, stack, depth)) {
        lira_any_render_append_cstr(state, "...");
        lira_any_render_append_cstr(state, "}");
        return;
    }
    LiraAny *checked = lira_any_checked(value);
    stack[depth].tag = checked->tag;
    stack[depth].payload = checked->payload;
    size_t count = lira_any_desc_object_field_count(type);
    if (count > SIZE_MAX / sizeof(LiraAnyObjectField)) {
        lira_rt_panic("Any object has too many printable fields");
        return;
    }
    LiraAnyObjectField *fields = NULL;
    if (count > 0) {
        fields = (LiraAnyObjectField *)lira_rt_mem_try_alloc(count * sizeof(*fields), 0);
        if (fields == NULL) {
            lira_rt_panic(lira_gc_last_allocation_error());
            return;
        }
    }
    for (size_t ordinal = 0; ordinal < count; ordinal++) {
        if (!lira_any_desc_object_field_at(type, ordinal, &fields[ordinal])) {
            lira_rt_mem_free(fields);
            lira_rt_panic("invalid Any object descriptor");
            return;
        }
    }
    if (count > 1) {
        qsort(fields, count, sizeof(*fields), lira_any_render_object_field_compare);
    }
    for (size_t ordinal = 0; ordinal < count; ordinal++) {
        if (state->truncated) {
            break;
        }
        LiraAnyObjectField field = fields[ordinal];
        if (ordinal != 0) {
            lira_any_render_append_cstr(state, ", ");
        }
        lira_any_render_append(state, field.name, field.name_len);
        lira_any_render_append_cstr(state, ": ");
        LiraAny *element = lira_any_from_typed_slot(
            lira_any_read_object_field(object, &field), field.type);
        lira_any_render(element, stack, depth + 1, state);
    }
    lira_rt_mem_free(fields);
    lira_any_render_append_cstr(state, "}");
}

static void lira_any_render(const LiraAny *value, LiraAnyRenderFrame *stack, size_t depth,
                            LiraAnyRenderState *state) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_NULL:
            lira_any_render_append_cstr(state, "null");
            return;
        case LIRA_ANY_BOOL:
        {
            LiraStr *text = lira_rt_bool_to_str(checked->payload != 0 ? 1 : 0);
            lira_any_render_append(state, text->data, (size_t)text->len);
            return;
        }
        case LIRA_ANY_INT:
        {
            LiraStr *text = lira_rt_int_to_str(lira_any_as_int(value));
            lira_any_render_append(state, text->data, (size_t)text->len);
            return;
        }
        case LIRA_ANY_FLOAT:
        {
            LiraStr *text = lira_rt_float_to_str(lira_any_as_float(value));
            lira_any_render_append(state, text->data, (size_t)text->len);
            return;
        }
        case LIRA_ANY_STRING:
        {
            LiraStr *text = lira_any_string_payload(value);
            lira_any_render_append(state, text->data, (size_t)text->len);
            return;
        }
        case LIRA_ANY_ARRAY:
            lira_any_render_array(value, stack, depth, state);
            return;
        case LIRA_ANY_OBJECT:
            lira_any_render_object(value, stack, depth, state);
            return;
        case LIRA_ANY_REF:
            lira_any_render_append_cstr(
                state,
                lira_any_desc_kind(lira_any_type_view(value)) == 'x' ? "{...}" : "<ref>");
            return;
        case LIRA_ANY_FUNCTION:
            lira_any_render_append_cstr(state, "<function>");
            return;
        case LIRA_ANY_CHANNEL:
            lira_any_render_append_cstr(state, "<channel>");
            return;
        case LIRA_ANY_FIBER:
            lira_any_render_append_cstr(state, "<fiber>");
            return;
        case LIRA_ANY_INTERFACE:
            lira_any_render_append_cstr(state, "<interface>");
            return;
        default:
            lira_rt_panic("invalid Any tag");
    }
}

LiraStr *lira_rt_any_to_string(const LiraAny *value) {
    LiraAnyRenderFrame stack[LIRA_ANY_RENDER_LIMIT];
    LiraAnyRenderState state = {NULL, 0, 0, 0};
    lira_any_render(value, stack, 0, &state);
    if (state.truncated) {
        lira_rt_mem_free(state.data);
        lira_rt_panic("one printed value exceeded the 8388608 byte output limit");
        return lira_rt_str_new("", 0);
    }
    LiraStr *result = lira_rt_str_new(state.data == NULL ? "" : state.data,
                                      (int64_t)state.len);
    lira_rt_mem_free(state.data);
    return result;
}

int64_t lira_rt_any_len(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_STRING: {
            LiraStr *string = lira_any_string_payload(value);
            return string->len;
        }
        case LIRA_ANY_ARRAY:
            return lira_rt_array_len((const LiraArray *)lira_rt_any_unbox_ref(value));
        case LIRA_ANY_OBJECT: {
            LiraAnyTypeView type = lira_any_type_view(value);
            if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
                return (int64_t)lira_any_enum_field_count(value, type);
            }
            if (lira_any_is_typed_object(type)) {
                return (int64_t)lira_any_desc_object_field_count(type);
            }
            return lira_rt_map_len(lira_rt_any_unbox_ref(value));
        }
        case LIRA_ANY_INTERFACE:
            lira_rt_panic("length is not defined for an interface");
            break;
        default:
            lira_rt_panic("length requires a string, array, or object");
    }
    return 0;
}

int64_t lira_rt_any_object_len(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag != LIRA_ANY_OBJECT) {
        lira_rt_panic("object length requires an object");
    }
    LiraAnyTypeView type = lira_any_type_view(value);
    if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
        return (int64_t)lira_any_enum_field_count(value, type);
    }
    if (lira_any_is_typed_object(type)) {
        return (int64_t)lira_any_desc_object_field_count(type);
    }
    return lira_rt_map_len(lira_rt_any_unbox_ref(value));
}

LiraStr *lira_rt_any_object_key_at(const LiraAny *value, int64_t index) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag != LIRA_ANY_OBJECT || index < 0) {
        lira_rt_panic("object key index is invalid");
    }
    LiraAnyTypeView type = lira_any_type_view(value);
    if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
        return lira_any_enum_key_at(value, type, index);
    }
    if (lira_any_is_typed_object(type)) {
        LiraAnyObjectField field;
        if (!lira_any_desc_object_field_at(type, (size_t)index, &field)) {
            lira_rt_panic("object key index out of bounds");
        }
        return lira_rt_str_new(field.name, (int64_t)field.name_len);
    }
    LiraArray *keys = lira_rt_map_keys(lira_rt_any_unbox_ref(value));
    return (LiraStr *)(uintptr_t)lira_rt_array_get(keys, index);
}

static int64_t lira_any_to_typed_slot(const LiraAny *value, LiraAnyTypeView type);

static const char lira_any_dynamic_array_descriptor[] = "a(y)";

static int lira_any_value_matches_descriptor(const LiraAny *value,
                                             LiraAnyTypeView descriptor) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag == LIRA_ANY_INTERFACE) {
        return 0;
    }
    switch (lira_any_desc_kind(descriptor)) {
        case 'y':
            return 1;
        case 'b':
            return checked->tag == LIRA_ANY_BOOL;
        case 'i':
            return checked->tag == LIRA_ANY_INT;
        case 'u':
            return checked->tag == LIRA_ANY_INT;
        case 'f':
            return checked->tag == LIRA_ANY_FLOAT;
        case 's':
            return checked->tag == LIRA_ANY_STRING;
        case 'a':
        case 't': {
            if (checked->tag != LIRA_ANY_ARRAY) {
                return 0;
            }
            LiraAnyTypeView actual = lira_any_type_view(value);
            return actual.data != NULL && actual.len == descriptor.len &&
                   memcmp(actual.data, descriptor.data, descriptor.len) == 0;
        }
        case 'm':
        case 'o':
        case 'S':
        case 'C':
        case 'e':
        case 'R':
        case 'F':
        case 'c':
        case 'r':
        case 'x': {
            LiraAnyTypeView actual = lira_any_type_view(value);
            return actual.data != NULL && actual.len == descriptor.len &&
                   memcmp(actual.data, descriptor.data, descriptor.len) == 0;
        }
        case 'q':
            if (checked->tag == LIRA_ANY_NULL) {
                return 1;
            }
            return lira_any_value_matches_descriptor(value, lira_any_desc_child(descriptor));
        default:
            return 0;
    }
}

/* Convert a typed array behind an erased wrapper to the canonical `[any]`
 * representation before accepting a heterogeneous push. The old array is
 * never modified: typed aliases continue to observe their original uniform
 * slots, while this wrapper points at newly allocated Any slots. */
static void lira_any_widen_array(LiraAny *object, const LiraAny *value,
                                 LiraAnyTypeView element_type) {
    LiraArray *old_array = (LiraArray *)lira_rt_any_unbox_ref(object);
    int64_t old_len = lira_rt_array_len(old_array);
    if (old_len == INT64_MAX) {
        lira_rt_panic("array length overflow while widening Any array");
    }
    LiraArray *dynamic = lira_rt_array_new(old_len + 1);
    for (int64_t index = 0; index < old_len; index++) {
        LiraAny *element = lira_any_from_typed_slot(
            lira_rt_array_get(old_array, index), element_type);
        lira_rt_array_push(dynamic, (int64_t)(uintptr_t)element);
    }
    lira_rt_array_push(dynamic, (int64_t)(uintptr_t)lira_any_checked(value));
    object->payload = lira_any_ptr_payload(dynamic);
    object->type_data = (uint64_t)(uintptr_t)lira_any_dynamic_array_descriptor;
    object->type_len = sizeof(lira_any_dynamic_array_descriptor) - 1;
}

LiraAny *lira_rt_any_array_at(const LiraAny *object, int64_t index) {
    LiraAny *checked = lira_any_checked(object);
    if (checked->tag != LIRA_ANY_ARRAY) {
        lira_rt_panic("array access requires an array");
    }
    LiraArray *array = (LiraArray *)lira_rt_any_unbox_ref(object);
    return lira_any_from_typed_slot(lira_rt_array_get(array, index),
                                    lira_any_aggregate_element(object, index));
}

LiraAny *lira_rt_any_object_at(const LiraAny *object, const LiraStr *key) {
    LiraAny *checked = lira_any_checked(object);
    if (checked->tag != LIRA_ANY_OBJECT) {
        lira_rt_panic("object access requires an object");
    }
    LiraAnyTypeView type = lira_any_type_view(object);
    if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
        return lira_any_enum_field(object, key);
    }
    if (lira_any_is_typed_object(type)) {
        LiraAnyObjectField field;
        if (!lira_any_desc_object_field(type, key, &field)) {
            lira_rt_panic("object field does not exist");
        }
        void *object_ptr = lira_rt_any_unbox_ref(object);
        return lira_any_from_typed_slot(lira_any_read_object_field(object_ptr, &field),
                                        field.type);
    }
    void *map = lira_rt_any_unbox_ref(object);
    return lira_any_from_typed_slot(lira_rt_map_get(map, key),
                                    lira_any_aggregate_element(object, 0));
}

void lira_rt_any_set(const LiraAny *object, const LiraAny *key, const LiraAny *value) {
    LiraAny *checked = lira_any_checked(object);
    LiraAny *key_checked = lira_any_checked(key);
    if (checked->tag == LIRA_ANY_ARRAY) {
        if (lira_any_desc_kind(lira_any_type_view(object)) == 't') {
            lira_rt_panic("tuple indexes are immutable");
        }
        if (key_checked->tag != LIRA_ANY_INT) {
            lira_rt_panic("array index must be an integer");
        }
        int64_t index = lira_any_as_int(key);
        LiraAnyTypeView element_type = lira_any_aggregate_element(object, index);
        int64_t slot = lira_any_to_typed_slot(value, element_type);
        lira_rt_array_set((LiraArray *)lira_rt_any_unbox_ref(object), index, slot);
        return;
    }
    if (checked->tag == LIRA_ANY_OBJECT) {
        if (key_checked->tag != LIRA_ANY_STRING) {
            lira_rt_panic("object index must be a string");
        }
        LiraStr *string = lira_any_string_payload(key);
        LiraAnyTypeView type = lira_any_type_view(object);
        if (lira_any_is_enum_descriptor(type) || lira_any_is_result_descriptor(type)) {
            lira_rt_panic("enum fields are read-only");
        }
        if (lira_any_is_typed_object(type)) {
            LiraAnyObjectField field;
            if (!lira_any_desc_object_field(type, string, &field)) {
                lira_rt_panic("object field does not exist");
            }
            int64_t raw = lira_any_to_typed_slot(value, field.type);
            lira_any_write_object_field(lira_rt_any_unbox_ref(object), &field, raw);
            return;
        }
        LiraAnyTypeView element_type = lira_any_aggregate_element(object, 0);
        int64_t slot = lira_any_to_typed_slot(value, element_type);
        lira_rt_map_set(lira_rt_any_unbox_ref(object), string, slot);
        return;
    }
    lira_rt_panic("assignment requires an array or object");
}

LiraAny *lira_rt_any_index(const LiraAny *object, const LiraAny *key) {
    LiraAny *checked = lira_any_checked(object);
    LiraAny *key_checked = lira_any_checked(key);
    if (checked->tag == LIRA_ANY_ARRAY) {
        if (key_checked->tag != LIRA_ANY_INT) {
            lira_rt_panic("array index must be an integer");
        }
        return lira_rt_any_array_at(object, lira_any_as_int(key));
    }
    if (checked->tag == LIRA_ANY_OBJECT) {
        if (key_checked->tag != LIRA_ANY_STRING) {
            lira_rt_panic("object index must be a string");
        }
        LiraStr *string = lira_any_string_payload(key);
        return lira_rt_any_object_at(object, string);
    }
    if (checked->tag == LIRA_ANY_STRING) {
        if (key_checked->tag != LIRA_ANY_INT) {
            lira_rt_panic("string index must be an integer");
        }
        return lira_rt_any_box_string(
            lira_rt_str_index(lira_any_string_payload(object), lira_any_as_int(key)));
    }
    lira_rt_panic("index requires an array, object, or string");
    return lira_rt_any_null();
}

static int64_t lira_any_to_typed_slot(const LiraAny *value, LiraAnyTypeView type) {
    LiraAny *checked = lira_any_checked(value);
    switch (lira_any_desc_kind(type)) {
        case 'y':
            return (int64_t)(uintptr_t)checked;
        case 'b':
            lira_any_require_tag(value, LIRA_ANY_BOOL);
            return checked->payload != 0 ? 1 : 0;
        case 'i':
            lira_any_require_tag(value, LIRA_ANY_INT);
            return (int64_t)checked->payload;
        case 'f':
            lira_any_require_tag(value, LIRA_ANY_FLOAT);
            return (int64_t)checked->payload;
        case 's':
            return (int64_t)(uintptr_t)lira_rt_any_unbox_string(value);
        case 'a':
            lira_any_require_tag(value, LIRA_ANY_ARRAY);
            if (lira_any_type_view(value).data != NULL) {
                lira_any_require_descriptor_view(value, type);
            } else if (lira_any_desc_kind(lira_any_desc_child(type)) != 'y') {
                lira_rt_panic("untyped Any array cannot be used as a typed array");
            }
            return (int64_t)(uintptr_t)lira_rt_any_unbox_array(value);
        case 'm':
            lira_any_require_tag(value, LIRA_ANY_OBJECT);
            if (lira_any_type_view(value).data != NULL) {
                lira_any_require_descriptor_view(value, type);
            } else {
                lira_rt_panic("untyped Any object cannot be used as a typed map");
            }
            return (int64_t)(uintptr_t)lira_any_unbox_map_view(value, type);
        case 't':
            lira_any_require_tag(value, LIRA_ANY_ARRAY);
            lira_any_require_descriptor_view(value, type);
            return (int64_t)(uintptr_t)lira_rt_any_unbox_array(value);
        case 'q': {
            if (checked->tag == LIRA_ANY_NULL) {
                return 0;
            }
            LiraAnyTypeView child = lira_any_desc_child(type);
            char child_kind = lira_any_desc_kind(child);
            int64_t payload = lira_any_to_typed_slot(value, child);
            if (child_kind == 'b' || child_kind == 'i' || child_kind == 'u' || child_kind == 'f') {
                LiraHeader *optional =
                    (LiraHeader *)lira_rt_alloc(LIRA_ANY_OPTIONAL_SLOT_OFFSET + sizeof(int64_t),
                                                LIRA_KIND_STRUCT);
                memcpy((unsigned char *)optional + LIRA_ANY_OPTIONAL_SLOT_OFFSET, &payload,
                       sizeof(payload));
                return (int64_t)(uintptr_t)optional;
            }
            return payload;
        }
        case 'o':
        case 'S':
        case 'C':
            lira_any_require_tag(value, LIRA_ANY_OBJECT);
            lira_any_require_descriptor_view(value, type);
            return (int64_t)lira_any_checked(value)->payload;
        case 'e':
        case 'R':
            lira_any_require_tag(value, LIRA_ANY_OBJECT);
            lira_any_require_descriptor_view(value, type);
            return (int64_t)lira_any_checked(value)->payload;
        case 'F':
            lira_any_require_tag(value, LIRA_ANY_FUNCTION);
            if (type.data != NULL && type.len > 1 && type.data[1] == '(') {
                lira_any_require_descriptor_view(value, type);
            }
            return (int64_t)lira_any_checked(value)->payload;
        case 'c':
            lira_any_require_tag(value, LIRA_ANY_CHANNEL);
            if (type.data != NULL && type.len > 1 && type.data[1] == '(') {
                lira_any_require_descriptor_view(value, type);
            }
            return (int64_t)lira_any_checked(value)->payload;
        case 'r':
        case 'x':
            return (int64_t)(uintptr_t)lira_rt_any_unbox_ref(value);
        default:
            lira_rt_panic("invalid Any element descriptor");
    }
    return 0;
}

void *lira_rt_any_unbox_optional(const LiraAny *value, const LiraStr *type) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag == LIRA_ANY_NULL) {
        return NULL;
    }
    LiraAnyTypeView descriptor = lira_any_descriptor(type);
    if (lira_any_desc_kind(descriptor) != 'q') {
        lira_rt_panic("invalid Any optional descriptor");
    }
    LiraAnyTypeView child = lira_any_desc_child(descriptor);
    char child_kind = lira_any_desc_kind(child);
    int64_t payload = lira_any_to_typed_slot(value, child);
    if (child_kind == 'b' || child_kind == 'i' || child_kind == 'u' || child_kind == 'f') {
        LiraHeader *optional =
            (LiraHeader *)lira_rt_alloc(LIRA_ANY_OPTIONAL_SLOT_OFFSET + sizeof(int64_t),
                                         LIRA_KIND_STRUCT);
        memcpy((unsigned char *)optional + LIRA_ANY_OPTIONAL_SLOT_OFFSET, &payload,
               sizeof(payload));
        return optional;
    }
    return (void *)(uintptr_t)payload;
}

void lira_rt_any_push(const LiraAny *object, const LiraAny *value) {
    LiraAny *checked = lira_any_checked(object);
    if (checked->tag != LIRA_ANY_ARRAY) {
        lira_rt_panic("push requires an array");
    }
    if (lira_any_desc_kind(lira_any_type_view(object)) == 't') {
        lira_rt_panic("tuples are immutable");
    }
    LiraAnyTypeView element_type = lira_any_aggregate_element(object, 0);
    if (lira_any_desc_kind(lira_any_type_view(object)) == 'a' &&
        lira_any_desc_kind(element_type) != 'y' &&
        !lira_any_value_matches_descriptor(value, element_type)) {
        /* A dynamic Any receiver may have originated as `[T]` across an
         * erased call. A mismatched push widens that wrapper by copying every
         * existing typed slot into a tagged Any slot; it never writes a raw
         * pointer into the old `[T]` storage. */
        lira_any_widen_array(checked, value, element_type);
        return;
    }
    int64_t slot = lira_any_to_typed_slot(value, element_type);
    lira_rt_array_push((LiraArray *)lira_rt_any_unbox_ref(object), slot);
}

LiraAny *lira_rt_any_pop(const LiraAny *object) {
    LiraAny *checked = lira_any_checked(object);
    if (checked->tag != LIRA_ANY_ARRAY) {
        lira_rt_panic("pop requires an array");
    }
    if (lira_any_desc_kind(lira_any_type_view(object)) == 't') {
        lira_rt_panic("tuples are immutable");
    }
    LiraArray *array = (LiraArray *)lira_rt_any_unbox_ref(object);
    /* The raw array helper deliberately rejects empty arrays.  Dynamic Any
     * pop has the language-level optional contract, so return the canonical
     * Any null before decoding a descriptor or touching the array slot. */
    if (lira_rt_array_len(array) == 0) {
        return lira_rt_any_null();
    }
    LiraAnyTypeView element_type = lira_any_aggregate_element(object, 0);
    return lira_any_from_typed_slot(lira_rt_array_pop(array), element_type);
}

static int64_t lira_any_float_to_int(double value) {
    if (isnan(value)) {
        return 0;
    }
    if (value >= 0x1p63) {
        return INT64_MAX;
    }
    if (value <= -0x1p63) {
        return INT64_MIN;
    }
    return (int64_t)value;
}

static int lira_any_parse_int(const LiraStr *string, int64_t *result) {
    for (int64_t i = 0; i < string->len; i++) {
        if (isspace((unsigned char)string->data[i])) {
            return 0;
        }
    }
    char *end = NULL;
    errno = 0;
    long long parsed = strtoll(string->data, &end, 10);
    if (end == string->data || end == NULL || *end != '\0' || errno == ERANGE) {
        return 0;
    }
    *result = (int64_t)parsed;
    return 1;
}

static int lira_any_parse_float(const LiraStr *string, double *result) {
    for (int64_t i = 0; i < string->len; i++) {
        /* Rust's `str::parse::<f64>()` used by the VM rejects surrounding
         * whitespace; libc's strtod accepts it, so reject it explicitly before
         * delegating to the platform parser. */
        if (isspace((unsigned char)string->data[i])) {
            return 0;
        }
    }
    char *end = NULL;
    errno = 0;
    double parsed = strtod(string->data, &end);
    if (end == string->data || end == NULL || *end != '\0') {
        return 0;
    }
    *result = parsed;
    return 1;
}

/* Explicit casts intentionally follow VM Cast rather than strict unboxing:
 * invalid strings and unrelated tags produce the VM's zero fallback. */
int64_t lira_rt_any_cast_int(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_INT:
            return (int64_t)checked->payload;
        case LIRA_ANY_FLOAT: {
            double number = lira_any_as_float(value);
            return lira_any_float_to_int(number);
        }
        case LIRA_ANY_BOOL:
            return checked->payload != 0 ? 1 : 0;
        case LIRA_ANY_STRING: {
            int64_t result = 0;
            return lira_any_parse_int(lira_any_string_payload(value), &result) ? result : 0;
        }
        default:
            return 0;
    }
}

double lira_rt_any_cast_float(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    switch (checked->tag) {
        case LIRA_ANY_INT:
            return (double)(int64_t)checked->payload;
        case LIRA_ANY_FLOAT:
            return lira_any_as_float(value);
        case LIRA_ANY_BOOL:
            return checked->payload != 0 ? 1.0 : 0.0;
        case LIRA_ANY_STRING: {
            double result = 0.0;
            return lira_any_parse_float(lira_any_string_payload(value), &result) ? result : 0.0;
        }
        default:
            return 0.0;
    }
}

int8_t lira_rt_any_cast_bool(const LiraAny *value) {
    return lira_rt_any_truthy(value);
}

/* ------------------------------------------------------------------ */
/* Dynamic arithmetic and comparison                                   */
/* ------------------------------------------------------------------ */

static void lira_any_binary_error(int64_t op, const LiraAny *left, const LiraAny *right) {
    char message[160];
    const char *verb = "operate on";
    switch (op) {
        case 0:
            verb = "add";
            break;
        case 1:
            verb = "subtract";
            break;
        case 2:
            verb = "multiply";
            break;
        case 3:
            verb = "divide";
            break;
        case 4:
            verb = "modulo";
            break;
        case 5:
            verb = "raise";
            break;
        default:
            break;
    }
    snprintf(message, sizeof(message), "cannot %s %s and %s", verb,
             lira_any_tag_name(lira_any_checked(left)->tag),
             lira_any_tag_name(lira_any_checked(right)->tag));
    lira_rt_panic(message);
}

LiraAny *lira_rt_any_binary(int64_t op, const LiraAny *left, const LiraAny *right) {
    LiraAny *a = lira_any_checked(left);
    LiraAny *b = lira_any_checked(right);
    if (op == 0 && (a->tag == LIRA_ANY_STRING || b->tag == LIRA_ANY_STRING)) {
        return lira_rt_any_box_string(lira_rt_str_concat(lira_rt_any_to_string(left),
                                                         lira_rt_any_to_string(right)));
    }

    if (op >= 6 && op <= 11) {
        if (a->tag != LIRA_ANY_INT || b->tag != LIRA_ANY_INT) {
            const char *name = op == 6 ? "bitwise AND" : op == 7 ? "bitwise OR"
                                  : op == 8 ? "bitwise XOR" : op == 9 ? "shift left"
                                  : op == 10 ? "shift right" : "unsigned shift right";
            char message[128];
            snprintf(message, sizeof(message), "%s requires integer operands", name);
            lira_rt_panic(message);
        }
        int64_t x = lira_any_as_int(left);
        int64_t y = lira_any_as_int(right);
        if (op == 6) {
            return lira_rt_any_box_int((int64_t)((uint64_t)x & (uint64_t)y));
        }
        if (op == 7) {
            return lira_rt_any_box_int((int64_t)((uint64_t)x | (uint64_t)y));
        }
        if (op == 8) {
            return lira_rt_any_box_int((int64_t)((uint64_t)x ^ (uint64_t)y));
        }
        /* Match the VM's u32 conversion followed by min(63), including
         * negative and values below -2^32, rather than sign-extending. */
        uint64_t shift = (uint64_t)(uint32_t)y;
        if (shift > 63U) {
            shift = 63U;
        }
        if (op == 9) {
            return lira_rt_any_box_int((int64_t)((uint64_t)x << shift));
        }
        if (op == 10) {
            uint64_t shifted = (uint64_t)x >> shift;
            if (x < 0 && shift != 0) {
                shifted |= UINT64_MAX << (64U - shift);
            }
            return lira_rt_any_box_int((int64_t)shifted);
        }
        return lira_rt_any_box_int((int64_t)((uint64_t)x >> shift));
    }

    if (op < 0 || op > 5) {
        lira_rt_panic("unknown Any binary operation");
    }
    if (!lira_any_is_numeric(left) || !lira_any_is_numeric(right)) {
        lira_any_binary_error(op, left, right);
    }
    if (a->tag == LIRA_ANY_INT && b->tag == LIRA_ANY_INT) {
        int64_t x = lira_any_as_int(left);
        int64_t y = lira_any_as_int(right);
        switch (op) {
            case 0:
                return lira_rt_any_box_int((int64_t)((uint64_t)x + (uint64_t)y));
            case 1:
                return lira_rt_any_box_int((int64_t)((uint64_t)x - (uint64_t)y));
            case 2:
                return lira_rt_any_box_int((int64_t)((uint64_t)x * (uint64_t)y));
            case 3:
                return lira_rt_any_box_int(lira_rt_idiv(x, y));
            case 4:
                return lira_rt_any_box_int(lira_rt_imod(x, y));
            case 5:
                if (y < 0) {
                    lira_rt_panic("Negative exponent not supported for integers");
                }
                return lira_rt_any_box_int(lira_rt_ipow(x, y));
            default:
                break;
        }
    }

    double x = lira_any_as_float(left);
    double y = lira_any_as_float(right);
    switch (op) {
        case 0:
            return lira_rt_any_box_float(x + y);
        case 1:
            return lira_rt_any_box_float(x - y);
        case 2:
            return lira_rt_any_box_float(x * y);
        case 3:
            return lira_rt_any_box_float(x / y);
        case 4:
            return lira_rt_any_box_float(fmod(x, y));
        case 5:
            return lira_rt_any_box_float(pow(x, y));
        default:
            break;
    }
    lira_rt_panic("unknown Any binary operation");
    return lira_rt_any_null();
}

LiraAny *lira_rt_any_neg(const LiraAny *value) {
    LiraAny *checked = lira_any_checked(value);
    if (checked->tag == LIRA_ANY_INT) {
        return lira_rt_any_box_int((int64_t)(0U - (uint64_t)lira_any_as_int(value)));
    }
    if (checked->tag == LIRA_ANY_FLOAT) {
        return lira_rt_any_box_float(-lira_any_as_float(value));
    }
    lira_rt_panic("cannot negate non-numeric Any value");
    return lira_rt_any_null();
}

LiraAny *lira_rt_any_bit_not(const LiraAny *value) {
    lira_any_require_tag(value, LIRA_ANY_INT);
    return lira_rt_any_box_int((int64_t)~(uint64_t)lira_any_as_int(value));
}

static int8_t lira_any_equal(const LiraAny *left, const LiraAny *right) {
    LiraAny *a = lira_any_checked(left);
    LiraAny *b = lira_any_checked(right);
    if (lira_any_is_numeric(left) && lira_any_is_numeric(right)) {
        if (a->tag == LIRA_ANY_FLOAT && isnan(lira_any_as_float(left))) {
            return 0;
        }
        if (b->tag == LIRA_ANY_FLOAT && isnan(lira_any_as_float(right))) {
            return 0;
        }
        if (a->tag == LIRA_ANY_INT && b->tag == LIRA_ANY_INT) {
            return lira_any_as_int(left) == lira_any_as_int(right) ? 1 : 0;
        }
        return lira_any_as_float(left) == lira_any_as_float(right) ? 1 : 0;
    }
    if (a->tag != b->tag) {
        return 0;
    }
    switch (a->tag) {
        case LIRA_ANY_NULL:
            return 1;
        case LIRA_ANY_BOOL:
        case LIRA_ANY_INT:
            return a->payload == b->payload ? 1 : 0;
        case LIRA_ANY_FLOAT:
            return lira_any_as_float(left) == lira_any_as_float(right) ? 1 : 0;
        case LIRA_ANY_STRING:
            return lira_rt_str_eq(lira_any_string_payload(left), lira_any_string_payload(right));
        case LIRA_ANY_ARRAY:
        case LIRA_ANY_OBJECT:
        case LIRA_ANY_REF:
        case LIRA_ANY_FUNCTION:
        case LIRA_ANY_CHANNEL:
        case LIRA_ANY_FIBER:
        case LIRA_ANY_INTERFACE:
            /* The VM deliberately does not define identity equality for
             * aggregate/reference values. Only null, scalar, and string
             * values compare equal. */
            return 0;
        default:
            lira_rt_panic("invalid Any tag");
    }
    return 0;
}

int8_t lira_rt_any_compare(int64_t op, const LiraAny *left, const LiraAny *right) {
    if (op != 0 && op != 1 && (op < 2 || op > 5)) {
        lira_rt_panic("unknown Any comparison operation");
    }
    if (op == 0) {
        return lira_any_equal(left, right);
    }
    if (op == 1) {
        return lira_any_equal(left, right) ? 0 : 1;
    }

    LiraAny *a = lira_any_checked(left);
    LiraAny *b = lira_any_checked(right);
    int order;
    if (lira_any_is_numeric(left) && lira_any_is_numeric(right)) {
        if (a->tag == LIRA_ANY_INT && b->tag == LIRA_ANY_INT) {
            int64_t x = lira_any_as_int(left);
            int64_t y = lira_any_as_int(right);
            order = x < y ? -1 : x > y ? 1 : 0;
        } else {
            /* Mixed int/float ordering follows the VM's int-to-double
             * coercion; two ints never pass through a lossy double. */
            double x = lira_any_as_float(left);
            double y = lira_any_as_float(right);
            if (isnan(x) || isnan(y)) {
                return 0;
            }
            order = x < y ? -1 : x > y ? 1 : 0;
        }
    } else if (a->tag == LIRA_ANY_STRING && b->tag == LIRA_ANY_STRING) {
        order = (int)lira_rt_str_cmp(lira_any_string_payload(left), lira_any_string_payload(right));
    } else {
        /* The bytecode VM treats incomparable ordering as false. */
        return 0;
    }
    switch (op) {
        case 2:
            return order < 0 ? 1 : 0;
        case 3:
            return order <= 0 ? 1 : 0;
        case 4:
            return order > 0 ? 1 : 0;
        case 5:
            return order >= 0 ? 1 : 0;
        default:
            lira_rt_panic("unknown Any comparison operation");
    }
    return 0;
}

int8_t lira_rt_any_is(const LiraAny *value, int64_t runtime_kind) {
    int64_t tag = lira_any_checked(value)->tag;
    switch (runtime_kind) {
        case 0:
            return tag == LIRA_ANY_NULL ? 1 : 0;
        case 1:
            return tag == LIRA_ANY_BOOL ? 1 : 0;
        case 2:
            return tag == LIRA_ANY_INT ? 1 : 0;
        case 3:
            return tag == LIRA_ANY_FLOAT ? 1 : 0;
        case 4:
            return tag == LIRA_ANY_STRING ? 1 : 0;
        case 5: {
            if (tag != LIRA_ANY_ARRAY) {
                return 0;
            }
            LiraAnyTypeView type = lira_any_type_view(value);
            return lira_any_desc_kind(type) != 't' ? 1 : 0;
        }
        case 6:
            return tag == LIRA_ANY_OBJECT ? 1 : 0;
        case 7:
            return tag == LIRA_ANY_FUNCTION ? 1 : 0;
        case 8: {
            if (tag != LIRA_ANY_ARRAY) {
                return 0;
            }
            LiraAnyTypeView type = lira_any_type_view(value);
            return lira_any_desc_kind(type) == 't' ? 1 : 0;
        }
        case 9:
            return tag == LIRA_ANY_CHANNEL ? 1 : 0;
        case 10:
            return tag == LIRA_ANY_FIBER ? 1 : 0;
        case 11:
            return tag == LIRA_ANY_INTERFACE ? 1 : 0;
        default:
            return 0;
    }
}

int8_t lira_rt_any_is_typed(const LiraAny *value, const LiraStr *type) {
    /* lira_any_type_view validates the registry entry before reading it. */
    if (lira_any_checked(value)->tag == LIRA_ANY_INTERFACE) {
        return 0;
    }
    LiraAnyTypeView actual = lira_any_type_view(value);
    LiraAnyTypeView expected = lira_any_descriptor(type);
    if (expected.data == NULL || expected.len == 0 || actual.data == NULL ||
        actual.len == 0 || actual.len != expected.len) {
        return 0;
    }
    return memcmp(actual.data, expected.data, expected.len) == 0 ? 1 : 0;
}

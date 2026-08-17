/*
 * String-keyed maps.
 *
 * The bytecode VM represents a map as an object with string field names, so
 * keys here are strings too. Values are the same uniform 8-byte cells arrays
 * and enum payloads use; the code generator knows the value type statically and
 * converts on the way in and out.
 *
 * Open addressing with linear probing. Entries are never removed, only
 * overwritten, which is all the language currently offers.
 */
#include "lira_rt.h"

#include <stdlib.h>
#include <string.h>

void lira_rt_panic(const char *message);

typedef struct {
    LiraStr *key;
    int64_t value;
} LiraMapEntry;

typedef struct LiraMap {
    LiraHeader hdr;
    int64_t len;
    int64_t cap;
    LiraMapEntry *entries;
} LiraMap;

static void lira_validate_map(const LiraMap *map) {
    if (map == NULL) {
        lira_rt_panic("map operation on null");
    }
    if (map->len < 0 || map->cap < 0 || map->len > map->cap ||
        (uint64_t)map->cap > SIZE_MAX / sizeof(LiraMapEntry) ||
        (map->cap > 0 && (map->cap & (map->cap - 1)) != 0) ||
        (map->cap == 0 && map->entries != NULL) ||
        (map->cap > 0 && map->entries == NULL)) {
        lira_rt_panic("map metadata is invalid");
    }
}

/* FNV-1a over the key bytes. */
static uint64_t lira_hash(const LiraStr *key) {
    if (key == NULL || key->len < 0 ||
        (uint64_t)key->len > (uint64_t)UINT32_MAX - sizeof(LiraStr)) {
        lira_rt_panic("map key string is invalid");
    }
    uint64_t hash = 1469598103934665603ULL;
    for (int64_t i = 0; i < key->len; i++) {
        hash ^= (unsigned char)key->data[i];
        hash *= 1099511628211ULL;
    }
    return hash;
}

static int lira_keys_equal(const LiraStr *a, const LiraStr *b) {
    if (a == NULL || b == NULL || a->len < 0 || b->len < 0 ||
        (uint64_t)a->len > (uint64_t)UINT32_MAX - sizeof(LiraStr) ||
        (uint64_t)b->len > (uint64_t)UINT32_MAX - sizeof(LiraStr)) {
        if (a != NULL && b != NULL) {
            lira_rt_panic("map key string is invalid");
        }
        return 0;
    }
    return a->len == b->len && memcmp(a->data, b->data, (size_t)a->len) == 0;
}

/* Index of `key`'s entry, or of the first free slot if it is absent. */
static int64_t lira_map_slot(const LiraMap *map, const LiraStr *key) {
    int64_t mask = map->cap - 1;
    int64_t index = (int64_t)(lira_hash(key) & (uint64_t)mask);
    while (map->entries[index].key != NULL) {
        if (lira_keys_equal(map->entries[index].key, key)) {
            return index;
        }
        index = (index + 1) & mask;
    }
    return index;
}

static int lira_map_grow(LiraMap *map) {
    int64_t old_cap = map->cap;
    LiraMapEntry *old = map->entries;

    if (old_cap > INT64_MAX / 2) {
        lira_rt_panic("map capacity is too large");
        return 0;
    }
    int64_t new_cap = old_cap > 0 ? old_cap * 2 : 8;
    if ((uint64_t)new_cap > SIZE_MAX / sizeof(LiraMapEntry)) {
        lira_rt_panic("map capacity is too large");
        return 0;
    }
    size_t new_bytes = (size_t)new_cap * sizeof(LiraMapEntry);
    LiraMapEntry *entries = (LiraMapEntry *)lira_rt_mem_try_alloc(new_bytes, 1);
    if (entries == NULL) {
        lira_rt_panic(lira_gc_last_allocation_error());
        return 0;
    }
    map->cap = new_cap;
    map->entries = entries;
    for (int64_t i = 0; i < old_cap; i++) {
        if (old[i].key != NULL) {
            map->entries[lira_map_slot(map, old[i].key)] = old[i];
        }
    }
    lira_rt_mem_free(old);
    return 1;
}

void *lira_rt_map_new(void) {
    LiraMap *map = (LiraMap *)lira_rt_alloc((int64_t)sizeof(LiraMap), LIRA_KIND_MAP);
    map->len = 0;
    map->cap = 0;
    map->entries = NULL;
    return map;
}

void lira_map_gc_scan(const void *handle) {
    const LiraMap *map = (const LiraMap *)handle;
    if (map == NULL || map->entries == NULL || map->cap <= 0 ||
        (uint64_t)map->cap > SIZE_MAX / sizeof(LiraMapEntry)) {
        return;
    }
    for (int64_t i = 0; i < map->cap; i++) {
        lira_gc_mark_ptr(map->entries[i].key);
        lira_gc_mark_ptr((const void *)(uintptr_t)map->entries[i].value);
    }
}

void lira_map_gc_destroy(void *handle) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || map->entries == NULL) {
        return;
    }
    lira_rt_mem_free(map->entries);
    map->entries = NULL;
}

void lira_rt_map_set(void *handle, LiraStr *key, int64_t value) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || key == NULL) {
        lira_rt_panic("map operation on null");
    }
    lira_validate_map(map);
    /* Keep the table at most half full so probing stays short. */
    if (map->cap == 0 || map->len >= map->cap / 2) {
        if (!lira_map_grow(map)) {
            return;
        }
    }
    int64_t index = lira_map_slot(map, key);
    if (map->entries[index].key == NULL) {
        map->entries[index].key = key;
        map->len++;
    }
    map->entries[index].value = value;
}

int64_t lira_rt_map_get(void *handle, const LiraStr *key) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || key == NULL) {
        return 0;
    }
    lira_validate_map(map);
    if (map->cap == 0) {
        return 0;
    }
    int64_t index = lira_map_slot(map, key);
    return map->entries[index].key != NULL ? map->entries[index].value : 0;
}

int8_t lira_rt_map_has(void *handle, const LiraStr *key) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || key == NULL) {
        return 0;
    }
    lira_validate_map(map);
    if (map->cap == 0) {
        return 0;
    }
    return map->entries[lira_map_slot(map, key)].key != NULL ? 1 : 0;
}

int64_t lira_rt_map_len(void *handle) {
    LiraMap *map = (LiraMap *)handle;
    if (map != NULL) {
        lira_validate_map(map);
    }
    return map != NULL ? map->len : 0;
}

LiraArray *lira_rt_map_keys(void *handle) {
    LiraMap *map = (LiraMap *)handle;
    if (map != NULL) {
        lira_validate_map(map);
    }
    LiraArray *keys = lira_rt_array_new(map != NULL ? map->len : 0);
    if (map == NULL) {
        return keys;
    }
    /* The keys array grows via allocations below; each can trigger a GC while
     * the partially-built array is only reachable from this C frame. Root it
     * for the duration of the build loop. */
    lira_gc_register_root_slot(&keys);
    for (int64_t i = 0; i < map->cap; i++) {
        if (map->entries[i].key != NULL) {
            lira_rt_array_push(keys, (int64_t)(intptr_t)map->entries[i].key);
        }
    }
    lira_gc_unregister_root_slot(&keys);
    return keys;
}

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

/* FNV-1a over the key bytes. */
static uint64_t lira_hash(const LiraStr *key) {
    uint64_t hash = 1469598103934665603ULL;
    for (int64_t i = 0; i < key->len; i++) {
        hash ^= (unsigned char)key->data[i];
        hash *= 1099511628211ULL;
    }
    return hash;
}

static int lira_keys_equal(const LiraStr *a, const LiraStr *b) {
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

static void lira_map_grow(LiraMap *map) {
    int64_t old_cap = map->cap;
    LiraMapEntry *old = map->entries;

    map->cap = old_cap > 0 ? old_cap * 2 : 8;
    map->entries = (LiraMapEntry *)calloc((size_t)map->cap, sizeof(LiraMapEntry));
    if (map->entries == NULL) {
        lira_rt_panic("out of memory");
    }
    for (int64_t i = 0; i < old_cap; i++) {
        if (old[i].key != NULL) {
            map->entries[lira_map_slot(map, old[i].key)] = old[i];
        }
    }
    free(old);
}

void *lira_rt_map_new(void) {
    LiraMap *map = (LiraMap *)lira_rt_alloc((int64_t)sizeof(LiraMap), LIRA_KIND_MAP);
    map->len = 0;
    map->cap = 0;
    map->entries = NULL;
    return map;
}

void lira_rt_map_set(void *handle, LiraStr *key, int64_t value) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || key == NULL) {
        lira_rt_panic("map operation on null");
    }
    /* Keep the table at most half full so probing stays short. */
    if ((map->len + 1) * 2 > map->cap) {
        lira_map_grow(map);
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
    if (map == NULL || key == NULL || map->cap == 0) {
        return 0;
    }
    int64_t index = lira_map_slot(map, key);
    return map->entries[index].key != NULL ? map->entries[index].value : 0;
}

int8_t lira_rt_map_has(void *handle, const LiraStr *key) {
    LiraMap *map = (LiraMap *)handle;
    if (map == NULL || key == NULL || map->cap == 0) {
        return 0;
    }
    return map->entries[lira_map_slot(map, key)].key != NULL ? 1 : 0;
}

int64_t lira_rt_map_len(void *handle) {
    LiraMap *map = (LiraMap *)handle;
    return map != NULL ? map->len : 0;
}

LiraArray *lira_rt_map_keys(void *handle) {
    LiraMap *map = (LiraMap *)handle;
    LiraArray *keys = lira_rt_array_new(map != NULL ? map->len : 0);
    if (map == NULL) {
        return keys;
    }
    for (int64_t i = 0; i < map->cap; i++) {
        if (map->entries[i].key != NULL) {
            lira_rt_array_push(keys, (int64_t)(intptr_t)map->entries[i].key);
        }
    }
    return keys;
}

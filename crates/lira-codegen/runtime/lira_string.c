/*
 * String built-ins.
 *
 * The bytecode VM operates on Rust `String`s, so its indices count Unicode
 * scalar values, not bytes. These match that: `str_char_code(s, 2)` is the
 * third character, whatever its UTF-8 width.
 */
#include "lira_rt.h"

#include <stdlib.h>
#include <string.h>

void lira_rt_panic(const char *message);

static int lira_valid_string_length(const LiraStr *s) {
    return s != NULL && s->len >= 0 &&
           (uint64_t)s->len <= (uint64_t)UINT32_MAX - sizeof(LiraStr);
}

/* Decode one scalar without ever reading beyond `remaining` bytes. Invalid
 * ABI data is consumed as one byte, which keeps all string operations bounded
 * while valid Lira/Rust UTF-8 retains VM-compatible scalar semantics. */
static int64_t lira_utf8_decode(const char *p, int64_t remaining, int *width) {
    unsigned char b = (unsigned char)p[0];
    int n = 1;
    if (b < 0x80) {
        *width = 1;
        return b;
    }
    if (b >= 0xC2 && b <= 0xDF) {
        n = 2;
    } else if (b >= 0xE0 && b <= 0xEF) {
        n = 3;
    } else if (b >= 0xF0 && b <= 0xF4) {
        n = 4;
    }
    if (remaining < n) {
        *width = 1;
        return b;
    }
    for (int i = 1; i < n; i++) {
        if (((unsigned char)p[i] & 0xC0) != 0x80) {
            *width = 1;
            return b;
        }
    }
    unsigned char b1 = n > 1 ? (unsigned char)p[1] : 0;
    if ((n == 3 && ((b == 0xE0 && b1 < 0xA0) || (b == 0xED && b1 >= 0xA0))) ||
        (n == 4 && ((b == 0xF0 && b1 < 0x90) || (b == 0xF4 && b1 > 0x8F)))) {
        *width = 1;
        return b;
    }
    *width = n;
    if (n == 2) {
        return ((int64_t)(b & 0x1F) << 6) | (b1 & 0x3F);
    }
    if (n == 3) {
        return ((int64_t)(b & 0x0F) << 12) | ((int64_t)(b1 & 0x3F) << 6) |
               ((unsigned char)p[2] & 0x3F);
    }
    return ((int64_t)(b & 0x07) << 18) | ((int64_t)(b1 & 0x3F) << 12) |
           ((int64_t)((unsigned char)p[2] & 0x3F) << 6) | ((unsigned char)p[3] & 0x3F);
}

static int lira_utf8_width(const char *p, int64_t remaining) {
    int width = 1;
    (void)lira_utf8_decode(p, remaining, &width);
    return width;
}

/* Byte offset of character `index`, or the string's length if it runs off the end. */
static int64_t lira_char_offset(const LiraStr *s, int64_t index) {
    int64_t offset = 0;
    int64_t seen = 0;
    while (offset < s->len && seen < index) {
        offset += lira_utf8_width(s->data + offset, s->len - offset);
        seen++;
    }
    return offset;
}

static int lira_encode_utf8(char *buf, int64_t cp) {
    uint32_t c = (uint32_t)cp;
    if (c < 0x80) {
        buf[0] = (char)c;
        return 1;
    }
    if (c < 0x800) {
        buf[0] = (char)(0xC0 | (c >> 6));
        buf[1] = (char)(0x80 | (c & 0x3F));
        return 2;
    }
    if (c < 0x10000) {
        buf[0] = (char)(0xE0 | (c >> 12));
        buf[1] = (char)(0x80 | ((c >> 6) & 0x3F));
        buf[2] = (char)(0x80 | (c & 0x3F));
        return 3;
    }
    buf[0] = (char)(0xF0 | (c >> 18));
    buf[1] = (char)(0x80 | ((c >> 12) & 0x3F));
    buf[2] = (char)(0x80 | ((c >> 6) & 0x3F));
    buf[3] = (char)(0x80 | (c & 0x3F));
    return 4;
}

int64_t lira_rt_str_char_code(const LiraStr *s, int64_t index) {
    if (!lira_valid_string_length(s) || index < 0) {
        return -1;
    }
    int64_t offset = lira_char_offset(s, index);
    if (offset >= s->len) {
        return -1;
    }
    int width = 0;
    return lira_utf8_decode(s->data + offset, s->len - offset, &width);
}

LiraStr *lira_rt_str_index(const LiraStr *s, int64_t index) {
    if (s == NULL) {
        lira_rt_panic("index into null string");
    }
    if (!lira_valid_string_length(s)) {
        lira_rt_panic("string length is invalid");
    }
    if (index < 0) {
        lira_rt_panic("index out of bounds: negative string index");
    }
    int64_t offset = lira_char_offset(s, index);
    if (offset >= s->len) {
        lira_rt_panic("string index out of bounds");
    }
    int width = lira_utf8_width(s->data + offset, s->len - offset);
    return lira_rt_str_new(s->data + offset, width);
}

LiraStr *lira_rt_str_from_char_code(int64_t code) {
    if (code < 0 || code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
        return lira_rt_str_new("", 0);
    }
    char buf[4];
    int n = lira_encode_utf8(buf, code);
    return lira_rt_str_new(buf, n);
}

/* ASCII-only case mapping, matching what the VM does for ASCII input. Non-ASCII
 * scalars are passed through unchanged rather than half-cased. */
static LiraStr *lira_map_ascii_case(const LiraStr *s, int to_upper) {
    if (!lira_valid_string_length(s)) {
        if (s != NULL) {
            lira_rt_panic("string length is invalid");
        }
        return lira_rt_str_new("", 0);
    }
    LiraStr *out = lira_rt_str_new(s->data, s->len);
    for (int64_t i = 0; i < out->len; i++) {
        unsigned char c = (unsigned char)out->data[i];
        if (to_upper && c >= 'a' && c <= 'z') {
            out->data[i] = (char)(c - 32);
        } else if (!to_upper && c >= 'A' && c <= 'Z') {
            out->data[i] = (char)(c + 32);
        }
    }
    return out;
}

LiraStr *lira_rt_str_to_upper(const LiraStr *s) { return lira_map_ascii_case(s, 1); }
LiraStr *lira_rt_str_to_lower(const LiraStr *s) { return lira_map_ascii_case(s, 0); }

LiraStr *lira_rt_str_substring(const LiraStr *s, int64_t start, int64_t end) {
    if (!lira_valid_string_length(s)) {
        if (s != NULL) {
            lira_rt_panic("string length is invalid");
        }
        return lira_rt_str_new("", 0);
    }
    if (start < 0) {
        start = 0;
    }
    if (end < 0) {
        end = 0;
    }
    if (start >= end) {
        return lira_rt_str_new("", 0);
    }
    int64_t from = lira_char_offset(s, start);
    int64_t to = lira_char_offset(s, end);
    return lira_rt_str_new(s->data + from, to - from);
}

int64_t lira_rt_str_index_of(const LiraStr *s, const LiraStr *needle) {
    if (!lira_valid_string_length(s) || !lira_valid_string_length(needle)) {
        return -1;
    }
    if (needle->len == 0) {
        return 0;
    }
    if (needle->len > s->len) {
        return -1;
    }
    for (int64_t i = 0; i + needle->len <= s->len; i++) {
        if (memcmp(s->data + i, needle->data, (size_t)needle->len) == 0) {
            /* The VM reports a character index, so count characters up to the
             * byte position we matched at. */
            int64_t chars = 0;
            for (int64_t o = 0; o < i;) {
                o += lira_utf8_width(s->data + o, s->len - o);
                chars++;
            }
            return chars;
        }
    }
    return -1;
}

LiraArray *lira_rt_str_split(const LiraStr *s, const LiraStr *delimiter) {
    LiraArray *parts = lira_rt_array_new(0);
    /* The parts array is built with per-element allocations below; each can
     * trigger a collection while the partially-built array is only reachable
     * from this C frame (scheduler stack, not scanned by the GC). Root it so
     * a mid-loop collection cannot sweep it out from under the pushes. */
    lira_gc_register_root_slot(&parts);
    if (!lira_valid_string_length(s)) {
        if (s != NULL) {
            lira_rt_panic("string length is invalid");
        }
        lira_gc_unregister_root_slot(&parts);
        return parts;
    }
    if (delimiter != NULL && !lira_valid_string_length(delimiter)) {
        lira_rt_panic("string length is invalid");
    }
    /* An empty delimiter splits into characters, as in the VM. */
    if (delimiter == NULL || delimiter->len == 0) {
        for (int64_t offset = 0; offset < s->len;) {
            int width = lira_utf8_width(s->data + offset, s->len - offset);
            lira_rt_array_push(parts, (int64_t)(intptr_t)lira_rt_str_new(s->data + offset, width));
            offset += width;
        }
        lira_gc_unregister_root_slot(&parts);
        return parts;
    }
    int64_t start = 0;
    for (int64_t i = 0; i + delimiter->len <= s->len;) {
        if (memcmp(s->data + i, delimiter->data, (size_t)delimiter->len) == 0) {
            lira_rt_array_push(parts,
                               (int64_t)(intptr_t)lira_rt_str_new(s->data + start, i - start));
            i += delimiter->len;
            start = i;
        } else {
            i++;
        }
    }
    lira_rt_array_push(parts, (int64_t)(intptr_t)lira_rt_str_new(s->data + start, s->len - start));
    lira_gc_unregister_root_slot(&parts);
    return parts;
}

static int lira_is_space(unsigned char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

static LiraStr *lira_trim(const LiraStr *s, int from_start, int from_end) {
    if (!lira_valid_string_length(s)) {
        if (s != NULL) {
            lira_rt_panic("string length is invalid");
        }
        return lira_rt_str_new("", 0);
    }
    int64_t begin = 0;
    int64_t end = s->len;
    if (from_start) {
        while (begin < end && lira_is_space((unsigned char)s->data[begin])) {
            begin++;
        }
    }
    if (from_end) {
        while (end > begin && lira_is_space((unsigned char)s->data[end - 1])) {
            end--;
        }
    }
    return lira_rt_str_new(s->data + begin, end - begin);
}

LiraStr *lira_rt_str_trim(const LiraStr *s) { return lira_trim(s, 1, 1); }
LiraStr *lira_rt_str_trim_start(const LiraStr *s) { return lira_trim(s, 1, 0); }
LiraStr *lira_rt_str_trim_end(const LiraStr *s) { return lira_trim(s, 0, 1); }

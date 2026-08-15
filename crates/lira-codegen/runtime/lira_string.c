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

/* Byte length of the UTF-8 sequence starting at `b`. */
static int lira_utf8_width(unsigned char b) {
    if (b < 0x80) {
        return 1;
    }
    if ((b & 0xE0) == 0xC0) {
        return 2;
    }
    if ((b & 0xF0) == 0xE0) {
        return 3;
    }
    if ((b & 0xF8) == 0xF0) {
        return 4;
    }
    return 1; /* malformed: treat as a single byte so scanning terminates */
}

/* Decode the scalar value at `p`, writing its width to `width`. */
static int64_t lira_utf8_decode(const char *p, int *width) {
    unsigned char b = (unsigned char)*p;
    int n = lira_utf8_width(b);
    *width = n;
    switch (n) {
        case 1:
            return b;
        case 2:
            return ((int64_t)(b & 0x1F) << 6) | ((unsigned char)p[1] & 0x3F);
        case 3:
            return ((int64_t)(b & 0x0F) << 12) | (((unsigned char)p[1] & 0x3F) << 6) |
                   ((unsigned char)p[2] & 0x3F);
        default:
            return ((int64_t)(b & 0x07) << 18) | (((unsigned char)p[1] & 0x3F) << 12) |
                   (((unsigned char)p[2] & 0x3F) << 6) | ((unsigned char)p[3] & 0x3F);
    }
}

/* Byte offset of character `index`, or the string's length if it runs off the end. */
static int64_t lira_char_offset(const LiraStr *s, int64_t index) {
    int64_t offset = 0;
    int64_t seen = 0;
    while (offset < s->len && seen < index) {
        offset += lira_utf8_width((unsigned char)s->data[offset]);
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
    if (s == NULL || index < 0) {
        return -1;
    }
    int64_t offset = lira_char_offset(s, index);
    if (offset >= s->len) {
        return -1;
    }
    int width = 0;
    return lira_utf8_decode(s->data + offset, &width);
}

LiraStr *lira_rt_str_from_char_code(int64_t code) {
    if (code < 0 || code > 0x10FFFF) {
        return lira_rt_str_new("", 0);
    }
    char buf[4];
    int n = lira_encode_utf8(buf, code);
    return lira_rt_str_new(buf, n);
}

/* ASCII-only case mapping, matching what the VM does for ASCII input. Non-ASCII
 * scalars are passed through unchanged rather than half-cased. */
static LiraStr *lira_map_ascii_case(const LiraStr *s, int to_upper) {
    if (s == NULL) {
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
    if (s == NULL) {
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
    if (s == NULL || needle == NULL) {
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
                o += lira_utf8_width((unsigned char)s->data[o]);
                chars++;
            }
            return chars;
        }
    }
    return -1;
}

LiraArray *lira_rt_str_split(const LiraStr *s, const LiraStr *delimiter) {
    LiraArray *parts = lira_rt_array_new(0);
    if (s == NULL) {
        return parts;
    }
    /* An empty delimiter splits into characters, as in the VM. */
    if (delimiter == NULL || delimiter->len == 0) {
        for (int64_t offset = 0; offset < s->len;) {
            int width = lira_utf8_width((unsigned char)s->data[offset]);
            lira_rt_array_push(parts, (int64_t)(intptr_t)lira_rt_str_new(s->data + offset, width));
            offset += width;
        }
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
    return parts;
}

static int lira_is_space(unsigned char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

static LiraStr *lira_trim(const LiraStr *s, int from_start, int from_end) {
    if (s == NULL) {
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

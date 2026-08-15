/*
 * Encoding and hashing built-ins: base64, URL escaping, MD5, SHA-1, SHA-2 and
 * UUIDs.
 *
 * The hashes are compact reference implementations. They exist so the native
 * backend matches the bytecode VM, which reaches for the `md5` and `sha2`
 * crates; the digests are byte-for-byte identical, which the parity tests check.
 */
#include "lira_rt.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void lira_rt_panic(const char *message);
uint64_t lira_rt_random_bits(void);
int64_t lira_rt_time_ms(void);

/* ------------------------------------------------------------------ */
/* base64                                                              */
/* ------------------------------------------------------------------ */

static const char BASE64_STANDARD[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static const char BASE64_URL[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

static LiraStr *lira_base64_encode(const LiraStr *input, const char *alphabet, int pad) {
    if (input == NULL) {
        return lira_rt_str_new("", 0);
    }
    int64_t groups = (input->len + 2) / 3;
    int64_t capacity = groups * 4;
    char *out = (char *)malloc((size_t)capacity + 1);
    if (out == NULL) {
        lira_rt_panic("out of memory");
    }

    int64_t written = 0;
    for (int64_t i = 0; i < input->len; i += 3) {
        int64_t remaining = input->len - i;
        uint32_t chunk = (uint32_t)(unsigned char)input->data[i] << 16;
        if (remaining > 1) {
            chunk |= (uint32_t)(unsigned char)input->data[i + 1] << 8;
        }
        if (remaining > 2) {
            chunk |= (uint32_t)(unsigned char)input->data[i + 2];
        }
        out[written++] = alphabet[(chunk >> 18) & 0x3F];
        out[written++] = alphabet[(chunk >> 12) & 0x3F];
        if (remaining > 1) {
            out[written++] = alphabet[(chunk >> 6) & 0x3F];
        } else if (pad) {
            out[written++] = '=';
        }
        if (remaining > 2) {
            out[written++] = alphabet[chunk & 0x3F];
        } else if (pad) {
            out[written++] = '=';
        }
    }
    LiraStr *result = lira_rt_str_new(out, written);
    free(out);
    return result;
}

static int lira_base64_value(char c, const char *alphabet) {
    const char *found = memchr(alphabet, c, 64);
    return found != NULL ? (int)(found - alphabet) : -1;
}

static LiraStr *lira_base64_decode(const LiraStr *input, const char *alphabet) {
    if (input == NULL) {
        return lira_rt_str_new("", 0);
    }
    char *out = (char *)malloc((size_t)input->len + 1);
    if (out == NULL) {
        lira_rt_panic("out of memory");
    }
    int64_t written = 0;
    uint32_t accumulator = 0;
    int bits = 0;
    for (int64_t i = 0; i < input->len; i++) {
        char c = input->data[i];
        if (c == '=' || c == '\n' || c == '\r') {
            continue;
        }
        int value = lira_base64_value(c, alphabet);
        if (value < 0) {
            continue; /* skip anything outside the alphabet, as the VM does */
        }
        accumulator = (accumulator << 6) | (uint32_t)value;
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out[written++] = (char)((accumulator >> bits) & 0xFF);
        }
    }
    LiraStr *result = lira_rt_str_new(out, written);
    free(out);
    return result;
}

LiraStr *lira_rt_base64_encode(const LiraStr *s) {
    return lira_base64_encode(s, BASE64_STANDARD, 1);
}
LiraStr *lira_rt_base64_decode(const LiraStr *s) {
    return lira_base64_decode(s, BASE64_STANDARD);
}
LiraStr *lira_rt_base64_encode_url(const LiraStr *s) {
    return lira_base64_encode(s, BASE64_URL, 0);
}
LiraStr *lira_rt_base64_decode_url(const LiraStr *s) {
    return lira_base64_decode(s, BASE64_URL);
}

/* ------------------------------------------------------------------ */
/* URL escaping                                                        */
/* ------------------------------------------------------------------ */

static int lira_url_unreserved(unsigned char c) {
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-' ||
           c == '_' || c == '.' || c == '~';
}

LiraStr *lira_rt_url_encode(const LiraStr *s) {
    if (s == NULL) {
        return lira_rt_str_new("", 0);
    }
    char *out = (char *)malloc((size_t)s->len * 3 + 1);
    if (out == NULL) {
        lira_rt_panic("out of memory");
    }
    int64_t written = 0;
    for (int64_t i = 0; i < s->len; i++) {
        unsigned char c = (unsigned char)s->data[i];
        if (lira_url_unreserved(c)) {
            out[written++] = (char)c;
        } else if (c == ' ') {
            /* Form encoding, matching the bytecode VM. `url_decode` maps it back. */
            out[written++] = '+';
        } else {
            written += snprintf(out + written, 4, "%%%02X", c);
        }
    }
    LiraStr *result = lira_rt_str_new(out, written);
    free(out);
    return result;
}

static int lira_hex_value(char c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

LiraStr *lira_rt_url_decode(const LiraStr *s) {
    if (s == NULL) {
        return lira_rt_str_new("", 0);
    }
    char *out = (char *)malloc((size_t)s->len + 1);
    if (out == NULL) {
        lira_rt_panic("out of memory");
    }
    int64_t written = 0;
    for (int64_t i = 0; i < s->len; i++) {
        char c = s->data[i];
        if (c == '%' && i + 2 < s->len) {
            int hi = lira_hex_value(s->data[i + 1]);
            int lo = lira_hex_value(s->data[i + 2]);
            if (hi >= 0 && lo >= 0) {
                out[written++] = (char)((hi << 4) | lo);
                i += 2;
                continue;
            }
        }
        out[written++] = c == '+' ? ' ' : c;
    }
    LiraStr *result = lira_rt_str_new(out, written);
    free(out);
    return result;
}

/* ------------------------------------------------------------------ */
/* Hex output                                                          */
/* ------------------------------------------------------------------ */

static LiraStr *lira_to_hex(const unsigned char *bytes, int count) {
    char buf[129];
    for (int i = 0; i < count; i++) {
        snprintf(buf + i * 2, 3, "%02x", bytes[i]);
    }
    return lira_rt_str_new(buf, count * 2);
}

/* ------------------------------------------------------------------ */
/* MD5 (RFC 1321)                                                      */
/* ------------------------------------------------------------------ */

static uint32_t lira_rotl32(uint32_t x, int c) { return (x << c) | (x >> (32 - c)); }

static void lira_md5(const unsigned char *message, int64_t len, unsigned char digest[16]) {
    static const uint32_t K[64] = {
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391};
    static const int S[64] = {7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
                              5, 9,  14, 20, 5, 9,  14, 20, 5, 9,  14, 20, 5, 9,  14, 20,
                              4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
                              6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21};

    uint32_t h[4] = {0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476};
    int64_t padded = ((len + 8) / 64 + 1) * 64;
    unsigned char *buffer = (unsigned char *)calloc(1, (size_t)padded);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    memcpy(buffer, message, (size_t)len);
    buffer[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) {
        buffer[padded - 8 + i] = (unsigned char)(bits >> (8 * i));
    }

    for (int64_t offset = 0; offset < padded; offset += 64) {
        uint32_t m[16];
        for (int i = 0; i < 16; i++) {
            const unsigned char *p = buffer + offset + i * 4;
            m[i] = (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) |
                   ((uint32_t)p[3] << 24);
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3];
        for (int i = 0; i < 64; i++) {
            uint32_t f;
            int g;
            if (i < 16) {
                f = (b & c) | (~b & d);
                g = i;
            } else if (i < 32) {
                f = (d & b) | (~d & c);
                g = (5 * i + 1) % 16;
            } else if (i < 48) {
                f = b ^ c ^ d;
                g = (3 * i + 5) % 16;
            } else {
                f = c ^ (b | ~d);
                g = (7 * i) % 16;
            }
            uint32_t temp = d;
            d = c;
            c = b;
            b = b + lira_rotl32(a + f + K[i] + m[g], S[i]);
            a = temp;
        }
        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
    }
    free(buffer);
    for (int i = 0; i < 4; i++) {
        for (int j = 0; j < 4; j++) {
            digest[i * 4 + j] = (unsigned char)(h[i] >> (8 * j));
        }
    }
}

LiraStr *lira_rt_md5(const LiraStr *s) {
    unsigned char digest[16];
    lira_md5(s ? (const unsigned char *)s->data : (const unsigned char *)"", s ? s->len : 0,
             digest);
    return lira_to_hex(digest, 16);
}

/* ------------------------------------------------------------------ */
/* SHA-1 (FIPS 180-4)                                                  */
/* ------------------------------------------------------------------ */

static void lira_sha1(const unsigned char *message, int64_t len, unsigned char digest[20]) {
    uint32_t h[5] = {0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0};
    int64_t padded = ((len + 8) / 64 + 1) * 64;
    unsigned char *buffer = (unsigned char *)calloc(1, (size_t)padded);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    memcpy(buffer, message, (size_t)len);
    buffer[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) {
        buffer[padded - 1 - i] = (unsigned char)(bits >> (8 * i));
    }

    for (int64_t offset = 0; offset < padded; offset += 64) {
        uint32_t w[80];
        for (int i = 0; i < 16; i++) {
            const unsigned char *p = buffer + offset + i * 4;
            w[i] = ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) |
                   (uint32_t)p[3];
        }
        for (int i = 16; i < 80; i++) {
            w[i] = lira_rotl32(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4];
        for (int i = 0; i < 80; i++) {
            uint32_t f, k;
            if (i < 20) {
                f = (b & c) | (~b & d);
                k = 0x5A827999;
            } else if (i < 40) {
                f = b ^ c ^ d;
                k = 0x6ED9EBA1;
            } else if (i < 60) {
                f = (b & c) | (b & d) | (c & d);
                k = 0x8F1BBCDC;
            } else {
                f = b ^ c ^ d;
                k = 0xCA62C1D6;
            }
            uint32_t temp = lira_rotl32(a, 5) + f + e + k + w[i];
            e = d;
            d = c;
            c = lira_rotl32(b, 30);
            b = a;
            a = temp;
        }
        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
    }
    free(buffer);
    for (int i = 0; i < 5; i++) {
        for (int j = 0; j < 4; j++) {
            digest[i * 4 + j] = (unsigned char)(h[i] >> (24 - 8 * j));
        }
    }
}

LiraStr *lira_rt_sha1(const LiraStr *s) {
    unsigned char digest[20];
    lira_sha1(s ? (const unsigned char *)s->data : (const unsigned char *)"", s ? s->len : 0,
              digest);
    return lira_to_hex(digest, 20);
}

/* ------------------------------------------------------------------ */
/* SHA-256 and SHA-512 (FIPS 180-4)                                    */
/* ------------------------------------------------------------------ */

static uint32_t lira_rotr32(uint32_t x, int c) { return (x >> c) | (x << (32 - c)); }
static uint64_t lira_rotr64(uint64_t x, int c) { return (x >> c) | (x << (64 - c)); }

static void lira_sha256(const unsigned char *message, int64_t len, unsigned char digest[32]) {
    static const uint32_t K[64] = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2};

    uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                     0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    int64_t padded = ((len + 8) / 64 + 1) * 64;
    unsigned char *buffer = (unsigned char *)calloc(1, (size_t)padded);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    memcpy(buffer, message, (size_t)len);
    buffer[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) {
        buffer[padded - 1 - i] = (unsigned char)(bits >> (8 * i));
    }

    for (int64_t offset = 0; offset < padded; offset += 64) {
        uint32_t w[64];
        for (int i = 0; i < 16; i++) {
            const unsigned char *p = buffer + offset + i * 4;
            w[i] = ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) |
                   (uint32_t)p[3];
        }
        for (int i = 16; i < 64; i++) {
            uint32_t s0 = lira_rotr32(w[i - 15], 7) ^ lira_rotr32(w[i - 15], 18) ^ (w[i - 15] >> 3);
            uint32_t s1 = lira_rotr32(w[i - 2], 17) ^ lira_rotr32(w[i - 2], 19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3];
        uint32_t e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 64; i++) {
            uint32_t s1 = lira_rotr32(e, 6) ^ lira_rotr32(e, 11) ^ lira_rotr32(e, 25);
            uint32_t ch = (e & f) ^ (~e & g);
            uint32_t t1 = hh + s1 + ch + K[i] + w[i];
            uint32_t s0 = lira_rotr32(a, 2) ^ lira_rotr32(a, 13) ^ lira_rotr32(a, 22);
            uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
            uint32_t t2 = s0 + maj;
            hh = g;
            g = f;
            f = e;
            e = d + t1;
            d = c;
            c = b;
            b = a;
            a = t1 + t2;
        }
        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
        h[5] += f;
        h[6] += g;
        h[7] += hh;
    }
    free(buffer);
    for (int i = 0; i < 8; i++) {
        for (int j = 0; j < 4; j++) {
            digest[i * 4 + j] = (unsigned char)(h[i] >> (24 - 8 * j));
        }
    }
}

LiraStr *lira_rt_sha256(const LiraStr *s) {
    unsigned char digest[32];
    lira_sha256(s ? (const unsigned char *)s->data : (const unsigned char *)"", s ? s->len : 0,
                digest);
    return lira_to_hex(digest, 32);
}

static void lira_sha512(const unsigned char *message, int64_t len, unsigned char digest[64]) {
    static const uint64_t K[80] = {
        0x428a2f98d728ae22ULL, 0x7137449123ef65cdULL, 0xb5c0fbcfec4d3b2fULL, 0xe9b5dba58189dbbcULL,
        0x3956c25bf348b538ULL, 0x59f111f1b605d019ULL, 0x923f82a4af194f9bULL, 0xab1c5ed5da6d8118ULL,
        0xd807aa98a3030242ULL, 0x12835b0145706fbeULL, 0x243185be4ee4b28cULL, 0x550c7dc3d5ffb4e2ULL,
        0x72be5d74f27b896fULL, 0x80deb1fe3b1696b1ULL, 0x9bdc06a725c71235ULL, 0xc19bf174cf692694ULL,
        0xe49b69c19ef14ad2ULL, 0xefbe4786384f25e3ULL, 0x0fc19dc68b8cd5b5ULL, 0x240ca1cc77ac9c65ULL,
        0x2de92c6f592b0275ULL, 0x4a7484aa6ea6e483ULL, 0x5cb0a9dcbd41fbd4ULL, 0x76f988da831153b5ULL,
        0x983e5152ee66dfabULL, 0xa831c66d2db43210ULL, 0xb00327c898fb213fULL, 0xbf597fc7beef0ee4ULL,
        0xc6e00bf33da88fc2ULL, 0xd5a79147930aa725ULL, 0x06ca6351e003826fULL, 0x142929670a0e6e70ULL,
        0x27b70a8546d22ffcULL, 0x2e1b21385c26c926ULL, 0x4d2c6dfc5ac42aedULL, 0x53380d139d95b3dfULL,
        0x650a73548baf63deULL, 0x766a0abb3c77b2a8ULL, 0x81c2c92e47edaee6ULL, 0x92722c851482353bULL,
        0xa2bfe8a14cf10364ULL, 0xa81a664bbc423001ULL, 0xc24b8b70d0f89791ULL, 0xc76c51a30654be30ULL,
        0xd192e819d6ef5218ULL, 0xd69906245565a910ULL, 0xf40e35855771202aULL, 0x106aa07032bbd1b8ULL,
        0x19a4c116b8d2d0c8ULL, 0x1e376c085141ab53ULL, 0x2748774cdf8eeb99ULL, 0x34b0bcb5e19b48a8ULL,
        0x391c0cb3c5c95a63ULL, 0x4ed8aa4ae3418acbULL, 0x5b9cca4f7763e373ULL, 0x682e6ff3d6b2b8a3ULL,
        0x748f82ee5defb2fcULL, 0x78a5636f43172f60ULL, 0x84c87814a1f0ab72ULL, 0x8cc702081a6439ecULL,
        0x90befffa23631e28ULL, 0xa4506cebde82bde9ULL, 0xbef9a3f7b2c67915ULL, 0xc67178f2e372532bULL,
        0xca273eceea26619cULL, 0xd186b8c721c0c207ULL, 0xeada7dd6cde0eb1eULL, 0xf57d4f7fee6ed178ULL,
        0x06f067aa72176fbaULL, 0x0a637dc5a2c898a6ULL, 0x113f9804bef90daeULL, 0x1b710b35131c471bULL,
        0x28db77f523047d84ULL, 0x32caab7b40c72493ULL, 0x3c9ebe0a15c9bebcULL, 0x431d67c49c100d4cULL,
        0x4cc5d4becb3e42b6ULL, 0x597f299cfc657e2aULL, 0x5fcb6fab3ad6faecULL, 0x6c44198c4a475817ULL};

    uint64_t h[8] = {0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL, 0x3c6ef372fe94f82bULL,
                     0xa54ff53a5f1d36f1ULL, 0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
                     0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL};
    int64_t padded = ((len + 16) / 128 + 1) * 128;
    unsigned char *buffer = (unsigned char *)calloc(1, (size_t)padded);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    memcpy(buffer, message, (size_t)len);
    buffer[len] = 0x80;
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 0; i < 8; i++) {
        buffer[padded - 1 - i] = (unsigned char)(bits >> (8 * i));
    }

    for (int64_t offset = 0; offset < padded; offset += 128) {
        uint64_t w[80];
        for (int i = 0; i < 16; i++) {
            const unsigned char *p = buffer + offset + i * 8;
            w[i] = 0;
            for (int j = 0; j < 8; j++) {
                w[i] = (w[i] << 8) | (uint64_t)p[j];
            }
        }
        for (int i = 16; i < 80; i++) {
            uint64_t s0 = lira_rotr64(w[i - 15], 1) ^ lira_rotr64(w[i - 15], 8) ^ (w[i - 15] >> 7);
            uint64_t s1 = lira_rotr64(w[i - 2], 19) ^ lira_rotr64(w[i - 2], 61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint64_t a = h[0], b = h[1], c = h[2], d = h[3];
        uint64_t e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 80; i++) {
            uint64_t s1 = lira_rotr64(e, 14) ^ lira_rotr64(e, 18) ^ lira_rotr64(e, 41);
            uint64_t ch = (e & f) ^ (~e & g);
            uint64_t t1 = hh + s1 + ch + K[i] + w[i];
            uint64_t s0 = lira_rotr64(a, 28) ^ lira_rotr64(a, 34) ^ lira_rotr64(a, 39);
            uint64_t maj = (a & b) ^ (a & c) ^ (b & c);
            uint64_t t2 = s0 + maj;
            hh = g;
            g = f;
            f = e;
            e = d + t1;
            d = c;
            c = b;
            b = a;
            a = t1 + t2;
        }
        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
        h[5] += f;
        h[6] += g;
        h[7] += hh;
    }
    free(buffer);
    for (int i = 0; i < 8; i++) {
        for (int j = 0; j < 8; j++) {
            digest[i * 8 + j] = (unsigned char)(h[i] >> (56 - 8 * j));
        }
    }
}

LiraStr *lira_rt_sha512(const LiraStr *s) {
    unsigned char digest[64];
    lira_sha512(s ? (const unsigned char *)s->data : (const unsigned char *)"", s ? s->len : 0,
                digest);
    return lira_to_hex(digest, 64);
}

/* ------------------------------------------------------------------ */
/* UUID                                                                */
/* ------------------------------------------------------------------ */

static LiraStr *lira_uuid_format(const unsigned char b[16]) {
    char buf[37];
    snprintf(buf, sizeof(buf),
             "%02x%02x%02x%02x-%02x%02x-%02x%02x-%02x%02x-%02x%02x%02x%02x%02x%02x", b[0], b[1],
             b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14],
             b[15]);
    return lira_rt_str_new(buf, 36);
}

LiraStr *lira_rt_uuid_v4(void) {
    unsigned char b[16];
    for (int i = 0; i < 16; i += 8) {
        uint64_t bits = lira_rt_random_bits();
        for (int j = 0; j < 8; j++) {
            b[i + j] = (unsigned char)(bits >> (8 * j));
        }
    }
    b[6] = (unsigned char)((b[6] & 0x0F) | 0x40); /* version 4 */
    b[8] = (unsigned char)((b[8] & 0x3F) | 0x80); /* RFC 4122 variant */
    return lira_uuid_format(b);
}

LiraStr *lira_rt_uuid_v7(void) {
    unsigned char b[16];
    /* 48-bit big-endian millisecond timestamp, then randomness. */
    uint64_t millis = (uint64_t)lira_rt_time_ms();
    for (int i = 0; i < 6; i++) {
        b[i] = (unsigned char)(millis >> (40 - 8 * i));
    }
    uint64_t bits = lira_rt_random_bits();
    for (int i = 6; i < 14; i++) {
        b[i] = (unsigned char)(bits >> (8 * (i - 6)));
    }
    uint64_t more = lira_rt_random_bits();
    b[14] = (unsigned char)more;
    b[15] = (unsigned char)(more >> 8);
    b[6] = (unsigned char)((b[6] & 0x0F) | 0x70); /* version 7 */
    b[8] = (unsigned char)((b[8] & 0x3F) | 0x80);
    return lira_uuid_format(b);
}

LiraStr *lira_rt_uuid_nil(void) {
    return lira_rt_str_new("00000000-0000-0000-0000-000000000000", 36);
}

int8_t lira_rt_uuid_is_valid(const LiraStr *s) {
    if (s == NULL || s->len != 36) {
        return 0;
    }
    for (int i = 0; i < 36; i++) {
        char c = s->data[i];
        if (i == 8 || i == 13 || i == 18 || i == 23) {
            if (c != '-') {
                return 0;
            }
        } else if (lira_hex_value(c) < 0) {
            return 0;
        }
    }
    return 1;
}

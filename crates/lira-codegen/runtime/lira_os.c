/*
 * Time, randomness, environment, files and the filesystem.
 *
 * File handles are small integers indexed into a table owned by the runtime,
 * matching the bytecode VM's model rather than exposing raw file descriptors.
 */
#include "lira_rt.h"

#include <dirent.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

void lira_rt_panic(const char *message);

/* ------------------------------------------------------------------ */
/* Time                                                                */
/* ------------------------------------------------------------------ */

static int64_t lira_now_nanos(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + (int64_t)ts.tv_nsec;
}

int64_t lira_rt_time_nanos(void) { return lira_now_nanos(); }
int64_t lira_rt_time_micros(void) { return lira_now_nanos() / 1000; }
int64_t lira_rt_time_ms(void) { return lira_now_nanos() / 1000000; }
int64_t lira_rt_time_secs(void) { return lira_now_nanos() / 1000000000; }

void lira_rt_sleep(int64_t millis) {
    if (millis <= 0) {
        return;
    }
    /* A running native fiber parks on the bounded I/O pool.  Calls made by an
     * embedding thread (outside lira_rt_boot) retain the old synchronous
     * behaviour because there is no fiber to resume. */
    int8_t parked = lira_rt_io_sleep(millis);
    if (parked > 0) {
        return;
    }
    if (parked < 0) {
        lira_rt_panic("I/O worker pool is unavailable or full");
        return;
    }
    struct timespec ts;
    ts.tv_sec = (time_t)(millis / 1000);
    ts.tv_nsec = (long)((millis % 1000) * 1000000);
    while (nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
}

LiraStr *lira_rt_time_format_iso(int64_t millis) {
    time_t seconds = (time_t)(millis / 1000);
    struct tm utc;
    if (gmtime_r(&seconds, &utc) == NULL) {
        return lira_rt_str_new("", 0);
    }
    char buf[64];
    /* The bytecode VM renders the offset explicitly rather than as "Z". */
    int n = snprintf(buf, sizeof(buf), "%04d-%02d-%02dT%02d:%02d:%02d.%03d+00:00",
                     utc.tm_year + 1900, utc.tm_mon + 1, utc.tm_mday, utc.tm_hour, utc.tm_min,
                     utc.tm_sec, (int)(millis % 1000));
    return lira_rt_str_new(buf, n);
}

int64_t lira_rt_time_parse_iso(const LiraStr *text) {
    if (text == NULL) {
        return 0;
    }
    int year = 0, month = 0, day = 0, hour = 0, minute = 0, second = 0, millis = 0;
    int matched = sscanf(text->data, "%d-%d-%dT%d:%d:%d.%dZ", &year, &month, &day, &hour, &minute,
                         &second, &millis);
    if (matched < 6) {
        return 0;
    }
    struct tm utc;
    memset(&utc, 0, sizeof(utc));
    utc.tm_year = year - 1900;
    utc.tm_mon = month - 1;
    utc.tm_mday = day;
    utc.tm_hour = hour;
    utc.tm_min = minute;
    utc.tm_sec = second;
    time_t seconds = timegm(&utc);
    if (seconds == (time_t)-1) {
        return 0;
    }
    return (int64_t)seconds * 1000 + (matched >= 7 ? millis : 0);
}

/* [year, month, day, hour, minute, second] in UTC. */
LiraArray *lira_rt_time_components(int64_t millis) {
    LiraArray *parts = lira_rt_array_new(6);
    time_t seconds = (time_t)(millis / 1000);
    struct tm utc;
    if (gmtime_r(&seconds, &utc) == NULL) {
        for (int i = 0; i < 6; i++) {
            lira_rt_array_push(parts, 0);
        }
        return parts;
    }
    lira_rt_array_push(parts, utc.tm_year + 1900);
    lira_rt_array_push(parts, utc.tm_mon + 1);
    lira_rt_array_push(parts, utc.tm_mday);
    lira_rt_array_push(parts, utc.tm_hour);
    lira_rt_array_push(parts, utc.tm_min);
    lira_rt_array_push(parts, utc.tm_sec);
    return parts;
}

int64_t lira_rt_time_from_components(int64_t year, int64_t month, int64_t day, int64_t hour,
                                     int64_t minute, int64_t second) {
    struct tm utc;
    memset(&utc, 0, sizeof(utc));
    /* Clamp component values into the platform `struct tm` (int) ranges so the
     * signed `year - 1900` / `month - 1` subtraction and the `(int)` casts
     * cannot overflow (that would be UB for extreme `year` inputs). Values
     * outside the representable range fail closed (return 0), matching a
     * `timegm` failure on an out-of-range date. */
    if (year < INT64_MIN + 1900 || year > (int64_t)INT_MAX + 1900) {
        return 0;
    }
    int64_t month_shift = month - 1;
    if (month < INT64_MIN + 1 || month > (int64_t)INT_MAX + 1) {
        return 0;
    }
    if (day > INT_MAX || day < INT_MIN || hour > INT_MAX || hour < INT_MIN ||
        minute > INT_MAX || minute < INT_MIN || second > INT_MAX || second < INT_MIN) {
        return 0;
    }
    utc.tm_year = (int)(year - 1900);
    utc.tm_mon = (int)(month_shift);
    utc.tm_mday = (int)day;
    utc.tm_hour = (int)hour;
    utc.tm_min = (int)minute;
    utc.tm_sec = (int)second;
    time_t seconds = timegm(&utc);
    return seconds == (time_t)-1 ? 0 : (int64_t)seconds * 1000;
}

/* strftime with the caller's format, on the UTC breakdown of `millis`. */
LiraStr *lira_rt_time_format(int64_t millis, const LiraStr *format) {
    if (format == NULL) {
        return lira_rt_str_new("", 0);
    }
    time_t seconds = (time_t)(millis / 1000);
    struct tm utc;
    if (gmtime_r(&seconds, &utc) == NULL) {
        return lira_rt_str_new("", 0);
    }
    char buf[256];
    size_t n = strftime(buf, sizeof(buf), format->data, &utc);
    return lira_rt_str_new(buf, (int64_t)n);
}

int64_t lira_rt_time_timezone_offset(void) {
    time_t now = time(NULL);
    struct tm local;
    struct tm utc;
    if (localtime_r(&now, &local) == NULL || gmtime_r(&now, &utc) == NULL) {
        return 0;
    }
    return (int64_t)(timegm(&local) - timegm(&utc)) / 60;
}

/* ------------------------------------------------------------------ */
/* Random                                                              */
/* ------------------------------------------------------------------ */

/* xoshiro256++, seeded once from the clock and the process id. Deterministic
 * within a run and cheap; not cryptographic. */
static uint64_t g_rng_state[4];
static int g_rng_ready = 0;

static uint64_t lira_splitmix64(uint64_t *seed) {
    uint64_t z = (*seed += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static void lira_rng_seed(void) {
    uint64_t seed = (uint64_t)lira_now_nanos() ^ ((uint64_t)getpid() << 32);
    for (int i = 0; i < 4; i++) {
        g_rng_state[i] = lira_splitmix64(&seed);
    }
    g_rng_ready = 1;
}

static uint64_t lira_rotl(uint64_t x, int k) { return (x << k) | (x >> (64 - k)); }

uint64_t lira_rt_random_bits(void) {
    if (!g_rng_ready) {
        lira_rng_seed();
    }
    uint64_t result = lira_rotl(g_rng_state[0] + g_rng_state[3], 23) + g_rng_state[0];
    uint64_t t = g_rng_state[1] << 17;
    g_rng_state[2] ^= g_rng_state[0];
    g_rng_state[3] ^= g_rng_state[1];
    g_rng_state[1] ^= g_rng_state[2];
    g_rng_state[0] ^= g_rng_state[3];
    g_rng_state[2] ^= t;
    g_rng_state[3] = lira_rotl(g_rng_state[3], 45);
    return result;
}

double lira_rt_random(void) {
    /* 53 significant bits, the most a double represents exactly. */
    return (double)(lira_rt_random_bits() >> 11) * (1.0 / 9007199254740992.0);
}

int64_t lira_rt_random_int(int64_t low, int64_t high) {
    if (high <= low) {
        return low;
    }
    uint64_t lo = (uint64_t)low;
    uint64_t hi = (uint64_t)high;
    /* Inclusive span. hi > lo here, so hi - lo is in [1, UINT64_MAX]; adding
     * one wraps to 0 exactly when the range is the full 2^64-domain, in which
     * case every random value is in range. All arithmetic is unsigned, so no
     * signed-overflow UB can occur for any (low, high) pair. */
    uint64_t span = hi - lo + 1;
    uint64_t bits = lira_rt_random_bits();
    if (span == 0) {
        return (int64_t)(lo + bits);
    }
    return (int64_t)(lo + bits % span);
}

/* ------------------------------------------------------------------ */
/* Environment                                                         */
/* ------------------------------------------------------------------ */

/* argv, captured by `main` so `env_args` can report it. */
static int g_argc = 0;
static char **g_argv = NULL;

void lira_rt_set_args(int argc, char **argv) {
    g_argc = argc;
    g_argv = argv;
}

/* Declared `string?` by the checker, so an unset variable is null rather than
 * an empty string — a program can tell "unset" from "set to nothing". */
LiraStr *lira_rt_env_get(const LiraStr *name) {
    if (name == NULL) {
        return NULL;
    }
    const char *value = getenv(name->data);
    return value ? lira_rt_str_new(value, (int64_t)strlen(value)) : NULL;
}

int8_t lira_rt_env_set(const LiraStr *name, const LiraStr *value) {
    if (name == NULL || value == NULL) {
        return 0;
    }
    return setenv(name->data, value->data, 1) == 0 ? 1 : 0;
}

int8_t lira_rt_env_remove(const LiraStr *name) {
    return name != NULL && unsetenv(name->data) == 0 ? 1 : 0;
}

int8_t lira_rt_env_has(const LiraStr *name) {
    return name != NULL && getenv(name->data) != NULL ? 1 : 0;
}

LiraArray *lira_rt_env_args(void) {
    LiraArray *args = lira_rt_array_new(g_argc > 0 ? g_argc : 0);
    for (int i = 0; i < g_argc; i++) {
        lira_rt_array_push(
            args, (int64_t)(intptr_t)lira_rt_str_new(g_argv[i], (int64_t)strlen(g_argv[i])));
    }
    return args;
}

/* `keys_only` picks between "NAME" and "NAME=value" entries. */
static LiraArray *lira_env_list(int keys_only) {
    LiraArray *out = lira_rt_array_new(0);
    for (char **entry = environ; entry != NULL && *entry != NULL; entry++) {
        const char *text = *entry;
        int64_t len = (int64_t)strlen(text);
        if (keys_only) {
            const char *eq = strchr(text, '=');
            len = eq != NULL ? (int64_t)(eq - text) : len;
        }
        lira_rt_array_push(out, (int64_t)(intptr_t)lira_rt_str_new(text, len));
    }
    return out;
}

LiraArray *lira_rt_env_all(void) { return lira_env_list(0); }
LiraArray *lira_rt_env_keys(void) { return lira_env_list(1); }

LiraStr *lira_rt_env_exe(void) {
    char buf[4096];
#if defined(__APPLE__)
    /* _NSGetExecutablePath needs a header we would rather not pull in here;
     * argv[0] is what the VM reports on this platform anyway. */
    if (g_argc > 0) {
        return lira_rt_str_new(g_argv[0], (int64_t)strlen(g_argv[0]));
    }
    (void)buf;
    return lira_rt_str_new("", 0);
#else
    ssize_t n = readlink("/proc/self/exe", buf, sizeof(buf) - 1);
    if (n > 0) {
        return lira_rt_str_new(buf, n);
    }
    if (g_argc > 0) {
        return lira_rt_str_new(g_argv[0], (int64_t)strlen(g_argv[0]));
    }
    return lira_rt_str_new("", 0);
#endif
}

static LiraStr *lira_env_or(const char *name, const char *fallback) {
    const char *value = getenv(name);
    if (value == NULL || *value == '\0') {
        value = fallback;
    }
    return lira_rt_str_new(value, (int64_t)strlen(value));
}

LiraStr *lira_rt_env_temp_dir(void) { return lira_env_or("TMPDIR", "/tmp"); }
LiraStr *lira_rt_env_home_dir(void) { return lira_env_or("HOME", ""); }

/* ------------------------------------------------------------------ */
/* Files                                                               */
/* ------------------------------------------------------------------ */

/* Small integer handles into a table, as the VM does — a Lira program never
 * sees a raw file descriptor. Numbering starts at 10 for the same reason the VM
 * does it: so a handle is never mistaken for stdin, stdout or stderr. */
#define LIRA_FIRST_FILE_HANDLE 10
#define LIRA_MAX_FILES 256

/* Handles are never reused, matching the VM's monotonic counter: closing a file
 * and opening another gives a different number. */
typedef struct {
    int64_t handle;
    FILE *file;
    int8_t busy;
} LiraOpenFile;

static LiraOpenFile g_files[LIRA_MAX_FILES];
static int64_t g_next_file_handle = LIRA_FIRST_FILE_HANDLE;

typedef struct {
    char *path;
    int64_t mode;
    int64_t *slot;
} FileOpenArg;
typedef struct {
    int64_t *slot;
    FILE *file;
    int owns_resource;
} FileOpenResult;

static void destroy_file_open_result(void *ptr) {
    FileOpenResult *result = (FileOpenResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->file != NULL) {
            fclose(result->file);
        }
        lira_rt_mem_free(result);
    }
}

static const char *file_mode(int64_t mode) {
    switch (mode) {
        case 1: return "wb";
        case 2: return "ab";
        case 3: return "r+b";
        default: return "rb";
    }
}

static void destroy_file_open_arg(void *ptr) {
    FileOpenArg *arg = (FileOpenArg *)ptr;
    if (arg != NULL) {
        lira_rt_mem_free(arg->path);
        lira_rt_mem_free(arg);
    }
}

static int file_open_work(void *ptr, void **out) {
    FileOpenArg *arg = (FileOpenArg *)ptr;
    FileOpenResult *result = (FileOpenResult *)lira_rt_mem_try_alloc(sizeof(FileOpenResult), 1);
    if (result == NULL) return -1;
    result->slot = arg->slot;
    result->file = fopen(arg->path, file_mode(arg->mode));
    result->owns_resource = result->file != NULL;
    *out = result;
    return 0;
}

static void file_open_complete(void *owner, uint64_t generation, void *ptr, int status,
                               void *failure_arg) {
    (void)failure_arg;
    if (status != 0) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    FileOpenResult *result = (FileOpenResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    int64_t handle = -1;
    if (status == 0 && result->file != NULL) {
        for (int i = 0; i < LIRA_MAX_FILES; i++) {
            if (g_files[i].file == NULL) {
                if (g_next_file_handle >= INT64_MAX) {
                    break;
                }
                g_files[i].file = result->file;
                g_files[i].busy = 0;
                g_files[i].handle = g_next_file_handle++;
                handle = g_files[i].handle;
                result->file = NULL;
                result->owns_resource = 0;
                break;
            }
        }
    }
    if (result->file != NULL) {
        fclose(result->file);
        result->file = NULL;
        result->owns_resource = 0;
    }
    *result->slot = handle;
    lira_rt_io_wake(owner, generation, 0);
}

int64_t lira_rt_file_open(const LiraStr *path, int64_t mode) {
    if (path == NULL || path->len < 0 || (uint64_t)path->len > SIZE_MAX - 1) return -1;
    FileOpenArg *arg = (FileOpenArg *)lira_rt_mem_try_alloc(sizeof(FileOpenArg), 1);
    if (arg == NULL) return -1;
    arg->path = (char *)lira_rt_mem_try_alloc((size_t)path->len + 1, 0);
    if (arg->path == NULL) { destroy_file_open_arg(arg); return -1; }
    memcpy(arg->path, path->data, (size_t)path->len);
    arg->path[path->len] = '\0';
    arg->mode = mode;
    int64_t result = -1;
    arg->slot = &result;
    int8_t parked = lira_rt_io_submit_current(file_open_work, arg,
                                               destroy_file_open_arg,
                                               file_open_complete, destroy_file_open_result);
    if (parked == 1) return result;
    if (parked < 0) { destroy_file_open_arg(arg); return -1; }
    FILE *file = fopen(arg->path, file_mode(arg->mode));
    destroy_file_open_arg(arg);
    if (file == NULL) return -1;
    for (int i = 0; i < LIRA_MAX_FILES; i++) {
        if (g_files[i].file == NULL) {
            if (g_next_file_handle >= INT64_MAX) {
                break;
            }
            g_files[i].file = file;
            g_files[i].busy = 0;
            g_files[i].handle = g_next_file_handle++;
            return g_files[i].handle;
        }
    }
    fclose(file);
    return -1;
}

static LiraOpenFile *lira_file_slot(int64_t handle) {
    if (handle < LIRA_FIRST_FILE_HANDLE) {
        return NULL;
    }
    for (int i = 0; i < LIRA_MAX_FILES; i++) {
        if (g_files[i].file != NULL && g_files[i].handle == handle) {
            return &g_files[i];
        }
    }
    return NULL;
}

typedef struct {
    int64_t handle;
    FILE *file;
    int64_t *slot;
    int64_t max_bytes;
    int owns_resource;
} FileReadArg;
typedef struct {
    int64_t handle;
    FILE *file;
    int64_t *slot;
    char *data;
    int64_t len;
    int owns_resource;
} FileReadResult;

static int file_read_work(void *ptr, void **out) {
    FileReadArg *arg = (FileReadArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_FILE_READ_RESULT")) return -1;
    FileReadResult *result = (FileReadResult *)lira_rt_mem_try_alloc(sizeof(FileReadResult), 1);
    if (result == NULL) return -1;
    result->handle = arg->handle;
    result->file = arg->file;
    result->slot = arg->slot;
    result->owns_resource = 1;
    result->data = (char *)lira_rt_mem_try_alloc((size_t)arg->max_bytes, 0);
    if (result->data == NULL) { lira_rt_mem_free(result); return -1; }
    arg->owns_resource = 0;
    result->len = (int64_t)fread(result->data, 1, (size_t)arg->max_bytes, arg->file);
    *out = result;
    return 0;
}

static void file_busy_done(int64_t handle) {
    LiraOpenFile *slot = lira_file_slot(handle);
    if (slot != NULL) slot->busy = 0;
}

static int file_utf8_valid(const char *bytes, int64_t len) {
    size_t i = 0;
    while (i < (size_t)len) {
        unsigned char c = (unsigned char)bytes[i++];
        size_t need = c < 0x80 ? 0 : (c >= 0xc2 && c <= 0xdf ? 1 : (c >= 0xe0 && c <= 0xef ? 2 : (c >= 0xf0 && c <= 0xf4 ? 3 : 99)));
        if (need == 99 || i + need > (size_t)len) return 0;
        if (need >= 1 && ((unsigned char)bytes[i] < 0x80 || (unsigned char)bytes[i] > 0xbf)) return 0;
        if (need >= 2 && ((unsigned char)bytes[i + 1] < 0x80 || (unsigned char)bytes[i + 1] > 0xbf)) return 0;
        if (need == 3 && ((unsigned char)bytes[i + 2] < 0x80 || (unsigned char)bytes[i + 2] > 0xbf)) return 0;
        if (need == 2 && c == 0xe0 && (unsigned char)bytes[i] < 0xa0) return 0;
        if (need == 2 && c == 0xed && (unsigned char)bytes[i] >= 0xa0) return 0;
        if (need == 3 && c == 0xf0 && (unsigned char)bytes[i] < 0x90) return 0;
        if (need == 3 && c == 0xf4 && (unsigned char)bytes[i] >= 0x90) return 0;
        i += need;
    }
    return 1;
}

static void destroy_file_read_result(void *ptr) {
    FileReadResult *result = (FileReadResult *)ptr;
    if (result != NULL && result->owns_resource && result->file != NULL) {
        fclose(result->file);
    }
    if (result != NULL) { lira_rt_mem_free(result->data); lira_rt_mem_free(result); }
}

static void destroy_file_read_arg(void *ptr) {
    FileReadArg *arg = (FileReadArg *)ptr;
    if (arg != NULL) {
        if (arg->owns_resource && arg->file != NULL) fclose(arg->file);
        lira_rt_mem_free(arg);
    }
}

static void file_read_complete(void *owner, uint64_t generation, void *ptr, int status,
                               void *failure_arg) {
    if (status != 0) {
        FileReadArg *arg = (FileReadArg *)failure_arg;
        if (arg != NULL) { file_busy_done(arg->handle); arg->owns_resource = 0; }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    FileReadResult *result = (FileReadResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    LiraStr *value = status == 0 && file_utf8_valid(result->data, result->len) ? lira_rt_str_new(result->data, result->len)
                                 : lira_rt_str_new("", 0);
    *(LiraStr **)result->slot = value;
    file_busy_done(result->handle);
    result->owns_resource = 0;
    lira_rt_io_wake(owner, generation, 0);
}

LiraStr *lira_rt_file_read(int64_t handle, int64_t max_bytes) {
    LiraOpenFile *slot = lira_file_slot(handle);
    FILE *file = slot != NULL ? slot->file : NULL;
    if (file == NULL || max_bytes <= 0 || slot->busy) {
        return lira_rt_str_new("", 0);
    }
    if (max_bytes > 1024 * 1024) {
        max_bytes = 1024 * 1024; /* the VM caps reads at 1 MiB */
    }
    slot->busy = 1;
    FileReadArg *arg = (FileReadArg *)lira_rt_mem_try_alloc(sizeof(FileReadArg), 1);
    if (arg == NULL) { slot->busy = 0; return lira_rt_str_new("", 0); }
    arg->handle = handle; arg->file = file; arg->max_bytes = max_bytes;
    arg->owns_resource = 1;
    LiraStr *result = NULL; arg->slot = (int64_t *)&result;
    int8_t parked = lira_rt_io_submit_current(file_read_work, arg, destroy_file_read_arg,
                                               file_read_complete,
                                               destroy_file_read_result);
    if (parked == 1) return result != NULL ? result : lira_rt_str_new("", 0);
    slot->busy = 0;
    if (parked < 0) { arg->owns_resource = 0; destroy_file_read_arg(arg); return lira_rt_str_new("", 0); }
    void *out = NULL; int status = file_read_work(arg, &out); arg->owns_resource = 0; destroy_file_read_arg(arg);
    FileReadResult *sync = (FileReadResult *)out;
    if (sync == NULL || status != 0) { destroy_file_read_result(sync); return lira_rt_str_new("", 0); }
    LiraStr *value = file_utf8_valid(sync->data, sync->len)
                         ? lira_rt_str_new(sync->data, sync->len)
                         : lira_rt_str_new("", 0);
    sync->owns_resource = 0;
    destroy_file_read_result(sync); return value;
}

typedef struct {
    int64_t handle;
    FILE *file;
    int64_t *slot;
    char *data;
    int64_t len;
    int owns_resource;
} FileWriteArg;
typedef struct {
    int64_t handle;
    FILE *file;
    int64_t *slot;
    int64_t value;
    int owns_resource;
} FileWriteResult;

static void destroy_file_write_result(void *ptr) {
    FileWriteResult *result = (FileWriteResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->file != NULL) {
            fclose(result->file);
        }
        lira_rt_mem_free(result);
    }
}

static void destroy_file_write_arg(void *ptr) {
    FileWriteArg *arg = (FileWriteArg *)ptr;
    if (arg != NULL) {
        if (arg->owns_resource && arg->file != NULL) fclose(arg->file);
        lira_rt_mem_free(arg->data);
        lira_rt_mem_free(arg);
    }
}
static int file_write_work(void *ptr, void **out) {
    FileWriteArg *arg = (FileWriteArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_FILE_WRITE_RESULT")) return -1;
    FileWriteResult *result = (FileWriteResult *)lira_rt_mem_try_alloc(sizeof(FileWriteResult), 1);
    if (result == NULL) return -1;
    result->handle = arg->handle; result->file = arg->file; result->slot = arg->slot;
    result->owns_resource = 1;
    arg->owns_resource = 0;
    result->value = (int64_t)fwrite(arg->data, 1, (size_t)arg->len, arg->file);
    if (fflush(arg->file) != 0) result->value = -1;
    *out = result; return 0;
}
static void file_write_complete(void *owner, uint64_t generation, void *ptr, int status,
                                void *failure_arg) {
    if (status != 0) {
        FileWriteArg *arg = (FileWriteArg *)failure_arg;
        if (arg != NULL) { file_busy_done(arg->handle); arg->owns_resource = 0; }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    FileWriteResult *result = (FileWriteResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    *result->slot = status == 0 ? result->value : -1;
    file_busy_done(result->handle);
    result->owns_resource = 0;
    lira_rt_io_wake(owner, generation, 0);
}

int64_t lira_rt_file_write(int64_t handle, const LiraStr *data) {
    LiraOpenFile *slot = lira_file_slot(handle);
    FILE *file = slot != NULL ? slot->file : NULL;
    if (file == NULL || data == NULL || data->len < 0 ||
        (uint64_t)data->len > SIZE_MAX - 1 || slot->busy) {
        return -1;
    }
    slot->busy = 1;
    FileWriteArg *arg = (FileWriteArg *)lira_rt_mem_try_alloc(sizeof(FileWriteArg), 1);
    if (arg == NULL) { slot->busy = 0; return -1; }
    arg->data = (char *)lira_rt_mem_try_alloc((size_t)data->len + 1, 0);
    if (arg->data == NULL) { destroy_file_write_arg(arg); slot->busy = 0; return -1; }
    memcpy(arg->data, data->data, (size_t)data->len);
    arg->handle = handle; arg->file = file; arg->len = data->len;
    arg->owns_resource = 1;
    int64_t result = -1; arg->slot = &result;
    int8_t parked = lira_rt_io_submit_current(file_write_work, arg,
                                               destroy_file_write_arg,
                                               file_write_complete, destroy_file_write_result);
    if (parked == 1) return result;
    slot->busy = 0;
    if (parked < 0) { arg->owns_resource = 0; destroy_file_write_arg(arg); return -1; }
    void *out = NULL; file_write_work(arg, &out); arg->owns_resource = 0; destroy_file_write_arg(arg);
    FileWriteResult *sync = (FileWriteResult *)out;
    result = sync != NULL ? sync->value : -1;
    if (sync != NULL) sync->owns_resource = 0;
    destroy_file_write_result(sync);
    return result;
}

typedef struct { int64_t handle; FILE *file; int8_t *slot; int owns_resource; } FileCloseArg;
typedef struct { int64_t handle; FILE *file; int8_t *slot; int8_t value; } FileCloseResult;

static void destroy_file_close_arg(void *ptr) {
    FileCloseArg *arg = (FileCloseArg *)ptr;
    if (arg != NULL) { if (arg->owns_resource && arg->file != NULL) fclose(arg->file); lira_rt_mem_free(arg); }
}

static int file_close_work(void *ptr, void **out) {
    FileCloseArg *arg = (FileCloseArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_FILE_CLOSE_RESULT")) return -1;
    FileCloseResult *result = (FileCloseResult *)lira_rt_mem_try_alloc(sizeof(FileCloseResult), 1);
    if (result == NULL) return -1;
    result->handle = arg->handle; result->file = arg->file; result->slot = arg->slot;
    result->value = fclose(arg->file) == 0 ? 1 : 0;
    arg->file = NULL;
    arg->owns_resource = 0;
    *out = result; return 0;
}

static void destroy_file_close_result(void *ptr) { lira_rt_mem_free(ptr); }

static void file_close_complete(void *owner, uint64_t generation, void *ptr, int status,
                                void *failure_arg) {
    if (status != 0) {
        FileCloseArg *arg = (FileCloseArg *)failure_arg;
        if (arg != NULL) { file_busy_done(arg->handle); arg->owns_resource = 0; }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    FileCloseResult *result = (FileCloseResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    LiraOpenFile *slot = lira_file_slot(result->handle);
    if (slot != NULL) { slot->file = NULL; slot->busy = 0; }
    *result->slot = status == 0 ? result->value : 0;
    lira_rt_io_wake(owner, generation, 0);
}

void lira_rt_file_reap_orphans(void) {
    for (int i = 0; i < LIRA_MAX_FILES; i++) {
        if (g_files[i].busy) {
            g_files[i].file = NULL;
            g_files[i].busy = 0;
        }
    }
}

int8_t lira_rt_file_close(int64_t handle) {
    LiraOpenFile *slot = lira_file_slot(handle);
    if (slot == NULL || slot->busy) {
        return 0;
    }
    FILE *file = slot->file;
    FileCloseArg *arg = (FileCloseArg *)lira_rt_mem_try_alloc(sizeof(FileCloseArg), 1);
    if (arg == NULL) return 0;
    int8_t result = 0;
    arg->handle = handle; arg->file = file; arg->slot = &result;
    arg->owns_resource = 1;
    slot->busy = 1;
    int8_t parked = lira_rt_io_submit_current(file_close_work, arg, destroy_file_close_arg,
                                               file_close_complete, destroy_file_close_result);
    if (parked == 1) return result;
    slot->busy = 0;
    if (parked < 0) { arg->owns_resource = 0; destroy_file_close_arg(arg); return 0; }
    void *out = NULL; int status = file_close_work(arg, &out);
    FileCloseResult *sync = (FileCloseResult *)out;
    arg->owns_resource = 0;
    result = sync != NULL && status == 0 ? sync->value : 0;
    file_close_complete(NULL, 0, sync, status, NULL);
    destroy_file_close_result(sync);
    destroy_file_close_arg(arg);
    return result;
}

void lira_rt_file_cancel_all(void) {
    for (int i = 0; i < LIRA_MAX_FILES; i++) {
        /* A busy FILE* may still be used by an orphaned worker.  Keep it
         * alive until that worker returns instead of closing it underneath
         * fread/fwrite. */
        if (g_files[i].file != NULL && !g_files[i].busy) {
            fclose(g_files[i].file);
            g_files[i].file = NULL;
            g_files[i].busy = 0;
        }
    }
}

typedef struct { int64_t handle; FILE *file; int64_t offset; int origin; int64_t *slot; int owns_resource; } FileSeekArg;
typedef struct { int64_t handle; FILE *file; int64_t *slot; int64_t value; int owns_resource; } FileSeekResult;

static int file_seek_work(void *ptr, void **out) {
    FileSeekArg *arg = (FileSeekArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_FILE_SEEK_RESULT")) return -1;
    FileSeekResult *result = (FileSeekResult *)lira_rt_mem_try_alloc(sizeof(FileSeekResult), 1);
    if (result == NULL) return -1;
    result->handle = arg->handle; result->file = arg->file; result->slot = arg->slot;
    result->value = -1; result->owns_resource = 1;
    arg->owns_resource = 0;
    if (fseek(arg->file, (long)arg->offset, arg->origin) == 0) result->value = (int64_t)ftell(arg->file);
    *out = result; return 0;
}

static void destroy_file_seek_result(void *ptr) {
    FileSeekResult *result = (FileSeekResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->file != NULL) fclose(result->file);
        lira_rt_mem_free(result);
    }
}

static void destroy_file_seek_arg(void *ptr) {
    FileSeekArg *arg = (FileSeekArg *)ptr;
    if (arg != NULL) {
        if (arg->owns_resource && arg->file != NULL) fclose(arg->file);
        lira_rt_mem_free(arg);
    }
}

static void file_seek_complete(void *owner, uint64_t generation, void *ptr, int status,
                               void *failure_arg) {
    if (status != 0) {
        FileSeekArg *arg = (FileSeekArg *)failure_arg;
        if (arg != NULL) { file_busy_done(arg->handle); arg->owns_resource = 0; }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    FileSeekResult *result = (FileSeekResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    *result->slot = status == 0 ? result->value : -1;
    file_busy_done(result->handle);
    result->file = NULL;
    result->owns_resource = 0;
    lira_rt_io_wake(owner, generation, 0);
}

int64_t lira_rt_file_seek(int64_t handle, int64_t offset, int64_t whence) {
    LiraOpenFile *slot = lira_file_slot(handle);
    FILE *file = slot != NULL ? slot->file : NULL;
    if (file == NULL || slot->busy) {
        return -1;
    }
    int origin = whence == 1 ? SEEK_CUR : (whence == 2 ? SEEK_END : SEEK_SET);
    FileSeekArg *arg = (FileSeekArg *)lira_rt_mem_try_alloc(sizeof(FileSeekArg), 1);
    if (arg == NULL) return -1;
    int64_t result = -1;
    arg->handle = handle; arg->file = file; arg->offset = offset; arg->origin = origin; arg->slot = &result;
    arg->owns_resource = 1;
    slot->busy = 1;
    int8_t parked = lira_rt_io_submit_current(file_seek_work, arg, destroy_file_seek_arg,
                                               file_seek_complete, destroy_file_seek_result);
    if (parked == 1) return result;
    slot->busy = 0;
    if (parked < 0) { arg->owns_resource = 0; destroy_file_seek_arg(arg); return -1; }
    void *out = NULL; int status = file_seek_work(arg, &out); arg->owns_resource = 0; destroy_file_seek_arg(arg);
    FileSeekResult *sync = (FileSeekResult *)out;
    result = sync != NULL && status == 0 ? sync->value : -1;
    if (sync != NULL) sync->owns_resource = 0;
    destroy_file_seek_result(sync);
    return result;
}

/* ------------------------------------------------------------------ */
/* Filesystem operations                                                */
/* ------------------------------------------------------------------ */

enum LiraFsOp {
    FS_EXISTS, FS_SIZE, FS_GETCWD, FS_CHDIR, FS_MKDIR, FS_MKDIR_ALL,
    FS_RMDIR, FS_REMOVE, FS_REMOVE_ALL, FS_LISTDIR, FS_IS_DIR, FS_IS_FILE,
    FS_RENAME, FS_COPY
};

typedef struct {
    int op;
    char *a;
    char *b;
    int8_t *bool_slot;
    int64_t *int_slot;
    LiraStr **str_slot;
    LiraArray **array_slot;
} LiraFsArg;

typedef struct {
    int op;
    int8_t boolean;
    int64_t integer;
    char *text;
    char **names;
    size_t name_count;
    int8_t *bool_slot;
    int64_t *int_slot;
    LiraStr **str_slot;
    LiraArray **array_slot;
} LiraFsResult;

static char *fs_copy_text(const LiraStr *value) {
    if (value == NULL || value->len < 0 || (uint64_t)value->len > SIZE_MAX - 1) {
        return NULL;
    }
    char *copy = (char *)lira_rt_mem_try_alloc((size_t)value->len + 1, 0);
    if (copy == NULL) return NULL;
    memcpy(copy, value->data, (size_t)value->len);
    copy[value->len] = '\0';
    return copy;
}

static LiraFsArg *fs_arg_new(int op, const LiraStr *a, const LiraStr *b) {
    LiraFsArg *arg = (LiraFsArg *)lira_rt_mem_try_alloc(sizeof(LiraFsArg), 1);
    if (arg == NULL) return NULL;
    arg->op = op;
    arg->a = fs_copy_text(a);
    arg->b = fs_copy_text(b);
    if ((a != NULL && arg->a == NULL) || (b != NULL && arg->b == NULL)) {
        lira_rt_mem_free(arg->a); lira_rt_mem_free(arg->b); lira_rt_mem_free(arg); return NULL;
    }
    return arg;
}

static void fs_destroy_arg(void *ptr) {
    LiraFsArg *arg = (LiraFsArg *)ptr;
    if (arg != NULL) { lira_rt_mem_free(arg->a); lira_rt_mem_free(arg->b); lira_rt_mem_free(arg); }
}

static void fs_free_names(LiraFsResult *result) {
    if (result == NULL) return;
    for (size_t i = 0; i < result->name_count; i++) {
        lira_rt_mem_free(result->names[i]);
    }
    lira_rt_mem_free(result->names);
    result->names = NULL;
    result->name_count = 0;
}

static int fs_mkdir_all(const char *path) {
    if (path == NULL || *path == '\0') return 0;
    char *copy = lira_rt_mem_try_strdup(path);
    if (copy == NULL) return 0;
    for (char *p = copy + 1; *p != '\0'; p++) {
        if (*p != '/') continue;
        *p = '\0';
        if (mkdir(copy, 0777) != 0 && errno != EEXIST) { lira_rt_mem_free(copy); return 0; }
        *p = '/';
    }
    int ok = mkdir(copy, 0777) == 0 || errno == EEXIST;
    lira_rt_mem_free(copy);
    return ok;
}

static int lira_remove_tree(const char *path) {
    struct stat info;
    if (lstat(path, &info) != 0) {
        return 0;
    }
    if (!S_ISDIR(info.st_mode)) {
        return unlink(path) == 0;
    }
    DIR *dir = opendir(path);
    if (dir == NULL) {
        return 0;
    }
    struct dirent *entry;
    char child[4096];
    int ok = 1;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        snprintf(child, sizeof(child), "%s/%s", path, entry->d_name);
        if (!lira_remove_tree(child)) {
            ok = 0;
        }
    }
    closedir(dir);
    return ok && rmdir(path) == 0;
}

static int fs_work(void *ptr, void **out) {
    LiraFsArg *arg = (LiraFsArg *)ptr;
    LiraFsResult *result = (LiraFsResult *)lira_rt_mem_try_alloc(sizeof(LiraFsResult), 1);
    if (result == NULL) return -1;
    int failed = 0;
    result->op = arg->op;
    result->bool_slot = arg->bool_slot;
    result->int_slot = arg->int_slot;
    result->str_slot = arg->str_slot;
    result->array_slot = arg->array_slot;
    struct stat info;
    switch (arg->op) {
        case FS_EXISTS: result->boolean = arg->a != NULL && stat(arg->a, &info) == 0; break;
        case FS_SIZE: result->integer = arg->a != NULL && stat(arg->a, &info) == 0 ? (int64_t)info.st_size : -1; break;
        case FS_GETCWD: {
            char buf[4096];
            if (getcwd(buf, sizeof(buf)) != NULL) {
                result->text = lira_rt_mem_try_strdup(buf);
                if (result->text == NULL) failed = 1;
            }
            break;
        }
        case FS_CHDIR: result->boolean = arg->a != NULL && chdir(arg->a) == 0; break;
        case FS_MKDIR: result->boolean = arg->a != NULL && mkdir(arg->a, 0777) == 0; break;
        case FS_MKDIR_ALL: result->boolean = fs_mkdir_all(arg->a); break;
        case FS_RMDIR: result->boolean = arg->a != NULL && rmdir(arg->a) == 0; break;
        case FS_REMOVE: result->boolean = arg->a != NULL && unlink(arg->a) == 0; break;
        case FS_REMOVE_ALL: result->boolean = arg->a != NULL && lira_remove_tree(arg->a); break;
        case FS_IS_DIR: result->boolean = arg->a != NULL && stat(arg->a, &info) == 0 && S_ISDIR(info.st_mode); break;
        case FS_IS_FILE: result->boolean = arg->a != NULL && stat(arg->a, &info) == 0 && S_ISREG(info.st_mode); break;
        case FS_RENAME: result->boolean = arg->a != NULL && arg->b != NULL && rename(arg->a, arg->b) == 0; break;
        case FS_COPY: {
            FILE *in = arg->a != NULL ? fopen(arg->a, "rb") : NULL;
            FILE *out_file = in != NULL && arg->b != NULL ? fopen(arg->b, "wb") : NULL;
            char buffer[8192]; size_t n; int ok = in != NULL && out_file != NULL;
            while (ok && (n = fread(buffer, 1, sizeof(buffer), in)) > 0) if (fwrite(buffer, 1, n, out_file) != n) ok = 0;
            if (in != NULL) fclose(in); if (out_file != NULL) ok = fclose(out_file) == 0 && ok;
            result->boolean = ok; break;
        }
        case FS_LISTDIR: {
            DIR *dir = arg->a != NULL ? opendir(arg->a) : NULL;
            if (dir != NULL) {
                struct dirent *entry;
                while ((entry = readdir(dir)) != NULL) {
                    if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
                    char *name = lira_rt_mem_try_strdup(entry->d_name);
                    if (name == NULL) { failed = 1; break; }
                    if (result->name_count >= SIZE_MAX / sizeof(char *)) {
                        lira_rt_mem_free(name);
                        failed = 1;
                        break;
                    }
                    char **grown = (char **)lira_rt_mem_try_realloc(
                        result->names, (result->name_count + 1) * sizeof(char *));
                    if (grown == NULL) {
                        lira_rt_mem_free(name);
                        failed = 1;
                        break;
                    }
                    result->names = grown; result->names[result->name_count++] = name;
                }
                closedir(dir);
            }
            break;
        }
    }
    if (failed) {
        fs_free_names(result);
        lira_rt_mem_free(result->text);
        lira_rt_mem_free(result);
        return -1;
    }
    *out = result; return 0;
}

static void fs_destroy_result(void *ptr) {
    LiraFsResult *result = (LiraFsResult *)ptr;
    if (result != NULL) {
        lira_rt_mem_free(result->text);
        fs_free_names(result);
        lira_rt_mem_free(result);
    }
}

static void fs_complete(void *owner, uint64_t generation, void *ptr, int status,
                        void *failure_arg) {
    (void)failure_arg;
    if (status != 0) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    LiraFsResult *result = (LiraFsResult *)ptr;
    if (result == NULL) { lira_rt_io_wake(owner, generation, status); return; }
    if (result->bool_slot != NULL) *result->bool_slot = result->boolean;
    if (result->int_slot != NULL) *result->int_slot = result->integer;
    if (result->str_slot != NULL) *result->str_slot = lira_rt_str_new(result->text != NULL ? result->text : "", result->text != NULL ? strlen(result->text) : 0);
    if (result->array_slot != NULL) {
        LiraArray *array = lira_rt_array_new((int64_t)result->name_count);
        /* The array is built from C code on the scheduler stack, which the
         * collector does not scan. Each str_new below can trigger a
         * collection, so keep the partially-built array alive through a root
         * slot or it would be swept mid-loop (use-after-free). */
        lira_gc_register_root_slot(&array);
        for (size_t i = 0; i < result->name_count; i++)
            lira_rt_array_push(array, (int64_t)(intptr_t)lira_rt_str_new(result->names[i], strlen(result->names[i])));
        *result->array_slot = array;
        lira_gc_unregister_root_slot(&array);
    }
    lira_rt_io_wake(owner, generation, 0);
}

static int8_t fs_submit(LiraFsArg *arg) {
    int8_t parked = lira_rt_io_submit_current(fs_work, arg, fs_destroy_arg, fs_complete, fs_destroy_result);
    if (parked == 1) return 1;
    if (parked < 0) { fs_destroy_arg(arg); return -1; }
    void *out = NULL; int status = fs_work(arg, &out); fs_complete(NULL, 0, out, status, NULL); fs_destroy_result(out); fs_destroy_arg(arg); return 0;
}

#define FS_BOOL(name, op) int8_t name(const LiraStr *path) { int8_t value = 0; LiraFsArg *arg = fs_arg_new(op, path, NULL); if (arg == NULL) return 0; arg->bool_slot = &value; fs_submit(arg); return value; }
FS_BOOL(lira_rt_file_exists, FS_EXISTS)
FS_BOOL(lira_rt_chdir, FS_CHDIR)
FS_BOOL(lira_rt_mkdir, FS_MKDIR)
FS_BOOL(lira_rt_mkdir_all, FS_MKDIR_ALL)
FS_BOOL(lira_rt_rmdir, FS_RMDIR)
FS_BOOL(lira_rt_remove, FS_REMOVE)
FS_BOOL(lira_rt_remove_all, FS_REMOVE_ALL)
FS_BOOL(lira_rt_is_dir, FS_IS_DIR)
FS_BOOL(lira_rt_is_file, FS_IS_FILE)

int64_t lira_rt_file_size(const LiraStr *path) { int64_t value = -1; LiraFsArg *arg = fs_arg_new(FS_SIZE, path, NULL); if (arg != NULL) { arg->int_slot = &value; fs_submit(arg); } return value; }
LiraStr *lira_rt_getcwd(void) { LiraStr *value = NULL; LiraFsArg *arg = fs_arg_new(FS_GETCWD, NULL, NULL); if (arg != NULL) { arg->str_slot = &value; fs_submit(arg); } return value != NULL ? value : lira_rt_str_new("", 0); }
int8_t lira_rt_rename(const LiraStr *from, const LiraStr *to) { int8_t value = 0; LiraFsArg *arg = fs_arg_new(FS_RENAME, from, to); if (arg != NULL) { arg->bool_slot = &value; fs_submit(arg); } return value; }
int8_t lira_rt_copy(const LiraStr *from, const LiraStr *to) { int8_t value = 0; LiraFsArg *arg = fs_arg_new(FS_COPY, from, to); if (arg != NULL) { arg->bool_slot = &value; fs_submit(arg); } return value; }
LiraArray *lira_rt_listdir(const LiraStr *path) { LiraArray *value = NULL; LiraFsArg *arg = fs_arg_new(FS_LISTDIR, path, NULL); if (arg != NULL) { arg->array_slot = &value; fs_submit(arg); } return value != NULL ? value : lira_rt_array_new(0); }

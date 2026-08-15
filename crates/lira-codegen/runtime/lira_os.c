/*
 * Time, randomness, environment, files and the filesystem.
 *
 * File handles are small integers indexed into a table owned by the runtime,
 * matching the bytecode VM's model rather than exposing raw file descriptors.
 */
#include "lira_rt.h"

#include <dirent.h>
#include <errno.h>
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
    struct timespec ts;
    ts.tv_sec = (time_t)(millis / 1000);
    ts.tv_nsec = (long)((millis % 1000) * 1000000);
    /* Fibers are cooperative and share one OS thread, so this parks the whole
     * program. Matching the VM, which also blocks. */
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
    utc.tm_year = (int)(year - 1900);
    utc.tm_mon = (int)(month - 1);
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
    uint64_t span = (uint64_t)(high - low);
    return low + (int64_t)(lira_rt_random_bits() % span);
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
} LiraOpenFile;

static LiraOpenFile g_files[LIRA_MAX_FILES];
static int64_t g_next_file_handle = LIRA_FIRST_FILE_HANDLE;

int64_t lira_rt_file_open(const LiraStr *path, int64_t mode) {
    if (path == NULL) {
        return -1;
    }
    /* 0 = read, 1 = write, 2 = append, 3 = read+write */
    const char *flags;
    switch (mode) {
        case 1:
            flags = "wb";
            break;
        case 2:
            flags = "ab";
            break;
        case 3:
            flags = "r+b";
            break;
        default:
            flags = "rb";
            break;
    }
    FILE *file = fopen(path->data, flags);
    if (file == NULL) {
        return -1;
    }
    for (int i = 0; i < LIRA_MAX_FILES; i++) {
        if (g_files[i].file == NULL) {
            g_files[i].file = file;
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

static FILE *lira_file(int64_t handle) {
    LiraOpenFile *slot = lira_file_slot(handle);
    return slot != NULL ? slot->file : NULL;
}

LiraStr *lira_rt_file_read(int64_t handle, int64_t max_bytes) {
    FILE *file = lira_file(handle);
    if (file == NULL || max_bytes <= 0) {
        return lira_rt_str_new("", 0);
    }
    if (max_bytes > 1024 * 1024) {
        max_bytes = 1024 * 1024; /* the VM caps reads at 1 MiB */
    }
    char *buffer = (char *)malloc((size_t)max_bytes);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    size_t read = fread(buffer, 1, (size_t)max_bytes, file);
    LiraStr *out = lira_rt_str_new(buffer, (int64_t)read);
    free(buffer);
    return out;
}

int64_t lira_rt_file_write(int64_t handle, const LiraStr *data) {
    FILE *file = lira_file(handle);
    if (file == NULL || data == NULL) {
        return -1;
    }
    return (int64_t)fwrite(data->data, 1, (size_t)data->len, file);
}

int8_t lira_rt_file_close(int64_t handle) {
    LiraOpenFile *slot = lira_file_slot(handle);
    if (slot == NULL) {
        return 0;
    }
    FILE *file = slot->file;
    slot->file = NULL;
    return fclose(file) == 0 ? 1 : 0;
}

int64_t lira_rt_file_seek(int64_t handle, int64_t offset, int64_t whence) {
    FILE *file = lira_file(handle);
    if (file == NULL) {
        return -1;
    }
    int origin = whence == 1 ? SEEK_CUR : (whence == 2 ? SEEK_END : SEEK_SET);
    if (fseek(file, (long)offset, origin) != 0) {
        return -1;
    }
    return (int64_t)ftell(file);
}

int8_t lira_rt_file_exists(const LiraStr *path) {
    struct stat info;
    return path != NULL && stat(path->data, &info) == 0 ? 1 : 0;
}

int64_t lira_rt_file_size(const LiraStr *path) {
    struct stat info;
    if (path == NULL || stat(path->data, &info) != 0) {
        return -1;
    }
    return (int64_t)info.st_size;
}

/* ------------------------------------------------------------------ */
/* Filesystem                                                          */
/* ------------------------------------------------------------------ */

LiraStr *lira_rt_getcwd(void) {
    char buf[4096];
    if (getcwd(buf, sizeof(buf)) == NULL) {
        return lira_rt_str_new("", 0);
    }
    return lira_rt_str_new(buf, (int64_t)strlen(buf));
}

int8_t lira_rt_chdir(const LiraStr *path) {
    return path != NULL && chdir(path->data) == 0 ? 1 : 0;
}

int8_t lira_rt_mkdir(const LiraStr *path) {
    return path != NULL && mkdir(path->data, 0777) == 0 ? 1 : 0;
}

int8_t lira_rt_mkdir_all(const LiraStr *path) {
    if (path == NULL || path->len == 0) {
        return 0;
    }
    char *copy = strdup(path->data);
    if (copy == NULL) {
        lira_rt_panic("out of memory");
    }
    /* Create each prefix in turn; an existing directory is not a failure. */
    for (char *p = copy + 1; *p != '\0'; p++) {
        if (*p != '/') {
            continue;
        }
        *p = '\0';
        if (mkdir(copy, 0777) != 0 && errno != EEXIST) {
            free(copy);
            return 0;
        }
        *p = '/';
    }
    int ok = mkdir(copy, 0777) == 0 || errno == EEXIST;
    free(copy);
    return ok ? 1 : 0;
}

int8_t lira_rt_rmdir(const LiraStr *path) {
    return path != NULL && rmdir(path->data) == 0 ? 1 : 0;
}

int8_t lira_rt_remove(const LiraStr *path) {
    return path != NULL && unlink(path->data) == 0 ? 1 : 0;
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

int8_t lira_rt_remove_all(const LiraStr *path) {
    return path != NULL && lira_remove_tree(path->data) ? 1 : 0;
}

LiraArray *lira_rt_listdir(const LiraStr *path) {
    LiraArray *entries = lira_rt_array_new(0);
    if (path == NULL) {
        return entries;
    }
    DIR *dir = opendir(path->data);
    if (dir == NULL) {
        return entries;
    }
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        lira_rt_array_push(entries, (int64_t)(intptr_t)lira_rt_str_new(
                                        entry->d_name, (int64_t)strlen(entry->d_name)));
    }
    closedir(dir);
    return entries;
}

int8_t lira_rt_is_dir(const LiraStr *path) {
    struct stat info;
    return path != NULL && stat(path->data, &info) == 0 && S_ISDIR(info.st_mode) ? 1 : 0;
}

int8_t lira_rt_is_file(const LiraStr *path) {
    struct stat info;
    return path != NULL && stat(path->data, &info) == 0 && S_ISREG(info.st_mode) ? 1 : 0;
}

int8_t lira_rt_rename(const LiraStr *from, const LiraStr *to) {
    return from != NULL && to != NULL && rename(from->data, to->data) == 0 ? 1 : 0;
}

int8_t lira_rt_copy(const LiraStr *from, const LiraStr *to) {
    if (from == NULL || to == NULL) {
        return 0;
    }
    FILE *in = fopen(from->data, "rb");
    if (in == NULL) {
        return 0;
    }
    FILE *out = fopen(to->data, "wb");
    if (out == NULL) {
        fclose(in);
        return 0;
    }
    char buffer[8192];
    size_t n;
    int ok = 1;
    while ((n = fread(buffer, 1, sizeof(buffer), in)) > 0) {
        if (fwrite(buffer, 1, n, out) != n) {
            ok = 0;
            break;
        }
    }
    fclose(in);
    ok = (fclose(out) == 0) && ok;
    return ok ? 1 : 0;
}

/* TCP and DNS built-ins. Blocking operations run on the native I/O pool. */
#include "lira_rt.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define LIRA_MAX_SOCKETS 128
#define LIRA_TCP_CONNECT_TIMEOUT_MS 2000

typedef struct {
    int fd;
    int8_t busy;
} LiraSocket;

static LiraSocket g_sockets[LIRA_MAX_SOCKETS];
static int g_sockets_ready = 0;

static void lira_sockets_init(void) {
    if (g_sockets_ready) {
        return;
    }
    memset(g_sockets, 0, sizeof(g_sockets));
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        g_sockets[i].fd = -1;
    }
    g_sockets_ready = 1;
}

static int lira_socket_fd(int64_t handle) {
    lira_sockets_init();
    if (handle < 0 || handle >= LIRA_MAX_SOCKETS) {
        return -1;
    }
    return g_sockets[handle].fd;
}

static int lira_socket_slot(int fd) {
    lira_sockets_init();
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        if (g_sockets[i].fd < 0) {
            g_sockets[i].fd = fd;
            g_sockets[i].busy = 0;
            return i;
        }
    }
    return -1;
}

static void lira_socket_done(int fd) {
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        if (g_sockets[i].fd == fd) {
            g_sockets[i].busy = 0;
            return;
        }
    }
}

static int64_t monotonic_millis(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        return -1;
    }
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

/* POSIX connect has no timeout parameter. Use a temporary nonblocking socket
 * and wait only until the same two-second deadline as the bytecode VM, then
 * restore blocking mode for the existing read/write runtime contract. */
static int connect_with_timeout(int fd, const struct sockaddr *address,
                                socklen_t address_len) {
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) != 0) {
        return -1;
    }
    if (connect(fd, address, address_len) == 0) {
        return fcntl(fd, F_SETFL, flags) == 0 ? 0 : -1;
    }
    if (errno != EINPROGRESS && errno != EWOULDBLOCK) {
        return -1;
    }

    int64_t started = monotonic_millis();
    if (started < 0) {
        return -1;
    }
    int64_t deadline = started + LIRA_TCP_CONNECT_TIMEOUT_MS;
    struct pollfd event;
    event.fd = fd;
    event.events = POLLOUT;
    event.revents = 0;
    for (;;) {
        int64_t now = monotonic_millis();
        if (now < 0 || now >= deadline) {
            return -1;
        }
        int remaining = (int)(deadline - now);
        int ready = poll(&event, 1, remaining);
        if (ready == 0) {
            return -1;
        }
        if (ready < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        int socket_error = 0;
        socklen_t error_len = sizeof(socket_error);
        if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &socket_error, &error_len) != 0 ||
            socket_error != 0) {
            return -1;
        }
        return fcntl(fd, F_SETFL, flags) == 0 ? 0 : -1;
    }
}

static int connect_host(const char *host, int64_t port) {
    if (host == NULL || port <= 0 || port > 65535) {
        return -1;
    }
    char service[16];
    snprintf(service, sizeof(service), "%d", (int)port);
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *results = NULL;
    if (getaddrinfo(host, service, &hints, &results) != 0) {
        return -1;
    }
    int fd = -1;
    for (struct addrinfo *it = results; it != NULL; it = it->ai_next) {
        fd = socket(it->ai_family, it->ai_socktype, it->ai_protocol);
        if (fd < 0) {
            continue;
        }
        if (connect_with_timeout(fd, it->ai_addr, it->ai_addrlen) == 0) {
            break;
        }
        close(fd);
        fd = -1;
    }
    freeaddrinfo(results);
    return fd;
}

typedef struct {
    char *host;
    int64_t port;
    int64_t *slot;
} ConnectArg;
typedef struct {
    int64_t *slot;
    int fd;
    int owns_resource;
} ConnectResult;

static void destroy_connect_result(void *ptr) {
    ConnectResult *result = (ConnectResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->fd >= 0) {
            close(result->fd);
        }
        lira_rt_mem_free(result);
    }
}

static void destroy_connect_arg(void *ptr) {
    ConnectArg *arg = (ConnectArg *)ptr;
    if (arg != NULL) {
        lira_rt_mem_free(arg->host);
        lira_rt_mem_free(arg);
    }
}

static int connect_work(void *ptr, void **out) {
    ConnectArg *arg = (ConnectArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_TCP_CONNECT_RESULT")) return -1;
    ConnectResult *result = (ConnectResult *)lira_rt_mem_try_alloc(sizeof(ConnectResult), 1);
    if (result == NULL) {
        return -1;
    }
    result->slot = arg->slot;
    result->fd = connect_host(arg->host, arg->port);
    result->owns_resource = result->fd >= 0;
    *out = result;
    return 0;
}

static void connect_complete(void *owner, uint64_t generation, void *ptr, int status,
                             void *failure_arg) {
    (void)failure_arg;
    if (status != 0) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    ConnectResult *result = (ConnectResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    int64_t handle = -1;
    if (status == 0 && result->fd >= 0) {
        int slot = lira_socket_slot(result->fd);
        if (slot >= 0) {
            handle = slot;
            result->owns_resource = 0;
        } else {
            close(result->fd);
            result->owns_resource = 0;
        }
    }
    if (status != 0 && result->owns_resource) {
        close(result->fd);
        result->owns_resource = 0;
    }
    *result->slot = handle;
    lira_rt_io_wake(owner, generation, 0);
}

int64_t lira_rt_tcp_connect(const LiraStr *host, int64_t port) {
    if (host == NULL || host->len < 0 || (uint64_t)host->len > SIZE_MAX - 1) {
        return -1;
    }
    lira_sockets_init();
    ConnectArg *arg = (ConnectArg *)lira_rt_mem_try_alloc(sizeof(ConnectArg), 1);
    if (arg == NULL) {
        return -1;
    }
    arg->host = (char *)lira_rt_mem_try_alloc((size_t)host->len + 1, 0);
    if (arg->host == NULL) {
        destroy_connect_arg(arg);
        return -1;
    }
    memcpy(arg->host, host->data, (size_t)host->len);
    arg->host[host->len] = '\0';
    arg->port = port;
    int64_t result = -1;
    arg->slot = &result;
    int8_t parked = lira_rt_io_submit_current(connect_work, arg,
                                               destroy_connect_arg,
                                               connect_complete, destroy_connect_result);
    if (parked == 1) {
        return result;
    }
    if (parked < 0) {
        destroy_connect_arg(arg);
        return -1;
    }
    int fd = connect_host(arg->host, arg->port);
    destroy_connect_arg(arg);
    return fd < 0 ? -1 : lira_socket_slot(fd);
}

typedef struct {
    int fd;
    int64_t *slot;
    char *data;
    int64_t len;
    int owns_resource;
} WriteArg;
typedef struct {
    int64_t *slot;
    int64_t value;
    int fd;
    int owns_resource;
} WriteResult;

static void destroy_write_result(void *ptr) {
    WriteResult *result = (WriteResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->fd >= 0) {
            close(result->fd);
        }
        lira_rt_mem_free(result);
    }
}

static void destroy_write_arg(void *ptr) {
    WriteArg *arg = (WriteArg *)ptr;
    if (arg != NULL) {
        if (arg->owns_resource && arg->fd >= 0) close(arg->fd);
        lira_rt_mem_free(arg->data);
        lira_rt_mem_free(arg);
    }
}

static int write_work(void *ptr, void **out) {
    WriteArg *arg = (WriteArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_TCP_WRITE_RESULT")) return -1;
    WriteResult *result = (WriteResult *)lira_rt_mem_try_alloc(sizeof(WriteResult), 1);
    if (result == NULL) {
        return -1;
    }
    result->slot = arg->slot;
    result->fd = arg->fd;
    result->owns_resource = 1;
    arg->owns_resource = 0;
    ssize_t written = send(arg->fd, arg->data, (size_t)arg->len, 0);
    result->value = written < 0 ? -1 : (int64_t)written;
    *out = result;
    return 0;
}

static void write_complete(void *owner, uint64_t generation, void *ptr, int status,
                           void *failure_arg) {
    if (status != 0) {
        WriteArg *arg = (WriteArg *)failure_arg;
        if (arg != NULL) {
            lira_socket_done(arg->fd);
            arg->owns_resource = 0;
        }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    WriteResult *result = (WriteResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    *result->slot = status == 0 ? result->value : -1;
    lira_socket_done(result->fd);
    result->owns_resource = 0;
    lira_rt_io_wake(owner, generation, 0);
}

int64_t lira_rt_tcp_write(int64_t handle, const LiraStr *data) {
    int fd = lira_socket_fd(handle);
    if (fd < 0 || data == NULL || data->len < 0 ||
        (uint64_t)data->len > SIZE_MAX - 1 || g_sockets[handle].busy) {
        return -1;
    }
    g_sockets[handle].busy = 1;
    WriteArg *arg = (WriteArg *)lira_rt_mem_try_alloc(sizeof(WriteArg), 1);
    if (arg == NULL || (arg->data = (char *)lira_rt_mem_try_alloc(
                           data->len > 0 ? (size_t)data->len : 1, 0)) == NULL) {
        lira_rt_mem_free(arg);
        g_sockets[handle].busy = 0;
        return -1;
    }
    memcpy(arg->data, data->data, (size_t)data->len);
    arg->fd = fd;
    arg->len = data->len;
    arg->owns_resource = 1;
    int64_t result = -1;
    arg->slot = &result;
    int8_t parked = lira_rt_io_submit_current(write_work, arg,
                                               destroy_write_arg,
                                               write_complete, destroy_write_result);
    if (parked == 1) {
        return result;
    }
    g_sockets[handle].busy = 0;
    if (parked < 0) {
        arg->owns_resource = 0;
        destroy_write_arg(arg);
        return -1;
    }
    void *out = NULL;
    write_work(arg, &out);
    arg->owns_resource = 0;
    destroy_write_arg(arg);
    WriteResult *sync = (WriteResult *)out;
    result = sync != NULL ? sync->value : -1;
    if (sync != NULL) {
        sync->owns_resource = 0;
    }
    destroy_write_result(sync);
    return result;
}

typedef struct {
    int fd;
    int64_t *slot;
    int64_t max_bytes;
    int owns_resource;
} ReadArg;
typedef struct {
    int64_t *slot;
    char *data;
    int64_t len;
    int fd;
    int owns_resource;
} ReadResult;

/* Return the valid scalar width, or the negative number of bytes Rust's
 * from_utf8_lossy consumes for one replacement.  In particular, a truncated
 * but otherwise well-formed prefix consumes its continuation prefix together
 * with the leading byte, while an invalid scalar boundary consumes only the
 * lead byte. */
static int utf8_decode_width(const unsigned char *bytes, size_t len) {
    if (len == 0) return 0;
    unsigned char c = bytes[0];
    if (c < 0x80) return 1;
    if (c >= 0xc2 && c <= 0xdf) {
        if (len < 2) return -1;
        return bytes[1] >= 0x80 && bytes[1] <= 0xbf ? 2 : -1;
    }
    if (c >= 0xe0 && c <= 0xef) {
        if (len < 2 || bytes[1] < 0x80 || bytes[1] > 0xbf) return -1;
        if (c == 0xe0 && bytes[1] < 0xa0) return -1;
        if (c == 0xed && bytes[1] >= 0xa0) return -1;
        if (len < 3) return -2;
        return bytes[2] >= 0x80 && bytes[2] <= 0xbf ? 3 : -2;
    }
    if (c >= 0xf0 && c <= 0xf4) {
        if (len < 2 || bytes[1] < 0x80 || bytes[1] > 0xbf) return -1;
        if (c == 0xf0 && bytes[1] < 0x90) return -1;
        if (c == 0xf4 && bytes[1] > 0x8f) return -1;
        if (len < 3 || bytes[2] < 0x80 || bytes[2] > 0xbf) return -2;
        if (len < 4) return -3;
        return bytes[3] >= 0x80 && bytes[3] <= 0xbf ? 4 : -3;
    }
    return -1;
}

static LiraStr *tcp_lossy_string(const char *bytes, int64_t len) {
    size_t cap = (size_t)len * 3 + 1;
    char *out = (char *)lira_rt_mem_try_alloc(cap, 0);
    if (out == NULL) return lira_rt_str_new("", 0);
    size_t i = 0, used = 0;
    while (i < (size_t)len) {
        int width = utf8_decode_width((const unsigned char *)bytes + i, (size_t)len - i);
        if (width <= 0) {
            out[used++] = (char)0xef; out[used++] = (char)0xbf; out[used++] = (char)0xbd;
            i += width < 0 ? (size_t)-width : 1;
        } else {
            memcpy(out + used, bytes + i, width); used += width; i += width;
        }
    }
    LiraStr *value = lira_rt_str_new(out, (int64_t)used);
    lira_rt_mem_free(out);
    return value;
}

static int read_work(void *ptr, void **out) {
    ReadArg *arg = (ReadArg *)ptr;
    if (lira_io_test_fail_result_alloc("LIRA_TEST_FAIL_TCP_READ_RESULT")) return -1;
    ReadResult *result = (ReadResult *)lira_rt_mem_try_alloc(sizeof(ReadResult), 1);
    if (result == NULL) {
        return -1;
    }
    result->slot = arg->slot;
    result->fd = arg->fd;
    result->owns_resource = 1;
    result->data = (char *)lira_rt_mem_try_alloc((size_t)arg->max_bytes, 0);
    if (result->data == NULL) {
        lira_rt_mem_free(result);
        return -1;
    }
    arg->owns_resource = 0;
    ssize_t n = recv(arg->fd, result->data, (size_t)arg->max_bytes, 0);
    result->len = n > 0 ? (int64_t)n : 0;
    *out = result;
    return 0;
}

static void read_complete(void *owner, uint64_t generation, void *ptr, int status,
                          void *failure_arg) {
    if (status != 0) {
        ReadArg *arg = (ReadArg *)failure_arg;
        if (arg != NULL) {
            lira_socket_done(arg->fd);
            arg->owns_resource = 0;
        }
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    ReadResult *result = (ReadResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    LiraStr *value = status == 0 ? tcp_lossy_string(result->data, result->len)
                                 : lira_rt_str_new("", 0);
    *(LiraStr **)result->slot = value;
    lira_socket_done(result->fd);
    result->owns_resource = 0;
    lira_rt_io_wake(owner, generation, 0);
}

static void destroy_read_result(void *ptr) {
    ReadResult *result = (ReadResult *)ptr;
    if (result != NULL) {
        if (result->owns_resource && result->fd >= 0) {
            close(result->fd);
        }
        lira_rt_mem_free(result->data);
        lira_rt_mem_free(result);
    }
}

static void destroy_read_arg(void *ptr) {
    ReadArg *arg = (ReadArg *)ptr;
    if (arg != NULL) {
        if (arg->owns_resource && arg->fd >= 0) close(arg->fd);
        lira_rt_mem_free(arg);
    }
}

LiraStr *lira_rt_tcp_read(int64_t handle, int64_t max_bytes) {
    int fd = lira_socket_fd(handle);
    if (fd < 0 || max_bytes <= 0 || g_sockets[handle].busy) {
        return lira_rt_str_new("", 0);
    }
    if (max_bytes > 1024 * 1024) {
        max_bytes = 1024 * 1024;
    }
    g_sockets[handle].busy = 1;
    ReadArg *arg = (ReadArg *)lira_rt_mem_try_alloc(sizeof(ReadArg), 1);
    if (arg == NULL) {
        g_sockets[handle].busy = 0;
        return lira_rt_str_new("", 0);
    }
    arg->fd = fd;
    arg->max_bytes = max_bytes;
    arg->owns_resource = 1;
    LiraStr *result = NULL;
    arg->slot = (int64_t *)&result;
    int8_t parked = lira_rt_io_submit_current(read_work, arg, destroy_read_arg,
                                               read_complete,
                                               destroy_read_result);
    if (parked == 1) {
        return result != NULL ? result : lira_rt_str_new("", 0);
    }
    g_sockets[handle].busy = 0;
    if (parked < 0) {
        arg->owns_resource = 0;
        destroy_read_arg(arg);
        return lira_rt_str_new("", 0);
    }
    void *out = NULL;
    int status = read_work(arg, &out);
    arg->owns_resource = 0;
    destroy_read_arg(arg);
    ReadResult *sync = (ReadResult *)out;
    if (sync == NULL || status != 0) {
        destroy_read_result(sync);
        return lira_rt_str_new("", 0);
    }
    LiraStr *value = tcp_lossy_string(sync->data, sync->len);
    sync->owns_resource = 0;
    destroy_read_result(sync);
    return value;
}

int8_t lira_rt_tcp_close(int64_t handle) {
    int fd = lira_socket_fd(handle);
    if (fd < 0 || g_sockets[handle].busy) {
        return 0;
    }
    g_sockets[handle].fd = -1;
    return close(fd) == 0 ? 1 : 0;
}

void lira_rt_tcp_cancel_all(void) {
    lira_sockets_init();
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        /* A busy descriptor belongs to an orphanable worker after a fatal
         * runtime error.  Leave its OS lifetime with that worker; closing it
         * here could race recv/send and let a later run reuse the fd. */
        if (g_sockets[i].fd >= 0 && !g_sockets[i].busy) {
            shutdown(g_sockets[i].fd, SHUT_RDWR);
            close(g_sockets[i].fd);
            g_sockets[i].fd = -1;
            g_sockets[i].busy = 0;
        }
    }
}

void lira_rt_tcp_reap_orphans(void) {
    lira_sockets_init();
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        if (g_sockets[i].busy) {
            g_sockets[i].fd = -1;
            g_sockets[i].busy = 0;
        }
    }
}

typedef struct {
    char *host;
    int64_t *slot;
} DnsArg;
typedef struct {
    int64_t *slot;
    char text[INET6_ADDRSTRLEN];
} DnsResult;

static int dns_work(void *ptr, void **out) {
    DnsArg *arg = (DnsArg *)ptr;
    DnsResult *result = (DnsResult *)lira_rt_mem_try_alloc(sizeof(DnsResult), 1);
    if (result == NULL) {
        return -1;
    }
    result->slot = arg->slot;
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *results = NULL;
    if (getaddrinfo(arg->host, NULL, &hints, &results) == 0 && results != NULL) {
        if (results->ai_family == AF_INET) {
            struct sockaddr_in *addr = (struct sockaddr_in *)results->ai_addr;
            inet_ntop(AF_INET, &addr->sin_addr, result->text, sizeof(result->text));
        } else if (results->ai_family == AF_INET6) {
            struct sockaddr_in6 *addr = (struct sockaddr_in6 *)results->ai_addr;
            inet_ntop(AF_INET6, &addr->sin6_addr, result->text, sizeof(result->text));
        }
    }
    if (results != NULL) {
        freeaddrinfo(results);
    }
    *out = result;
    return 0;
}

static void destroy_dns_arg(void *ptr) {
    DnsArg *arg = (DnsArg *)ptr;
    if (arg != NULL) {
        lira_rt_mem_free(arg->host);
        lira_rt_mem_free(arg);
    }
}

static void dns_complete(void *owner, uint64_t generation, void *ptr, int status,
                         void *failure_arg) {
    (void)failure_arg;
    if (status != 0) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    DnsResult *result = (DnsResult *)ptr;
    if (result == NULL) {
        lira_rt_io_wake(owner, generation, status);
        return;
    }
    *(LiraStr **)result->slot =
        lira_rt_str_new(status == 0 ? result->text : "", status == 0 ? strlen(result->text) : 0);
    lira_rt_io_wake(owner, generation, 0);
}

LiraStr *lira_rt_dns_lookup(const LiraStr *host) {
    if (host == NULL || host->len < 0 || (uint64_t)host->len > SIZE_MAX - 1) {
        return lira_rt_str_new("", 0);
    }
    DnsArg *arg = (DnsArg *)lira_rt_mem_try_alloc(sizeof(DnsArg), 1);
    if (arg == NULL) {
        return lira_rt_str_new("", 0);
    }
    arg->host = (char *)lira_rt_mem_try_alloc((size_t)host->len + 1, 0);
    if (arg->host == NULL) {
        lira_rt_mem_free(arg);
        return lira_rt_str_new("", 0);
    }
    memcpy(arg->host, host->data, (size_t)host->len);
    arg->host[host->len] = '\0';
    LiraStr *result = NULL;
    arg->slot = (int64_t *)&result;
    int8_t parked = lira_rt_io_submit_current(dns_work, arg, destroy_dns_arg,
                                               dns_complete, lira_rt_mem_free);
    if (parked == 1) {
        return result != NULL ? result : lira_rt_str_new("", 0);
    }
    if (parked < 0) {
        destroy_dns_arg(arg);
        return lira_rt_str_new("", 0);
    }
    void *out = NULL;
    int status = dns_work(arg, &out);
    destroy_dns_arg(arg);
    DnsResult *sync = (DnsResult *)out;
    if (sync == NULL || status != 0) {
        lira_rt_mem_free(sync);
        return lira_rt_str_new("", 0);
    }
    result = lira_rt_str_new(sync->text, strlen(sync->text));
    lira_rt_mem_free(sync);
    return result;
}

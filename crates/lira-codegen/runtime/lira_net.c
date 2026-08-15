/*
 * TCP and DNS built-ins.
 *
 * Sockets are blocking, so a fiber that reads from one parks the whole process
 * rather than yielding — the same limitation the bytecode VM has outside its
 * I/O pool. Handles are small integers into a table, matching the file API.
 */
#include "lira_rt.h"

#include <arpa/inet.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

void lira_rt_panic(const char *message);

#define LIRA_MAX_SOCKETS 128
static int g_sockets[LIRA_MAX_SOCKETS];
static int g_sockets_ready = 0;

static void lira_sockets_init(void) {
    if (g_sockets_ready) {
        return;
    }
    for (int i = 0; i < LIRA_MAX_SOCKETS; i++) {
        g_sockets[i] = -1;
    }
    g_sockets_ready = 1;
}

int64_t lira_rt_tcp_connect(const LiraStr *host, int64_t port) {
    lira_sockets_init();
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
    if (getaddrinfo(host->data, service, &hints, &results) != 0) {
        return -1;
    }

    int fd = -1;
    for (struct addrinfo *it = results; it != NULL; it = it->ai_next) {
        fd = socket(it->ai_family, it->ai_socktype, it->ai_protocol);
        if (fd < 0) {
            continue;
        }
        if (connect(fd, it->ai_addr, it->ai_addrlen) == 0) {
            break;
        }
        close(fd);
        fd = -1;
    }
    freeaddrinfo(results);
    if (fd < 0) {
        return -1;
    }

    for (int64_t handle = 0; handle < LIRA_MAX_SOCKETS; handle++) {
        if (g_sockets[handle] < 0) {
            g_sockets[handle] = fd;
            return handle;
        }
    }
    close(fd);
    return -1;
}

static int lira_socket(int64_t handle) {
    lira_sockets_init();
    if (handle < 0 || handle >= LIRA_MAX_SOCKETS) {
        return -1;
    }
    return g_sockets[handle];
}

int64_t lira_rt_tcp_write(int64_t handle, const LiraStr *data) {
    int fd = lira_socket(handle);
    if (fd < 0 || data == NULL) {
        return -1;
    }
    ssize_t written = send(fd, data->data, (size_t)data->len, 0);
    return (int64_t)written;
}

LiraStr *lira_rt_tcp_read(int64_t handle, int64_t max_bytes) {
    int fd = lira_socket(handle);
    if (fd < 0 || max_bytes <= 0) {
        return lira_rt_str_new("", 0);
    }
    if (max_bytes > 1024 * 1024) {
        max_bytes = 1024 * 1024;
    }
    char *buffer = (char *)malloc((size_t)max_bytes);
    if (buffer == NULL) {
        lira_rt_panic("out of memory");
    }
    ssize_t read_bytes = recv(fd, buffer, (size_t)max_bytes, 0);
    LiraStr *out = lira_rt_str_new(buffer, read_bytes > 0 ? (int64_t)read_bytes : 0);
    free(buffer);
    return out;
}

int8_t lira_rt_tcp_close(int64_t handle) {
    int fd = lira_socket(handle);
    if (fd < 0) {
        return 0;
    }
    g_sockets[handle] = -1;
    return close(fd) == 0 ? 1 : 0;
}

LiraStr *lira_rt_dns_lookup(const LiraStr *host) {
    if (host == NULL) {
        return lira_rt_str_new("", 0);
    }
    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    struct addrinfo *results = NULL;
    if (getaddrinfo(host->data, NULL, &hints, &results) != 0 || results == NULL) {
        return lira_rt_str_new("", 0);
    }

    char text[INET6_ADDRSTRLEN];
    text[0] = '\0';
    if (results->ai_family == AF_INET) {
        struct sockaddr_in *addr = (struct sockaddr_in *)results->ai_addr;
        inet_ntop(AF_INET, &addr->sin_addr, text, sizeof(text));
    } else if (results->ai_family == AF_INET6) {
        struct sockaddr_in6 *addr = (struct sockaddr_in6 *)results->ai_addr;
        inet_ntop(AF_INET6, &addr->sin6_addr, text, sizeof(text));
    }
    freeaddrinfo(results);
    return lira_rt_str_new(text, (int64_t)strlen(text));
}

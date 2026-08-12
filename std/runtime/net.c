/*
 * net.c — TCP networking operations (uses platform socket API).
 */
#include "platform.h"
#include "net.h"

/* lazy-init flag for WSAStartup on Windows */
static int _net_ready = 0;

long long gobol_tcp_connect(const char *addr, long long port) {
    if (!addr) return 0;
    if (!_net_ready) { _net_ready = 1; gobol_net_init(); }
#ifdef _WIN32
    SOCKET fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd == INVALID_SOCKET) return 0;
#else
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 0;
#endif
    struct sockaddr_in sa;
    memset(&sa, 0, sizeof(sa));
    sa.sin_family = AF_INET;
    sa.sin_port = htons((uint16_t)port);
#ifdef _WIN32
    if (inet_pton(AF_INET, addr, &sa.sin_addr) <= 0) { closesocket(fd); return 0; }
    if (connect(fd, (struct sockaddr *)&sa, sizeof(sa)) == SOCKET_ERROR) { closesocket(fd); return 0; }
#else
    if (inet_pton(AF_INET, addr, &sa.sin_addr) <= 0) { close(fd); return 0; }
    if (connect(fd, (struct sockaddr *)&sa, sizeof(sa)) < 0) { close(fd); return 0; }
#endif
    return (long long)fd;
}

long long gobol_tcp_send(long long fd, const char *data) {
    if (!data) return 0;
#ifdef _WIN32
    return (long long)send((SOCKET)fd, data, (int)strlen(data), 0);
#else
    return (long long)send((int)fd, data, strlen(data), 0);
#endif
}

char *gobol_tcp_recv(long long fd, long long len) {
    if (len < 0) len = 0;
    char *buf = (char *)malloc((size_t)len + 1);
    if (!buf) return gobol_strdup("");
#ifdef _WIN32
    long long n = recv((SOCKET)fd, buf, (int)len, 0);
#else
    long long n = recv((int)fd, buf, (size_t)len, 0);
#endif
    if (n < 0) n = 0;
    buf[n] = '\0';
    return buf;
}

void gobol_tcp_close(long long fd) {
    if (fd <= 0) return;
    GOBOL_CLOSE_SOCKET((intptr_t)fd);
}

long long gobol_tcp_bind(const char *addr, long long port) {
    if (!addr) return 0;
    if (!_net_ready) { _net_ready = 1; gobol_net_init(); }
#ifdef _WIN32
    SOCKET fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd == INVALID_SOCKET) return 0;
    BOOL opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, (const char *)&opt, sizeof(opt));
#else
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return 0;
    int opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
#endif
    struct sockaddr_in sa;
    memset(&sa, 0, sizeof(sa));
    sa.sin_family = AF_INET;
    sa.sin_port = htons((uint16_t)port);
    if (inet_pton(AF_INET, addr, &sa.sin_addr) <= 0) { GOBOL_CLOSE_SOCKET(fd); return 0; }
    if (bind(fd, (struct sockaddr *)&sa, sizeof(sa)) < 0) { GOBOL_CLOSE_SOCKET(fd); return 0; }
#ifdef _WIN32
    if (listen(fd, 16) == SOCKET_ERROR) { closesocket(fd); return 0; }
#else
    if (listen(fd, 16) < 0) { close(fd); return 0; }
#endif
    return (long long)fd;
}

long long gobol_tcp_accept(long long fd) {
    struct sockaddr_in ca;
#ifdef _WIN32
    int clen = sizeof(ca);
    SOCKET cfd = accept((SOCKET)fd, (struct sockaddr *)&ca, &clen);
    return (cfd == INVALID_SOCKET) ? 0 : (long long)cfd;
#else
    socklen_t clen = sizeof(ca);
    int cfd = accept((int)fd, (struct sockaddr *)&ca, &clen);
    return cfd < 0 ? 0 : (long long)cfd;
#endif
}

/*
 * net.c — TCP networking operations.
 */
#include "platform.h"
#include "net.h"

#if defined(_WIN32)
  #include <winsock2.h>
  #include <ws2tcpip.h>
#else
  #include <sys/socket.h>
  #include <sys/select.h>
  #include <netinet/in.h>
  #include <netdb.h>
  #include <fcntl.h>
  #include <poll.h>
  #include <unistd.h>
  #include <arpa/inet.h>
#endif

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <stdarg.h>
#include <errno.h>

/* Provided by Gobol runtime (declaration matches gc.h / gc.c) */
extern void *gobol_gc_alloc(long long size);
extern void gobol_free(void *ptr);

/* Error codes (must match net.h) */
#define NET_ERR_GENERAL         -1
#define NET_ERR_CONN_REFUSED    -2
#define NET_ERR_TIMEOUT         -3
#define NET_ERR_NET_UNREACH     -4
#define NET_ERR_HOST_NOT_FOUND  -5
#define NET_ERR_WOULD_BLOCK     -6
#define NET_ERR_CONN_CLOSED     -7
#define NET_ERR_INVALID_PARAM   -8
#define NET_ERR_TIMEOUT_EXPIRED -9
#define NET_ERR_TOO_MANY_FDS    -10
#define NET_ERR_ADDR_IN_USE     -11
#define NET_ERR_PERM_DENIED     -12
#define NET_ERR_INTR            -13
#define NET_ERR_NOT_CONNECTED   -14

static int _net_initialized = 0;

/* === Error Helpers === */

static int map_socket_error(void) {
#ifdef _WIN32
    int err = WSAGetLastError();
    switch (err) {
        case WSAECONNREFUSED: return NET_ERR_CONN_REFUSED;
        case WSAETIMEDOUT:    return NET_ERR_TIMEOUT;
        case WSAENETUNREACH:  return NET_ERR_NET_UNREACH;
        case WSAEHOSTUNREACH: return NET_ERR_HOST_NOT_FOUND;
        case WSAEWOULDBLOCK:  return NET_ERR_WOULD_BLOCK;
        case WSAECONNRESET:   return NET_ERR_CONN_CLOSED;
        case WSAEINVAL:       return NET_ERR_INVALID_PARAM;
        case WSAEMFILE:       return NET_ERR_TOO_MANY_FDS;
        case WSAEADDRINUSE:   return NET_ERR_ADDR_IN_USE;
        case WSAEACCES:       return NET_ERR_PERM_DENIED;
        case WSAEINTR:        return NET_ERR_INTR;
        case WSAENOTCONN:     return NET_ERR_NOT_CONNECTED;
        default:              return NET_ERR_GENERAL;
    }
#else
    switch (errno) {
        case ECONNREFUSED: return NET_ERR_CONN_REFUSED;
        case ETIMEDOUT:    return NET_ERR_TIMEOUT;
        case ENETUNREACH:  return NET_ERR_NET_UNREACH;
        case EHOSTUNREACH: return NET_ERR_HOST_NOT_FOUND;
        case EAGAIN:       return NET_ERR_WOULD_BLOCK;
        case ECONNRESET:   return NET_ERR_CONN_CLOSED;
        case EPIPE:        return NET_ERR_CONN_CLOSED;
        case EINVAL:       return NET_ERR_INVALID_PARAM;
        case EMFILE:       return NET_ERR_TOO_MANY_FDS;
        case EADDRINUSE:   return NET_ERR_ADDR_IN_USE;
        case EACCES:       return NET_ERR_PERM_DENIED;
        case EINTR:        return NET_ERR_INTR;
        case ENOTCONN:     return NET_ERR_NOT_CONNECTED;
        default:           return NET_ERR_GENERAL;
    }
#endif
}

static void set_error(char **err_msg, const char *fmt, ...) {
    if (!err_msg) return;
    char buf[512];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buf, sizeof(buf), fmt, args);
    va_end(args);
    size_t len = strlen(buf) + 1;
    char *msg = (char *)gobol_gc_alloc(len);
    if (msg) { memcpy(msg, buf, len); *err_msg = msg; }
}

static void set_system_error(char **err_msg, const char *context) {
#ifdef _WIN32
    char *msg_buf = NULL;
    FormatMessageA(FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM,
                   NULL, WSAGetLastError(), 0, (LPSTR)&msg_buf, 0, NULL);
    if (msg_buf) {
        set_error(err_msg, "%s: %s", context, msg_buf);
        LocalFree(msg_buf);
    } else {
        set_error(err_msg, "%s: unknown error", context);
    }
#else
    set_error(err_msg, "%s: %s", context, strerror(errno));
#endif
}

/* === Socket Helpers === */

static int set_socket_timeout(net_socket_t fd, long long timeout_ms,
                              int is_recv, char **err_msg) {
#ifdef _WIN32
    DWORD tout = (DWORD)timeout_ms;
    int opt = is_recv ? SO_RCVTIMEO : SO_SNDTIMEO;
    if (setsockopt((SOCKET)fd, SOL_SOCKET, opt, (const char *)&tout, sizeof(tout)) < 0) {
        set_system_error(err_msg, is_recv ? "setsockopt(SO_RCVTIMEO)" : "setsockopt(SO_SNDTIMEO)");
        return map_socket_error();
    }
#else
    struct timeval tv;
    tv.tv_sec = timeout_ms / 1000;
    tv.tv_usec = (timeout_ms % 1000) * 1000;
    int opt = is_recv ? SO_RCVTIMEO : SO_SNDTIMEO;
    if (setsockopt((int)fd, SOL_SOCKET, opt, &tv, sizeof(tv)) < 0) {
        set_system_error(err_msg, is_recv ? "setsockopt(SO_RCVTIMEO)" : "setsockopt(SO_SNDTIMEO)");
        return map_socket_error();
    }
#endif
    return 0;
}

static net_socket_t create_socket(const char *addr, long long port,
                                  int passive, char **err_msg) {
    struct addrinfo hints, *res, *rp;
    int sock = -1;
    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%lld", port);

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    if (passive) hints.ai_flags = AI_PASSIVE;

    if (getaddrinfo(addr, port_str, &hints, &res) != 0) {
        set_system_error(err_msg, "getaddrinfo");
        return NET_ERR_HOST_NOT_FOUND;
    }

    for (rp = res; rp != NULL; rp = rp->ai_next) {
#ifdef _WIN32
        SOCKET fd = socket(rp->ai_family, rp->ai_socktype, rp->ai_protocol);
        if (fd == INVALID_SOCKET) continue;
        sock = (net_socket_t)fd;
#else
        int fd = socket(rp->ai_family, rp->ai_socktype, rp->ai_protocol);
        if (fd < 0) continue;
        sock = (net_socket_t)fd;
#endif
        break;
    }
    freeaddrinfo(res);

    if (sock < 0) {
        set_error(err_msg, "No address available for %s:%lld", addr, port);
        return NET_ERR_GENERAL;
    }
    return (net_socket_t)sock;
}

static int connect_with_timeout(net_socket_t fd, struct addrinfo *addr_list,
                                long long timeout_ms, char **err_msg) {
    int connected = 0;
    int last_err = 0;

    for (struct addrinfo *rp = addr_list; rp != NULL; rp = rp->ai_next) {
#ifdef _WIN32
        SOCKET s = (SOCKET)fd;
#else
        int s = (int)fd;
#endif

        if (timeout_ms <= 0) {
#ifdef _WIN32
            if (connect(s, rp->ai_addr, (int)rp->ai_addrlen) == 0) {
                connected = 1; break;
            }
#else
            if (connect(s, rp->ai_addr, rp->ai_addrlen) == 0) {
                connected = 1; break;
            }
#endif
            last_err = map_socket_error();
            continue;
        }

#ifdef _WIN32
        u_long mode = 1;
        ioctlsocket(s, FIONBIO, &mode);
#else
        int flags = fcntl(s, F_GETFL, 0);
        fcntl(s, F_SETFL, flags | O_NONBLOCK);
#endif

#ifdef _WIN32
        int ret = connect(s, rp->ai_addr, (int)rp->ai_addrlen);
        if (ret == 0) {
            connected = 1;
            u_long mode0 = 0;
            ioctlsocket(s, FIONBIO, &mode0);
            break;
        }
        if (WSAGetLastError() != WSAEWOULDBLOCK) {
            last_err = map_socket_error();
            u_long mode0 = 0;
            ioctlsocket(s, FIONBIO, &mode0);
            continue;
        }
#else
        int ret = connect(s, rp->ai_addr, rp->ai_addrlen);
        if (ret == 0) {
            connected = 1;
            fcntl(s, F_SETFL, flags);
            break;
        }
        if (errno != EINPROGRESS) {
            last_err = map_socket_error();
            fcntl(s, F_SETFL, flags);
            continue;
        }
#endif

#ifdef _WIN32
        fd_set wfds, efds;
        FD_ZERO(&wfds);
        FD_ZERO(&efds);
        FD_SET(s, &wfds);
        FD_SET(s, &efds);
        struct timeval tv;
        tv.tv_sec = (long)(timeout_ms / 1000);
        tv.tv_usec = (long)((timeout_ms % 1000) * 1000);
        ret = select((int)s + 1, NULL, &wfds, &efds, &tv);
        u_long mode0 = 0;
        ioctlsocket(s, FIONBIO, &mode0);
        if (ret < 0) { last_err = map_socket_error(); continue; }
        if (ret == 0) { last_err = NET_ERR_TIMEOUT; continue; }
        if (FD_ISSET(s, &wfds)) { connected = 1; break; }
        if (FD_ISSET(s, &efds)) { last_err = map_socket_error(); continue; }
#else
        struct pollfd pfd;
        pfd.fd = s;
        pfd.events = POLLOUT;
        ret = poll(&pfd, 1, (int)timeout_ms);
        fcntl(s, F_SETFL, flags);
        if (ret < 0) { last_err = map_socket_error(); continue; }
        if (ret == 0) { last_err = NET_ERR_TIMEOUT; continue; }
        if (pfd.revents & POLLOUT) {
            int err;
            socklen_t len = sizeof(err);
            getsockopt(s, SOL_SOCKET, SO_ERROR, &err, &len);
            if (err == 0) { connected = 1; break; }
            last_err = map_socket_error();
            continue;
        }
#endif
    }

    if (!connected) {
        if (last_err == 0) last_err = NET_ERR_GENERAL;
        set_error(err_msg, "connect failed (all addresses tried)");
        return last_err;
    }
    return 0;
}

/* === Public Functions === */

/* Lazy network init. On Windows this drives WSAStartup; on POSIX it is a
 * no-op. Guarded by _net_initialized so we only init once. The public
 * gobol_net_init()/gobol_net_cleanup() entry points live in platform.h
 * (they wrap WSACleanup). */
static int net_init_impl(char **err_msg) {
    if (_net_initialized) return 1;
#ifdef _WIN32
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        set_system_error(err_msg, "WSAStartup");
        return NET_ERR_GENERAL;
    }
#endif
    _net_initialized = 1;
    return 1;
}

static void net_cleanup_impl(void) {
#ifdef _WIN32
    if (_net_initialized) { WSACleanup(); _net_initialized = 0; }
#endif
}

net_socket_t gobol_tcp_connect(const char *addr, long long port,
                               long long timeout_ms, char **err_msg) {
    if (!addr || port <= 0) {
        set_error(err_msg, "Invalid address or port");
        return NET_ERR_INVALID_PARAM;
    }
    if (!_net_initialized) {
        int r = net_init_impl(err_msg);
        if (r < 0) return r;
    }

    net_socket_t fd = create_socket(addr, port, 0, err_msg);
    if (fd < 0) return fd;

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%lld", port);
    struct addrinfo hints, *res;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;

    if (getaddrinfo(addr, port_str, &hints, &res) != 0) {
        set_system_error(err_msg, "getaddrinfo");
        gobol_tcp_close(fd, NULL);
        return NET_ERR_HOST_NOT_FOUND;
    }

    int err = connect_with_timeout(fd, res, timeout_ms, err_msg);
    freeaddrinfo(res);

    if (err < 0) { gobol_tcp_close(fd, NULL); return err; }
    return fd;
}

long long gobol_tcp_send_str(net_socket_t fd, const char *data, char **err_msg) {
    if (fd <= 0 || !data) {
        set_error(err_msg, "Invalid fd or null data");
        return NET_ERR_INVALID_PARAM;
    }
    return gobol_tcp_send_bytes(fd, (const unsigned char *)data,
                                (long long)strlen(data), err_msg);
}

long long gobol_tcp_send_bytes(net_socket_t fd, const unsigned char *buf,
                               long long len, char **err_msg) {
    if (fd <= 0 || !buf || len <= 0) {
        set_error(err_msg, "Invalid fd, null buffer, or zero length");
        return NET_ERR_INVALID_PARAM;
    }

    long long total = 0;
    while (total < len) {
#ifdef _WIN32
        long long n = (long long)send((SOCKET)fd, (const char *)(buf + total),
                                      (int)(len - total), 0);
        if (n < 0) {
            if (WSAGetLastError() == WSAEINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "send");
            return err;
        }
#else
        long long n = (long long)send((int)fd, buf + total,
                                      (size_t)(len - total), 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "send");
            return err;
        }
#endif
        if (n == 0) { set_error(err_msg, "Connection closed"); return NET_ERR_CONN_CLOSED; }
        total += n;
    }
    return total;
}

unsigned char *gobol_tcp_recv_bytes(net_socket_t fd, long long max_len,
                                    long long *out_len, char **err_msg) {
    if (fd <= 0 || max_len <= 0) {
        set_error(err_msg, "Invalid fd or max_len");
        if (out_len) *out_len = 0;
        return NULL;
    }

    unsigned char *buf = (unsigned char *)gobol_gc_alloc((size_t)max_len + 1);
    if (!buf) {
        set_error(err_msg, "Memory allocation failed");
        if (out_len) *out_len = 0;
        return NULL;
    }

    while (1) {
#ifdef _WIN32
        long long n = (long long)recv((SOCKET)fd, (char *)buf, (int)max_len, 0);
        if (n < 0) {
            if (WSAGetLastError() == WSAEINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "recv");
            if (out_len) *out_len = 0;
            gobol_free(buf);
            return NULL;
        }
#else
        long long n = (long long)recv((int)fd, buf, (size_t)max_len, 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "recv");
            if (out_len) *out_len = 0;
            gobol_free(buf);
            return NULL;
        }
#endif
        if (n == 0) {
            set_error(err_msg, "Connection closed by peer");
            if (out_len) *out_len = 0;
            gobol_free(buf);
            return NULL;
        }
        if (out_len) *out_len = n;
        return buf;
    }
}

unsigned char *gobol_tcp_recv_exact(net_socket_t fd, long long len,
                                    long long *out_len, char **err_msg) {
    if (fd <= 0 || len <= 0) {
        set_error(err_msg, "Invalid fd or len");
        if (out_len) *out_len = 0;
        return NULL;
    }

    unsigned char *buf = (unsigned char *)gobol_gc_alloc((size_t)len + 1);
    if (!buf) {
        set_error(err_msg, "Memory allocation failed");
        if (out_len) *out_len = 0;
        return NULL;
    }

    long long total = 0;
    while (total < len) {
#ifdef _WIN32
        long long n = (long long)recv((SOCKET)fd, (char *)(buf + total),
                                      (int)(len - total), 0);
        if (n < 0) {
            if (WSAGetLastError() == WSAEINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "recv");
            if (out_len) *out_len = total;
            gobol_free(buf);
            return NULL;
        }
#else
        long long n = (long long)recv((int)fd, buf + total,
                                      (size_t)(len - total), 0);
        if (n < 0) {
            if (errno == EINTR) continue;
            int err = map_socket_error();
            set_system_error(err_msg, "recv");
            if (out_len) *out_len = total;
            gobol_free(buf);
            return NULL;
        }
#endif
        if (n == 0) {
            set_error(err_msg, "Connection closed unexpectedly");
            if (out_len) *out_len = total;
            gobol_free(buf);
            return NULL;
        }
        total += n;
    }

    if (out_len) *out_len = total;
    return buf;
}

long long gobol_tcp_close(net_socket_t fd, char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NET_ERR_INVALID_PARAM;
    }
#ifdef _WIN32
    if (closesocket((SOCKET)fd) != 0) {
        set_system_error(err_msg, "closesocket");
        return map_socket_error();
    }
#else
    if (close((int)fd) != 0) {
        if (errno == EINTR) return gobol_tcp_close(fd, err_msg);
        set_system_error(err_msg, "close");
        return map_socket_error();
    }
#endif
    return 1;
}

long long gobol_tcp_set_read_timeout(net_socket_t fd, long long timeout_ms,
                                     char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NET_ERR_INVALID_PARAM;
    }
    return set_socket_timeout(fd, timeout_ms, 1, err_msg);
}

long long gobol_tcp_set_write_timeout(net_socket_t fd, long long timeout_ms,
                                      char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NET_ERR_INVALID_PARAM;
    }
    return set_socket_timeout(fd, timeout_ms, 0, err_msg);
}

long long gobol_tcp_set_keepalive(net_socket_t fd, long long enable,
                                  long long idle_secs, long long interval_secs,
                                  long long probes, char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NET_ERR_INVALID_PARAM;
    }

    int val = enable ? 1 : 0;
#ifdef _WIN32
    if (setsockopt((SOCKET)fd, SOL_SOCKET, SO_KEEPALIVE,
                   (const char *)&val, sizeof(val)) < 0) {
        set_system_error(err_msg, "setsockopt(SO_KEEPALIVE)");
        return map_socket_error();
    }
#else
    if (setsockopt((int)fd, SOL_SOCKET, SO_KEEPALIVE, &val, sizeof(val)) < 0) {
        set_system_error(err_msg, "setsockopt(SO_KEEPALIVE)");
        return map_socket_error();
    }
    if (enable) {
        int tcp_idle = (int)(idle_secs > 0 ? idle_secs : 7200);
        int tcp_intvl = (int)(interval_secs > 0 ? interval_secs : 75);
        int tcp_cnt = (int)(probes > 0 ? probes : 9);
#ifdef TCP_KEEPIDLE
        if (setsockopt((int)fd, IPPROTO_TCP, TCP_KEEPIDLE, &tcp_idle, sizeof(tcp_idle)) < 0) {
            set_system_error(err_msg, "setsockopt(TCP_KEEPIDLE)");
            return map_socket_error();
        }
#endif
#ifdef TCP_KEEPINTVL
        if (setsockopt((int)fd, IPPROTO_TCP, TCP_KEEPINTVL, &tcp_intvl, sizeof(tcp_intvl)) < 0) {
            set_system_error(err_msg, "setsockopt(TCP_KEEPINTVL)");
            return map_socket_error();
        }
#endif
#ifdef TCP_KEEPCNT
        if (setsockopt((int)fd, IPPROTO_TCP, TCP_KEEPCNT, &tcp_cnt, sizeof(tcp_cnt)) < 0) {
            set_system_error(err_msg, "setsockopt(TCP_KEEPCNT)");
            return map_socket_error();
        }
#endif
    }
#endif
    return 1;
}

char *gobol_tcp_local_addr(net_socket_t fd, char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NULL;
    }
    struct sockaddr_storage sa;
    socklen_t len = sizeof(sa);
    char addr_str[256];
#ifdef _WIN32
    if (getsockname((SOCKET)fd, (struct sockaddr *)&sa, &len) != 0) {
        set_system_error(err_msg, "getsockname");
        return NULL;
    }
#else
    if (getsockname((int)fd, (struct sockaddr *)&sa, &len) != 0) {
        if (errno == EINTR) return gobol_tcp_local_addr(fd, err_msg);
        set_system_error(err_msg, "getsockname");
        return NULL;
    }
#endif
    if (getnameinfo((struct sockaddr *)&sa, len, addr_str, sizeof(addr_str),
                    NULL, 0, NI_NUMERICHOST) != 0) {
        set_system_error(err_msg, "getnameinfo");
        return NULL;
    }
    size_t slen = strlen(addr_str) + 1;
    char *result = (char *)gobol_gc_alloc(slen);
    if (result) memcpy(result, addr_str, slen);
    return result;
}

char *gobol_tcp_remote_addr(net_socket_t fd, char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid fd");
        return NULL;
    }
    struct sockaddr_storage sa;
    socklen_t len = sizeof(sa);
    char addr_str[256];
#ifdef _WIN32
    if (getpeername((SOCKET)fd, (struct sockaddr *)&sa, &len) != 0) {
        set_system_error(err_msg, "getpeername");
        return NULL;
    }
#else
    if (getpeername((int)fd, (struct sockaddr *)&sa, &len) != 0) {
        if (errno == EINTR) return gobol_tcp_remote_addr(fd, err_msg);
        set_system_error(err_msg, "getpeername");
        return NULL;
    }
#endif
    if (getnameinfo((struct sockaddr *)&sa, len, addr_str, sizeof(addr_str),
                    NULL, 0, NI_NUMERICHOST) != 0) {
        set_system_error(err_msg, "getnameinfo");
        return NULL;
    }
    size_t slen = strlen(addr_str) + 1;
    char *result = (char *)gobol_gc_alloc(slen);
    if (result) memcpy(result, addr_str, slen);
    return result;
}

long long gobol_tcp_is_alive(net_socket_t fd) {
    if (fd <= 0) return 0;
    char buf[1];
#ifdef _WIN32
    long long r = recv((SOCKET)fd, buf, 0, MSG_PEEK);
    if (r < 0 && WSAGetLastError() == WSAEINTR) return gobol_tcp_is_alive(fd);
    return r >= 0;
#else
    long long r = recv((int)fd, buf, 0, MSG_PEEK);
    if (r < 0 && errno == EINTR) return gobol_tcp_is_alive(fd);
    if (r < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return 1;
    return r >= 0;
#endif
}

net_socket_t gobol_tcp_bind(const char *addr, long long port,
                            long long backlog, long long ipv6_only,
                            char **err_msg) {
    if (!addr || port <= 0) {
        set_error(err_msg, "Invalid address or port");
        return NET_ERR_INVALID_PARAM;
    }
    if (!_net_initialized) {
        int r = net_init_impl(err_msg);
        if (r < 0) return r;
    }

    net_socket_t fd = create_socket(addr, port, 1, err_msg);
    if (fd < 0) return fd;

#ifdef _WIN32
    BOOL opt = 1;
    if (setsockopt((SOCKET)fd, SOL_SOCKET, SO_REUSEADDR,
                   (const char *)&opt, sizeof(opt)) < 0) {
        set_system_error(err_msg, "setsockopt(SO_REUSEADDR)");
        gobol_tcp_close(fd, NULL);
        return map_socket_error();
    }
#else
    int opt = 1;
    if (setsockopt((int)fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) < 0) {
        set_system_error(err_msg, "setsockopt(SO_REUSEADDR)");
        gobol_tcp_close(fd, NULL);
        return map_socket_error();
    }
#endif

#ifdef IPV6_V6ONLY
    {
        int val = ipv6_only ? 1 : 0;
        int is_ipv6 = 0;

#ifdef __linux__
        // Linux: use SO_DOMAIN to get socket address family
        int af;
        socklen_t len = sizeof(af);
        if (getsockopt((int)fd, SOL_SOCKET, SO_DOMAIN, &af, &len) == 0) {
            is_ipv6 = (af == AF_INET6);
        }
#else
        // macOS, BSD, Windows: use getsockname to get address family
        struct sockaddr_storage sa;
        socklen_t sa_len = sizeof(sa);
        if (getsockname((int)fd, (struct sockaddr *)&sa, &sa_len) == 0) {
            is_ipv6 = (sa.ss_family == AF_INET6);
        }
#endif

        if (is_ipv6) {
            if (setsockopt((int)fd, IPPROTO_IPV6, IPV6_V6ONLY, &val, sizeof(val)) < 0) {
                // Non-fatal on some platforms (e.g., IPv4 socket returns error)
                if (errno != ENOPROTOOPT && errno != EINVAL) {
                    set_system_error(err_msg, "setsockopt(IPV6_V6ONLY)");
                }
            }
        }
    }
#endif

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%lld", port);
    struct addrinfo hints, *res;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    hints.ai_flags = AI_PASSIVE;

    if (getaddrinfo(addr, port_str, &hints, &res) != 0) {
        set_system_error(err_msg, "getaddrinfo");
        gobol_tcp_close(fd, NULL);
        return NET_ERR_HOST_NOT_FOUND;
    }

    int bound = 0;
    for (struct addrinfo *rp = res; rp != NULL; rp = rp->ai_next) {
#ifdef _WIN32
        if (bind((SOCKET)fd, rp->ai_addr, (int)rp->ai_addrlen) == 0) {
            bound = 1; break;
        }
#else
        if (bind((int)fd, rp->ai_addr, rp->ai_addrlen) == 0) {
            bound = 1; break;
        }
#endif
    }
    freeaddrinfo(res);

    if (!bound) {
        set_system_error(err_msg, "bind");
        gobol_tcp_close(fd, NULL);
        return map_socket_error();
    }

    int backlog_val = (backlog <= 0) ? 128 : (int)backlog;
#ifdef _WIN32
    if (listen((SOCKET)fd, backlog_val) == SOCKET_ERROR) {
        set_system_error(err_msg, "listen");
        gobol_tcp_close(fd, NULL);
        return map_socket_error();
    }
#else
    if (listen((int)fd, backlog_val) < 0) {
        if (errno == EINTR) return gobol_tcp_bind(addr, port, backlog, ipv6_only, err_msg);
        set_system_error(err_msg, "listen");
        gobol_tcp_close(fd, NULL);
        return map_socket_error();
    }
#endif

    return fd;
}

net_socket_t gobol_tcp_accept(net_socket_t fd, long long timeout_ms,
                              char **err_msg) {
    if (fd <= 0) {
        set_error(err_msg, "Invalid listener fd");
        return NET_ERR_INVALID_PARAM;
    }

    if (timeout_ms > 0) {
        long long r = set_socket_timeout(fd, timeout_ms, 1, err_msg);
        if (r < 0) return r;
    }

    struct sockaddr_storage ca;
#ifdef _WIN32
    int clen = sizeof(ca);
    SOCKET cfd;
    while (1) {
        cfd = accept((SOCKET)fd, (struct sockaddr *)&ca, &clen);
        if (cfd != INVALID_SOCKET) break;
        if (WSAGetLastError() == WSAEINTR) continue;
        set_system_error(err_msg, "accept");
        return map_socket_error();
    }
    return (net_socket_t)cfd;
#else
    socklen_t clen = sizeof(ca);
    int cfd;
    while (1) {
        cfd = accept((int)fd, (struct sockaddr *)&ca, &clen);
        if (cfd >= 0) break;
        if (errno == EINTR) continue;
        set_system_error(err_msg, "accept");
        return map_socket_error();
    }
    return (net_socket_t)cfd;
#endif
}

long long gobol_tcp_listener_close(net_socket_t fd, char **err_msg) {
    return gobol_tcp_close(fd, err_msg);
}
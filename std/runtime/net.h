/*
 * net.h — TCP networking operations.
 */
#ifndef GOBOL_RT_NET_H
#define GOBOL_RT_NET_H

typedef long long net_socket_t;

/* TcpStream */
net_socket_t gobol_tcp_connect(const char *addr, long long port,
                               long long timeout_ms, char **err_msg);
long long gobol_tcp_send_str(net_socket_t fd, const char *data, char **err_msg);
long long gobol_tcp_send_bytes(net_socket_t fd, const unsigned char *buf,
                               long long len, char **err_msg);
unsigned char *gobol_tcp_recv_bytes(net_socket_t fd, long long max_len,
                                    long long *out_len, char **err_msg);
unsigned char *gobol_tcp_recv_exact(net_socket_t fd, long long len,
                                    long long *out_len, char **err_msg);
long long gobol_tcp_close(net_socket_t fd, char **err_msg);
long long gobol_tcp_set_read_timeout(net_socket_t fd, long long timeout_ms,
                                     char **err_msg);
long long gobol_tcp_set_write_timeout(net_socket_t fd, long long timeout_ms,
                                      char **err_msg);
long long gobol_tcp_set_keepalive(net_socket_t fd, long long enable,
                                  long long idle_secs, long long interval_secs,
                                  long long probes, char **err_msg);
char *gobol_tcp_local_addr(net_socket_t fd, char **err_msg);
char *gobol_tcp_remote_addr(net_socket_t fd, char **err_msg);
long long gobol_tcp_is_alive(net_socket_t fd);

/* TcpListener */
net_socket_t gobol_tcp_bind(const char *addr, long long port,
                            long long backlog, long long ipv6_only,
                            char **err_msg);
net_socket_t gobol_tcp_accept(net_socket_t fd, long long timeout_ms,
                              char **err_msg);
long long gobol_tcp_listener_close(net_socket_t fd, char **err_msg);

#endif
/*
 * net.h — TCP networking operations.
 */
#ifndef GOBOL_RT_NET_H
#define GOBOL_RT_NET_H

long long gobol_tcp_connect(const char *addr, long long port);
long long gobol_tcp_send(long long fd, const char *data);
char *gobol_tcp_recv(long long fd, long long len);
void  gobol_tcp_close(long long fd);
long long gobol_tcp_bind(const char *addr, long long port);
long long gobol_tcp_accept(long long fd);

#endif /* GOBOL_RT_NET_H */

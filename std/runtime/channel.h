/*
 * channel.h — Thread-safe message queue (mutex + condition variable).
 */
#ifndef GOBOL_RT_CHANNEL_H
#define GOBOL_RT_CHANNEL_H

long long gobol_chan_create(void);
long long gobol_chan_send(long long chan_id, long long data);
long long gobol_chan_recv(long long chan_id);
void  gobol_chan_destroy(long long chan_id);

#endif /* GOBOL_RT_CHANNEL_H */

/*
 * time.h — Hosted clock and sleep functions.
 */
#ifndef GOBOL_RT_TIME_H
#define GOBOL_RT_TIME_H

long long gobol_time_monotonic_ms(void);
long long gobol_time_unix_ms(void);
long long gobol_time_sleep_ms(long long milliseconds);

#endif /* GOBOL_RT_TIME_H */

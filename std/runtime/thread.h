/*
 * thread.h — Thread spawn / join.
 */
#ifndef GOBOL_RT_THREAD_H
#define GOBOL_RT_THREAD_H

long long gobol_thread_spawn(long long func_ptr, long long arg);
long long gobol_thread_join(long long thread_id);

#endif /* GOBOL_RT_THREAD_H */

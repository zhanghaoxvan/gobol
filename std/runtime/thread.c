/*
 * thread.c — Thread spawn / join (uses platform thread API).
 */
#include "platform.h"
#include "thread.h"

typedef long long (*_gobol_thread_fn)(long long);

typedef struct {
    _gobol_thread_fn fn;
    long long arg;
} _gobol_thread_arg;

static void *_gobol_thread_trampoline(void *p) {
    _gobol_thread_arg *ta = (_gobol_thread_arg *)p;
    long long ret = ta->fn(ta->arg);
    free(ta);
    return (void *)(intptr_t)ret;
}

long long gobol_thread_spawn(long long func_ptr, long long arg) {
    if (!func_ptr) return -1;
    _gobol_thread_arg *ta = (_gobol_thread_arg *)malloc(sizeof(_gobol_thread_arg));
    if (!ta) return -1;
    ta->fn  = (_gobol_thread_fn)func_ptr;
    ta->arg = arg;
    gobol_thread_t tid;
    if (gobol_thread_create(&tid, _gobol_thread_trampoline, ta) != 0) {
        free(ta);
        return -1;
    }
    return (long long)tid;
}

long long gobol_thread_join(long long thread_id) {
    if (thread_id <= 0) return -1;
    gobol_thread_t tid = (gobol_thread_t)thread_id;
    return gobol_thread_join_val(tid);
}

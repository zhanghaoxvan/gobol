/*
 * thread.c — Thread spawn / join (uses platform thread API).
 */
#include "platform.h"
#include "thread.h"

typedef long long (*_gobol_thread_fn)(long long);

typedef struct {
    _gobol_thread_fn fn;
    long long arg;
    /* OS thread exit codes are only 32 bits on Windows. */
    long long result;
    gobol_thread_t thread;
} _gobol_thread_state;

#ifdef _WIN32
static DWORD WINAPI _gobol_thread_trampoline(LPVOID p) {
    _gobol_thread_state *state = (_gobol_thread_state *)p;
    state->result = state->fn(state->arg);
    return 0;
}
#else
static void *_gobol_thread_trampoline(void *p) {
    _gobol_thread_state *state = (_gobol_thread_state *)p;
    state->result = state->fn(state->arg);
    return NULL;
}
#endif

long long gobol_thread_spawn(long long func_ptr, long long arg) {
    if (!func_ptr) return -1;
    _gobol_thread_state *state =
        (_gobol_thread_state *)calloc(1, sizeof(_gobol_thread_state));
    if (!state) return -1;
    state->fn = (_gobol_thread_fn)(intptr_t)func_ptr;
    state->arg = arg;
    if (gobol_thread_create(&state->thread, _gobol_thread_trampoline, state) != 0) {
        free(state);
        return -1;
    }
    /* Keep the result with an opaque handle until gobol_thread_join reads it. */
    return (long long)(intptr_t)state;
}

long long gobol_thread_join(long long thread_id) {
    if (thread_id == 0 || thread_id == -1) return -1;
    _gobol_thread_state *state = (_gobol_thread_state *)(intptr_t)thread_id;
    if (gobol_thread_wait_and_close(state->thread) != 0) return -1;
    long long result = state->result;
    free(state);
    return result;
}

/*
 * platform.h — Cross-platform abstractions for the Gobol runtime.
 *
 * Abstracts threading, synchronisation, and socket APIs so the rest of
 * the runtime can be written against a uniform interface regardless of
 * the target OS.
 */
#ifndef GOBOL_PLATFORM_H
#define GOBOL_PLATFORM_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32

/* ---- Windows ---- */

#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0600
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif

#include <windows.h>
#include <winsock2.h>
#include <ws2tcpip.h>
#include <io.h>
#include <process.h>

#define GOBOL_ACCESS(path, mode) _access(path, mode)
#define GOBOL_F_OK 0
#define GOBOL_CLOSE_SOCKET(s) closesocket(s)
#define GOBOL_CLOSE_FD(fd) _close(fd)

typedef HANDLE gobol_thread_t;
typedef CRITICAL_SECTION gobol_mutex_t;
typedef CONDITION_VARIABLE gobol_cond_t;

#define gobol_ssize_t long long

/* ---- POSIX / Unix ---- */

#else

#include <unistd.h>
#include <pthread.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#define GOBOL_ACCESS(path, mode) access(path, mode)
#define GOBOL_F_OK F_OK
#define GOBOL_CLOSE_SOCKET(s) close(s)
#define GOBOL_CLOSE_FD(fd) close(fd)

typedef pthread_t gobol_thread_t;
typedef pthread_mutex_t gobol_mutex_t;
typedef pthread_cond_t gobol_cond_t;

#include <sys/types.h>
#define gobol_ssize_t ssize_t

#endif /* _WIN32 */

/* ---- Common helpers ---- */

static inline char *gobol_strdup(const char *s) {
    if (!s) return NULL;
    size_t n = strlen(s) + 1;
    char *d = (char *)malloc(n);
    if (d) memcpy(d, s, n);
    return d;
}

#ifdef _WIN32
/* Windows doesn't have getline(3); provide a simple implementation. */
static inline gobol_ssize_t gobol_getline(char **lineptr, size_t *n, FILE *stream) {
    if (!lineptr || !n || !stream) return -1;
    size_t pos = 0;
    int c;
    if (!*lineptr) { *n = 128; *lineptr = (char *)malloc(*n); }
    while ((c = fgetc(stream)) != EOF) {
        if (pos + 2 > *n) {
            *n *= 2;
            *lineptr = (char *)realloc(*lineptr, *n);
        }
        (*lineptr)[pos++] = (char)c;
        if (c == '\n') break;
    }
    if (pos == 0 && c == EOF) return -1;
    (*lineptr)[pos] = '\0';
    return (gobol_ssize_t)pos;
}
#else
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
static inline gobol_ssize_t gobol_getline(char **lineptr, size_t *n, FILE *stream) {
    return getline(lineptr, n, stream);
}
#endif

/* ---- Platform thread API ---- */

static inline int gobol_thread_create(gobol_thread_t *t, void *(*fn)(void *), void *arg) {
#ifdef _WIN32
    HANDLE h = CreateThread(NULL, 0, (LPTHREAD_START_ROUTINE)fn, arg, 0, NULL);
    if (!h) return -1;
    *t = h;
    return 0;
#else
    return pthread_create(t, NULL, fn, arg);
#endif
}

static inline long long gobol_thread_join_val(gobol_thread_t t) {
#ifdef _WIN32
    WaitForSingleObject(t, INFINITE);
    DWORD ret;
    GetExitCodeThread(t, &ret);
    CloseHandle(t);
    return (long long)(intptr_t)ret;
#else
    void *ret = NULL;
    pthread_join(t, &ret);
    return (long long)(intptr_t)ret;
#endif
}

/* ---- Platform mutex API ---- */

static inline void gobol_mutex_init(gobol_mutex_t *m) {
#ifdef _WIN32
    InitializeCriticalSection(m);
#else
    pthread_mutex_init(m, NULL);
#endif
}

static inline void gobol_mutex_lock(gobol_mutex_t *m) {
#ifdef _WIN32
    EnterCriticalSection(m);
#else
    pthread_mutex_lock(m);
#endif
}

static inline void gobol_mutex_unlock(gobol_mutex_t *m) {
#ifdef _WIN32
    LeaveCriticalSection(m);
#else
    pthread_mutex_unlock(m);
#endif
}

static inline void gobol_mutex_destroy(gobol_mutex_t *m) {
#ifdef _WIN32
    DeleteCriticalSection(m);
#else
    pthread_mutex_destroy(m);
#endif
}

/* ---- Platform condition variable API ---- */

static inline void gobol_cond_init(gobol_cond_t *c) {
#ifdef _WIN32
    InitializeConditionVariable(c);
#else
    pthread_cond_init(c, NULL);
#endif
}

static inline void gobol_cond_wait(gobol_cond_t *c, gobol_mutex_t *m) {
#ifdef _WIN32
    SleepConditionVariableCS(c, m, INFINITE);
#else
    pthread_cond_wait(c, m);
#endif
}

static inline void gobol_cond_signal(gobol_cond_t *c) {
#ifdef _WIN32
    WakeConditionVariable(c);
#else
    pthread_cond_signal(c);
#endif
}

static inline void gobol_cond_broadcast(gobol_cond_t *c) {
#ifdef _WIN32
    WakeAllConditionVariable(c);
#else
    pthread_cond_broadcast(c);
#endif
}

static inline void gobol_cond_destroy(gobol_cond_t *c) {
#ifdef _WIN32
    /* CONDITION_VARIABLE has no destroy */
    (void)c;
#else
    pthread_cond_destroy(c);
#endif
}

/* ---- Platform socket init / cleanup (Windows needs WSAStartup) ---- */

static inline int gobol_net_init(void) {
#ifdef _WIN32
    WSADATA wsa;
    return WSAStartup(MAKEWORD(2, 2), &wsa);
#else
    return 0;
#endif
}

static inline void gobol_net_cleanup(void) {
#ifdef _WIN32
    WSACleanup();
#endif
}

#endif /* GOBOL_PLATFORM_H */

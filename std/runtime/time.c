/*
 * time.c — Hosted clock and sleep functions.
 */
#ifndef _WIN32
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#endif

#include "platform.h"
#include "time.h"

#include <errno.h>
#include <time.h>

long long gobol_time_monotonic_ms(void) {
#ifdef _WIN32
    LARGE_INTEGER counter;
    LARGE_INTEGER frequency;
    if (!QueryPerformanceCounter(&counter) || !QueryPerformanceFrequency(&frequency)
        || frequency.QuadPart <= 0) {
        return -1;
    }
    return (counter.QuadPart / frequency.QuadPart) * 1000
        + (counter.QuadPart % frequency.QuadPart) * 1000 / frequency.QuadPart;
#else
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (long long)now.tv_sec * 1000 + now.tv_nsec / 1000000;
#endif
}

long long gobol_time_unix_ms(void) {
#ifdef _WIN32
    FILETIME now;
    ULARGE_INTEGER ticks;
    GetSystemTimeAsFileTime(&now);
    ticks.LowPart = now.dwLowDateTime;
    ticks.HighPart = now.dwHighDateTime;
    return (long long)(ticks.QuadPart / 10000ULL) - 11644473600000LL;
#else
    struct timespec now;
    if (clock_gettime(CLOCK_REALTIME, &now) != 0) return -1;
    return (long long)now.tv_sec * 1000 + now.tv_nsec / 1000000;
#endif
}

long long gobol_time_sleep_ms(long long milliseconds) {
    if (milliseconds < 0) return -1;
    if (milliseconds == 0) return 0;

#ifdef _WIN32
    while (milliseconds > 0) {
        DWORD chunk = milliseconds > (long long)MAXDWORD
            ? MAXDWORD
            : (DWORD)milliseconds;
        Sleep(chunk);
        milliseconds -= chunk;
    }
    return 0;
#else
    struct timespec requested = {
        (time_t)(milliseconds / 1000),
        (long)((milliseconds % 1000) * 1000000)
    };
    while (nanosleep(&requested, &requested) != 0) {
        if (errno != EINTR) return -1;
    }
    return 0;
#endif
}

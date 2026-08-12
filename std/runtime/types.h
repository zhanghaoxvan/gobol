/*
 * types.h — Shared types used across the Gobol runtime.
 */
#ifndef GOBOL_RT_TYPES_H
#define GOBOL_RT_TYPES_H

#include "platform.h"

/* GobolArray — matches the #[repr(C)] layout expected by the compiler.
 * { data: *mut i64, len: i64, cap: i64 }                 */
typedef struct {
    long long *data;
    long long  len;
    long long  cap;
} GobolArray;

#endif /* GOBOL_RT_TYPES_H */

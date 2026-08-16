/*
 * array.h — Growable array type (backing store for vec<T> / T[]).
 */
#ifndef GOBOL_RT_ARRAY_H
#define GOBOL_RT_ARRAY_H

#include "types.h"

void *gobol_array_new(void);
void *gobol_array_new_with_size(long long size);
void *gobol_array_new_2d(long long rows, long long cols);
void  gobol_array_add(GobolArray *arr, long long val);
long long gobol_array_len(const GobolArray *arr);
long long gobol_array_get(const GobolArray *arr, long long i);
void  gobol_array_set(GobolArray *arr, long long i, long long val);

#endif /* GOBOL_RT_ARRAY_H */

/*
 * mem.c — Memory allocation and raw pointer access.
 */
#include "platform.h"
#include "types.h"
#include "mem.h"

void *gobol_alloc(long long size) {
    if (size < 0) size = 0;
    return calloc(1, (size_t)size);
}

long long gobol_mem_load(long long addr) {
    if (!addr) return 0;
    return *(long long *)addr;
}

void gobol_mem_store(long long addr, long long val) {
    if (!addr) return;
    *(long long *)addr = val;
}

long long gobol_array_elem_addr(void *arr, long long i) {
    GobolArray *a = (GobolArray *)arr;
    if (!a || i < 0 || i >= a->len) return 0;
    return (long long)(a->data + i);
}

void *gobol_malloc(long long size) {
    if (size <= 0) return NULL;
    return malloc((size_t)size);
}

void gobol_free(void *ptr) {
    if (ptr) free(ptr);
}

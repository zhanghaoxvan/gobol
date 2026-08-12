/*
 * array.c — Growable array type.
 */
#include "platform.h"
#include "types.h"
#include "array.h"

void *gobol_array_new(void) {
    GobolArray *a = (GobolArray *)calloc(1, sizeof(GobolArray));
    return a;
}

void gobol_array_add(GobolArray *arr, long long val) {
    if (!arr) return;
    if (arr->len >= arr->cap) {
        long long new_cap = arr->cap == 0 ? 8 : arr->cap * 2;
        long long *new_data = (long long *)calloc((size_t)new_cap, sizeof(long long));
        if (!new_data) return;
        if (arr->data && arr->len > 0)
            memcpy(new_data, arr->data, (size_t)arr->len * sizeof(long long));
        free(arr->data);
        arr->data = new_data;
        arr->cap = new_cap;
    }
    arr->data[arr->len++] = val;
}

long long gobol_array_len(const GobolArray *arr) {
    return arr ? arr->len : 0;
}

long long gobol_array_get(const GobolArray *arr, long long i) {
    if (!arr || i < 0 || i >= arr->len) return 0;
    return arr->data[i];
}

void gobol_array_set(GobolArray *arr, long long i, long long val) {
    if (!arr || i < 0 || i >= arr->len) return;
    arr->data[i] = val;
}

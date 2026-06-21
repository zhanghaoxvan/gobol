// __builtins__.c — single C companion for the gobol standard library.
// Every function declared here has a matching signature in a gobol source
// file under std/ (e.g. std/io.gbl).
//
// std/io.gbl
//   func print(value: str)    →  void print(const char* value)
//   func println(value: str)  →  void println(const char* value)
//   func read(): str          →  char* read(void)
//
// Helpers:
//   gobol_str_int(i64)   — converts int to static string buffer
//   gobol_str_float(f64) — converts float to static string buffer

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

// ---- public API (matches gobol signatures) ----

void print(const char* value) {
    printf("%s", value);
}

void println(const char* value) {
    printf("%s\n", value);
}

char* read(void) {
    static char buf[4096];
    if (fgets(buf, sizeof(buf), stdin)) {
        size_t len = strlen(buf);
        if (len > 0 && buf[len-1] == '\n') buf[len-1] = '\0';
        return buf;
    }
    return "";
}

// ---- conversion helpers (called by generated code) ----

char* gobol_str_int(int64_t n) {
    static char buf[32];
    snprintf(buf, sizeof(buf), "%ld", n);
    return buf;
}

char* gobol_str_float(double f) {
    static char buf[64];
    snprintf(buf, sizeof(buf), "%g", f);
    return buf;
}

char* gobol_str_cat(const char* a, const char* b) {
    static char buf[8192];
    snprintf(buf, sizeof(buf), "%s%s", a, b);
    return buf;
}

// ---- array runtime ----

typedef struct { int64_t* data; int64_t len; int64_t cap; } gobol_array_t;

void gobol_array_add(gobol_array_t* arr, int64_t val) {
    if (arr->len >= arr->cap) {
        int64_t new_cap = arr->cap ? arr->cap * 2 : 8;
        arr->data = realloc(arr->data, new_cap * sizeof(int64_t));
        arr->cap = new_cap;
    }
    arr->data[arr->len++] = val;
}

int64_t gobol_array_len(gobol_array_t* arr) { return arr->len; }

int64_t gobol_array_get(gobol_array_t* arr, int64_t i) { return arr->data[i]; }

void gobol_array_set(gobol_array_t* arr, int64_t i, int64_t val) { arr->data[i] = val; }

// 2D helpers: flat indexing with stride
int64_t gobol_array_get_flat(gobol_array_t* arr, int64_t i, int64_t j) { return arr->data[i + j]; }
void gobol_array_set_flat(gobol_array_t* arr, int64_t i, int64_t j, int64_t val) { arr->data[i + j] = val; }

/*
 * Gobol AOT runtime — C link-time support library.
 *
 * Provides the same extern "C" functions as the Rust JIT runtime in
 * cranelift.rs, so that Cranelift ObjectModule output can be linked
 * into a standalone executable.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- string helpers ---- */

void gobol_print(const char *s) {
    if (s) fputs(s, stdout);
}

void gobol_println(const char *s) {
    if (s) puts(s);
    else putchar('\n');
}

char *gobol_read(void) {
    char *line = NULL;
    size_t cap = 0;
    ssize_t len = getline(&line, &cap, stdin);
    if (len < 0) return strdup("");
    /* strip trailing newline */
    while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
        line[--len] = '\0';
    return line;
}

char *gobol_str_int(long long n) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%lld", n);
    return strdup(buf);
}

char *gobol_str_float(double f) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%g", f);
    return strdup(buf);
}

char *gobol_str_bool(signed char b) {
    return strdup(b ? "true" : "false");
}

char *gobol_str_cat(const char *a, const char *b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t la = strlen(a), lb = strlen(b);
    char *r = (char *)malloc(la + lb + 1);
    memcpy(r, a, la);
    memcpy(r + la, b, lb);
    r[la + lb] = '\0';
    return r;
}

signed char gobol_str_eq(const char *a, const char *b) {
    if (!a) a = "";
    if (!b) b = "";
    return strcmp(a, b) == 0 ? 1 : 0;
}

long long gobol_str_len(const char *s) {
    return s ? (long long)strlen(s) : 0;
}

long long gobol_str_get(const char *s, long long i) {
    if (!s) return 0;
    long long len = (long long)strlen(s);
    if (i < 0 || i >= len) return 0;
    return (long long)(unsigned char)s[i];
}

/* ---- memory ---- */

void *gobol_alloc(long long size) {
    if (size < 0) size = 0;
    return calloc(1, (size_t)size);
}

/* ---- array runtime ----
 * GobolArray matches the #[repr(C)] struct in cranelift.rs:
 *   { data: *mut i64, len: i64, cap: i64 }
 */

typedef struct {
    long long *data;
    long long len;
    long long cap;
} GobolArray;

void *gobol_array_new(void) {
    GobolArray *a = (GobolArray *)calloc(1, sizeof(GobolArray));
    return a;
}

void gobol_array_add(GobolArray *arr, long long val) {
    if (!arr) return;
    if (arr->len >= arr->cap) {
        long long new_cap = arr->cap == 0 ? 8 : arr->cap * 2;
        long long *new_data = (long long *)calloc((size_t)new_cap, sizeof(long long));
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

/* ---- entry point ----
 * The Cranelift ObjectModule emits the Gobol main body as `gbl_main`
 * (returns i64). The C runtime provides the standard `main` that the
 * system linker's _start expects, and forwards the exit code.
 */
extern long long gbl_main(void);

int main(void) {
    return (int)gbl_main();
}

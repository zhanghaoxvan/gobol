/*
 * str.c — String conversion and manipulation helpers.
 */
#include "platform.h"
#include "str.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

char *gobol_str_int(long long n) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%lld", n);
    return gobol_strdup(buf);
}

char *gobol_str_float(double f) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%g", f);
    return gobol_strdup(buf);
}

char *gobol_str_bool(signed char b) {
    return gobol_strdup(b ? "true" : "false");
}

char *gobol_str_cat(const char *a, const char *b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t la = strlen(a), lb = strlen(b);
    char *r = (char *)malloc(la + lb + 1);
    if (!r) return NULL;
    memcpy(r, a, la);
    memcpy(r + la, b, lb);
    r[la + lb] = '\0';
    return r;
}

long long gobol_str_eq(const char *a, const char *b) {
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

char *gobol_str_char(long long code) {
    char buf[2];
    buf[0] = (char)(unsigned char)code;
    buf[1] = '\0';
    return gobol_strdup(buf);
}

long long gobol_str_contains(const char *s, const char *sub) {
    if (!s) s = "";
    if (!sub) sub = "";
    return strstr(s, sub) != NULL ? 1 : 0;
}

char *gobol_str_trim(const char *s) {
    if (!s) return gobol_strdup("");
    const char *start = s;
    while (*start == ' ' || *start == '\t' || *start == '\n' || *start == '\r')
        start++;
    const char *end = s + strlen(s);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\n' || end[-1] == '\r'))
        end--;
    size_t len = (size_t)(end - start);
    char *r = (char *)malloc(len + 1);
    if (!r) return NULL;
    memcpy(r, start, len);
    r[len] = '\0';
    return r;
}

char *gobol_str_replace(const char *s, const char *from, const char *to) {
    if (!s) s = "";
    if (!from || !*from) return gobol_strdup(s);
    if (!to) to = "";
    size_t from_len = strlen(from);
    size_t to_len   = strlen(to);
    /* count occurrences */
    size_t count = 0;
    const char *p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += from_len; }
    size_t s_len = strlen(s);
    size_t result_len = s_len + count * (to_len - from_len) + 1;
    char *r = (char *)malloc(result_len);
    if (!r) return NULL;
    char *out = r;
    p = s;
    const char *prev = s;
    while ((p = strstr(p, from)) != NULL) {
        size_t copy = (size_t)(p - prev);
        memcpy(out, prev, copy);
        out += copy;
        memcpy(out, to, to_len);
        out += to_len;
        p += from_len;
        prev = p;
    }
    strcpy(out, prev);
    return r;
}

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

static unsigned long gobol_utf8_decode(
    const unsigned char *s, size_t remaining, size_t *width
) {
    unsigned char first = s[0];
    *width = 1;

    if (first < 0x80) {
        return first;
    }

    size_t count;
    unsigned long codepoint;
    if (first >= 0xC2 && first <= 0xDF) {
        count = 2;
        codepoint = first & 0x1F;
    } else if (first >= 0xE0 && first <= 0xEF) {
        count = 3;
        codepoint = first & 0x0F;
    } else if (first >= 0xF0 && first <= 0xF4) {
        count = 4;
        codepoint = first & 0x07;
    } else {
        return 0xFFFD;
    }

    if (remaining < count) {
        return 0xFFFD;
    }
    for (size_t i = 1; i < count; i++) {
        if ((s[i] & 0xC0) != 0x80) {
            return 0xFFFD;
        }
        codepoint = (codepoint << 6) | (s[i] & 0x3F);
    }

    if ((count == 3 && codepoint < 0x800) ||
        (count == 4 && codepoint < 0x10000) ||
        (codepoint >= 0xD800 && codepoint <= 0xDFFF) ||
        codepoint > 0x10FFFF) {
        return 0xFFFD;
    }

    *width = count;
    return codepoint;
}

long long gobol_str_len(const char *s) {
    if (!s) return 0;

    size_t bytes = strlen(s);
    size_t offset = 0;
    long long count = 0;
    while (offset < bytes) {
        size_t width;
        gobol_utf8_decode((const unsigned char *)s + offset, bytes - offset, &width);
        offset += width;
        count++;
    }
    return count;
}

long long gobol_str_get(const char *s, long long i) {
    if (!s || i < 0) return 0;

    size_t bytes = strlen(s);
    size_t offset = 0;
    long long index = 0;
    while (offset < bytes) {
        size_t width;
        unsigned long codepoint =
            gobol_utf8_decode((const unsigned char *)s + offset, bytes - offset, &width);
        if (index == i) return (long long)codepoint;
        offset += width;
        index++;
    }
    return 0;
}

char *gobol_str_char(long long code) {
    if (code < 0 || code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF))
        code = 0xFFFD;

    char buf[5];
    size_t len;
    if (code <= 0x7F) {
        buf[0] = (char)code;
        len = 1;
    } else if (code <= 0x7FF) {
        buf[0] = (char)(0xC0 | (code >> 6));
        buf[1] = (char)(0x80 | (code & 0x3F));
        len = 2;
    } else if (code <= 0xFFFF) {
        buf[0] = (char)(0xE0 | (code >> 12));
        buf[1] = (char)(0x80 | ((code >> 6) & 0x3F));
        buf[2] = (char)(0x80 | (code & 0x3F));
        len = 3;
    } else {
        buf[0] = (char)(0xF0 | (code >> 18));
        buf[1] = (char)(0x80 | ((code >> 12) & 0x3F));
        buf[2] = (char)(0x80 | ((code >> 6) & 0x3F));
        buf[3] = (char)(0x80 | (code & 0x3F));
        len = 4;
    }
    buf[len] = '\0';
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
    size_t result_len = s_len;
    if (to_len >= from_len) {
        result_len += count * (to_len - from_len);
    } else {
        result_len -= count * (from_len - to_len);
    }
    result_len += 1;
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

long long gobol_str_starts_with(const char *s, const char *prefix) {
    if (!s) s = "";
    if (!prefix) prefix = "";
    size_t n = strlen(prefix);
    return strncmp(s, prefix, n) == 0 ? 1 : 0;
}

long long gobol_str_ends_with(const char *s, const char *suffix) {
    if (!s) s = "";
    if (!suffix) suffix = "";
    size_t n = strlen(s);
    size_t m = strlen(suffix);
    return m <= n && memcmp(s + n - m, suffix, m) == 0 ? 1 : 0;
}

long long gobol_str_index_of(const char *s, const char *sub) {
    if (!s) s = "";
    if (!sub) sub = "";
    const char *found = strstr(s, sub);
    return found ? (long long)(found - s) : -1;
}

long long gobol_str_count(const char *s, const char *sub) {
    if (!s) s = "";
    if (!sub || !*sub) return 0;
    size_t count = 0;
    size_t width = strlen(sub);
    const char *p = s;
    while ((p = strstr(p, sub)) != NULL) {
        count++;
        p += width;
    }
    return (long long)count;
}

static char *gobol_str_case_map(const char *s, int upper) {
    if (!s) s = "";
    size_t n = strlen(s);
    char *result = (char *)malloc(n + 1);
    if (!result) return NULL;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        result[i] = (char)(upper ? (c >= 'a' && c <= 'z' ? c - 'a' + 'A' : c)
                                 : (c >= 'A' && c <= 'Z' ? c - 'A' + 'a' : c));
    }
    result[n] = '\0';
    return result;
}

char *gobol_str_to_upper(const char *s) {
    return gobol_str_case_map(s, 1);
}

char *gobol_str_to_lower(const char *s) {
    return gobol_str_case_map(s, 0);
}

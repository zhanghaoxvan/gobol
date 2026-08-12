/*
 * str.h — String conversion and manipulation helpers.
 */
#ifndef GOBOL_RT_STR_H
#define GOBOL_RT_STR_H

char *gobol_str_int(long long n);
char *gobol_str_float(double f);
char *gobol_str_bool(signed char b);
char *gobol_str_cat(const char *a, const char *b);
long long gobol_str_eq(const char *a, const char *b);
long long gobol_str_len(const char *s);
long long gobol_str_get(const char *s, long long i);
long long gobol_str_contains(const char *s, const char *sub);
char *gobol_str_trim(const char *s);
char *gobol_str_replace(const char *s, const char *from, const char *to);

#endif /* GOBOL_RT_STR_H */

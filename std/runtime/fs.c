/*
 * fs.c — File-system operations.
 */
#include "platform.h"
#include "fs.h"

long long gobol_fs_open(const char *path, const char *mode) {
    if (!path || !mode) return 0;
#ifdef _WIN32
    FILE *f = NULL;
    fopen_s(&f, path, mode);
#else
    FILE *f = fopen(path, mode);
#endif
    return f ? (long long)(intptr_t)f : 0;
}

char *gobol_fs_read_all(long long handle) {
    FILE *f = (FILE *)(intptr_t)handle;
    if (!f) return gobol_strdup("");
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (sz < 0) sz = 0;
    char *buf = (char *)malloc((size_t)sz + 1);
    if (!buf) return gobol_strdup("");
    size_t nread = fread(buf, 1, (size_t)sz, f);
    buf[nread] = '\0';
    return buf;
}

long long gobol_fs_write(long long handle, const char *data) {
    FILE *f = (FILE *)(intptr_t)handle;
    if (!f || !data) return 0;
    return (long long)fwrite(data, 1, strlen(data), f);
}

void gobol_fs_close(long long handle) {
    FILE *f = (FILE *)(intptr_t)handle;
    if (f) fclose(f);
}

long long gobol_fs_exists(const char *path) {
    if (!path) return 0;
    return GOBOL_ACCESS(path, GOBOL_F_OK) == 0 ? 1 : 0;
}

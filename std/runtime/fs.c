/*
 * fs.c — File-system operations.
 */
#include "platform.h"
#include "fs.h"

static long long gobol_fs_error = 0;

long long gobol_fs_open(const char *path, const char *mode) {
    gobol_fs_error = 0;
    if (!path || !mode) {
        gobol_fs_error = 1;
        return 0;
    }
#ifdef _WIN32
    FILE *f = NULL;
    fopen_s(&f, path, mode);
#else
    FILE *f = fopen(path, mode);
#endif
    if (!f) gobol_fs_error = 1;
    return f ? (long long)(intptr_t)f : 0;
}

char *gobol_fs_read_all(long long handle) {
    FILE *f = (FILE *)(intptr_t)handle;
    gobol_fs_error = 0;
    if (!f) {
        gobol_fs_error = 1;
        return gobol_strdup("");
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        gobol_fs_error = 1;
        return gobol_strdup("");
    }
    long long size = (long long)ftell(f);
    if (size < 0 || fseek(f, 0, SEEK_SET) != 0) {
        gobol_fs_error = 1;
        return gobol_strdup("");
    }
    char *buf = (char *)malloc((size_t)size + 1);
    if (!buf) {
        gobol_fs_error = 1;
        return gobol_strdup("");
    }
    size_t nread = fread(buf, 1, (size_t)size, f);
    if (nread != (size_t)size && ferror(f)) gobol_fs_error = 1;
    buf[nread] = '\0';
    return buf;
}

long long gobol_fs_write(long long handle, const char *data) {
    FILE *f = (FILE *)(intptr_t)handle;
    gobol_fs_error = 0;
    if (!f || !data) {
        gobol_fs_error = 1;
        return 0;
    }
    size_t written = fwrite(data, 1, strlen(data), f);
    if (written != strlen(data) || fflush(f) != 0) gobol_fs_error = 1;
    return (long long)written;
}

void gobol_fs_close(long long handle) {
    FILE *f = (FILE *)(intptr_t)handle;
    if (f) fclose(f);
}

long long gobol_fs_exists(const char *path) {
    if (!path) return 0;
    return GOBOL_ACCESS(path, GOBOL_F_OK) == 0 ? 1 : 0;
}

long long gobol_fs_valid(long long handle) {
    return handle != 0 ? 1 : 0;
}

long long gobol_fs_last_error(void) {
    return gobol_fs_error;
}

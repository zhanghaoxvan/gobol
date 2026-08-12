/*
 * fs.h — File-system operations.
 */
#ifndef GOBOL_RT_FS_H
#define GOBOL_RT_FS_H

long long gobol_fs_open(const char *path, const char *mode);
char *gobol_fs_read_all(long long handle);
long long gobol_fs_write(long long handle, const char *data);
void  gobol_fs_close(long long handle);
long long gobol_fs_exists(const char *path);

#endif /* GOBOL_RT_FS_H */

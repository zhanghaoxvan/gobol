/*
 * io.h — Standard I/O (stdout / stderr / stdin).
 */
#ifndef GOBOL_RT_IO_H
#define GOBOL_RT_IO_H

void gobol_print(const char *s);
void gobol_println(const char *s);
void gobol_eprint(const char *s);
void gobol_eprintln(const char *s);
char *gobol_read(void);

#endif /* GOBOL_RT_IO_H */

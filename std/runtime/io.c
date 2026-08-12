/*
 * io.c — Standard I/O (stdout / stderr / stdin).
 */
#include "platform.h"
#include "io.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void gobol_print(const char *s) {
    if (s) fputs(s, stdout);
}

void gobol_println(const char *s) {
    if (s) puts(s);
    else putchar('\n');
}

void gobol_eprint(const char *s) {
    if (s) fputs(s, stderr);
}

void gobol_eprintln(const char *s) {
    if (s) fputs(s, stderr);
    fputc('\n', stderr);
}

char *gobol_read(void) {
    char *line = NULL;
    size_t cap = 0;
    gobol_ssize_t len = gobol_getline(&line, &cap, stdin);
    if (len < 0) return gobol_strdup("");
    /* strip trailing newline */
    while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
        line[--len] = '\0';
    return line;
}

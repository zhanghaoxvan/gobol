/*
 * entry.c — Standard main() wrapping the Gobol gbl_main() entry point.
 */
#include "entry.h"

static int gobol_argc;
static char **gobol_argv;

long long gobol_args_count(void) {
    return gobol_argc;
}

char *gobol_args_get(long long index) {
    if (index < 0 || index >= gobol_argc) {
        return "";
    }
    return gobol_argv[index];
}

int main(int argc, char **argv) {
    gobol_argc = argc;
    gobol_argv = argv;
    return (int)gbl_main();
}

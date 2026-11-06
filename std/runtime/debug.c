#include "platform.h"

#define GOBOL_DEBUG_MAX_BREAKPOINTS 256

static long long gobol_debug_breakpoints[GOBOL_DEBUG_MAX_BREAKPOINTS];
static size_t gobol_debug_breakpoint_count;
static int gobol_debug_initialized;
static int gobol_debug_started;
static int gobol_debug_step;
static int gobol_debug_start_requested;

static void gobol_debug_add_breakpoint(long long line) {
    if (line <= 0) return;
    for (size_t i = 0; i < gobol_debug_breakpoint_count; ++i) {
        if (gobol_debug_breakpoints[i] == line) return;
    }
    if (gobol_debug_breakpoint_count == GOBOL_DEBUG_MAX_BREAKPOINTS) {
        fprintf(stderr, "Debugger: breakpoint limit reached\n");
        return;
    }
    gobol_debug_breakpoints[gobol_debug_breakpoint_count++] = line;
    printf("Breakpoint added at line %lld\n", line);
}

static void gobol_debug_remove_breakpoint(long long line) {
    for (size_t i = 0; i < gobol_debug_breakpoint_count; ++i) {
        if (gobol_debug_breakpoints[i] == line) {
            gobol_debug_breakpoints[i] =
                gobol_debug_breakpoints[--gobol_debug_breakpoint_count];
            printf("Breakpoint removed from line %lld\n", line);
            return;
        }
    }
    printf("No breakpoint at line %lld\n", line);
}

static int gobol_debug_has_breakpoint(long long line) {
    for (size_t i = 0; i < gobol_debug_breakpoint_count; ++i) {
        if (gobol_debug_breakpoints[i] == line) return 1;
    }
    return 0;
}

static void gobol_debug_initialize(void) {
    if (gobol_debug_initialized) return;
    gobol_debug_initialized = 1;
    gobol_debug_start_requested = getenv("GOBOL_DEBUG_START") != NULL;

    const char *list = getenv("GOBOL_DEBUG_BREAKPOINTS");
    if (!list) return;
    while (*list) {
        char *end = NULL;
        long long line = strtoll(list, &end, 10);
        if (end == list) {
            while (*list && *list != ',') ++list;
        } else {
            gobol_debug_add_breakpoint(line);
            list = end;
        }
        if (*list == ',') ++list;
    }
}

void gobol_debug_hook(long long line) {
    gobol_debug_initialize();
    int should_stop = gobol_debug_has_breakpoint(line) || gobol_debug_step;
    if (!gobol_debug_started) {
        gobol_debug_started = 1;
        should_stop = should_stop || gobol_debug_start_requested;
    }
    if (!should_stop) return;

    printf("\nStopped at line %lld\n", line);
    for (;;) {
        char command[256];
        printf("(c)ontinue, (s)tep, (b)reak <line>, (d)elete <line>, (q)uit > ");
        fflush(stdout);
        if (!fgets(command, sizeof(command), stdin)) {
            gobol_debug_step = 0;
            return;
        }

        char *end = NULL;
        long long target = strtoll(command + 1, &end, 10);
        switch (command[0]) {
            case 'c':
                gobol_debug_step = 0;
                return;
            case 's':
                gobol_debug_step = 1;
                return;
            case 'b':
                if (end == command + 1) {
                    puts("Usage: b <line>");
                } else {
                    gobol_debug_add_breakpoint(target);
                }
                break;
            case 'd':
                if (end == command + 1) {
                    puts("Usage: d <line>");
                } else {
                    gobol_debug_remove_breakpoint(target);
                }
                break;
            case 'q':
                exit(0);
            default:
                puts("Commands: c, s, b <line>, d <line>, q");
                break;
        }
    }
}

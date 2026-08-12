/*
 * gc.c — Simple mark-sweep garbage collector.
 */
#include "platform.h"
#include "gc.h"

typedef struct GcHeader {
    long long  size;
    unsigned char mark;
    struct GcHeader *next;
} GcHeader;

static GcHeader *gc_head = NULL;

void *gobol_gc_alloc(long long size) {
    if (size <= 0) return NULL;
    GcHeader *h = (GcHeader *)malloc(sizeof(GcHeader) + (size_t)size);
    if (!h) return NULL;
    memset((char *)h + sizeof(GcHeader), 0, (size_t)size);
    h->size = size;
    h->mark = 0;
    h->next = gc_head;
    gc_head  = h;
    return (void *)(h + 1);
}

void gobol_gc_mark(void *ptr) {
    if (!ptr) return;
    GcHeader *h = (GcHeader *)ptr - 1;
    if (h->mark) return;
    h->mark = 1;
}

void gobol_gc_sweep(void) {
    GcHeader **link = &gc_head;
    while (*link) {
        GcHeader *h = *link;
        if (!h->mark) {
            *link = h->next;
            free(h);
        } else {
            h->mark = 0;
            link = &h->next;
        }
    }
}

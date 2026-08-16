/*
 * gc.c — Mark-sweep garbage collector.
 *
 * This is a conservative stop-the-world GC.  The Cranelift backend is
 * responsible for generating mark calls for live root variables before
 * calling gobol_gc_collect().
 *
 * Memory layout per allocation:
 *   [ GcHeader ][ user payload ... ]
 *     ^            ^
 *     |            `-- returned to caller
 *     `-- GcHeader tracks size, mark bit, and allocation list
 */
#include "gc.h"
#include "platform.h"

typedef struct GcHeader {
    long long  size;     /* size of the user payload in bytes */
    unsigned char mark;  /* 1 = reachable, 0 = garbage */
    struct GcHeader *next;
} GcHeader;

/* Head of the allocation list — every live GcHeader is linked here. */
static GcHeader *gc_head = NULL;

/* Total number of allocations since the last collection. */
static long long gc_alloc_counter = 0;

/* Total number of collections that have occurred. */
static long long gc_collections = 0;

/* Reset the collector state (called at program start / reset). */
void gobol_gc_reset(void) {
    GcHeader *h = gc_head;
    while (h) {
        GcHeader *next = h->next;
        free(h);
        h = next;
    }
    gc_head = NULL;
    gc_alloc_counter = 0;
    gc_collections = 0;
}

/* Allocate a block of `size` bytes on the GC heap.
 * Returns NULL if size <= 0 or if malloc fails. */
void *gobol_gc_alloc(long long size) {
    if (size <= 0) return NULL;
    GcHeader *h = (GcHeader *)malloc(sizeof(GcHeader) + (size_t)size);
    if (!h) return NULL;
    memset((char *)h + sizeof(GcHeader), 0, (size_t)size);
    h->size  = size;
    h->mark  = 0;
    h->next  = gc_head;
    gc_head  = h;
    gc_alloc_counter++;
    return (void *)(h + 1);
}

/* Mark a root pointer as live during the mark phase. */
void gobol_gc_mark(void *ptr) {
    if (!ptr) return;
    GcHeader *h = (GcHeader *)ptr - 1;
    if (h->mark) return;
    h->mark = 1;
}

/* Sweep phase — walk the allocation list and free every unmarked block. */
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

/* Full collection.  The caller is responsible for marking all live roots
 * before calling this.  After collection the mark bits are zeroed so the
 * next collection starts clean. */
void gobol_gc_collect(void) {
    gobol_gc_sweep();
    gc_alloc_counter = 0;
    gc_collections++;
}

/* Force an immediate collection regardless of the allocation count. */
void gobol_gc_collect_now(void) {
    gobol_gc_collect();
}

/* Return how many allocations have happened since the last collection. */
long long gobol_gc_alloc_count(void) {
    return gc_alloc_counter;
}

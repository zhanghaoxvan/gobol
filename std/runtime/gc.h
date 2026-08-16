/*
 * gc.h — Mark-sweep garbage collector.
 *
 * The collector is a conservative stop-the-world mark-sweep GC that
 * cooperates with the Cranelift backend.  The backend:
 *   1. Calls gobol_gc_alloc() for all heap allocations.
 *   2. Calls gobol_gc_mark(ptr) for every live root variable before
 *      calling gobol_gc_collect().
 *   3. Calls gobol_gc_collect() at safe points (function return,
 *      loop back-edges, or explicit GC_TRIGGER thresholds).
 */
#ifndef GOBOL_RT_GC_H
#define GOBOL_RT_GC_H

/* Allocation — returns a pointer to the usable payload. */
void *gobol_gc_alloc(long long size);

/* Root marking — mark a live pointer as reachable. */
void  gobol_gc_mark(void *ptr);

/* Sweep phase — free all unmarked objects and reset marks. */
void  gobol_gc_sweep(void);

/* Full collection (mark + sweep).  The caller must have marked all
 * live roots before calling this. */
void  gobol_gc_collect(void);

/* Force an immediate collection regardless of the threshold. */
void  gobol_gc_collect_now(void);

/* Reset the collector state (called at program start). */
void  gobol_gc_reset(void);

/* Return current allocation count (for debugging / statistics). */
long long gobol_gc_alloc_count(void);

/* GC trigger threshold — collection runs after this many allocs
 * unless explicitly triggered sooner. */
#ifndef GOBOL_GC_THRESHOLD
#define GOBOL_GC_THRESHOLD 1024
#endif

#endif /* GOBOL_RT_GC_H */

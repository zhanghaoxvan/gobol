/*
 * gc.h — Simple mark-sweep garbage collector (compiler internal).
 */
#ifndef GOBOL_RT_GC_H
#define GOBOL_RT_GC_H

void *gobol_gc_alloc(long long size);
void  gobol_gc_mark(void *ptr);
void  gobol_gc_sweep(void);

#endif /* GOBOL_RT_GC_H */

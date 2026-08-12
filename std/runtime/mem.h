/*
 * mem.h — Memory allocation and raw pointer access (used by Ref<T>).
 */
#ifndef GOBOL_RT_MEM_H
#define GOBOL_RT_MEM_H

void *gobol_alloc(long long size);

/* Raw memory load / store — all Gobol values are 8-byte slots. */
long long gobol_mem_load(long long addr);
void gobol_mem_store(long long addr, long long val);
long long gobol_array_elem_addr(void *arr, long long i);

/* Manual allocator (compiler internal). */
void *gobol_malloc(long long size);
void  gobol_free(void *ptr);

#endif /* GOBOL_RT_MEM_H */

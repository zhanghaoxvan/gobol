/*
 * entry.h — Entry point (wraps gbl_main → main).
 */
#ifndef GOBOL_RT_ENTRY_H
#define GOBOL_RT_ENTRY_H

extern long long gbl_main(void);
long long gobol_args_count(void);
char *gobol_args_get(long long index);

#endif /* GOBOL_RT_ENTRY_H */

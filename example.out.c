#include <stdio.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <stdlib.h>

// std/c/__builtins__.c
void println(const char* v);
void print(const char* v);
char* read(void);
char* gobol_str_int(int64_t n);
char* gobol_str_float(double f);
char* gobol_str_cat(const char* a, const char* b);
// array runtime
typedef struct { int64_t* data; int64_t len; int64_t cap; } gobol_array_t;
void gobol_array_add(gobol_array_t* arr, int64_t val);
int64_t gobol_array_len(gobol_array_t* arr);
int64_t gobol_array_get(gobol_array_t* arr, int64_t i);
void gobol_array_set(gobol_array_t* arr, int64_t i, int64_t val);
int64_t gobol_array_get_flat(gobol_array_t* arr, int64_t i, int64_t j);
void gobol_array_set_flat(gobol_array_t* arr, int64_t i, int64_t j, int64_t val);

void gobol_print(const char* s) { printf("%s", s); }

char* gobol_read(void) {
    static char buf[4096];
    if (fgets(buf, sizeof(buf), stdin)) {
        size_t len = strlen(buf);
        if (len > 0 && buf[len-1] == '\n') buf[len-1] = '\0';
        return buf;
    }
    return "";
}

typedef struct Point { int64_t x; int64_t y; } Point;

int64_t add(Point p);
void io_print(void* value);
void io_println(void* value);
const char* io_read();
void Point_constructor(int64_t x, int64_t y);

int64_t add(Point p) {
return (p.x + p.y)    ;
    return 0;
}

void Point_constructor(int64_t x, int64_t y) {
    Point self = {0};
self.x = x    ;
self.y = y    ;
}

int main(void) {
const char* name = "Gobol"    ;
gobol_array_t arr = {0};    
gobol_array_set_flat(&arr, 2, 2, 114514)    ;
bool flag = true    ;
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "Hello from "), name), "\n"))    ;
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "arr[2] is "), gobol_str_int(gobol_array_get_flat(&arr, 2, 2))), "!\n"))    ;
for (int64_t i = 0; i < 10; i++)     {
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "Number "), gobol_str_int(i)), "\n"))        ;
    }
Point p = (Point){.x = 1, .y = 2}    ;
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "Calc: "), gobol_str_int(add(p))), "\n"))    ;
if ((gobol_array_get_flat(&arr, 2, 2) == 114514)    ) {
print("Right!\n")        ;
    } else {
print("Wrong!\n")        ;
    }
print("End of Task!\n")    ;
if ((85 == 100)    ) {
"A+"        ;
    } else {
if ((85 == 90)        ) {
"A"            ;
        } else {
if (true            ) {
"B"                ;
            }
        }
    }
int64_t grade = 0    ;
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "Grade: "), gobol_str_int(grade)), "\n"))    ;
int64_t tmp = (p.x + p.y)    ;
(tmp * 2)    ;
int64_t doubled = 0    ;
print(gobol_str_cat(gobol_str_cat(gobol_str_cat("", "Doubled: "), gobol_str_int(doubled)), "\n"))    ;
gobol_array_t items = {0};gobol_array_add(&items, 10); gobol_array_add(&items, 20); gobol_array_add(&items, 30);     
print("Items: ")    ;
    for (int64_t _i = 0; _i < gobol_array_len(&items); _i++) {
        int64_t i = _i;
        int64_t v = gobol_array_get(&items, _i);
print(gobol_str_int(i))        ;
print(":")        ;
print(gobol_str_int(v))        ;
print(" ")        ;
    }
print("\n")    ;
print("Chars: ")    ;
for (const char* _p = "Go"    ; *_p; _p++) {
        char ch = *_p;
print(gobol_str_int(ch))        ;
    }
print("\n")    ;
    return 0;
}


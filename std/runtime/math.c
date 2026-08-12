/*
 * math.c — Math intrinsic wrappers (thin libm wrappers).
 */
#include <math.h>
#include "math.h"

double gobol_math_sin(double x)  { return sin(x); }
double gobol_math_cos(double x)  { return cos(x); }
double gobol_math_pow(double b, double e) { return pow(b, e); }

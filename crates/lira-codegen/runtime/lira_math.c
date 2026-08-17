/*
 * Math built-ins.
 *
 * These are thin wrappers rather than direct libm calls so that every symbol
 * the code generator references carries the `lira_rt_` prefix: the JIT resolves
 * runtime symbols by taking their address in this crate, and a wrapper keeps
 * that uniform without declaring libc prototypes on the Rust side.
 *
 * `sqrt`, `abs`, `floor`, `ceil` and `trunc` are absent on purpose — the
 * backend emits Cranelift instructions for those and never calls out.
 */
#include "lira_rt.h"

#include <math.h>

double lira_rt_math_pow(double base, double exponent) { return pow(base, exponent); }
double lira_rt_math_exp(double v) { return exp(v); }
double lira_rt_math_ln(double v) { return log(v); }
double lira_rt_math_log10(double v) { return log10(v); }
double lira_rt_math_log2(double v) { return log2(v); }
double lira_rt_math_sin(double v) { return sin(v); }
double lira_rt_math_cos(double v) { return cos(v); }
double lira_rt_math_tan(double v) { return tan(v); }
double lira_rt_math_asin(double v) { return asin(v); }
double lira_rt_math_acos(double v) { return acos(v); }
double lira_rt_math_atan(double v) { return atan(v); }
double lira_rt_math_atan2(double y, double x) { return atan2(y, x); }
double lira_rt_math_sinh(double v) { return sinh(v); }
double lira_rt_math_cosh(double v) { return cosh(v); }
double lira_rt_math_tanh(double v) { return tanh(v); }

/* Rust's `f64::round` — and therefore the bytecode VM — rounds half away from
 * zero, which is C's `round`, not Cranelift's `nearest` (half to even). */
double lira_rt_math_round(double v) { return round(v); }

# math

Lira Standard Library - Math Module
Mathematical functions and constants

## Contents

- [Functions](#functions)

## Functions

### `math_pi`

```lira
fn math_pi() -> float
```

These are provided as functions to work around module-level visibility issues

**Returns:** `float`

---

### `math_e`

```lira
fn math_e() -> float
```

**Returns:** `float`

---

### `math_tau`

```lira
fn math_tau() -> float
```

**Returns:** `float`

---

### `math_sqrt2`

```lira
fn math_sqrt2() -> float
```

**Returns:** `float`

---

### `math_sqrt3`

```lira
fn math_sqrt3() -> float
```

**Returns:** `float`

---

### `math_ln2`

```lira
fn math_ln2() -> float
```

**Returns:** `float`

---

### `math_ln10`

```lira
fn math_ln10() -> float
```

**Returns:** `float`

---

### `math_log2e`

```lira
fn math_log2e() -> float
```

**Returns:** `float`

---

### `math_log10e`

```lira
fn math_log10e() -> float
```

**Returns:** `float`

---

### `abs_int`

```lira
fn abs_int(x: int) -> int
```

Absolute value for integers

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `x`  | `int` |

**Returns:** `int`

---

### `sign`

```lira
fn sign(x: float) -> int
```

Sign function: returns -1, 0, or 1

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `int`

---

### `sign_int`

```lira
fn sign_int(x: int) -> int
```

Sign function for integers

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `x`  | `int` |

**Returns:** `int`

---

### `clamp`

```lira
fn clamp(x: float, min_val: float, max_val: float) -> float
```

Clamp value to range

**Parameters:**

| Name      | Type    |
| --------- | ------- |
| `x`       | `float` |
| `min_val` | `float` |
| `max_val` | `float` |

**Returns:** `float`

---

### `clamp_int`

```lira
fn clamp_int(x: int, min_val: int, max_val: int) -> int
```

Clamp integer to range

**Parameters:**

| Name      | Type  |
| --------- | ----- |
| `x`       | `int` |
| `min_val` | `int` |
| `max_val` | `int` |

**Returns:** `int`

---

### `lerp`

```lira
fn lerp(a: float, b: float, t: float) -> float
```

Linear interpolation

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `a`  | `float` |
| `b`  | `float` |
| `t`  | `float` |

**Returns:** `float`

---

### `inverse_lerp`

```lira
fn inverse_lerp(a: float, b: float, value: float) -> float
```

Inverse linear interpolation (find t given value)

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `a`     | `float` |
| `b`     | `float` |
| `value` | `float` |

**Returns:** `float`

---

### `smoothstep`

```lira
fn smoothstep(edge0: float, edge1: float, x: float) -> float
```

Smooth step (Hermite interpolation)

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `edge0` | `float` |
| `edge1` | `float` |
| `x`     | `float` |

**Returns:** `float`

---

### `radians`

```lira
fn radians(degrees: float) -> float
```

Degrees to radians

**Parameters:**

| Name      | Type    |
| --------- | ------- |
| `degrees` | `float` |

**Returns:** `float`

---

### `degrees`

```lira
fn degrees(rad: float) -> float
```

Radians to degrees

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `rad` | `float` |

**Returns:** `float`

---

### `hypot`

```lira
fn hypot(x: float, y: float) -> float
```

Hypotenuse (sqrt(x^2 + y^2))

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |
| `y`  | `float` |

**Returns:** `float`

---

### `distance`

```lira
fn distance(x1: float, y1: float, x2: float, y2: float) -> float
```

Distance between two 2D points

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x1` | `float` |
| `y1` | `float` |
| `x2` | `float` |
| `y2` | `float` |

**Returns:** `float`

---

### `distance3d`

```lira
fn distance3d(x1: float, y1: float, z1: float, x2: float, y2: float, z2: float) -> float
```

Distance between two 3D points

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x1` | `float` |
| `y1` | `float` |
| `z1` | `float` |
| `x2` | `float` |
| `y2` | `float` |
| `z2` | `float` |

**Returns:** `float`

---

### `min_float`

```lira
fn min_float(a: float, b: float) -> float
```

Minimum of two floats

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `a`  | `float` |
| `b`  | `float` |

**Returns:** `float`

---

### `max_float`

```lira
fn max_float(a: float, b: float) -> float
```

Maximum of two floats

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `a`  | `float` |
| `b`  | `float` |

**Returns:** `float`

---

### `min_int`

```lira
fn min_int(a: int, b: int) -> int
```

Minimum of two integers

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `max_int`

```lira
fn max_int(a: int, b: int) -> int
```

Maximum of two integers

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `wrap_angle`

```lira
fn wrap_angle(angle: float) -> float
```

Wrap angle to [0, 2\*PI)

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `angle` | `float` |

**Returns:** `float`

---

### `normalize_angle`

```lira
fn normalize_angle(angle: float) -> float
```

Normalize angle to [-PI, PI]

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `angle` | `float` |

**Returns:** `float`

---

### `factorial`

```lira
fn factorial(n: int) -> int
```

Factorial (non-recursive for safety)

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `binomial`

```lira
fn binomial(n: int, k: int) -> int
```

Binomial coefficient (n choose k)

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |
| `k`  | `int` |

**Returns:** `int`

---

### `pow_int`

```lira
fn pow_int(base: int, exp: int) -> int
```

Power function for integers

**Parameters:**

| Name   | Type  |
| ------ | ----- |
| `base` | `int` |
| `exp`  | `int` |

**Returns:** `int`

---

### `gcd`

```lira
fn gcd(a: int, b: int) -> int
```

Greatest common divisor

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `lcm`

```lira
fn lcm(a: int, b: int) -> int
```

Least common multiple

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `is_prime`

```lira
fn is_prime(n: int) -> bool
```

Check if number is prime

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `bool`

---

### `is_even`

```lira
fn is_even(n: int) -> bool
```

Check if number is even

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `bool`

---

### `is_odd`

```lira
fn is_odd(n: int) -> bool
```

Check if number is odd

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `bool`

---

### `mod_floor`

```lira
fn mod_floor(a: int, b: int) -> int
```

Floor modulo (handles negative numbers correctly)

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `square`

```lira
fn square(x: float) -> float
```

Square of a number

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `square_int`

```lira
fn square_int(x: int) -> int
```

Square of an integer

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `x`  | `int` |

**Returns:** `int`

---

### `cube`

```lira
fn cube(x: float) -> float
```

Cube of a number

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `cube_int`

```lira
fn cube_int(x: int) -> int
```

Cube of an integer

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `x`  | `int` |

**Returns:** `int`

---

### `cbrt`

```lira
fn cbrt(x: float) -> float
```

Cube root

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `nthroot`

```lira
fn nthroot(x: float, n: float) -> float
```

Nth root

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |
| `n`  | `float` |

**Returns:** `float`

---

### `log`

```lira
fn log(x: float, base: float) -> float
```

Logarithm with arbitrary base

**Parameters:**

| Name   | Type    |
| ------ | ------- |
| `x`    | `float` |
| `base` | `float` |

**Returns:** `float`

---

### `sec`

```lira
fn sec(x: float) -> float
```

Secant

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `csc`

```lira
fn csc(x: float) -> float
```

Cosecant

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `cot`

```lira
fn cot(x: float) -> float
```

Cotangent

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `asinh`

```lira
fn asinh(x: float) -> float
```

Hyperbolic arcsine

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `acosh`

```lira
fn acosh(x: float) -> float
```

Hyperbolic arccosine

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `atanh`

```lira
fn atanh(x: float) -> float
```

Hyperbolic arctangent

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `fibonacci`

```lira
fn fibonacci(n: int) -> int
```

Fibonacci number (iterative)

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `is_power_of_two`

```lira
fn is_power_of_two(n: int) -> bool
```

Check if number is a power of 2

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `bool`

---

### `next_power_of_two`

```lira
fn next_power_of_two(n: int) -> int
```

Next power of 2 greater than or equal to n

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `popcount`

```lira
fn popcount(n: int) -> int
```

Count set bits (population count)

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `approx_equal`

```lira
fn approx_equal(a: float, b: float, epsilon: float) -> bool
```

Check if two floats are approximately equal

**Parameters:**

| Name      | Type    |
| --------- | ------- |
| `a`       | `float` |
| `b`       | `float` |
| `epsilon` | `float` |

**Returns:** `bool`

---

### `map_range`

```lira
fn map_range(value: float, in_min: float, in_max: float, out_min: float, out_max: float) -> float
```

Map a value from one range to another

**Parameters:**

| Name      | Type    |
| --------- | ------- |
| `value`   | `float` |
| `in_min`  | `float` |
| `in_max`  | `float` |
| `out_min` | `float` |
| `out_max` | `float` |

**Returns:** `float`

---

### `sigmoid`

```lira
fn sigmoid(x: float) -> float
```

Sigmoid function (logistic function)

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `relu`

```lira
fn relu(x: float) -> float
```

ReLU (Rectified Linear Unit)

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `x`  | `float` |

**Returns:** `float`

---

### `leaky_relu`

```lira
fn leaky_relu(x: float, alpha: float) -> float
```

Leaky ReLU

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `x`     | `float` |
| `alpha` | `float` |

**Returns:** `float`

---

### `sum_to`

```lira
fn sum_to(n: int) -> int
```

Sum of integers from 1 to n

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `sum_squares_to`

```lira
fn sum_squares_to(n: int) -> int
```

Sum of squares from 1 to n

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

# lambda

Lambda Expression Tests
Tests anonymous functions with closures

## Contents

- [Functions](#functions)

## Functions

### `apply_twice`

```lira
fn apply_twice(f: fn(int) -> int, x: int) -> int
```

Function that takes a lambda as parameter

**Parameters:**

| Name | Type             |
| ---- | ---------------- |
| `f`  | `fn(int) -> int` |
| `x`  | `int`            |

**Returns:** `int`

---

### `make_adder`

```lira
fn make_adder(n: int) -> fn(int) -> int
```

Function that returns a closure capturing its parameter

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `fn(int) -> int`

---

### `make_linear`

```lira
fn make_linear(a: int, b: int) -> fn(int) -> int
```

Multiple captured variables

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `fn(int) -> int`

---

### `make_multiplier`

```lira
fn make_multiplier(n: int) -> fn(int) -> int
```

Nested closures

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `fn(int) -> int`

---

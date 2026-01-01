# core

Lira Standard Library - Core Module
Provides fundamental types and utilities

## Contents

- [Functions](#functions)


## Functions

### `abs`

```lira
fn abs(n: int) -> int
```

Absolute value

**Parameters:**

| Name | Type |
|------|------|
| `n` | `int` |

**Returns:** `int`

---

### `min`

```lira
fn min(a: int, b: int) -> int
```

Minimum of two values

**Parameters:**

| Name | Type |
|------|------|
| `a` | `int` |
| `b` | `int` |

**Returns:** `int`

---

### `max`

```lira
fn max(a: int, b: int) -> int
```

Maximum of two values

**Parameters:**

| Name | Type |
|------|------|
| `a` | `int` |
| `b` | `int` |

**Returns:** `int`

---

### `clamp`

```lira
fn clamp(value: int, min_val: int, max_val: int) -> int
```

Clamp value to range

**Parameters:**

| Name | Type |
|------|------|
| `value` | `int` |
| `min_val` | `int` |
| `max_val` | `int` |

**Returns:** `int`

---

### `is_empty`

```lira
fn is_empty(s: string) -> bool
```

Check if string is empty

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `repeat_str`

```lira
fn repeat_str(s: string, n: int) -> string
```

Repeat string n times

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `n` | `int` |

**Returns:** `string`

---

### `contains_int`

```lira
fn contains_int(arr: [int], value: int) -> bool
```

Check if array contains a value

**Parameters:**

| Name | Type |
|------|------|
| `arr` | `[int]` |
| `value` | `int` |

**Returns:** `bool`

---

### `sum`

```lira
fn sum(arr: [int]) -> int
```

Sum all elements in array

**Parameters:**

| Name | Type |
|------|------|
| `arr` | `[int]` |

**Returns:** `int`

---

### `index_of`

```lira
fn index_of(arr: [int], value: int) -> int
```

Find index of value in array (-1 if not found)

**Parameters:**

| Name | Type |
|------|------|
| `arr` | `[int]` |
| `value` | `int` |

**Returns:** `int`

---


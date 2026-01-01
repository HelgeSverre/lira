# function_types

Test function types as parameters and return values
@expect: 10
@expect: 25
@expect: 15
@expect: 50

## Contents

- [Functions](#functions)


## Functions

### `apply`

```lira
fn apply(f: fn(int) -> int, x: int) -> int
```

Function that takes a function as parameter

**Parameters:**

| Name | Type |
|------|------|
| `f` | `fn(int) -> int` |
| `x` | `int` |

**Returns:** `int`

---

### `compose`

```lira
fn compose(f: fn(int) -> int, g: fn(int) -> int, x: int) -> int
```

Function that takes two functions

**Parameters:**

| Name | Type |
|------|------|
| `f` | `fn(int) -> int` |
| `g` | `fn(int) -> int` |
| `x` | `int` |

**Returns:** `int`

---

### `double`

```lira
fn double(x: int) -> int
```

Functions to pass around

**Parameters:**

| Name | Type |
|------|------|
| `x` | `int` |

**Returns:** `int`

---

### `square`

```lira
fn square(x: int) -> int
```

**Parameters:**

| Name | Type |
|------|------|
| `x` | `int` |

**Returns:** `int`

---

### `add_five`

```lira
fn add_five(x: int) -> int
```

**Parameters:**

| Name | Type |
|------|------|
| `x` | `int` |

**Returns:** `int`

---

### `main`

```lira
fn main()
```

---


# mutual_recursion

Mutual Recursion Test
Tests forward references for functions

## Contents

- [Functions](#functions)


## Functions

### `is_even`

```lira
fn is_even(n: int) -> bool
```

is_even calls is_odd (defined later)

**Parameters:**

| Name | Type |
|------|------|
| `n` | `int` |

**Returns:** `bool`

---

### `is_odd`

```lira
fn is_odd(n: int) -> bool
```

is_odd calls is_even (defined earlier)

**Parameters:**

| Name | Type |
|------|------|
| `n` | `int` |

**Returns:** `bool`

---

### `caller`

```lira
fn caller() -> int
```

Function that calls another function defined later

**Returns:** `int`

---

### `callee`

```lira
fn callee() -> int
```

**Returns:** `int`

---


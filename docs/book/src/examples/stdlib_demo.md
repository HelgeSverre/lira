# stdlib_demo

Standard Library Demo
Demonstrates patterns from stdlib/ (import system pending Phase 7)

## Contents

- [Functions](#functions)


## Functions

### `abs`

```lira
fn abs(n: int) -> int
```

Inline implementations from stdlib/core.li

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

**Parameters:**

| Name | Type |
|------|------|
| `value` | `int` |
| `min_val` | `int` |
| `max_val` | `int` |

**Returns:** `int`

---

### `read_file`

```lira
fn read_file(path: string) -> string
```

Inline implementations from stdlib/fs.li

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |

**Returns:** `string`

---

### `write_file`

```lira
fn write_file(path: string, content: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |
| `content` | `string` |

**Returns:** `bool`

---


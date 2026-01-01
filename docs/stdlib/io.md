# io

Lira Standard Library - I/O Module
Input/output utilities

## Contents

- [Functions](#functions)


## Functions

### `print_str`

```lira
fn print_str(s: string)
```

Print without newline

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

---

### `print_line`

```lira
fn print_line(s: string)
```

Print with newline

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

---

### `print_fmt`

```lira
fn print_fmt(template: string, values: [string])
```

Print formatted message with values

**Parameters:**

| Name | Type |
|------|------|
| `template` | `string` |
| `values` | `[string]` |

---

### `debug`

```lira
fn debug(label: string, value: int)
```

Debug print with label

**Parameters:**

| Name | Type |
|------|------|
| `label` | `string` |
| `value` | `int` |

---

### `assert`

```lira
fn assert(condition: bool, message: string)
```

Assert condition

**Parameters:**

| Name | Type |
|------|------|
| `condition` | `bool` |
| `message` | `string` |

---

### `now_ms`

```lira
fn now_ms() -> int
```

Get current timestamp in milliseconds

**Returns:** `int`

---

### `delay`

```lira
fn delay(ms: int)
```

Sleep for specified milliseconds

**Parameters:**

| Name | Type |
|------|------|
| `ms` | `int` |

---

### `measure_time`

```lira
fn measure_time() -> int
```

Measure execution time of a block

**Returns:** `int`

---


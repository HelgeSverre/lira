# log

Lira Standard Library - Logging Module
Structured logging with levels

This is a stateless logging module. Each log function explicitly takes
the necessary configuration as parameters.

Built-in functions used (from time module):
time_ms() -> int - Current time in milliseconds since epoch (syscall 4)

## Contents

- [Functions](#functions)

## Functions

### `log_debug`

```lira
fn log_debug(current_level: int, message: string)
```

Log a debug message (level 0)

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `current_level` | `int`    |
| `message`       | `string` |

---

### `log_info`

```lira
fn log_info(current_level: int, message: string)
```

Log an info message (level 1)

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `current_level` | `int`    |
| `message`       | `string` |

---

### `log_warn`

```lira
fn log_warn(current_level: int, message: string)
```

Log a warning message (level 2)

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `current_level` | `int`    |
| `message`       | `string` |

---

### `log_error`

```lira
fn log_error(current_level: int, message: string)
```

Log an error message (level 3)

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `current_level` | `int`    |
| `message`       | `string` |

---

### `log_fatal`

```lira
fn log_fatal(current_level: int, message: string)
```

Log a fatal message (level 4)

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `current_level` | `int`    |
| `message`       | `string` |

---

### `debug`

```lira
fn debug(message: string)
```

Log debug - always prints

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |

---

### `info`

```lira
fn info(message: string)
```

Log info - always prints

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |

---

### `warn`

```lira
fn warn(message: string)
```

Log warning - always prints

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |

---

### `error`

```lira
fn error(message: string)
```

Log error - always prints

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |

---

### `fatal`

```lira
fn fatal(message: string)
```

Log fatal - always prints

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |

---

### `log`

```lira
fn log(level: int, message: string)
```

Log with explicit level (0=DEBUG, 1=INFO, 2=WARN, 3=ERROR, 4=FATAL)

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `level`   | `int`    |
| `message` | `string` |

---

### `log_kv`

```lira
fn log_kv(level: int, message: string, key: string, value: string)
```

Log with a single key-value pair

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `level`   | `int`    |
| `message` | `string` |
| `key`     | `string` |
| `value`   | `string` |

---

### `log_kv2`

```lira
fn log_kv2(level: int, message: string, k1: string, v1: string, k2: string, v2: string)
```

Log with two key-value pairs

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `level`   | `int`    |
| `message` | `string` |
| `k1`      | `string` |
| `v1`      | `string` |
| `k2`      | `string` |
| `v2`      | `string` |

---

### `log_kv3`

```lira
fn log_kv3(level: int, message: string, k1: string, v1: string, k2: string, v2: string, k3: string, v3: string)
```

Log with three key-value pairs

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `level`   | `int`    |
| `message` | `string` |
| `k1`      | `string` |
| `v1`      | `string` |
| `k2`      | `string` |
| `v2`      | `string` |
| `k3`      | `string` |
| `v3`      | `string` |

---

### `assert_true`

```lira
fn assert_true(condition: bool, message: string)
```

Assert that a condition is true, log error if not

**Parameters:**

| Name        | Type     |
| ----------- | -------- |
| `condition` | `bool`   |
| `message`   | `string` |

---

### `assert_equal`

```lira
fn assert_equal(a: int, b: int, message: string)
```

Assert that two integers are equal, log error with details if not

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `a`       | `int`    |
| `b`       | `int`    |
| `message` | `string` |

---

### `assert_equal_str`

```lira
fn assert_equal_str(a: string, b: string, message: string)
```

Assert that two strings are equal, log error if not

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `a`       | `string` |
| `b`       | `string` |
| `message` | `string` |

---

### `log_timing`

```lira
fn log_timing(operation: string, start_ms: int)
```

Log timing information for an operation

**Parameters:**

| Name        | Type     |
| ----------- | -------- |
| `operation` | `string` |
| `start_ms`  | `int`    |

---

### `log_timing_level`

```lira
fn log_timing_level(level: int, operation: string, start_ms: int)
```

Log timing with custom log level

**Parameters:**

| Name        | Type     |
| ----------- | -------- |
| `level`     | `int`    |
| `operation` | `string` |
| `start_ms`  | `int`    |

---

### `debug_kv`

```lira
fn debug_kv(message: string, key: string, value: string)
```

Debug log with key-value

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |
| `key`     | `string` |
| `value`   | `string` |

---

### `info_kv`

```lira
fn info_kv(message: string, key: string, value: string)
```

Info log with key-value

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |
| `key`     | `string` |
| `value`   | `string` |

---

### `warn_kv`

```lira
fn warn_kv(message: string, key: string, value: string)
```

Warn log with key-value

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |
| `key`     | `string` |
| `value`   | `string` |

---

### `error_kv`

```lira
fn error_kv(message: string, key: string, value: string)
```

Error log with key-value

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `message` | `string` |
| `key`     | `string` |
| `value`   | `string` |

---

### `level_name`

```lira
fn level_name(level: int) -> string
```

Get the name of a log level

**Parameters:**

| Name    | Type  |
| ------- | ----- |
| `level` | `int` |

**Returns:** `string`

---

### `parse_level`

```lira
fn parse_level(name: string) -> int
```

Parse a log level from name (returns 1/INFO if unknown)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `name` | `string` |

**Returns:** `int`

---

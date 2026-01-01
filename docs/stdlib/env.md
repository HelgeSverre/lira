# env

Lira Standard Library - Environment Module
Environment variable operations

## Contents

- [Functions](#functions)

## Functions

### `get_or`

```lira
fn get_or(name: string, default_value: string) -> string
```

Get env var with default value

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `name`          | `string` |
| `default_value` | `string` |

**Returns:** `string`

---

### `get_bool`

```lira
fn get_bool(name: string) -> bool
```

Get env var as bool

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `name` | `string` |

**Returns:** `bool`

---

### `is_ci`

```lira
fn is_ci() -> bool
```

Check if running in CI environment

**Returns:** `bool`

---

### `is_debug`

```lira
fn is_debug() -> bool
```

Check if debug mode

**Returns:** `bool`

---

### `user`

```lira
fn user() -> string
```

Get current user

**Returns:** `string`

---

### `shell`

```lira
fn shell() -> string
```

Get shell

**Returns:** `string`

---

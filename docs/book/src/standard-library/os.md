# os

Lira Standard Library - OS Module
Operating system operations

## Contents

- [Functions](#functions)

## Functions

### `walk`

```lira
fn walk(path: string) -> [string]
```

Walk directory tree recursively (returns all files and directories)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `[string]`

---

### `exists`

```lira
fn exists(path: string) -> bool
```

Check if a path exists (file or directory)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `bool`

---

### `home_dir`

```lira
fn home_dir() -> string
```

Get the home directory (via HOME environment variable)
Returns empty string if HOME is not set

**Returns:** `string`

---

### `temp_dir`

```lira
fn temp_dir() -> string
```

Get a temporary directory path

**Returns:** `string`

---

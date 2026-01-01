# fs

Lira Standard Library - File System Module
High-level file operations built on host primitives

## Contents

- [Functions](#functions)


## Functions

### `read_file`

```lira
fn read_file(path: string) -> string
```

Read entire file as string

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

Write string to file (overwrites existing)

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |
| `content` | `string` |

**Returns:** `bool`

---

### `append_file`

```lira
fn append_file(path: string, content: string) -> bool
```

Append string to file

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |
| `content` | `string` |

**Returns:** `bool`

---

### `exists`

```lira
fn exists(path: string) -> bool
```

Check if path exists (file or directory)

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |

**Returns:** `bool`

---

### `size`

```lira
fn size(path: string) -> int
```

Get file size in bytes (-1 if doesn't exist)

**Parameters:**

| Name | Type |
|------|------|
| `path` | `string` |

**Returns:** `int`

---


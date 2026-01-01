# path

Lira Standard Library - Path Module
Path manipulation utilities for file system paths
Currently uses Unix-style paths (forward slashes)

## Contents

- [Functions](#functions)

## Functions

### `find_last_char`

```lira
fn find_last_char(s: string, c: string) -> int
```

Find last occurrence of a character in a string
Returns -1 if not found

**Parameters:**

| Name | Type     |
| ---- | -------- |
| `s`  | `string` |
| `c`  | `string` |

**Returns:** `int`

---

### `find_char_from`

```lira
fn find_char_from(s: string, c: string, start: int) -> int
```

Find first occurrence of a character in a string starting from position
Returns -1 if not found

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `s`     | `string` |
| `c`     | `string` |
| `start` | `int`    |

**Returns:** `int`

---

### `substring`

```lira
fn substring(s: string, start: int, end: int) -> string
```

Get substring from start to end (exclusive)

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `s`     | `string` |
| `start` | `int`    |
| `end`   | `int`    |

**Returns:** `string`

---

### `starts_with`

```lira
fn starts_with(s: string, prefix: string) -> bool
```

Check if string starts with a prefix

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `s`      | `string` |
| `prefix` | `string` |

**Returns:** `bool`

---

### `ends_with`

```lira
fn ends_with(s: string, suffix: string) -> bool
```

Check if string ends with a suffix

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `s`      | `string` |
| `suffix` | `string` |

**Returns:** `bool`

---

### `dirname`

```lira
fn dirname(path: string) -> string
```

Get directory name (parent path)
"/foo/bar/baz.txt" -> "/foo/bar"
"foo/bar" -> "foo"
"/foo" -> "/"
"foo" -> "."
"/" -> "/"

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `basename`

```lira
fn basename(path: string) -> string
```

Get base name (file name)
"/foo/bar/baz.txt" -> "baz.txt"
"/foo/bar/" -> "bar"
"/" -> ""

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `extension`

```lira
fn extension(path: string) -> string
```

Get file extension (including dot)
"/foo/bar.txt" -> ".txt"
"/foo/bar" -> ""
"/foo/.hidden" -> ""
"/foo/bar.tar.gz" -> ".gz"

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `stem`

```lira
fn stem(path: string) -> string
```

Get file name without extension (stem)
"/foo/bar.txt" -> "bar"
"/foo/bar" -> "bar"
"/foo/.hidden" -> ".hidden"

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `is_absolute`

```lira
fn is_absolute(path: string) -> bool
```

Check if path is absolute (starts with /)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `bool`

---

### `is_relative`

```lira
fn is_relative(path: string) -> bool
```

Check if path is relative (does not start with /)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `bool`

---

### `parent`

```lira
fn parent(path: string) -> string
```

Get parent directory (alias for dirname)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `has_extension`

```lira
fn has_extension(path: string) -> bool
```

Check if path has an extension

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `bool`

---

### `path_join`

```lira
fn path_join(parts: [string]) -> string
```

Join path components with separator
Handles leading/trailing slashes appropriately

**Parameters:**

| Name    | Type       |
| ------- | ---------- |
| `parts` | `[string]` |

**Returns:** `string`

---

### `join`

```lira
fn join(path1: string, path2: string) -> string
```

Join two path components

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `path1` | `string` |
| `path2` | `string` |

**Returns:** `string`

---

### `normalize`

```lira
fn normalize(path: string) -> string
```

Normalize a path (remove . and .., collapse multiple slashes)
"/foo//bar/../baz" -> "/foo/baz"
"foo/./bar" -> "foo/bar"

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `string`

---

### `with_extension`

```lira
fn with_extension(path: string, ext: string) -> string
```

Replace or add extension to a path
with_extension("/foo/bar.txt", ".md") -> "/foo/bar.md"
with_extension("/foo/bar", ".txt") -> "/foo/bar.txt"

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |
| `ext`  | `string` |

**Returns:** `string`

---

### `components`

```lira
fn components(path: string) -> [string]
```

Split path into array of components
"/foo/bar/baz" -> ["", "foo", "bar", "baz"]
"foo/bar" -> ["foo", "bar"]

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `path` | `string` |

**Returns:** `[string]`

---

# url

Lira Standard Library - URL Module
URL parsing and encoding utilities

This module provides URL parsing, encoding, and manipulation functions.
It uses host primitives (syscalls 110-111) for URL encoding/decoding
and pure Lira for URL parsing.

Built-in functions (from host):

- url_encode(str) -> str: Percent-encode string
- url_decode(str) -> str: Decode percent-encoded string

## Contents

- [Structs](#structs)
- [Functions](#functions)

## Structs

### `URL`

```lira
struct URL {
    scheme: string,
    host: string,
    port: int,
    path: string,
    query: string,
    fragment: string,
}
```

URL components

#### Fields

| Field      | Type     | Visibility |
| ---------- | -------- | ---------- |
| `scheme`   | `string` | private    |
| `host`     | `string` | private    |
| `port`     | `int`    | private    |
| `path`     | `string` | private    |
| `query`    | `string` | private    |
| `fragment` | `string` | private    |

---

## Functions

### `parse_int`

```lira
fn parse_int(s: string) -> int
```

Parse integer from string (simple implementation)

**Parameters:**

| Name | Type     |
| ---- | -------- |
| `s`  | `string` |

**Returns:** `int`

---

### `url_index_of`

```lira
fn url_index_of(s: string, substr: string) -> int
```

Find first occurrence of substring (wrapper for str_index_of)

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `s`      | `string` |
| `substr` | `string` |

**Returns:** `int`

---

### `url_substring`

```lira
fn url_substring(s: string, start: int, end: int) -> string
```

Get substring from start to end (wrapper for str_substring)

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `s`     | `string` |
| `start` | `int`    |
| `end`   | `int`    |

**Returns:** `string`

---

### `url_parse`

```lira
fn url_parse(url_str: string) -> URL
```

Parse URL string into components

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `url_str` | `string` |

**Returns:** `URL`

---

### `url_build`

```lira
fn url_build(url: URL) -> string
```

Build URL string from components

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `string`

---

### `query_parse`

```lira
fn query_parse(query: string) -> [string]
```

Parse query string into array of key=value pairs

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `query` | `string` |

**Returns:** `[string]`

---

### `query_get`

```lira
fn query_get(query: string, key: string) -> string
```

Get query parameter value by key

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `query` | `string` |
| `key`   | `string` |

**Returns:** `string`

---

### `query_has`

```lira
fn query_has(query: string, key: string) -> bool
```

Check if query has a parameter

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `query` | `string` |
| `key`   | `string` |

**Returns:** `bool`

---

### `query_build`

```lira
fn query_build(pairs: [string]) -> string
```

Build query string from keys and values arrays
pairs should be [key1, value1, key2, value2, ...]

**Parameters:**

| Name    | Type       |
| ------- | ---------- |
| `pairs` | `[string]` |

**Returns:** `string`

---

### `url_origin`

```lira
fn url_origin(url: URL) -> string
```

Get just the origin (scheme + host + port)

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `string`

---

### `url_path_query`

```lira
fn url_path_query(url: URL) -> string
```

Get path with query and fragment

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `string`

---

### `url_is_absolute`

```lira
fn url_is_absolute(url_str: string) -> bool
```

Check if URL is absolute (has scheme)

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `url_str` | `string` |

**Returns:** `bool`

---

### `url_is_relative`

```lira
fn url_is_relative(url_str: string) -> bool
```

Check if URL is relative (no scheme)

**Parameters:**

| Name      | Type     |
| --------- | -------- |
| `url_str` | `string` |

**Returns:** `bool`

---

### `url_is_http`

```lira
fn url_is_http(url: URL) -> bool
```

Check if URL uses HTTP

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `bool`

---

### `url_is_https`

```lira
fn url_is_https(url: URL) -> bool
```

Check if URL uses HTTPS

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `bool`

---

### `url_is_secure`

```lira
fn url_is_secure(url: URL) -> bool
```

Check if URL is secure (HTTPS)

**Parameters:**

| Name  | Type  |
| ----- | ----- |
| `url` | `URL` |

**Returns:** `bool`

---

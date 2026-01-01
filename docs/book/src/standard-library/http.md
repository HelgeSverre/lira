# http

Lira Standard Library - HTTP Module
HTTP client functionality (inspired by Perl's LWP::UserAgent)

## Contents

- [Functions](#functions)

## Functions

### `is_success`

```lira
fn is_success(status: int) -> bool
```

Check if status code indicates success (2xx)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `status` | `int` |

**Returns:** `bool`

---

### `is_redirect`

```lira
fn is_redirect(status: int) -> bool
```

Check if status code indicates redirect (3xx)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `status` | `int` |

**Returns:** `bool`

---

### `is_client_error`

```lira
fn is_client_error(status: int) -> bool
```

Check if status code indicates client error (4xx)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `status` | `int` |

**Returns:** `bool`

---

### `is_server_error`

```lira
fn is_server_error(status: int) -> bool
```

Check if status code indicates server error (5xx)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `status` | `int` |

**Returns:** `bool`

---

### `is_error`

```lira
fn is_error(status: int) -> bool
```

Check if status code indicates any error (4xx or 5xx)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `status` | `int` |

**Returns:** `bool`

---

### `get`

```lira
fn get(url: string) -> string
```

Simple GET request that returns just the body (or empty string on error)

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `url` | `string` |

**Returns:** `string`

---

### `get_ok`

```lira
fn get_ok(url: string) -> string
```

GET request that only returns body if status is 2xx, empty string otherwise

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `url` | `string` |

**Returns:** `string`

---

### `post_json`

```lira
fn post_json(url: string, data: string) -> string
```

POST JSON data, returns response body

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `url`  | `string` |
| `data` | `string` |

**Returns:** `string`

---

### `post_form`

```lira
fn post_form(url: string, data: string) -> string
```

POST form data (application/x-www-form-urlencoded), returns response body

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `url`  | `string` |
| `data` | `string` |

**Returns:** `string`

---

### `post_text`

```lira
fn post_text(url: string, data: string) -> string
```

POST plain text, returns response body

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `url`  | `string` |
| `data` | `string` |

**Returns:** `string`

---

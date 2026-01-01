# net

Lira Standard Library - Network Module
TCP networking primitives

## Contents

- [Functions](#functions)

## Functions

### `is_connected`

```lira
fn is_connected(socket_id: int) -> bool
```

Check if socket is valid (connected successfully)

**Parameters:**

| Name        | Type  |
| ----------- | ----- |
| `socket_id` | `int` |

**Returns:** `bool`

---

### `tcp_write_line`

```lira
fn tcp_write_line(socket_id: int, line: string) -> int
```

Send line with CRLF (for text protocols like HTTP, SMTP, etc.)

**Parameters:**

| Name        | Type     |
| ----------- | -------- |
| `socket_id` | `int`    |
| `line`      | `string` |

**Returns:** `int`

---

### `tcp_write_lines`

```lira
fn tcp_write_lines(socket_id: int, lines: [string]) -> int
```

Send multiple lines at once

**Parameters:**

| Name        | Type       |
| ----------- | ---------- |
| `socket_id` | `int`      |
| `lines`     | `[string]` |

**Returns:** `int`

---

### `tcp_read_exact`

```lira
fn tcp_read_exact(socket_id: int, n: int) -> string
```

Read until we have at least n bytes

**Parameters:**

| Name        | Type  |
| ----------- | ----- |
| `socket_id` | `int` |
| `n`         | `int` |

**Returns:** `string`

---

### `tcp_try_connect`

```lira
fn tcp_try_connect(host: string, port: int) -> int
```

Connect with timeout check (non-blocking attempt)

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `host` | `string` |
| `port` | `int`    |

**Returns:** `int`

---

### `tcp_close_safe`

```lira
fn tcp_close_safe(socket_id: int) -> bool
```

Close socket safely (no error if already closed)

**Parameters:**

| Name        | Type  |
| ----------- | ----- |
| `socket_id` | `int` |

**Returns:** `bool`

---

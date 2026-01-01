# hash

Lira Standard Library - Hash Module
Cryptographic hash functions

## Contents

- [Functions](#functions)

## Functions

### `verify_md5`

```lira
fn verify_md5(input: string, expected_hash: string) -> bool
```

Helper: Verify hash matches expected

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `input`         | `string` |
| `expected_hash` | `string` |

**Returns:** `bool`

---

### `verify_sha1`

```lira
fn verify_sha1(input: string, expected_hash: string) -> bool
```

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `input`         | `string` |
| `expected_hash` | `string` |

**Returns:** `bool`

---

### `verify_sha256`

```lira
fn verify_sha256(input: string, expected_hash: string) -> bool
```

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `input`         | `string` |
| `expected_hash` | `string` |

**Returns:** `bool`

---

### `verify_sha512`

```lira
fn verify_sha512(input: string, expected_hash: string) -> bool
```

**Parameters:**

| Name            | Type     |
| --------------- | -------- |
| `input`         | `string` |
| `expected_hash` | `string` |

**Returns:** `bool`

---

### `md5_salted`

```lira
fn md5_salted(input: string, salt: string) -> string
```

Hash with salt (simple concatenation)

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `input` | `string` |
| `salt`  | `string` |

**Returns:** `string`

---

### `sha256_salted`

```lira
fn sha256_salted(input: string, salt: string) -> string
```

**Parameters:**

| Name    | Type     |
| ------- | -------- |
| `input` | `string` |
| `salt`  | `string` |

**Returns:** `string`

---

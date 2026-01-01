# regex

Lira Standard Library - Regex Module
Regular expression matching

## Contents

- [Functions](#functions)


## Functions

### `is_email`

```lira
fn is_email(text: string) -> bool
```

Common patterns as helpers

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `is_url`

```lira
fn is_url(text: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `is_phone`

```lira
fn is_phone(text: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `is_digits`

```lira
fn is_digits(text: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `is_alpha`

```lira
fn is_alpha(text: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `is_alphanumeric`

```lira
fn is_alphanumeric(text: string) -> bool
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `bool`

---

### `extract_numbers`

```lira
fn extract_numbers(text: string) -> [string]
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `[string]`

---

### `extract_words`

```lira
fn extract_words(text: string) -> [string]
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `[string]`

---

### `remove_whitespace`

```lira
fn remove_whitespace(text: string) -> string
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `string`

---

### `normalize_whitespace`

```lira
fn normalize_whitespace(text: string) -> string
```

**Parameters:**

| Name | Type |
|------|------|
| `text` | `string` |

**Returns:** `string`

---


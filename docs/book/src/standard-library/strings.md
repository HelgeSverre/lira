# strings

Lira Standard Library - Strings Module
String manipulation utilities
Note: Named "strings" (plural) to avoid conflict with the "string" type keyword.

This module provides comprehensive string manipulation functions.
It uses a combination of pure Lira implementations and host primitives
(syscalls 30-39) for operations that require character-level access.

## Contents

- [Functions](#functions)


## Functions

### `to_upper`

```lira
fn to_upper(s: string) -> string
```

Convert string to uppercase

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `to_lower`

```lira
fn to_lower(s: string) -> string
```

Convert string to lowercase

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `char_at`

```lira
fn char_at(s: string, index: int) -> string
```

Get character at index (as a string)
Returns empty string if index is out of bounds

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `index` | `int` |

**Returns:** `string`

---

### `substring`

```lira
fn substring(s: string, start: int, end: int) -> string
```

Get substring from start (inclusive) to end (exclusive)

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `start` | `int` |
| `end` | `int` |

**Returns:** `string`

---

### `char_code`

```lira
fn char_code(s: string, index: int) -> int
```

Get character code at index
Returns -1 if index is out of bounds

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `index` | `int` |

**Returns:** `int`

---

### `from_char_code`

```lira
fn from_char_code(code: int) -> string
```

Create a single-character string from a character code

**Parameters:**

| Name | Type |
|------|------|
| `code` | `int` |

**Returns:** `string`

---

### `index_of`

```lira
fn index_of(s: string, substr: string) -> int
```

Find first occurrence of substring
Returns -1 if not found

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `substr` | `string` |

**Returns:** `int`

---

### `last_index_of`

```lira
fn last_index_of(s: string, substr: string) -> int
```

Find last occurrence of substring
Returns -1 if not found

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `substr` | `string` |

**Returns:** `int`

---

### `contains`

```lira
fn contains(s: string, substr: string) -> bool
```

Check if string contains substring

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `substr` | `string` |

**Returns:** `bool`

---

### `starts_with`

```lira
fn starts_with(s: string, prefix: string) -> bool
```

Check if string starts with prefix

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `prefix` | `string` |

**Returns:** `bool`

---

### `ends_with`

```lira
fn ends_with(s: string, suffix: string) -> bool
```

Check if string ends with suffix

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `suffix` | `string` |

**Returns:** `bool`

---

### `trim`

```lira
fn trim(s: string) -> string
```

Trim whitespace from both ends

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `trim_start`

```lira
fn trim_start(s: string) -> string
```

Trim whitespace from start

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `trim_end`

```lira
fn trim_end(s: string) -> string
```

Trim whitespace from end

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `split`

```lira
fn split(s: string, delimiter: string) -> [string]
```

Split string by delimiter

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `delimiter` | `string` |

**Returns:** `[string]`

---

### `join`

```lira
fn join(arr: [string], delimiter: string) -> string
```

Join array of strings with delimiter

**Parameters:**

| Name | Type |
|------|------|
| `arr` | `[string]` |
| `delimiter` | `string` |

**Returns:** `string`

---

### `replace`

```lira
fn replace(s: string, old_str: string, new_str: string) -> string
```

Replace all occurrences of old_str with new_str

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `old_str` | `string` |
| `new_str` | `string` |

**Returns:** `string`

---

### `replace_first`

```lira
fn replace_first(s: string, old_str: string, new_str: string) -> string
```

Replace first occurrence of old_str with new_str

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `old_str` | `string` |
| `new_str` | `string` |

**Returns:** `string`

---

### `repeat`

```lira
fn repeat(s: string, count: int) -> string
```

Repeat string n times

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `count` | `int` |

**Returns:** `string`

---

### `reverse`

```lira
fn reverse(s: string) -> string
```

Reverse a string

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `pad_start`

```lira
fn pad_start(s: string, length: int, pad: string) -> string
```

Pad start of string to reach target length

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `length` | `int` |
| `pad` | `string` |

**Returns:** `string`

---

### `pad_end`

```lira
fn pad_end(s: string, length: int, pad: string) -> string
```

Pad end of string to reach target length

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `length` | `int` |
| `pad` | `string` |

**Returns:** `string`

---

### `is_empty`

```lira
fn is_empty(s: string) -> bool
```

Check if string is empty

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `is_blank`

```lira
fn is_blank(s: string) -> bool
```

Check if string is empty or contains only whitespace

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `is_numeric`

```lira
fn is_numeric(s: string) -> bool
```

Check if string consists only of digits

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `is_alpha`

```lira
fn is_alpha(s: string) -> bool
```

Check if string consists only of letters (a-z, A-Z)

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `is_alphanumeric`

```lira
fn is_alphanumeric(s: string) -> bool
```

Check if string consists only of letters and digits

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `bool`

---

### `capitalize`

```lira
fn capitalize(s: string) -> string
```

Capitalize first character

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `title_case`

```lira
fn title_case(s: string) -> string
```

Convert to title case (capitalize each word)

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `string`

---

### `count`

```lira
fn count(s: string, substr: string) -> int
```

Count occurrences of substring

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |
| `substr` | `string` |

**Returns:** `int`

---

### `word_count`

```lira
fn word_count(s: string) -> int
```

Count number of words (space-separated)

**Parameters:**

| Name | Type |
|------|------|
| `s` | `string` |

**Returns:** `int`

---


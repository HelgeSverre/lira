# test

Lira Standard Library - Test Module
Testing utilities and assertions

This module provides stateless testing utilities. Since Lira has
limitations with module-level mutable state, the test counters are
passed explicitly or tracked at the call site.

Usage:
import std.test
test("my test", assert_eq(2 + 2, 4))
summary(1, 1, 0) // total, passed, failed

## Contents

- [Functions](#functions)

## Functions

### `describe`

```lira
fn describe(name: string)
```

Print a test suite header

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `name` | `string` |

---

### `test`

```lira
fn test(name: string, passed: bool) -> bool
```

Run a single test case and print result
Returns true if passed, false if failed

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `name`   | `string` |
| `passed` | `bool`   |

**Returns:** `bool`

---

### `run_test`

```lira
fn run_test(name: string, passed: bool) -> int
```

Run a test and increment counters (returns 1 if passed, 0 if failed)

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `name`   | `string` |
| `passed` | `bool`   |

**Returns:** `int`

---

### `summary`

```lira
fn summary(total: int, passed: int, failed: int)
```

Print test summary

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `total`  | `int` |
| `passed` | `int` |
| `failed` | `int` |

---

### `assert`

```lira
fn assert(condition: bool) -> bool
```

Assert a condition is true

**Parameters:**

| Name        | Type   |
| ----------- | ------ |
| `condition` | `bool` |

**Returns:** `bool`

---

### `assert_eq`

```lira
fn assert_eq(actual: int, expected: int) -> bool
```

Assert two integers are equal

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_eq_str`

```lira
fn assert_eq_str(actual: string, expected: string) -> bool
```

Assert two strings are equal

**Parameters:**

| Name       | Type     |
| ---------- | -------- |
| `actual`   | `string` |
| `expected` | `string` |

**Returns:** `bool`

---

### `assert_eq_float`

```lira
fn assert_eq_float(actual: float, expected: float, epsilon: float) -> bool
```

Assert two floats are approximately equal (within epsilon)

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `actual`   | `float` |
| `expected` | `float` |
| `epsilon`  | `float` |

**Returns:** `bool`

---

### `assert_ne`

```lira
fn assert_ne(actual: int, expected: int) -> bool
```

Assert two integers are not equal

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_ne_str`

```lira
fn assert_ne_str(actual: string, expected: string) -> bool
```

Assert two strings are not equal

**Parameters:**

| Name       | Type     |
| ---------- | -------- |
| `actual`   | `string` |
| `expected` | `string` |

**Returns:** `bool`

---

### `assert_true`

```lira
fn assert_true(value: bool) -> bool
```

Assert a value is true

**Parameters:**

| Name    | Type   |
| ------- | ------ |
| `value` | `bool` |

**Returns:** `bool`

---

### `assert_false`

```lira
fn assert_false(value: bool) -> bool
```

Assert a value is false

**Parameters:**

| Name    | Type   |
| ------- | ------ |
| `value` | `bool` |

**Returns:** `bool`

---

### `assert_gt`

```lira
fn assert_gt(actual: int, expected: int) -> bool
```

Assert actual > expected

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_gte`

```lira
fn assert_gte(actual: int, expected: int) -> bool
```

Assert actual >= expected

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_lt`

```lira
fn assert_lt(actual: int, expected: int) -> bool
```

Assert actual < expected

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_lte`

```lira
fn assert_lte(actual: int, expected: int) -> bool
```

Assert actual <= expected

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `actual`   | `int` |
| `expected` | `int` |

**Returns:** `bool`

---

### `assert_between`

```lira
fn assert_between(value: int, min_val: int, max_val: int) -> bool
```

Assert value is between min_val and max_val (inclusive)

**Parameters:**

| Name      | Type  |
| --------- | ----- |
| `value`   | `int` |
| `min_val` | `int` |
| `max_val` | `int` |

**Returns:** `bool`

---

### `assert_gt_float`

```lira
fn assert_gt_float(actual: float, expected: float) -> bool
```

Assert actual > expected (float)

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `actual`   | `float` |
| `expected` | `float` |

**Returns:** `bool`

---

### `assert_gte_float`

```lira
fn assert_gte_float(actual: float, expected: float) -> bool
```

Assert actual >= expected (float)

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `actual`   | `float` |
| `expected` | `float` |

**Returns:** `bool`

---

### `assert_lt_float`

```lira
fn assert_lt_float(actual: float, expected: float) -> bool
```

Assert actual < expected (float)

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `actual`   | `float` |
| `expected` | `float` |

**Returns:** `bool`

---

### `assert_lte_float`

```lira
fn assert_lte_float(actual: float, expected: float) -> bool
```

Assert actual <= expected (float)

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `actual`   | `float` |
| `expected` | `float` |

**Returns:** `bool`

---

### `assert_contains`

```lira
fn assert_contains(haystack: string, needle: string) -> bool
```

Assert string contains substring

**Parameters:**

| Name       | Type     |
| ---------- | -------- |
| `haystack` | `string` |
| `needle`   | `string` |

**Returns:** `bool`

---

### `assert_starts_with`

```lira
fn assert_starts_with(str: string, prefix: string) -> bool
```

Assert string starts with prefix

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `str`    | `string` |
| `prefix` | `string` |

**Returns:** `bool`

---

### `assert_ends_with`

```lira
fn assert_ends_with(str: string, suffix: string) -> bool
```

Assert string ends with suffix

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `str`    | `string` |
| `suffix` | `string` |

**Returns:** `bool`

---

### `assert_not_contains`

```lira
fn assert_not_contains(haystack: string, needle: string) -> bool
```

Assert string does not contain substring

**Parameters:**

| Name       | Type     |
| ---------- | -------- |
| `haystack` | `string` |
| `needle`   | `string` |

**Returns:** `bool`

---

### `assert_str_len`

```lira
fn assert_str_len(str: string, expected_len: int) -> bool
```

Assert string matches expected length

**Parameters:**

| Name           | Type     |
| -------------- | -------- |
| `str`          | `string` |
| `expected_len` | `int`    |

**Returns:** `bool`

---

### `assert_str_empty`

```lira
fn assert_str_empty(str: string) -> bool
```

Assert string is empty

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `str` | `string` |

**Returns:** `bool`

---

### `assert_str_not_empty`

```lira
fn assert_str_not_empty(str: string) -> bool
```

Assert string is not empty

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `str` | `string` |

**Returns:** `bool`

---

### `assert_len`

```lira
fn assert_len(arr: [int], expected: int) -> bool
```

Assert array has expected length

**Parameters:**

| Name       | Type    |
| ---------- | ------- |
| `arr`      | `[int]` |
| `expected` | `int`   |

**Returns:** `bool`

---

### `assert_empty`

```lira
fn assert_empty(arr: [int]) -> bool
```

Assert array is empty

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `bool`

---

### `assert_not_empty`

```lira
fn assert_not_empty(arr: [int]) -> bool
```

Assert array is not empty

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `bool`

---

### `assert_array_contains`

```lira
fn assert_array_contains(arr: [int], value: int) -> bool
```

Assert array contains a value

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `arr`   | `[int]` |
| `value` | `int`   |

**Returns:** `bool`

---

### `assert_null`

```lira
fn assert_null(value: int?) -> bool
```

Assert value is null

**Parameters:**

| Name    | Type   |
| ------- | ------ |
| `value` | `int?` |

**Returns:** `bool`

---

### `assert_not_null`

```lira
fn assert_not_null(value: int?) -> bool
```

Assert value is not null

**Parameters:**

| Name    | Type   |
| ------- | ------ |
| `value` | `int?` |

**Returns:** `bool`

---

### `section`

```lira
fn section(name: string)
```

Print a section header for organizing tests

**Parameters:**

| Name   | Type     |
| ------ | -------- |
| `name` | `string` |

---

### `skip`

```lira
fn skip(name: string, reason: string)
```

Skip a test with a reason

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `name`   | `string` |
| `reason` | `string` |

---

### `debug_msg`

```lira
fn debug_msg(msg: string)
```

Print a debug message during testing

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `msg` | `string` |

---

### `expect_fail`

```lira
fn expect_fail(name: string, passed: bool) -> bool
```

Mark a test as expected to fail (for documenting known issues)

**Parameters:**

| Name     | Type     |
| -------- | -------- |
| `name`   | `string` |
| `passed` | `bool`   |

**Returns:** `bool`

---

### `count_passed`

```lira
fn count_passed(result: bool) -> int
```

Increment passed count if result is true, else increment failed
Returns: 1 if passed, 0 if failed (for summing)

**Parameters:**

| Name     | Type   |
| -------- | ------ |
| `result` | `bool` |

**Returns:** `int`

---

### `count_failed`

```lira
fn count_failed(result: bool) -> int
```

Returns: 0 if passed, 1 if failed (for summing)

**Parameters:**

| Name     | Type   |
| -------- | ------ |
| `result` | `bool` |

**Returns:** `int`

---

### `all_passed`

```lira
fn all_passed(failed: int) -> bool
```

Check if all tests passed (no failures)

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `failed` | `int` |

**Returns:** `bool`

---

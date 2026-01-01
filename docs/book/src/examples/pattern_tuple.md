# pattern_tuple

Tuple Pattern Matching Tests
Tests destructuring tuples in match expressions
@expect: first is 1
@expect: sum is 6
@expect: nested first is 1

## Contents

- [Functions](#functions)

## Functions

### `get_first`

```lira
fn get_first(t: (int, int)) -> int
```

Tuple Pattern Matching Tests
Tests destructuring tuples in match expressions
@expect: first is 1
@expect: sum is 6
@expect: nested first is 1

**Parameters:**

| Name | Type         |
| ---- | ------------ |
| `t`  | `(int, int)` |

**Returns:** `int`

---

### `sum_tuple`

```lira
fn sum_tuple(t: (int, int, int)) -> int
```

**Parameters:**

| Name | Type              |
| ---- | ----------------- |
| `t`  | `(int, int, int)` |

**Returns:** `int`

---

### `get_nested_first`

```lira
fn get_nested_first(t: ((int, int), int)) -> int
```

Test nested tuple pattern

**Parameters:**

| Name | Type                |
| ---- | ------------------- |
| `t`  | `((int, int), int)` |

**Returns:** `int`

---

# collections

Lira Standard Library - Collections Module
Enhanced array/list operations

## Contents

- [Functions](#functions)

## Functions

### `map_double`

```lira
fn map_double(arr: [int]) -> [int]
```

Double each number

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `map_square`

```lira
fn map_square(arr: [int]) -> [int]
```

Square each number

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `filter_positive`

```lira
fn filter_positive(arr: [int]) -> [int]
```

Filter positive numbers

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `filter_even`

```lira
fn filter_even(arr: [int]) -> [int]
```

Filter even numbers

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `filter_odd`

```lira
fn filter_odd(arr: [int]) -> [int]
```

Filter odd numbers

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `sum`

```lira
fn sum(arr: [int]) -> int
```

Sum all elements

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `int`

---

### `product`

```lira
fn product(arr: [int]) -> int
```

Product of all elements

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `int`

---

### `min_of`

```lira
fn min_of(arr: [int]) -> int
```

Find minimum

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `int`

---

### `max_of`

```lira
fn max_of(arr: [int]) -> int
```

Find maximum

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `int`

---

### `average`

```lira
fn average(arr: [int]) -> float
```

Average

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `float`

---

### `contains_int`

```lira
fn contains_int(arr: [int], value: int) -> bool
```

Check if array contains value

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `arr`   | `[int]` |
| `value` | `int`   |

**Returns:** `bool`

---

### `index_of_int`

```lira
fn index_of_int(arr: [int], value: int) -> int
```

Find index of value (-1 if not found)

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `arr`   | `[int]` |
| `value` | `int`   |

**Returns:** `int`

---

### `count`

```lira
fn count(arr: [int], value: int) -> int
```

Count occurrences

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `arr`   | `[int]` |
| `value` | `int`   |

**Returns:** `int`

---

### `reverse_int`

```lira
fn reverse_int(arr: [int]) -> [int]
```

Reverse array

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `take`

```lira
fn take(arr: [int], n: int) -> [int]
```

Take first n elements

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |
| `n`   | `int`   |

**Returns:** `[int]`

---

### `skip`

```lira
fn skip(arr: [int], n: int) -> [int]
```

Skip first n elements

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |
| `n`   | `int`   |

**Returns:** `[int]`

---

### `slice`

```lira
fn slice(arr: [int], start: int, end: int) -> [int]
```

Slice array from start to end (exclusive)

**Parameters:**

| Name    | Type    |
| ------- | ------- |
| `arr`   | `[int]` |
| `start` | `int`   |
| `end`   | `int`   |

**Returns:** `[int]`

---

### `concat`

```lira
fn concat(arr1: [int], arr2: [int]) -> [int]
```

Concatenate two arrays

**Parameters:**

| Name   | Type    |
| ------ | ------- |
| `arr1` | `[int]` |
| `arr2` | `[int]` |

**Returns:** `[int]`

---

### `unique`

```lira
fn unique(arr: [int]) -> [int]
```

Remove duplicates

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `flatten`

```lira
fn flatten(arrays: [[int]]) -> [int]
```

Flatten nested operation (combine multiple arrays)

**Parameters:**

| Name     | Type      |
| -------- | --------- |
| `arrays` | `[[int]]` |

**Returns:** `[int]`

---

### `sort`

```lira
fn sort(arr: [int]) -> [int]
```

Sorting (Selection sort - works by finding min and appending)

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `sort_desc`

```lira
fn sort_desc(arr: [int]) -> [int]
```

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `range`

```lira
fn range(start: int, end: int) -> [int]
```

Generate range [start, end)

**Parameters:**

| Name    | Type  |
| ------- | ----- |
| `start` | `int` |
| `end`   | `int` |

**Returns:** `[int]`

---

### `range_step`

```lira
fn range_step(start: int, end: int, step: int) -> [int]
```

Generate range with step

**Parameters:**

| Name    | Type  |
| ------- | ----- |
| `start` | `int` |
| `end`   | `int` |
| `step`  | `int` |

**Returns:** `[int]`

---

### `repeat_int`

```lira
fn repeat_int(value: int, n: int) -> [int]
```

Repeat value n times

**Parameters:**

| Name    | Type  |
| ------- | ----- |
| `value` | `int` |
| `n`     | `int` |

**Returns:** `[int]`

---

### `all_truthy`

```lira
fn all_truthy(arr: [int]) -> bool
```

Check if all elements are non-zero

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `bool`

---

### `any_truthy`

```lira
fn any_truthy(arr: [int]) -> bool
```

Check if any element is non-zero

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `bool`

---

### `none_truthy`

```lira
fn none_truthy(arr: [int]) -> bool
```

Check if no elements are non-zero

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `bool`

---

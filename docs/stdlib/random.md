# random

Lira Standard Library - Random Module
Random number generation utilities

## Contents

- [Functions](#functions)

## Functions

### `random_bool`

```lira
fn random_bool() -> bool
```

Generate random boolean

**Returns:** `bool`

---

### `random_range`

```lira
fn random_range(min: float, max: float) -> float
```

Generate random float in range [min, max)

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `min` | `float` |
| `max` | `float` |

**Returns:** `float`

---

### `random_index`

```lira
fn random_index(length: int) -> int
```

Generate random index for an array of given length

**Parameters:**

| Name     | Type  |
| -------- | ----- |
| `length` | `int` |

**Returns:** `int`

---

### `shuffle_int_array`

```lira
fn shuffle_int_array(arr: [int]) -> [int]
```

Shuffle array in place (Fisher-Yates algorithm)
Returns the same array for convenience

**Parameters:**

| Name  | Type    |
| ----- | ------- |
| `arr` | `[int]` |

**Returns:** `[int]`

---

### `random_digits`

```lira
fn random_digits(num_digits: int) -> int
```

Generate a random integer with specified number of digits

**Parameters:**

| Name         | Type  |
| ------------ | ----- |
| `num_digits` | `int` |

**Returns:** `int`

---

### `coin_flip`

```lira
fn coin_flip() -> string
```

Generate random coin flip (returns "heads" or "tails")

**Returns:** `string`

---

### `dice_roll`

```lira
fn dice_roll() -> int
```

Generate random dice roll (1-6 by default)

**Returns:** `int`

---

### `dice_roll_n`

```lira
fn dice_roll_n(sides: int) -> int
```

Generate random dice roll with custom number of sides

**Parameters:**

| Name    | Type  |
| ------- | ----- |
| `sides` | `int` |

**Returns:** `int`

---

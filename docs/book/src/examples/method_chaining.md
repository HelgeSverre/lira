# method_chaining

Test method chaining with impl blocks
@expect: Counter: 3
@expect: Counter: 1
@expect: Computed: 25

## Contents

- [Structs](#structs)
- [Implementations](#implementations)
- [Functions](#functions)

## Structs

### `Counter`

```lira
struct Counter {
    value: int,
}
```

Test method chaining with impl blocks
@expect: Counter: 3
@expect: Counter: 1
@expect: Computed: 25

#### Fields

| Field   | Type  | Visibility |
| ------- | ----- | ---------- |
| `value` | `int` | private    |

---

### `Calculator`

```lira
struct Calculator {
    result: int,
}
```

#### Fields

| Field    | Type  | Visibility |
| -------- | ----- | ---------- |
| `result` | `int` | private    |

---

## Implementations

### `impl Counter`

#### Methods

##### `increment`

```lira
fn increment(self: Self) -> Counter
```

##### `decrement`

```lira
fn decrement(self: Self) -> Counter
```

##### `add`

```lira
fn add(self: Self, n: int) -> Counter
```

##### `get`

```lira
fn get(self: Self) -> int
```

---

### `impl Calculator`

#### Methods

##### `add`

```lira
fn add(self: Self, n: int) -> Calculator
```

##### `multiply`

```lira
fn multiply(self: Self, n: int) -> Calculator
```

##### `value`

```lira
fn value(self: Self) -> int
```

---

## Functions

### `new_counter`

```lira
fn new_counter() -> Counter
```

Factory function (separate from impl block)

**Returns:** `Counter`

---

### `start_calc`

```lira
fn start_calc(n: int) -> Calculator
```

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `Calculator`

---

### `main`

```lira
fn main()
```

---

# type_alias

Test type alias declarations
@expect: 42
@expect: hello

## Contents

- [Type Aliases](#type-aliases)
- [Functions](#functions)

## Type Aliases

### `Integer`

```lira
type Integer = int
```

Simple type alias

---

### `Text`

```lira
type Text = string
```

Simple type alias

---

### `IntArray`

```lira
type IntArray = [int]
```

Type alias for compound types

---

### `StringPair`

```lira
type StringPair = (string, string)
```

Type alias for compound types

---

### `MaybeInt`

```lira
type MaybeInt = int?
```

Type alias for optional

---

### `IntToInt`

```lira
type IntToInt = fn(int) -> int
```

Type alias for function types

---

## Functions

### `double`

```lira
fn double(x: int) -> int
```

Type alias for function types

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `x`  | `int` |

**Returns:** `int`

---

### `main`

```lira
fn main()
```

---

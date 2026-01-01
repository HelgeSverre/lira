# classes_basic

Class Tests
Tests basic class definitions
Currently 'class' works similarly to 'struct' but has type-checking issues

## Contents

- [Structs](#structs)
- [Classes](#classes)
- [Functions](#functions)

## Structs

### `PointS`

```lira
struct PointS {
    x: int,
    y: int,
}
```

Using struct (works)

#### Fields

| Field | Type  | Visibility |
| ----- | ----- | ---------- |
| `x`   | `int` | private    |
| `y`   | `int` | private    |

---

### `Rectangle`

```lira
struct Rectangle {
    width: int,
    height: int,
}
```

#### Fields

| Field    | Type  | Visibility |
| -------- | ----- | ---------- |
| `width`  | `int` | private    |
| `height` | `int` | private    |

---

### `Address`

```lira
struct Address {
    street: string,
    city: string,
}
```

#### Fields

| Field    | Type     | Visibility |
| -------- | -------- | ---------- |
| `street` | `string` | private    |
| `city`   | `string` | private    |

---

### `Person`

```lira
struct Person {
    name: string,
    age: int,
}
```

#### Fields

| Field  | Type     | Visibility |
| ------ | -------- | ---------- |
| `name` | `string` | private    |
| `age`  | `int`    | private    |

---

## Classes

### `PointC`

Using class (works for literals but type checking differs)

#### Fields

| Field | Type  | Visibility |
| ----- | ----- | ---------- |
| `x`   | `int` | private    |
| `y`   | `int` | private    |

---

## Functions

### `rect_area`

```lira
fn rect_area(r: Rectangle) -> int
```

**Parameters:**

| Name | Type        |
| ---- | ----------- |
| `r`  | `Rectangle` |

**Returns:** `int`

---

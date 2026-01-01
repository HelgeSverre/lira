# nested_structures

Nested Structures Tests
Tests deeply nested arrays, objects, and mixed structures

## Contents

- [Structs](#structs)
- [Functions](#functions)

## Structs

### `Address`

```lira
struct Address {
    street: string,
    city: string,
    zip: int,
}
```

Define nested structures

#### Fields

| Field    | Type     | Visibility |
| -------- | -------- | ---------- |
| `street` | `string` | private    |
| `city`   | `string` | private    |
| `zip`    | `int`    | private    |

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

## Functions

### `add`

```lira
fn add(a: int, b: int) -> int
```

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

### `mul`

```lira
fn mul(a: int, b: int) -> int
```

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `a`  | `int` |
| `b`  | `int` |

**Returns:** `int`

---

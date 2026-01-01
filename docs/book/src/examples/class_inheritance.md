# class_inheritance

Class Inheritance Example
Demonstrates extends, override, and super
@expect: Dog name: Buddy
@expect: Dog breed: Labrador
@expect: Cat name: Whiskers
@expect: Cat color: orange

## Contents

- [Classes](#classes)
- [Functions](#functions)

## Classes

### `Animal`

Class Inheritance Example
Demonstrates extends, override, and super
@expect: Dog name: Buddy
@expect: Dog breed: Labrador
@expect: Cat name: Whiskers
@expect: Cat color: orange

#### Fields

| Field  | Type     | Visibility |
| ------ | -------- | ---------- |
| `name` | `string` | private    |

---

### `Dog` extends `Animal`

#### Fields

| Field   | Type     | Visibility |
| ------- | -------- | ---------- |
| `breed` | `string` | private    |

---

### `Cat` extends `Animal`

#### Fields

| Field   | Type     | Visibility |
| ------- | -------- | ---------- |
| `color` | `string` | private    |

---

## Functions

### `main`

```lira
fn main()
```

---

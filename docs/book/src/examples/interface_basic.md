# interface_basic

Test interface declarations and implementations
@expect: Shape area: 50
@expect: Shape area: 78

## Contents

- [Structs](#structs)
- [Implementations](#implementations)
- [Functions](#functions)


## Structs

### `Rectangle`

```lira
struct Rectangle {
    width: int,
    height: int,
}
```

Struct that will implement the interface

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `width` | `int` | private |
| `height` | `int` | private |

---

### `Circle`

```lira
struct Circle {
    radius: int,
}
```

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `radius` | `int` | private |

---

## Implementations

### `impl Rectangle`

#### Methods

##### `area`

```lira
fn area(self: Self) -> int
```

Implement methods for Rectangle

##### `perimeter`

```lira
fn perimeter(self: Self) -> int
```

---

### `impl Circle`

#### Methods

##### `area`

```lira
fn area(self: Self) -> int
```

##### `perimeter`

```lira
fn perimeter(self: Self) -> int
```

---

## Functions

### `main`

```lira
fn main()
```

---


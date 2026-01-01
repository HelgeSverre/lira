# structs

Struct Example
Demonstrates struct literals, field access, and methods

## Contents

- [Structs](#structs)


## Structs

### `Point`

```lira
struct Point {
    x: int,
    y: int,
}
```

Define structs

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `x` | `int` | private |
| `y` | `int` | private |

#### Methods

##### `sum`

```lira
fn sum(self: Self) -> int
```

##### `add`

```lira
fn add(self: Self, other: Point) -> Point
```

---

### `Person`

```lira
struct Person {
    name: string,
    age: int,
}
```

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `name` | `string` | private |
| `age` | `int` | private |

#### Methods

##### `greet`

```lira
fn greet(self: Self) -> string
```

---

### `Line`

```lira
struct Line {
    start: Point,
    end: Point,
}
```

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `start` | `Point` | private |
| `end` | `Point` | private |

---


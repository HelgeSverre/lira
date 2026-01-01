# impl_block

Test impl blocks and method dispatch
@expect: 0
@expect: 1
@expect: 2

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

Test impl blocks and method dispatch
@expect: 0
@expect: 1
@expect: 2

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `value` | `int` | private |

---

## Implementations

### `impl Counter`

#### Methods

##### `new`

```lira
fn new() -> Counter
```

##### `get`

```lira
fn get(self: Self) -> int
```

##### `increment`

```lira
fn increment(self: Self) -> Counter
```

---

## Functions

### `main`

```lira
fn main()
```

---


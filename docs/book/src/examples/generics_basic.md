# generics_basic

Generics Tests
Tests generic function and struct syntax parsing
@expect-contains: identity int: 42
@expect-contains: identity string: hello
@expect-contains: box value: 100

## Contents

- [Structs](#structs)
- [Functions](#functions)


## Structs

### `Box`<T>

```lira
struct Box<T> {
    value: T,
}
```

Generic Box struct

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `value` | `T` | private |

---

## Functions

### `identity`

```lira
fn identity<T>(x: T) -> T
```

Generic identity function

**Parameters:**

| Name | Type |
|------|------|
| `x` | `T` |

**Returns:** `T`

---


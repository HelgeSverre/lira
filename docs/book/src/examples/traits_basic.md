# traits_basic

Test trait declarations and implementations
@expect: Dog says: Woof!
@expect: Cat says: Meow!

## Contents

- [Structs](#structs)
- [Traits](#traits)
- [Implementations](#implementations)
- [Functions](#functions)


## Structs

### `Dog`

```lira
struct Dog {
    name: string,
}
```

Struct that implements the trait

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `name` | `string` | private |

---

### `Cat`

```lira
struct Cat {
    name: string,
}
```

#### Fields

| Field | Type | Visibility |
|-------|------|------------|
| `name` | `string` | private |

---

## Traits

### `Speak`

Define a trait

#### Required Methods

| Method | Default |
|--------|--------|
| `fn speak(self) -> string` | No |

---

## Implementations

### `impl Speak for Dog`

#### Methods

##### `speak`

```lira
fn speak(self: Self) -> string
```

Implement trait for Dog

---

### `impl Speak for Cat`

#### Methods

##### `speak`

```lira
fn speak(self: Self) -> string
```

Implement trait for Cat

---

## Functions

### `main`

```lira
fn main()
```

---


# named_arguments

Test named arguments in function calls
@expect: Hello, Alice! You are 30 years old.
@expect: Hello, Bob! You are 25 years old.
@expect: Sum: 15

## Contents

- [Functions](#functions)


## Functions

### `greet`

```lira
fn greet(name: string, age: int)
```

Test named arguments in function calls
@expect: Hello, Alice! You are 30 years old.
@expect: Hello, Bob! You are 25 years old.
@expect: Sum: 15

**Parameters:**

| Name | Type |
|------|------|
| `name` | `string` |
| `age` | `int` |

---

### `add`

```lira
fn add(a: int, b: int, c: int) -> int
```

**Parameters:**

| Name | Type |
|------|------|
| `a` | `int` |
| `b` | `int` |
| `c` | `int` |

**Returns:** `int`

---

### `main`

```lira
fn main()
```

---


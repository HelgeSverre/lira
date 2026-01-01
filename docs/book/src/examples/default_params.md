# default_params

Test default parameter values in function declarations (parsing only)
Note: Default params are parsed but may require explicit args at call site
@expect: Hello World
@expect: Hi Everyone
@expect: Greetings Universe

## Contents

- [Functions](#functions)


## Functions

### `greet`

```lira
fn greet(name: string, greeting: string)
```

Function with default parameter (syntax test)

**Parameters:**

| Name | Type |
|------|------|
| `name` | `string` |
| `greeting` | `string` |

---

### `main`

```lira
fn main()
```

---


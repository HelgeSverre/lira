# spawn_expression

Test spawn expressions for fiber creation
Note: Actual fiber execution requires fiber mode in VM
@expect: Fiber syntax test
@expect: Worker spawned
@expect: Computation spawned

## Contents

- [Functions](#functions)


## Functions

### `worker`

```lira
fn worker(id: int)
```

Test spawn expressions for fiber creation
Note: Actual fiber execution requires fiber mode in VM
@expect: Fiber syntax test
@expect: Worker spawned
@expect: Computation spawned

**Parameters:**

| Name | Type |
|------|------|
| `id` | `int` |

---

### `compute`

```lira
fn compute(a: int, b: int) -> int
```

**Parameters:**

| Name | Type |
|------|------|
| `a` | `int` |
| `b` | `int` |

**Returns:** `int`

---

### `main`

```lira
fn main()
```

---


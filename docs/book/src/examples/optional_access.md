# optional_access

Optional Access Tests
Tests null-safe field access with ?.
@expect-contains: name from valid: Alice
@expect-contains: name from null: null

## Contents

- [Structs](#structs)


## Structs

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

---


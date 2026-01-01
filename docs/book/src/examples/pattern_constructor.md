# pattern_constructor

Constructor Pattern Matching Tests
Tests matching enum variants with pattern matching
@expect: red variant
@expect: active status
@expect: Found some value: 42

## Contents

- [Enums](#enums)
- [Functions](#functions)


## Enums

### `Color`

```lira
enum Color {
    Red,
    Green,
    Blue,
}
```

Constructor Pattern Matching Tests
Tests matching enum variants with pattern matching
@expect: red variant
@expect: active status
@expect: Found some value: 42

#### Variants

| Variant | Fields |
|---------|--------|
| `Red` | - |
| `Green` | - |
| `Blue` | - |

---

### `Status`

```lira
enum Status {
    Active,
    Inactive,
    Pending,
}
```

#### Variants

| Variant | Fields |
|---------|--------|
| `Active` | - |
| `Inactive` | - |
| `Pending` | - |

---

## Functions

### `describe_color`

```lira
fn describe_color(c: Color) -> string
```

Match on simple enum variant

**Parameters:**

| Name | Type |
|------|------|
| `c` | `Color` |

**Returns:** `string`

---

### `describe_status`

```lira
fn describe_status(s: Status) -> string
```

**Parameters:**

| Name | Type |
|------|------|
| `s` | `Status` |

**Returns:** `string`

---

### `color_or_default`

```lira
fn color_or_default(c: Color) -> string
```

Test with wildcard fallback

**Parameters:**

| Name | Type |
|------|------|
| `c` | `Color` |

**Returns:** `string`

---


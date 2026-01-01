# pattern_constructor_verify

Constructor Pattern Verification Test
Verify that constructor patterns actually match correctly
@expect: blue matches blue
@expect: green matches green
@expect: red matches red

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

Constructor Pattern Verification Test
Verify that constructor patterns actually match correctly
@expect: blue matches blue
@expect: green matches green
@expect: red matches red

#### Variants

| Variant | Fields |
| ------- | ------ |
| `Red`   | -      |
| `Green` | -      |
| `Blue`  | -      |

---

## Functions

### `describe_color`

```lira
fn describe_color(c: Color) -> string
```

**Parameters:**

| Name | Type    |
| ---- | ------- |
| `c`  | `Color` |

**Returns:** `string`

---

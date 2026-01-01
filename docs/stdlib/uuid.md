# uuid

Lira Standard Library - UUID Module
UUID generation and validation

## Contents

- [Functions](#functions)


## Functions

### `uuid`

```lira
fn uuid() -> string
```

Aliases for convenience

**Returns:** `string`

---

### `generate`

```lira
fn generate() -> string
```

**Returns:** `string`

---

### `random`

```lira
fn random() -> string
```

**Returns:** `string`

---

### `time_ordered`

```lira
fn time_ordered() -> string
```

**Returns:** `string`

---

### `is_nil`

```lira
fn is_nil(uuid: string) -> bool
```

Check if UUID is nil

**Parameters:**

| Name | Type |
|------|------|
| `uuid` | `string` |

**Returns:** `bool`

---

### `version`

```lira
fn version(uuid: string) -> int
```

Get version from UUID string (simple extraction)

**Parameters:**

| Name | Type |
|------|------|
| `uuid` | `string` |

**Returns:** `int`

---


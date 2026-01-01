# test_random

Test random module functionality
@expect-contains: random() test passed
@expect-contains: random_int() test passed
@expect-contains: Different values generated
@expect-contains: All tests passed

## Contents

- [Functions](#functions)


## Functions

### `test_random_float`

```lira
fn test_random_float()
```

Test random() returns values between 0 and 1

---

### `test_random_int`

```lira
fn test_random_int()
```

Test random_int(min, max) returns values in range

---

### `test_random_bool`

```lira
fn test_random_bool()
```

Test random_bool returns true or false

---

### `test_randomness`

```lira
fn test_randomness()
```

Test that multiple calls return different values

---

### `test_dice_roll`

```lira
fn test_dice_roll()
```

Test dice_roll

---

### `test_coin_flip`

```lira
fn test_coin_flip()
```

Test coin_flip

---


# time

Lira Standard Library - Time Module
Time manipulation and formatting

Built-in functions (direct syscalls):
time_ms() -> int - Current time in milliseconds since epoch (syscall 4)
sleep(ms: int) - Sleep for milliseconds (syscall 5)
time_secs() -> int - Current time in seconds since epoch (syscall 6)
time_micros() -> int - Current time in microseconds since epoch (syscall 7)
time_nanos() -> int - Current time in nanoseconds since epoch (syscall 8)
time_format_iso(ms) -> string - Format timestamp as ISO 8601 string (syscall 130)
time_format(ms, fmt) -> string- Format timestamp with custom format (syscall 131)
time_parse_iso(str) -> int - Parse ISO 8601 string to timestamp (syscall 132)
time_timezone_offset() -> int - Get local timezone offset in minutes (syscall 133)
time_components(ms) -> [int] - Get [year, month, day, hour, min, sec] (syscall 134)
time_from_components(y, m, d, h, m, s) -> int - Create timestamp from components (syscall 135)

## Contents

- [Functions](#functions)

## Functions

### `seconds`

```lira
fn seconds(n: int) -> int
```

Convert seconds to milliseconds

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `minutes`

```lira
fn minutes(n: int) -> int
```

Convert minutes to milliseconds

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `hours`

```lira
fn hours(n: int) -> int
```

Convert hours to milliseconds

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `days`

```lira
fn days(n: int) -> int
```

Convert days to milliseconds

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `weeks`

```lira
fn weeks(n: int) -> int
```

Convert weeks to milliseconds

**Parameters:**

| Name | Type  |
| ---- | ----- |
| `n`  | `int` |

**Returns:** `int`

---

### `today`

```lira
fn today() -> string
```

Get current date as string (YYYY-MM-DD)

**Returns:** `string`

---

### `now_time`

```lira
fn now_time() -> string
```

Get current time as string (HH:MM:SS)

**Returns:** `string`

---

### `now_iso`

```lira
fn now_iso() -> string
```

Get current datetime as ISO 8601 string

**Returns:** `string`

---

### `now_format`

```lira
fn now_format(fmt: string) -> string
```

Get current datetime with custom format

**Parameters:**

| Name  | Type     |
| ----- | -------- |
| `fmt` | `string` |

**Returns:** `string`

---

### `elapsed_ms`

```lira
fn elapsed_ms(start_ms: int) -> int
```

Measure elapsed time from a start timestamp

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `start_ms` | `int` |

**Returns:** `int`

---

### `elapsed_secs`

```lira
fn elapsed_secs(start_ms: int) -> int
```

Measure elapsed time in seconds from a start timestamp

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `start_ms` | `int` |

**Returns:** `int`

---

### `add_ms`

```lira
fn add_ms(timestamp_ms: int, duration_ms: int) -> int
```

Add duration to timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `duration_ms`  | `int` |

**Returns:** `int`

---

### `diff_ms`

```lira
fn diff_ms(end_ms: int, start_ms: int) -> int
```

Subtract timestamps to get duration in milliseconds

**Parameters:**

| Name       | Type  |
| ---------- | ----- |
| `end_ms`   | `int` |
| `start_ms` | `int` |

**Returns:** `int`

---

### `add_seconds`

```lira
fn add_seconds(timestamp_ms: int, secs: int) -> int
```

Add seconds to timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `secs`         | `int` |

**Returns:** `int`

---

### `add_minutes`

```lira
fn add_minutes(timestamp_ms: int, mins: int) -> int
```

Add minutes to timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `mins`         | `int` |

**Returns:** `int`

---

### `add_hours`

```lira
fn add_hours(timestamp_ms: int, hrs: int) -> int
```

Add hours to timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `hrs`          | `int` |

**Returns:** `int`

---

### `add_days`

```lira
fn add_days(timestamp_ms: int, d: int) -> int
```

Add days to timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `d`            | `int` |

**Returns:** `int`

---

### `is_today`

```lira
fn is_today(timestamp_ms: int) -> bool
```

Check if timestamp is today

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `bool`

---

### `is_before`

```lira
fn is_before(timestamp_ms: int, other_ms: int) -> bool
```

Check if timestamp is before another

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `other_ms`     | `int` |

**Returns:** `bool`

---

### `is_after`

```lira
fn is_after(timestamp_ms: int, other_ms: int) -> bool
```

Check if timestamp is after another

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |
| `other_ms`     | `int` |

**Returns:** `bool`

---

### `is_past`

```lira
fn is_past(timestamp_ms: int) -> bool
```

Check if timestamp is in the past

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `bool`

---

### `is_future`

```lira
fn is_future(timestamp_ms: int) -> bool
```

Check if timestamp is in the future

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `bool`

---

### `get_year`

```lira
fn get_year(timestamp_ms: int) -> int
```

Get year from timestamp

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `get_month`

```lira
fn get_month(timestamp_ms: int) -> int
```

Get month from timestamp (1-12)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `get_day`

```lira
fn get_day(timestamp_ms: int) -> int
```

Get day from timestamp (1-31)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `get_hour`

```lira
fn get_hour(timestamp_ms: int) -> int
```

Get hour from timestamp (0-23)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `get_minute`

```lira
fn get_minute(timestamp_ms: int) -> int
```

Get minute from timestamp (0-59)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `get_second`

```lira
fn get_second(timestamp_ms: int) -> int
```

Get second from timestamp (0-59)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `int`

---

### `format_date`

```lira
fn format_date(timestamp_ms: int) -> string
```

Format as date only (YYYY-MM-DD)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `string`

---

### `format_time`

```lira
fn format_time(timestamp_ms: int) -> string
```

Format as time only (HH:MM:SS)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `string`

---

### `format_datetime`

```lira
fn format_datetime(timestamp_ms: int) -> string
```

Format as datetime (YYYY-MM-DD HH:MM:SS)

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `string`

---

### `format_readable`

```lira
fn format_readable(timestamp_ms: int) -> string
```

Format as human-readable date (e.g., "January 1, 2024")

**Parameters:**

| Name           | Type  |
| -------------- | ----- |
| `timestamp_ms` | `int` |

**Returns:** `string`

---

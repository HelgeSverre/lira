# Lira Standard Library Specification

## Document Information

| Property        | Value               |
| --------------- | ------------------- |
| **Document ID** | 30-standard-library |
| **Version**     | 1.0.0-draft         |
| **Status**      | Draft Specification |

---

## Table of Contents

1. [Overview](#1-overview)
2. [std.core](#2-stdcore)
3. [std.io](#3-stdio)
4. [std.fs](#4-stdfs)
5. [std.collections](#5-stdcollections)
6. [std.string](#6-stdstring)
7. [std.math](#7-stdmath)
8. [std.time](#8-stdtime)
9. [std.sync](#9-stdsync)
10. [std.os](#10-stdos)
11. [gui.core](#11-guicore)
12. [gui.widgets](#12-guiwidgets)

---

## 1. Overview

### 1.1 Library Structure

```
std/
├── core/           # Fundamental types and traits
├── io/             # Input/output operations
├── fs/             # File system
├── collections/    # Data structures
├── string/         # String utilities
├── math/           # Mathematical functions
├── time/           # Time and duration
├── sync/           # Synchronization primitives
├── os/             # OS interaction
├── net/            # Networking (future)
└── encoding/       # Encoding/decoding (future)

gui/
├── core/           # GUI core types
├── widgets/        # Widget library
├── events/         # Event handling
└── canvas/         # Custom drawing
```

### 1.2 Prelude (Auto-Imported)

These are available without explicit import:

```li
// Primitive types
bool, int, float, string, char, void, never

// Integer variants
int8, int16, int32, int64
uint8, uint16, uint32, uint64

// Collections
List<T>, Map<K,V>, Set<T>

// Option and Result
Option<T>, Some(T), None
Result<T, E>, Ok(T), Err(E)

// I/O functions
print, println

// Assertions
assert, panic

// Traits
Clone, Copy, Eq, Hash, Ord, Debug, ToString
```

---

## 2. std.core

### 2.1 Option Type

```li
/// Optional value type
pub enum Option<T> {
    Some(T),
    None,
}

impl<T> Option<T> {
    /// Check if contains a value
    pub fn is_some() -> bool

    /// Check if empty
    pub fn is_none() -> bool

    /// Unwrap value, panic if None
    pub fn unwrap() -> T

    /// Unwrap with default
    pub fn unwrap_or(default: T) -> T

    /// Unwrap with lazy default
    pub fn unwrap_or_else(f: fn() -> T) -> T

    /// Map the contained value
    pub fn map<U>(f: fn(T) -> U) -> Option<U>

    /// Flat map
    pub fn and_then<U>(f: fn(T) -> Option<U>) -> Option<U>

    /// Filter by predicate
    pub fn filter(predicate: fn(T) -> bool) -> Option<T>

    /// Get or insert default
    pub fn get_or_insert(default: T) -> T

    /// Take value, leaving None
    pub fn take() -> Option<T>

    /// Convert to Result
    pub fn ok_or<E>(err: E) -> Result<T, E>
}

// Usage
let name: string? = get_name()
let greeting = name.map(n => "Hello, " + n).unwrap_or("Hello, stranger")
```

### 2.2 Result Type

```li
/// Result type for error handling
pub enum Result<T, E> {
    Ok(T),
    Err(E),
}

impl<T, E> Result<T, E> {
    /// Check if Ok
    pub fn is_ok() -> bool

    /// Check if Err
    pub fn is_err() -> bool

    /// Unwrap Ok value, panic if Err
    pub fn unwrap() -> T

    /// Unwrap Err value, panic if Ok
    pub fn unwrap_err() -> E

    /// Unwrap with default
    pub fn unwrap_or(default: T) -> T

    /// Map Ok value
    pub fn map<U>(f: fn(T) -> U) -> Result<U, E>

    /// Map Err value
    pub fn map_err<F>(f: fn(E) -> F) -> Result<T, F>

    /// Flat map
    pub fn and_then<U>(f: fn(T) -> Result<U, E>) -> Result<U, E>

    /// Convert to Option
    pub fn ok() -> Option<T>
    pub fn err() -> Option<E>
}

// Usage
fn read_config() -> Result<Config, Error> {
    let content = read_file("config.json")?
    let config = parse_json(content)?
    Ok(config)
}
```

### 2.3 Core Traits

```li
/// Equality comparison
pub interface Eq {
    fn eq(other: Self) -> bool
    fn ne(other: Self) -> bool = !eq(other)
}

/// Ordering comparison
pub interface Ord: Eq {
    fn cmp(other: Self) -> Ordering
    fn lt(other: Self) -> bool
    fn le(other: Self) -> bool
    fn gt(other: Self) -> bool
    fn ge(other: Self) -> bool
}

pub enum Ordering {
    Less,
    Equal,
    Greater,
}

/// Hashable type
pub interface Hash {
    fn hash() -> int
}

/// Clonable type
pub interface Clone {
    fn clone() -> Self
}

/// Debug representation
pub interface Debug {
    fn debug_string() -> string
}

/// String conversion
pub interface ToString {
    fn to_string() -> string
}

pub interface FromString {
    fn from_string(s: string) -> Result<Self, ParseError>
}

/// Default value
pub interface Default {
    static fn default() -> Self
}

/// Iterator protocol
pub interface Iterator<T> {
    fn next() -> Option<T>

    // Provided methods
    fn count() -> int
    fn collect<C: FromIterator<T>>() -> C
    fn map<U>(f: fn(T) -> U) -> MapIterator<T, U>
    fn filter(f: fn(T) -> bool) -> FilterIterator<T>
    fn fold<A>(init: A, f: fn(A, T) -> A) -> A
    fn reduce(f: fn(T, T) -> T) -> Option<T>
    fn any(f: fn(T) -> bool) -> bool
    fn all(f: fn(T) -> bool) -> bool
    fn find(f: fn(T) -> bool) -> Option<T>
    fn enumerate() -> EnumerateIterator<T>
    fn take(n: int) -> TakeIterator<T>
    fn skip(n: int) -> SkipIterator<T>
    fn zip<U>(other: Iterator<U>) -> ZipIterator<T, U>
}
```

---

## 3. std.io

### 3.1 Print Functions

```li
/// Print to stdout
pub fn print(args: ...any)

/// Print line to stdout
pub fn println(args: ...any)

/// Print to stderr
pub fn eprint(args: ...any)

/// Print line to stderr
pub fn eprintln(args: ...any)

/// Format string
pub fn format(template: string, args: ...any) -> string

// Usage
println("Hello, World!")
println("Count:", count, "items")
let msg = format("User {} has {} points", name, score)
```

### 3.2 Input Functions

```li
/// Read line from stdin
pub fn read_line() -> Result<string, IOError>

/// Read line with prompt
pub fn input(prompt: string) -> Result<string, IOError>

// Usage
let name = input("Enter your name: ")?
println("Hello,", name)
```

### 3.3 Reader and Writer

```li
/// Readable stream
pub interface Reader {
    fn read(buffer: &mut [uint8]) -> Result<int, IOError>
    fn read_all() -> Result<[uint8], IOError>
    fn read_to_string() -> Result<string, IOError>
}

/// Writable stream
pub interface Writer {
    fn write(data: &[uint8]) -> Result<int, IOError>
    fn write_all(data: &[uint8]) -> Result<void, IOError>
    fn write_string(s: string) -> Result<void, IOError>
    fn flush() -> Result<void, IOError>
}

/// Buffered reader
pub class BufReader<R: Reader> {
    pub fn new(reader: R) -> BufReader<R>
    pub fn lines() -> Iterator<Result<string, IOError>>
    pub fn read_line() -> Result<string, IOError>
}

/// Buffered writer
pub class BufWriter<W: Writer> {
    pub fn new(writer: W) -> BufWriter<W>
    pub fn flush() -> Result<void, IOError>
}

/// Standard streams
pub let stdin: Reader
pub let stdout: Writer
pub let stderr: Writer
```

---

## 4. std.fs

### 4.1 File Operations

```li
/// File handle
pub class File: Reader, Writer {
    /// Open file for reading
    pub static fn open(path: string) -> Result<File, IOError>

    /// Create file for writing
    pub static fn create(path: string) -> Result<File, IOError>

    /// Open with options
    pub static fn with_options(path: string, options: OpenOptions) -> Result<File, IOError>

    /// Read entire file
    pub static fn read(path: string) -> Result<[uint8], IOError>

    /// Read file as string
    pub static fn read_to_string(path: string) -> Result<string, IOError>

    /// Write to file
    pub static fn write(path: string, data: &[uint8]) -> Result<void, IOError>

    /// Write string to file
    pub static fn write_string(path: string, content: string) -> Result<void, IOError>

    /// Append to file
    pub static fn append(path: string, data: &[uint8]) -> Result<void, IOError>

    // Instance methods
    pub fn read(buffer: &mut [uint8]) -> Result<int, IOError>
    pub fn write(data: &[uint8]) -> Result<int, IOError>
    pub fn seek(pos: int, whence: SeekFrom) -> Result<int, IOError>
    pub fn position() -> int
    pub fn size() -> Result<int, IOError>
    pub fn close() -> Result<void, IOError>
}

pub struct OpenOptions {
    read: bool = false,
    write: bool = false,
    append: bool = false,
    create: bool = false,
    truncate: bool = false,
}

pub enum SeekFrom {
    Start,
    Current,
    End,
}
```

### 4.2 Path Operations

```li
/// Path utilities
pub class Path {
    pub fn new(path: string) -> Path

    pub fn join(other: string) -> Path
    pub fn parent() -> Option<Path>
    pub fn file_name() -> Option<string>
    pub fn extension() -> Option<string>
    pub fn stem() -> Option<string>

    pub fn is_absolute() -> bool
    pub fn is_relative() -> bool

    pub fn exists() -> bool
    pub fn is_file() -> bool
    pub fn is_dir() -> bool

    pub fn to_string() -> string
}

// Usage
let path = Path.new("/home/user/file.txt")
println(path.parent())      // /home/user
println(path.file_name())   // file.txt
println(path.extension())   // txt
```

### 4.3 Directory Operations

```li
/// Create directory
pub fn create_dir(path: string) -> Result<void, IOError>

/// Create directory and parents
pub fn create_dir_all(path: string) -> Result<void, IOError>

/// Remove empty directory
pub fn remove_dir(path: string) -> Result<void, IOError>

/// Remove directory recursively
pub fn remove_dir_all(path: string) -> Result<void, IOError>

/// Read directory entries
pub fn read_dir(path: string) -> Result<Iterator<DirEntry>, IOError>

/// Directory entry
pub struct DirEntry {
    name: string,
    path: Path,
    is_file: bool,
    is_dir: bool,
    size: int,
    modified: DateTime,
}

/// File copy
pub fn copy(from: string, to: string) -> Result<int, IOError>

/// Move/rename
pub fn rename(from: string, to: string) -> Result<void, IOError>

/// Remove file
pub fn remove(path: string) -> Result<void, IOError>

/// Check existence
pub fn exists(path: string) -> bool
pub fn is_file(path: string) -> bool
pub fn is_dir(path: string) -> bool
```

---

## 5. std.collections

### 5.1 List

```li
/// Dynamic array
pub class List<T> {
    /// Create empty list
    pub static fn new() -> List<T>

    /// Create with capacity
    pub static fn with_capacity(cap: int) -> List<T>

    /// Create from iterator
    pub static fn from_iter<I: Iterator<T>>(iter: I) -> List<T>

    // Properties
    pub fn len() -> int
    pub fn is_empty() -> bool
    pub fn capacity() -> int

    // Access
    pub fn get(index: int) -> Option<T>
    pub fn first() -> Option<T>
    pub fn last() -> Option<T>

    // Modification
    pub fn push(item: T)
    pub fn pop() -> Option<T>
    pub fn insert(index: int, item: T)
    pub fn remove(index: int) -> T
    pub fn clear()
    pub fn truncate(len: int)
    pub fn extend<I: Iterator<T>>(iter: I)

    // Search
    pub fn contains(item: T) -> bool where T: Eq
    pub fn index_of(item: T) -> Option<int> where T: Eq
    pub fn find(predicate: fn(T) -> bool) -> Option<T>

    // Transform
    pub fn map<U>(f: fn(T) -> U) -> List<U>
    pub fn filter(f: fn(T) -> bool) -> List<T>
    pub fn fold<A>(init: A, f: fn(A, T) -> A) -> A
    pub fn reduce(f: fn(T, T) -> T) -> Option<T>

    // Sorting
    pub fn sort() where T: Ord
    pub fn sort_by(compare: fn(T, T) -> Ordering)
    pub fn reverse()

    // Slicing
    pub fn slice(start: int, end: int) -> List<T>

    // Iteration
    pub fn iter() -> Iterator<T>
    pub fn enumerate() -> Iterator<(int, T)>
}

// Indexing operator
list[0]     // Get
list[0] = x // Set
```

### 5.2 Map

```li
/// Hash map
pub class Map<K: Hash + Eq, V> {
    pub static fn new() -> Map<K, V>
    pub static fn with_capacity(cap: int) -> Map<K, V>

    // Properties
    pub fn len() -> int
    pub fn is_empty() -> bool

    // Access
    pub fn get(key: K) -> Option<V>
    pub fn contains_key(key: K) -> bool

    // Modification
    pub fn insert(key: K, value: V) -> Option<V>
    pub fn remove(key: K) -> Option<V>
    pub fn clear()

    // Access with default
    pub fn get_or_insert(key: K, default: V) -> V
    pub fn entry(key: K) -> Entry<K, V>

    // Iteration
    pub fn keys() -> Iterator<K>
    pub fn values() -> Iterator<V>
    pub fn iter() -> Iterator<(K, V)>
}

// Usage
let map = Map.new()
map.insert("name", "Alice")
map.insert("age", "30")
let name = map.get("name").unwrap_or("Unknown")
```

### 5.3 Set

```li
/// Hash set
pub class Set<T: Hash + Eq> {
    pub static fn new() -> Set<T>
    pub static fn from_list(list: List<T>) -> Set<T>

    // Properties
    pub fn len() -> int
    pub fn is_empty() -> bool

    // Access
    pub fn contains(item: T) -> bool

    // Modification
    pub fn insert(item: T) -> bool
    pub fn remove(item: T) -> bool
    pub fn clear()

    // Set operations
    pub fn union(other: Set<T>) -> Set<T>
    pub fn intersection(other: Set<T>) -> Set<T>
    pub fn difference(other: Set<T>) -> Set<T>
    pub fn symmetric_difference(other: Set<T>) -> Set<T>
    pub fn is_subset(other: Set<T>) -> bool
    pub fn is_superset(other: Set<T>) -> bool

    // Iteration
    pub fn iter() -> Iterator<T>
}
```

### 5.4 Other Collections

```li
/// Double-ended queue
pub class Deque<T> {
    pub fn push_front(item: T)
    pub fn push_back(item: T)
    pub fn pop_front() -> Option<T>
    pub fn pop_back() -> Option<T>
    pub fn front() -> Option<T>
    pub fn back() -> Option<T>
}

/// Stack (LIFO)
pub class Stack<T> {
    pub fn push(item: T)
    pub fn pop() -> Option<T>
    pub fn peek() -> Option<T>
}

/// Queue (FIFO)
pub class Queue<T> {
    pub fn enqueue(item: T)
    pub fn dequeue() -> Option<T>
    pub fn peek() -> Option<T>
}

/// Binary heap (priority queue)
pub class BinaryHeap<T: Ord> {
    pub fn push(item: T)
    pub fn pop() -> Option<T>
    pub fn peek() -> Option<T>
}

/// Ordered map (B-tree)
pub class BTreeMap<K: Ord, V> {
    // Same API as Map, but ordered by key
    pub fn range(start: K, end: K) -> Iterator<(K, V)>
}
```

---

## 6. std.string

### 6.1 String Methods

```li
impl string {
    // Properties
    pub fn len() -> int
    pub fn is_empty() -> bool

    // Case conversion
    pub fn to_lowercase() -> string
    pub fn to_uppercase() -> string
    pub fn capitalize() -> string

    // Trimming
    pub fn trim() -> string
    pub fn trim_start() -> string
    pub fn trim_end() -> string
    pub fn trim_char(c: char) -> string

    // Split and join
    pub fn split(separator: string) -> List<string>
    pub fn split_lines() -> List<string>
    pub fn split_whitespace() -> List<string>

    // Search
    pub fn contains(substr: string) -> bool
    pub fn starts_with(prefix: string) -> bool
    pub fn ends_with(suffix: string) -> bool
    pub fn index_of(substr: string) -> Option<int>
    pub fn last_index_of(substr: string) -> Option<int>

    // Replace
    pub fn replace(old: string, new: string) -> string
    pub fn replace_all(old: string, new: string) -> string

    // Substring
    pub fn substring(start: int, end: int) -> string
    pub fn slice(start: int, end: int) -> string
    pub fn char_at(index: int) -> Option<char>

    // Character operations
    pub fn chars() -> Iterator<char>
    pub fn bytes() -> Iterator<uint8>

    // Padding
    pub fn pad_start(width: int, fill: char = ' ') -> string
    pub fn pad_end(width: int, fill: char = ' ') -> string

    // Repetition
    pub fn repeat(count: int) -> string

    // Parsing
    pub fn parse<T: FromString>() -> Result<T, ParseError>

    // Encoding
    pub fn to_bytes() -> [uint8]
    pub static fn from_bytes(bytes: &[uint8]) -> Result<string, Utf8Error>
}

// String builder
pub class StringBuilder {
    pub fn new() -> StringBuilder
    pub fn append(s: string) -> StringBuilder
    pub fn append_char(c: char) -> StringBuilder
    pub fn append_line(s: string) -> StringBuilder
    pub fn clear()
    pub fn to_string() -> string
}
```

### 6.2 String Utilities

```li
/// Join strings
pub fn join(items: List<string>, separator: string) -> string

/// Format with placeholders
pub fn format(template: string, args: ...any) -> string

// Usage
let items = ["a", "b", "c"]
let joined = join(items, ", ")  // "a, b, c"

let msg = format("Hello, {}! You have {} messages.", name, count)
```

---

## 7. std.math

### 7.1 Constants

```li
pub const PI: float = 3.141592653589793
pub const E: float = 2.718281828459045
pub const TAU: float = 6.283185307179586
pub const SQRT_2: float = 1.4142135623730951
pub const LN_2: float = 0.6931471805599453
pub const LN_10: float = 2.302585092994046

pub const INT_MAX: int = 2147483647
pub const INT_MIN: int = -2147483648
pub const FLOAT_MAX: float = 1.7976931348623157e+308
pub const FLOAT_MIN: float = 2.2250738585072014e-308
```

### 7.2 Basic Functions

```li
// Absolute value
pub fn abs(x: int) -> int
pub fn abs(x: float) -> float

// Min/max
pub fn min(a: int, b: int) -> int
pub fn max(a: int, b: int) -> int
pub fn clamp(x: int, min: int, max: int) -> int

// Sign
pub fn sign(x: int) -> int   // -1, 0, or 1
pub fn sign(x: float) -> float

// Rounding
pub fn floor(x: float) -> float
pub fn ceil(x: float) -> float
pub fn round(x: float) -> float
pub fn trunc(x: float) -> float

// Conversion
pub fn to_int(x: float) -> int
pub fn to_float(x: int) -> float
```

### 7.3 Power and Roots

```li
pub fn pow(base: float, exp: float) -> float
pub fn sqrt(x: float) -> float
pub fn cbrt(x: float) -> float
pub fn hypot(x: float, y: float) -> float
```

### 7.4 Trigonometric Functions

```li
// Basic trig
pub fn sin(x: float) -> float
pub fn cos(x: float) -> float
pub fn tan(x: float) -> float

// Inverse trig
pub fn asin(x: float) -> float
pub fn acos(x: float) -> float
pub fn atan(x: float) -> float
pub fn atan2(y: float, x: float) -> float

// Hyperbolic
pub fn sinh(x: float) -> float
pub fn cosh(x: float) -> float
pub fn tanh(x: float) -> float

// Degree/radian conversion
pub fn to_degrees(radians: float) -> float
pub fn to_radians(degrees: float) -> float
```

### 7.5 Logarithms and Exponentials

```li
pub fn exp(x: float) -> float
pub fn exp2(x: float) -> float
pub fn ln(x: float) -> float
pub fn log(x: float, base: float) -> float
pub fn log2(x: float) -> float
pub fn log10(x: float) -> float
```

### 7.6 Random Numbers

```li
pub class Random {
    pub static fn new() -> Random
    pub static fn with_seed(seed: int) -> Random

    /// Random int in range [0, max)
    pub fn int(max: int) -> int

    /// Random int in range [min, max)
    pub fn int_range(min: int, max: int) -> int

    /// Random float in range [0, 1)
    pub fn float() -> float

    /// Random float in range [min, max)
    pub fn float_range(min: float, max: float) -> float

    /// Random boolean
    pub fn bool() -> bool

    /// Random element from list
    pub fn choice<T>(items: List<T>) -> T

    /// Shuffle list in place
    pub fn shuffle<T>(items: &mut List<T>)
}

// Global random functions
pub fn random() -> float                      // [0, 1)
pub fn random_int(max: int) -> int           // [0, max)
pub fn random_range(min: int, max: int) -> int
```

---

## 8. std.time

### 8.1 Duration

```li
pub struct Duration {
    // Creation
    pub static fn from_secs(secs: int) -> Duration
    pub static fn from_millis(millis: int) -> Duration
    pub static fn from_micros(micros: int) -> Duration
    pub static fn from_nanos(nanos: int) -> Duration

    // Conversion
    pub fn as_secs() -> int
    pub fn as_millis() -> int
    pub fn as_micros() -> int
    pub fn as_nanos() -> int

    // Arithmetic
    pub fn add(other: Duration) -> Duration
    pub fn sub(other: Duration) -> Duration
    pub fn mul(factor: int) -> Duration
    pub fn div(factor: int) -> Duration
}

// Convenience constructors
pub fn seconds(n: int) -> Duration
pub fn millis(n: int) -> Duration
pub fn micros(n: int) -> Duration
```

### 8.2 Instant

```li
/// Point in time (monotonic)
pub struct Instant {
    pub static fn now() -> Instant

    pub fn elapsed() -> Duration
    pub fn duration_since(earlier: Instant) -> Duration
}

// Usage
let start = Instant.now()
do_work()
let elapsed = start.elapsed()
println("Took", elapsed.as_millis(), "ms")
```

### 8.3 DateTime

```li
/// Calendar date and time
pub struct DateTime {
    pub static fn now() -> DateTime
    pub static fn from_timestamp(unix_secs: int) -> DateTime
    pub static fn parse(s: string, format: string) -> Result<DateTime, ParseError>

    // Components
    pub fn year() -> int
    pub fn month() -> int        // 1-12
    pub fn day() -> int          // 1-31
    pub fn hour() -> int         // 0-23
    pub fn minute() -> int       // 0-59
    pub fn second() -> int       // 0-59
    pub fn weekday() -> Weekday  // Monday-Sunday

    // Conversion
    pub fn timestamp() -> int
    pub fn format(pattern: string) -> string

    // Arithmetic
    pub fn add_days(days: int) -> DateTime
    pub fn add_hours(hours: int) -> DateTime
    pub fn add_minutes(minutes: int) -> DateTime
    pub fn duration_since(earlier: DateTime) -> Duration
}

pub enum Weekday {
    Monday, Tuesday, Wednesday, Thursday, Friday, Saturday, Sunday
}

// Usage
let now = DateTime.now()
println(now.format("YYYY-MM-DD HH:mm:ss"))
```

### 8.4 Sleep and Timers

```li
/// Sleep current fiber
pub fn sleep(duration: Duration)

/// Sleep for seconds
pub fn sleep_secs(secs: int)

/// Sleep for milliseconds
pub fn sleep_millis(millis: int)

/// Timer that fires periodically
pub class Timer {
    pub static fn new(interval: Duration) -> Timer
    pub static fn after(delay: Duration) -> Timer

    pub fn tick() -> Channel<void>
    pub fn stop()
}
```

---

## 9. std.sync

### 9.1 Mutex

```li
/// Mutual exclusion lock
pub class Mutex<T> {
    pub static fn new(value: T) -> Mutex<T>

    /// Lock and get access
    pub fn lock() -> MutexGuard<T>

    /// Try to lock without blocking
    pub fn try_lock() -> Option<MutexGuard<T>>
}

pub class MutexGuard<T> {
    /// Access the value
    pub fn get() -> &T
    pub fn get_mut() -> &mut T
    // Automatically unlocks when dropped
}

// Usage
let counter = Mutex.new(0)

spawn {
    let guard = counter.lock()
    *guard.get_mut() += 1
}
```

### 9.2 RwLock

```li
/// Reader-writer lock
pub class RwLock<T> {
    pub static fn new(value: T) -> RwLock<T>

    /// Acquire read lock (multiple readers allowed)
    pub fn read() -> ReadGuard<T>

    /// Acquire write lock (exclusive)
    pub fn write() -> WriteGuard<T>
}
```

### 9.3 Channel

```li
/// Communication channel between fibers
pub class Channel<T> {
    /// Create unbuffered channel
    pub static fn new() -> Channel<T>

    /// Create buffered channel
    pub static fn buffered(capacity: int) -> Channel<T>

    /// Send value (blocks if full)
    pub fn send(value: T)

    /// Receive value (blocks if empty)
    pub fn receive() -> T

    /// Try to send without blocking
    pub fn try_send(value: T) -> bool

    /// Try to receive without blocking
    pub fn try_receive() -> Option<T>

    /// Close channel
    pub fn close()

    /// Check if closed
    pub fn is_closed() -> bool

    /// Iterate over values
    pub fn iter() -> Iterator<T>
}
```

### 9.4 Other Synchronization Primitives

```li
/// Semaphore
pub class Semaphore {
    pub static fn new(permits: int) -> Semaphore
    pub fn acquire()
    pub fn release()
    pub fn try_acquire() -> bool
}

/// Wait group
pub class WaitGroup {
    pub static fn new() -> WaitGroup
    pub fn add(count: int)
    pub fn done()
    pub fn wait()
}

/// One-time initialization
pub class Once {
    pub static fn new() -> Once
    pub fn call(f: fn())
}

/// Condition variable
pub class Condvar {
    pub static fn new() -> Condvar
    pub fn wait<T>(guard: MutexGuard<T>)
    pub fn notify_one()
    pub fn notify_all()
}
```

---

## 10. std.os

### 10.1 Environment

```li
/// Get environment variable
pub fn env(name: string) -> Option<string>

/// Set environment variable
pub fn set_env(name: string, value: string)

/// Remove environment variable
pub fn remove_env(name: string)

/// Get all environment variables
pub fn env_vars() -> Map<string, string>

/// Command line arguments
pub fn args() -> List<string>

/// Current working directory
pub fn current_dir() -> Result<string, IOError>

/// Change working directory
pub fn set_current_dir(path: string) -> Result<void, IOError>

/// Home directory
pub fn home_dir() -> Option<string>
```

### 10.2 Process Control

```li
/// Exit process
pub fn exit(code: int) -> never

/// Get process ID
pub fn pid() -> int

/// Spawn child process
pub fn spawn(command: string, args: List<string>) -> Result<Process, IOError>

pub class Process {
    pub fn stdin() -> Writer
    pub fn stdout() -> Reader
    pub fn stderr() -> Reader
    pub fn wait() -> Result<int, IOError>
    pub fn kill()
    pub fn is_running() -> bool
}

/// Run command and wait
pub fn run(command: string, args: List<string>) -> Result<Output, IOError>

pub struct Output {
    status: int,
    stdout: string,
    stderr: string,
}
```

### 10.3 System Information

```li
/// Operating system name
pub fn os_name() -> string   // "macos", "linux"

/// CPU architecture
pub fn arch() -> string      // "x86_64"

/// Number of CPUs
pub fn num_cpus() -> int

/// Total memory in bytes
pub fn total_memory() -> int

/// Available memory in bytes
pub fn available_memory() -> int
```

---

## 11. gui.core

### 11.1 Application

```li
/// GUI application
pub class App {
    pub static fn new(name: string) -> App

    /// Set application icon
    pub fn set_icon(path: string)

    /// Run the event loop
    pub fn run()

    /// Quit the application
    pub fn quit()

    /// Get shared app instance
    pub static fn current() -> App
}
```

### 11.2 Window

```li
/// Application window
pub class Window {
    pub static fn new(title: string, width: int, height: int) -> Window

    /// Load from Lira UI file
    pub static fn from_liui(path: string) -> Window

    // Properties
    pub fn title() -> string
    pub fn set_title(title: string)
    pub fn size() -> (int, int)
    pub fn set_size(width: int, height: int)
    pub fn position() -> (int, int)
    pub fn set_position(x: int, y: int)

    // State
    pub fn show()
    pub fn hide()
    pub fn close()
    pub fn is_visible() -> bool
    pub fn minimize()
    pub fn maximize()
    pub fn restore()

    // Content
    pub fn set_content(widget: Widget)

    // Events
    pub fn on_close(handler: fn())
    pub fn on_resize(handler: fn(int, int))
}
```

### 11.3 Events

```li
/// Mouse event
pub struct MouseEvent {
    x: int,
    y: int,
    button: MouseButton,
    modifiers: Modifiers,
}

pub enum MouseButton {
    Left, Right, Middle,
}

/// Keyboard event
pub struct KeyEvent {
    key: Key,
    modifiers: Modifiers,
    repeat: bool,
}

pub struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
}

/// Touch event
pub struct TouchEvent {
    id: int,
    x: int,
    y: int,
    phase: TouchPhase,
}

pub enum TouchPhase {
    Began, Moved, Ended, Cancelled,
}
```

---

## 12. gui.widgets

### 12.1 Widget Base

```li
/// Base widget interface
pub interface Widget {
    fn id() -> string
    fn set_id(id: string)
    fn visible() -> bool
    fn set_visible(visible: bool)
    fn enabled() -> bool
    fn set_enabled(enabled: bool)
    fn focus()
    fn blur()
}
```

### 12.2 Layout Widgets

```li
pub class VBox: Widget {
    pub static fn new() -> VBox
    pub fn spacing() -> int
    pub fn set_spacing(spacing: int)
    pub fn add_child(widget: Widget)
    pub fn remove_child(widget: Widget)
    pub fn children() -> List<Widget>
}

pub class HBox: Widget {
    // Same as VBox
}

pub class Grid: Widget {
    pub static fn new(columns: int) -> Grid
    pub fn set_columns(columns: int)
    pub fn set_row_gap(gap: int)
    pub fn set_column_gap(gap: int)
}

pub class ScrollView: Widget {
    pub fn scroll_to(x: int, y: int)
    pub fn scroll_position() -> (int, int)
}
```

### 12.3 Input Widgets

```li
pub class Button: Widget {
    pub static fn new(text: string) -> Button
    pub fn text() -> string
    pub fn set_text(text: string)
    pub fn on_click(handler: fn())
}

pub class TextField: Widget {
    pub static fn new() -> TextField
    pub fn text() -> string
    pub fn set_text(text: string)
    pub fn placeholder() -> string
    pub fn set_placeholder(placeholder: string)
    pub fn on_change(handler: fn(string))
    pub fn on_submit(handler: fn())
}

pub class Checkbox: Widget {
    pub static fn new(label: string) -> Checkbox
    pub fn checked() -> bool
    pub fn set_checked(checked: bool)
    pub fn on_change(handler: fn(bool))
}

pub class Slider: Widget {
    pub static fn new(min: float, max: float) -> Slider
    pub fn value() -> float
    pub fn set_value(value: float)
    pub fn on_change(handler: fn(float))
}
```

### 12.4 Display Widgets

```li
pub class Label: Widget {
    pub static fn new(text: string) -> Label
    pub fn text() -> string
    pub fn set_text(text: string)
    pub fn set_font_size(size: int)
    pub fn set_color(color: Color)
}

pub class Image: Widget {
    pub static fn new(path: string) -> Image
    pub static fn from_bytes(data: &[uint8]) -> Image
    pub fn set_source(path: string)
}

pub class ProgressBar: Widget {
    pub static fn new() -> ProgressBar
    pub fn value() -> float  // 0.0 to 1.0
    pub fn set_value(value: float)
    pub fn set_indeterminate(indeterminate: bool)
}
```

### 12.5 Color

```li
pub struct Color {
    r: uint8,
    g: uint8,
    b: uint8,
    a: uint8,

    pub static fn rgb(r: uint8, g: uint8, b: uint8) -> Color
    pub static fn rgba(r: uint8, g: uint8, b: uint8, a: uint8) -> Color
    pub static fn hex(hex: string) -> Color

    // Named colors
    pub static let WHITE: Color
    pub static let BLACK: Color
    pub static let RED: Color
    pub static let GREEN: Color
    pub static let BLUE: Color
    pub static let TRANSPARENT: Color
}
```

---

## Appendix: Import Reference

```li
// Core (prelude - auto-imported)
// Nothing needed

// Standard library modules
import std.io.{print, println, read_line}
import std.fs.{File, Path, read_dir}
import std.collections.{List, Map, Set, Deque}
import std.string.{format, join}
import std.math.{sin, cos, sqrt, PI}
import std.time.{Duration, Instant, DateTime, sleep}
import std.sync.{Mutex, Channel, WaitGroup}
import std.os.{env, spawn, exit}

// GUI modules
import gui.core.{App, Window}
import gui.widgets.{Button, Label, TextField, VBox, HBox}
import gui.canvas.{Canvas, Context2D}
import gui.events.{MouseEvent, KeyEvent}
```

---

_This document is part of the Lira Language Specification._

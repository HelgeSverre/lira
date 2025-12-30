# Lira Type System Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 02-type-system |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **Prerequisites** | 00-lira-overview.md, 01-lexical-structure.md |

---

## Table of Contents

1. [Type System Overview](#1-type-system-overview)
2. [Primitive Types](#2-primitive-types)
3. [Compound Types](#3-compound-types)
4. [Optional Types](#4-optional-types)
5. [User-Defined Types](#5-user-defined-types)
6. [Function Types](#6-function-types)
7. [Generics](#7-generics)
8. [Type Inference](#8-type-inference)
9. [Type Compatibility](#9-type-compatibility)
10. [Type Aliases](#10-type-aliases)

---

## 1. Type System Overview

### 1.1 Design Principles

Lira's type system is designed around these principles:

1. **Static Typing**: All types are checked at compile time
2. **Type Inference**: Types can be inferred from context
3. **Null Safety**: Null is only allowed in explicitly optional types
4. **Structural for Interfaces**: Interfaces use structural subtyping
5. **Nominal for Classes**: Classes use nominal subtyping

### 1.2 Type Categories

```
Type ::= PrimitiveType
       | CompoundType
       | OptionalType
       | UserDefinedType
       | FunctionType
       | TypeParameter
       | NeverType
       | AnyType
```

### 1.3 Type Syntax

```
TypeAnnotation ::= ':' Type
Type           ::= TypeName TypeArguments?
TypeArguments  ::= '<' Type (',' Type)* '>'
OptionalType   ::= Type '?'
FunctionType   ::= 'fn' '(' ParameterTypes? ')' '->' Type
```

---

## 2. Primitive Types

### 2.1 Boolean Type

The `bool` type represents truth values:

```li
let active: bool = true
let disabled: bool = false
```

| Property | Value |
|----------|-------|
| Size | 1 byte |
| Values | `true`, `false` |
| Default | `false` |

### 2.2 Integer Types

#### Signed Integers

| Type | Size | Range |
|------|------|-------|
| `int8` | 1 byte | -128 to 127 |
| `int16` | 2 bytes | -32,768 to 32,767 |
| `int32` | 4 bytes | -2^31 to 2^31-1 |
| `int64` | 8 bytes | -2^63 to 2^63-1 |
| `int` | 8 bytes | Alias for `int64` |

#### Unsigned Integers

| Type | Size | Range |
|------|------|-------|
| `uint8` | 1 byte | 0 to 255 |
| `uint16` | 2 bytes | 0 to 65,535 |
| `uint32` | 4 bytes | 0 to 2^32-1 |
| `uint64` | 8 bytes | 0 to 2^64-1 |
| `uint` | 8 bytes | Alias for `uint64` |

```li
let a: int = 42              // int64
let b: int8 = -128           // int8
let c: uint8 = 255           // uint8
let d: int32 = 1_000_000     // int32
```

#### Integer Operations

All integer types support:
- Arithmetic: `+`, `-`, `*`, `/`, `%`
- Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>`
- Comparison: `==`, `!=`, `<`, `>`, `<=`, `>=`

Overflow behavior:
- Debug builds: Panic on overflow
- Release builds: Wrap around (two's complement)

### 2.3 Floating-Point Types

| Type | Size | Precision |
|------|------|-----------|
| `float32` | 4 bytes | IEEE 754 single |
| `float64` | 8 bytes | IEEE 754 double |
| `float` | 8 bytes | Alias for `float64` |

```li
let pi: float = 3.14159
let precise: float64 = 2.718281828459045
let fast: float32 = 1.5f32
```

#### Special Values

```li
let inf = float.INFINITY
let neg_inf = float.NEG_INFINITY
let nan = float.NAN

// Checks
value.is_nan()
value.is_infinite()
value.is_finite()
```

### 2.4 Character Type

The `char` type represents a Unicode scalar value (code point):

```li
let ch: char = 'A'
let emoji: char = '\u{1F600}'
```

| Property | Value |
|----------|-------|
| Size | 4 bytes |
| Range | U+0000 to U+10FFFF (excluding surrogates) |

### 2.5 String Type

The `string` type represents an immutable UTF-8 encoded string:

```li
let s: string = "Hello, World!"
let empty: string = ""
```

| Property | Value |
|----------|-------|
| Encoding | UTF-8 |
| Mutability | Immutable |
| Null-terminated | No (length-prefixed) |

#### String Operations

```li
let s = "Hello"
s.length          // 5 (byte count)
s.char_count      // 5 (character count)
s.is_empty        // false
s[0]              // 'H' (by byte index, returns char)
s.chars()         // Iterator over characters
s.bytes()         // Iterator over bytes
s + " World"      // Concatenation
"${s}!"           // Interpolation
```

### 2.6 Void Type

The `void` type represents the absence of a value:

```li
fn print_message(msg: string) -> void {
    // No return value
}

// void is the default return type
fn do_something() {
    // Implicitly returns void
}
```

### 2.7 Never Type

The `never` type represents a type that can never be instantiated:

```li
fn panic(msg: string) -> never {
    // This function never returns
    sys_exit(1)
}

// Useful in match expressions
fn handle(value: Result<int, string>) -> int {
    match value {
        Ok(n) => n,
        Err(msg) => panic(msg),  // never coerces to int
    }
}
```

---

## 3. Compound Types

### 3.1 List Type

`List<T>` is a dynamic array of elements of type `T`:

```li
let numbers: List<int> = [1, 2, 3, 4, 5]
let empty: List<string> = []
let inferred = [1.0, 2.0, 3.0]  // List<float>
```

#### List Operations

```li
let mut list = [1, 2, 3]

// Access
list[0]                // 1
list.first             // 1?
list.last              // 3?
list.length            // 3

// Modification (requires mut)
list.push(4)           // [1, 2, 3, 4]
list.pop()             // 4, list is [1, 2, 3]
list.insert(0, 0)      // [0, 1, 2, 3]
list.remove(0)         // 0, list is [1, 2, 3]
list.clear()           // []

// Iteration
for item in list { }
for (i, item) in list.enumerate() { }

// Functional
list.map(|x| x * 2)
list.filter(|x| x > 0)
list.reduce(0, |a, b| a + b)

// Slicing
list[1..3]             // Elements 1 and 2
list[..2]              // First 2 elements
list[1..]              // All except first
```

### 3.2 Map Type

`Map<K, V>` is a hash map from keys of type `K` to values of type `V`:

```li
let ages: Map<string, int> = {
    "alice": 30,
    "bob": 25,
}

let empty: Map<int, string> = {}
```

#### Map Operations

```li
let mut map = { "a": 1, "b": 2 }

// Access
map["a"]               // 1?
map.get("a")           // 1?
map.get_or("c", 0)     // 0

// Modification
map["c"] = 3           // Insert or update
map.insert("d", 4)     // Returns old value if exists
map.remove("a")        // Returns removed value
map.clear()

// Queries
map.contains_key("a")  // bool
map.keys()             // Iterator<string>
map.values()           // Iterator<int>
map.entries()          // Iterator<(string, int)>
map.length             // int

// Iteration
for (key, value) in map { }
```

**Key Requirements**: The key type `K` must implement `Hash` and `Eq`.

### 3.3 Set Type

`Set<T>` is a collection of unique values:

```li
let unique: Set<int> = {1, 2, 3}
let empty: Set<string> = {}

// Note: Set<T> and Map<K,V> use the same {} syntax
// Disambiguation by context:
let s: Set<int> = {1, 2, 3}        // Set
let m: Map<int, int> = {1: 2}      // Map (has colons)
```

#### Set Operations

```li
let mut set = {1, 2, 3}

// Modification
set.add(4)             // true if new
set.remove(1)          // true if existed
set.clear()

// Queries
set.contains(2)        // bool
set.length             // int
set.is_empty           // bool

// Set operations
set.union(other)
set.intersection(other)
set.difference(other)
set.symmetric_difference(other)
set.is_subset(other)
set.is_superset(other)
```

### 3.4 Tuple Types

Tuples are fixed-size heterogeneous collections:

```li
let pair: (int, string) = (42, "answer")
let triple: (int, float, bool) = (1, 2.0, true)
let empty: () = ()  // Unit tuple
```

#### Tuple Operations

```li
let t = (1, "hello", true)

// Access by index
t.0                    // 1
t.1                    // "hello"
t.2                    // true

// Destructuring
let (a, b, c) = t
let (first, _, _) = t  // Ignore with _

// Nested tuples
let nested = ((1, 2), (3, 4))
nested.0.1             // 2
```

#### Named Tuples

```li
// Anonymous struct syntax (named tuple)
let point: (x: int, y: int) = (x: 10, y: 20)
point.x                // 10
point.y                // 20
```

### 3.5 Array Type (Fixed-Size)

`[T; N]` is a fixed-size array:

```li
let fixed: [int; 5] = [1, 2, 3, 4, 5]
let zeros: [int; 10] = [0; 10]  // 10 zeros
```

Arrays are value types (copied on assignment) and their size is part of the type.

---

## 4. Optional Types

### 4.1 Optional Type Syntax

`T?` is shorthand for `Optional<T>`:

```li
let maybe: int? = 42
let nothing: int? = null
```

### 4.2 Optional Operations

#### Checking for Value

```li
let opt: int? = get_value()

if opt != null {
    // opt is still int? here
    let value = opt!  // Force unwrap
}

// If-let unwrapping
if let value = opt {
    // value is int here
    use(value)
}
```

#### Unwrapping

```li
let opt: int? = 42

// Force unwrap (panics if null)
let a = opt!           // int (or panic)

// Optional chaining
let len = opt?.to_string().length  // int?

// Null coalescing
let b = opt ?? 0       // int (default if null)

// Elvis operator
let c = opt ?: compute_default()  // int
```

#### Pattern Matching

```li
match opt {
    Some(value) => use(value),
    None => use_default(),
}

// Or with if let
if let Some(v) = opt {
    use(v)
} else {
    use_default()
}
```

### 4.3 Optional Chaining

Optional chaining propagates null through member access:

```li
class User {
    address: Address?
}

class Address {
    city: string
}

let user: User? = get_user()

// Safe navigation
let city: string? = user?.address?.city

// Method calls
let upper: string? = user?.address?.city?.to_uppercase()
```

### 4.4 Null Safety

Lira enforces null safety:

```li
let s: string = "hello"  // Cannot be null
let n: string? = null    // Can be null

// s = null             // ERROR: Cannot assign null to non-optional
// let len = n.length   // ERROR: Cannot access member on optional

// Must unwrap first
if let value = n {
    let len = value.length  // OK
}
```

---

## 5. User-Defined Types

### 5.1 Class Types

Classes are reference types with identity:

```li
class Person {
    // Fields
    pub let name: string
    pub var age: int
    priv var _id: int

    // Constructor
    fn new(name: string, age: int) -> Person {
        return Person {
            name: name,
            age: age,
            _id: generate_id(),
        }
    }

    // Instance method
    pub fn greet(this) -> string {
        return "Hi, I'm ${this.name}"
    }

    // Mutable method
    pub fn birthday(this mut) {
        this.age += 1
    }

    // Static method
    pub fn default() -> Person {
        return Person.new("Unknown", 0)
    }
}
```

#### Class Semantics

- **Reference semantics**: Assignment copies the reference, not the object
- **Identity**: Two references can point to the same object
- **Mutability**: Fields can be `let` (immutable) or `var` (mutable)

```li
let p1 = Person.new("Alice", 30)
let p2 = p1              // p2 points to same object
p2.age = 31              // Modifies the shared object
print(p1.age)            // 31
```

### 5.2 Struct Types

Structs are value types with copy semantics:

```li
struct Point {
    pub x: float
    pub y: float

    fn new(x: float, y: float) -> Point {
        return Point { x: x, y: y }
    }

    pub fn distance_to(this, other: Point) -> float {
        let dx = this.x - other.x
        let dy = this.y - other.y
        return (dx * dx + dy * dy).sqrt()
    }
}
```

#### Struct Semantics

- **Value semantics**: Assignment copies the entire struct
- **No identity**: Each copy is independent
- **All fields initialized**: Every field must be assigned

```li
var p1 = Point { x: 0.0, y: 0.0 }
var p2 = p1              // p2 is a copy
p2.x = 10.0              // Does NOT affect p1
print(p1.x)              // 0.0
```

### 5.3 Enum Types

#### Simple Enums

```li
enum Color {
    Red,
    Green,
    Blue,
}

let c = Color.Red
```

#### Enums with Values

```li
enum Status {
    Active = 1,
    Inactive = 0,
    Pending = 2,
}

let s = Status.Active
let value = s as int     // 1
```

#### Enums with Associated Data

```li
enum Result<T, E> {
    Ok(T),
    Err(E),
}

enum Event {
    Click { x: int, y: int },
    KeyPress { key: char, modifiers: int },
    Quit,
}

// Usage
let result: Result<int, string> = Ok(42)
let event = Event.Click { x: 100, y: 200 }

// Pattern matching
match event {
    Event.Click { x, y } => handle_click(x, y),
    Event.KeyPress { key, .. } => handle_key(key),
    Event.Quit => exit(),
}
```

#### Enum Methods

```li
enum Direction {
    North,
    South,
    East,
    West,

    pub fn opposite(this) -> Direction {
        match this {
            Direction.North => Direction.South,
            Direction.South => Direction.North,
            Direction.East => Direction.West,
            Direction.West => Direction.East,
        }
    }

    pub fn to_vector(this) -> (int, int) {
        match this {
            Direction.North => (0, -1),
            Direction.South => (0, 1),
            Direction.East => (1, 0),
            Direction.West => (-1, 0),
        }
    }
}
```

### 5.4 Interface Types

Interfaces define contracts for behavior:

```li
interface Drawable {
    fn draw(this, ctx: GraphicsContext)
    fn bounds(this) -> Rect
}

interface Named {
    fn name(this) -> string
}

// Multiple interface implementation
class Button : Drawable, Named {
    let label: string
    let rect: Rect

    fn draw(this, ctx: GraphicsContext) {
        ctx.fill_rect(this.rect, Color.Gray)
        ctx.draw_text(this.label, this.rect.center())
    }

    fn bounds(this) -> Rect {
        return this.rect
    }

    fn name(this) -> string {
        return this.label
    }
}
```

#### Interface Default Methods

```li
interface Comparable {
    fn compare(this, other: Self) -> int

    // Default implementation
    fn less_than(this, other: Self) -> bool {
        return this.compare(other) < 0
    }

    fn greater_than(this, other: Self) -> bool {
        return this.compare(other) > 0
    }

    fn equals(this, other: Self) -> bool {
        return this.compare(other) == 0
    }
}
```

#### Structural Subtyping

Interfaces use structural subtyping - any type that implements the required methods is compatible:

```li
interface HasLength {
    fn length(this) -> int
}

// string implicitly implements HasLength
fn print_length(item: HasLength) {
    print("Length: ${item.length()}")
}

print_length("hello")     // Works: string has length()
print_length([1, 2, 3])   // Works: List has length()
```

### 5.5 Traits

Traits are nominally-typed contracts, unlike interfaces which use structural subtyping. A type must explicitly implement a trait using an `impl` block.

#### Trait Declaration

```li
trait Eq {
    /// Required method - must be implemented
    fn eq(self, other: Self) -> bool

    /// Provided method - has default implementation
    fn ne(self, other: Self) -> bool {
        return !self.eq(other)
    }
}

trait Hash {
    fn hash(self) -> int
}

trait Clone {
    fn clone(self) -> Self
}

/// Trait with associated type
trait Iterator {
    type Item

    fn next(self mut) -> Self.Item?
    fn has_next(self) -> bool
}

/// Trait with generic parameter
trait Into<T> {
    fn into(self) -> T
}

/// Trait inheritance (supertrait)
trait Ord: Eq {
    fn cmp(self, other: Self) -> Ordering

    fn lt(self, other: Self) -> bool {
        return self.cmp(other) == Ordering.Less
    }

    fn gt(self, other: Self) -> bool {
        return self.cmp(other) == Ordering.Greater
    }
}
```

#### Trait vs Interface

| Aspect | Interface | Trait |
|--------|-----------|-------|
| Typing | Structural (duck typing) | Nominal (explicit) |
| Implementation | Implicit if methods match | Explicit `impl` required |
| Use case | Flexibility, interop | Type safety, coherence |
| Keyword | `interface` | `trait` |

```li
// Interface: structural subtyping
interface HasLength {
    fn length(self) -> int
}

fn print_len(x: HasLength) { print(x.length()) }
print_len("hello")  // Works: string has length()

// Trait: nominal subtyping
trait Serializable {
    fn serialize(self) -> string
}

fn save(x: Serializable) { write_file(x.serialize()) }
// save("hello")  // ERROR: string doesn't impl Serializable
```

### 5.6 Impl Blocks

Impl blocks add methods to types. There are two kinds:

1. **Inherent impl** - adds methods directly to a type
2. **Trait impl** - implements a trait for a type

#### Inherent Impl (Extension Methods)

Add methods to any type, including built-in types:

```li
impl string {
    /// Get string length
    fn len(self) -> int {
        return __builtin_string_len(self)
    }

    /// Check if empty
    fn is_empty(self) -> bool {
        return self.len() == 0
    }

    /// Convert to uppercase
    fn to_uppercase(self) -> string {
        return __builtin_string_upper(self)
    }

    /// Split by delimiter
    fn split(self, delimiter: string) -> List<string> {
        return __builtin_string_split(self, delimiter)
    }

    /// Trim whitespace
    fn trim(self) -> string {
        return __builtin_string_trim(self)
    }
}

// Usage - methods are called on the type
let s = "  hello world  "
print(s.trim())              // "hello world"
print(s.to_uppercase())      // "  HELLO WORLD  "
print(s.split(" "))          // ["", "", "hello", "world", "", ""]
print("".is_empty())         // true
```

#### Generic Inherent Impl

```li
impl<T> List<T> {
    fn is_empty(self) -> bool {
        return self.len() == 0
    }

    fn first(self) -> T? {
        return if self.len() > 0 { self[0] } else { null }
    }

    fn last(self) -> T? {
        let n = self.len()
        return if n > 0 { self[n - 1] } else { null }
    }

    fn map<U>(self, f: fn(T) -> U) -> List<U> {
        var result: List<U> = []
        for item in self {
            result.push(f(item))
        }
        return result
    }

    fn filter(self, predicate: fn(T) -> bool) -> List<T> {
        var result: List<T> = []
        for item in self {
            if predicate(item) {
                result.push(item)
            }
        }
        return result
    }
}

impl<K, V> Map<K, V> {
    fn is_empty(self) -> bool {
        return self.len() == 0
    }

    fn get_or(self, key: K, default: V) -> V {
        return self[key] ?? default
    }
}
```

#### Trait Impl

Implement a trait for a type:

```li
impl Eq for int {
    fn eq(self, other: int) -> bool {
        return __builtin_int_eq(self, other)
    }
}

impl Eq for string {
    fn eq(self, other: string) -> bool {
        return __builtin_string_eq(self, other)
    }
}

impl Hash for string {
    fn hash(self) -> int {
        return __builtin_string_hash(self)
    }
}

impl Clone for string {
    fn clone(self) -> string {
        return self  // Strings are immutable, can share
    }
}
```

#### Generic Trait Impl

```li
// Implement Eq for all Lists where element type is Eq
impl<T: Eq> Eq for List<T> {
    fn eq(self, other: List<T>) -> bool {
        if self.len() != other.len() {
            return false
        }
        for i in 0..self.len() {
            if !self[i].eq(other[i]) {
                return false
            }
        }
        return true
    }
}

// Implement Clone for Option if T is Clone
impl<T: Clone> Clone for Option<T> {
    fn clone(self) -> Option<T> {
        match self {
            Some(value) => Some(value.clone()),
            None => None,
        }
    }
}
```

#### Self Type

In impl blocks, `Self` refers to the implementing type:

```li
impl Point {
    fn new(x: float, y: float) -> Self {
        return Point { x: x, y: y }
    }

    fn origin() -> Self {
        return Self.new(0.0, 0.0)
    }

    fn clone(self) -> Self {
        return Self { x: self.x, y: self.y }
    }
}
```

#### Method Receiver Syntax

Methods can have different receiver types:

```li
impl Counter {
    // Immutable borrow - cannot modify self
    fn get(self) -> int {
        return self.value
    }

    // Mutable borrow - can modify self
    fn increment(self mut) {
        self.value += 1
    }

    // Take ownership (move) - consumes self
    fn into_value(self owned) -> int {
        return self.value
    }

    // No receiver - static/associated function
    fn new() -> Counter {
        return Counter { value: 0 }
    }
}

var c = Counter.new()   // Static method
print(c.get())          // Immutable method
c.increment()           // Mutable method
let v = c.into_value()  // Consuming method (c is invalid after)
```

| Receiver | Syntax | Description |
|----------|--------|-------------|
| `self` | Immutable borrow | Read-only access |
| `self mut` | Mutable borrow | Can modify |
| `self owned` | Ownership | Consumes value |
| (none) | Static | No instance needed |

#### Impl Block Constraints

```li
// Only implement for types where T: Debug
impl<T: Debug> List<T> {
    fn debug_print(self) {
        print("[")
        for (i, item) in self.enumerate() {
            if i > 0 { print(", ") }
            print(item.debug_string())
        }
        print("]")
    }
}

// Multiple constraints
impl<K: Hash + Eq, V> Map<K, V> {
    fn merge(self, other: Map<K, V>) -> Map<K, V> {
        // ...
    }
}
```

#### Coherence Rules

To prevent conflicting implementations:

1. **Orphan Rule**: You can only implement a trait for a type if either the trait or the type is defined in the current module
2. **Overlap Rule**: Two impl blocks cannot apply to the same type for the same trait

```li
// In module my_app:

// OK: implementing foreign trait for local type
impl Eq for MyType { ... }

// OK: implementing local trait for foreign type
impl MyTrait for string { ... }

// ERROR: implementing foreign trait for foreign type
// impl Eq for string { ... }  // Not allowed
```

---

## 6. Function Types

### 6.1 Function Type Syntax

```li
// Function type: fn(parameters) -> return_type
type IntToString = fn(int) -> string
type BinaryOp = fn(int, int) -> int
type Callback = fn() -> void
type Predicate<T> = fn(T) -> bool
```

### 6.2 Function Values

```li
// Function reference
fn add(a: int, b: int) -> int {
    return a + b
}

let op: fn(int, int) -> int = add
let result = op(2, 3)  // 5

// Lambda expression
let double = |x: int| -> int { x * 2 }
let square: fn(int) -> int = |x| { x * x }

// Shorthand lambda (single expression)
let cube = |x: int| x * x * x
```

### 6.3 Closures

Lambdas can capture variables from their enclosing scope:

```li
fn make_adder(n: int) -> fn(int) -> int {
    // Captures 'n' from enclosing scope
    return |x| { x + n }
}

let add5 = make_adder(5)
print(add5(10))  // 15
```

#### Capture Modes

```li
var counter = 0

// Capture by reference (default for var)
let increment = || { counter += 1 }
increment()
print(counter)  // 1

// Explicit copy capture
let snapshot = [counter] || { print(counter) }
counter = 100
snapshot()  // Prints 1 (captured value)
```

### 6.4 Higher-Order Functions

```li
fn apply<T, R>(value: T, f: fn(T) -> R) -> R {
    return f(value)
}

fn compose<A, B, C>(f: fn(B) -> C, g: fn(A) -> B) -> fn(A) -> C {
    return |x| { f(g(x)) }
}

// Usage
let result = apply(5, |x| x * 2)  // 10

let add1 = |x: int| x + 1
let double = |x: int| x * 2
let add1_then_double = compose(double, add1)
print(add1_then_double(3))  // 8
```

---

## 7. Generics

### 7.1 Generic Functions

```li
fn identity<T>(value: T) -> T {
    return value
}

fn swap<T>(a: T, b: T) -> (T, T) {
    return (b, a)
}

fn first<T>(items: List<T>) -> T? {
    return if items.length > 0 { items[0] } else { null }
}

// Usage (type inferred)
let x = identity(42)        // T = int
let (b, a) = swap(1, 2)     // T = int
let f = first([1, 2, 3])    // T = int
```

### 7.2 Generic Types

```li
struct Pair<T, U> {
    first: T
    second: U

    fn new(first: T, second: U) -> Pair<T, U> {
        return Pair { first: first, second: second }
    }

    fn swap(this) -> Pair<U, T> {
        return Pair { first: this.second, second: this.first }
    }
}

class Stack<T> {
    priv var items: List<T> = []

    pub fn push(this mut, item: T) {
        this.items.push(item)
    }

    pub fn pop(this mut) -> T? {
        return this.items.pop()
    }

    pub fn is_empty(this) -> bool {
        return this.items.length == 0
    }
}
```

### 7.3 Generic Constraints

Constrain type parameters with interface bounds:

```li
// Single constraint
fn print_all<T: ToString>(items: List<T>) {
    for item in items {
        print(item.to_string())
    }
}

// Multiple constraints
fn compare_and_print<T: Comparable + ToString>(a: T, b: T) {
    if a.less_than(b) {
        print("${a} < ${b}")
    }
}

// Where clause for complex constraints
fn process<K, V>(map: Map<K, V>) -> List<V>
    where K: Hash + Eq,
          V: Clone
{
    // ...
}
```

### 7.4 Associated Types

```li
interface Iterator {
    type Item

    fn next(this mut) -> Self.Item?
    fn has_next(this) -> bool
}

class RangeIterator : Iterator {
    type Item = int

    var current: int
    let end: int

    fn next(this mut) -> int? {
        if this.current < this.end {
            let value = this.current
            this.current += 1
            return value
        }
        return null
    }

    fn has_next(this) -> bool {
        return this.current < this.end
    }
}
```

### 7.5 Variance

Lira uses declaration-site variance annotations:

```li
// Covariant: can use subtype where supertype expected
interface Producer<out T> {
    fn produce(this) -> T
}

// Contravariant: can use supertype where subtype expected
interface Consumer<in T> {
    fn consume(this, value: T)
}

// Invariant: must be exact type (default)
class Container<T> {
    var value: T
}
```

---

## 8. Type Inference

### 8.1 Local Variable Inference

```li
// Type inferred from initializer
let x = 42              // int
let y = 3.14            // float
let z = "hello"         // string
let items = [1, 2, 3]   // List<int>
let empty = []          // ERROR: Cannot infer element type

// Explicit when needed
let empty: List<int> = []
let zero: float = 0     // Without annotation, would be int
```

### 8.2 Return Type Inference

```li
// Return type inferred from body
fn add(a: int, b: int) {
    return a + b        // Returns int
}

// Explicit return type required for:
// - Recursive functions
// - Public API functions (recommended)
// - Ambiguous cases

fn factorial(n: int) -> int {  // Required: recursive
    if n <= 1 { return 1 }
    return n * factorial(n - 1)
}
```

### 8.3 Generic Type Inference

```li
fn identity<T>(value: T) -> T { return value }

// T inferred from argument
let a = identity(42)        // T = int
let b = identity("hello")   // T = string

// T inferred from expected type
let c: float = identity(0)  // T = float

// Explicit type arguments when needed
let d = identity<string>(get_value())
```

### 8.4 Lambda Type Inference

```li
// Parameter types inferred from context
let numbers = [1, 2, 3]
let doubled = numbers.map(|x| { x * 2 })  // x: int inferred

// Return type inferred from body
let to_string = |n: int| { "${n}" }  // Returns string

// Full inference from higher-order function
fn apply<T, R>(value: T, f: fn(T) -> R) -> R { return f(value) }
apply(5, |x| { x * 2 })  // T = int, R = int inferred
```

### 8.5 Bidirectional Type Inference

Lira uses bidirectional type inference:

```li
// Forward inference (from initializer)
let x = 42

// Backward inference (from expected type)
let y: List<int> = []

// Combined inference
let items: List<_> = [1, 2, 3]  // _ = int inferred

// Flow-sensitive typing
let maybe: int? = get_value()
if maybe != null {
    // maybe is still int? here, but known non-null
    let value = maybe!  // Safe to unwrap
}
```

---

## 9. Type Compatibility

### 9.1 Subtyping Rules

#### Class Subtyping (Nominal)

```li
class Animal { }
class Dog : Animal { }
class Cat : Animal { }

let animal: Animal = Dog.new()  // OK: Dog is subtype of Animal
// let dog: Dog = Animal.new()  // ERROR: Animal is not subtype of Dog
```

#### Interface Subtyping (Structural)

```li
interface Printable {
    fn print(this)
}

class Document {
    fn print(this) { /* ... */ }
}

// Document is compatible with Printable (structural match)
let p: Printable = Document.new()
```

### 9.2 Type Coercion

Lira has minimal implicit coercion:

#### Numeric Widening

```li
let i: int8 = 42
let j: int16 = i   // OK: widening
let k: int32 = j   // OK: widening
let l: int64 = k   // OK: widening

// let m: int8 = l // ERROR: narrowing requires cast
let m: int8 = l as int8  // OK: explicit cast
```

#### Optional Promotion

```li
let x: int = 42
let y: int? = x    // OK: non-optional to optional

fn accept(value: int?) { }
accept(42)         // OK: int promoted to int?
```

#### Never Coercion

```li
fn panic(msg: string) -> never { sys_exit(1) }

fn get_value(opt: int?) -> int {
    return opt ?? panic("No value")  // never coerces to int
}
```

### 9.3 Type Casting

```li
// Safe casts (as)
let i: int64 = 1000
let j = i as int32      // May truncate

let f: float = 3.14
let k = f as int        // Truncates to 3

let obj: Animal = Dog.new()
let dog = obj as Dog?   // Returns Dog? (null if wrong type)

// Type assertions (is)
if obj is Dog {
    // obj can be used as Dog here
    obj.bark()
}
```

---

## 10. Type Aliases

### 10.1 Simple Aliases

```li
type UserId = int
type Username = string
type Callback = fn() -> void
```

### 10.2 Generic Aliases

```li
type Result<T> = Result<T, Error>
type StringMap<V> = Map<string, V>
type Pair<T> = (T, T)
```

### 10.3 Alias Transparency

Type aliases are transparent - the underlying type is fully compatible:

```li
type UserId = int

let id: UserId = 42
let n: int = id        // OK: same underlying type
let m: UserId = 100    // OK: int is compatible with UserId
```

### 10.4 Newtype Pattern

For type-safe wrappers, use a struct instead:

```li
struct UserId {
    value: int
}

struct OrderId {
    value: int
}

let user = UserId { value: 42 }
let order = OrderId { value: 42 }

// These are different types, cannot be mixed
fn get_user(id: UserId) -> User { /* ... */ }
// get_user(order)  // ERROR: OrderId != UserId
```

---

## Appendix A: Built-in Type Hierarchy

```
any (top type)
├── bool
├── int (int8, int16, int32, int64)
├── uint (uint8, uint16, uint32, uint64)
├── float (float32, float64)
├── char
├── string
├── void
├── List<T>
├── Map<K, V>
├── Set<T>
├── (T, U, ...) (tuples)
├── fn(...) -> R (functions)
├── T? (optionals)
├── class types
├── struct types
├── enum types
└── interface types

never (bottom type)
```

---

## Appendix B: Type Size and Alignment

| Type | Size | Alignment |
|------|------|-----------|
| `bool` | 1 byte | 1 byte |
| `int8`, `uint8` | 1 byte | 1 byte |
| `int16`, `uint16` | 2 bytes | 2 bytes |
| `int32`, `uint32`, `float32` | 4 bytes | 4 bytes |
| `int64`, `uint64`, `float64` | 8 bytes | 8 bytes |
| `char` | 4 bytes | 4 bytes |
| `string` | 16 bytes (reference) | 8 bytes |
| `List<T>` | 24 bytes (reference) | 8 bytes |
| `Map<K,V>` | 24 bytes (reference) | 8 bytes |
| Class reference | 8 bytes | 8 bytes |
| Struct | sum of fields | max field alignment |

---

*This document is part of the Lira Language Specification.*

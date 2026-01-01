# Lira Syntax Constructs Specification

## Document Information

| Property          | Value                            |
| ----------------- | -------------------------------- |
| **Document ID**   | 03-syntax-constructs             |
| **Version**       | 1.0.0-draft                      |
| **Status**        | Draft Specification              |
| **Prerequisites** | 00-02 (overview, lexical, types) |

---

## Table of Contents

1. [Program Structure](#1-program-structure)
2. [Declarations](#2-declarations)
3. [Statements](#3-statements)
4. [Expressions](#4-expressions)
5. [Control Flow](#5-control-flow)
6. [Pattern Matching](#6-pattern-matching)
7. [Classes and Inheritance](#7-classes-and-inheritance)
8. [Error Handling](#8-error-handling)

---

## 1. Program Structure

### 1.1 Source File Structure

A Lira source file consists of:

```
SourceFile ::= ModuleDeclaration? ImportDeclaration* Declaration*
```

```li
// Optional module declaration
mod my_module

// Imports
import std.io
import std.collections.{List, Map}

// Declarations
const VERSION = "1.0.0"

fn main() {
    print("Hello!")
}
```

### 1.2 Entry Point

The `main` function is the entry point for executable programs:

```li
// No arguments
fn main() {
    // Program starts here
}

// With command-line arguments
fn main(args: List<string>) {
    for arg in args {
        print(arg)
    }
}

// With exit code
fn main() -> int {
    return 0  // Success
}
```

---

## 2. Declarations

### 2.1 Variable Declarations

#### Immutable Binding (let)

```li
let name = "Alice"           // Immutable, type inferred
let age: int = 30            // Explicit type
let (x, y) = get_point()     // Destructuring
```

#### Mutable Binding (var)

```li
var counter = 0              // Mutable
counter += 1                 // OK

var items: List<int> = []    // Mutable list reference
items.push(1)                // OK: modifying list contents
items = [1, 2, 3]            // OK: reassigning reference
```

#### Constant Declaration (const)

```li
const PI = 3.14159           // Compile-time constant
const MAX_SIZE: int = 1024   // Explicit type
const NAME = "Lira"       // String constant

// Must be compile-time evaluable
const DOUBLE_PI = PI * 2     // OK: computed at compile time
// const NOW = get_time()    // ERROR: not compile-time
```

### 2.2 Function Declarations

```
FunctionDecl ::= Modifiers? 'fn' Identifier GenericParams?
                 '(' Parameters? ')' ('->' Type)? Block

Parameters   ::= Parameter (',' Parameter)* ','?
Parameter    ::= Identifier ':' Type ('=' Expression)?
```

#### Basic Functions

```li
fn greet(name: string) -> string {
    return "Hello, ${name}!"
}

fn add(a: int, b: int) -> int {
    return a + b
}

// Void return (implicit)
fn log(message: string) {
    print("[LOG] ${message}")
}
```

#### Default Parameters

```li
fn create_user(
    name: string,
    age: int = 0,
    active: bool = true,
) -> User {
    return User { name, age, active }
}

// Call with defaults
let user = create_user("Alice")
let user2 = create_user("Bob", 25)
let user3 = create_user("Charlie", active: false)
```

#### Named Arguments

```li
fn create_window(
    title: string,
    width: int,
    height: int,
    resizable: bool = true,
) -> Window {
    // ...
}

// Named arguments at call site
let window = create_window(
    title: "My App",
    width: 800,
    height: 600,
    resizable: false,
)
```

#### Variadic Functions

```li
fn sum(numbers: ...int) -> int {
    var total = 0
    for n in numbers {
        total += n
    }
    return total
}

sum(1, 2, 3)           // 6
sum(1, 2, 3, 4, 5)     // 15

// Spread operator
let nums = [1, 2, 3]
sum(...nums)           // 6
```

#### Expression Body

```li
// Short form for single-expression functions
fn square(x: int) -> int => x * x
fn is_even(n: int) -> bool => n % 2 == 0
fn greet(name: string) -> string => "Hello, ${name}!"
```

#### Generic Functions

```li
fn identity<T>(value: T) -> T {
    return value
}

fn pair<T, U>(first: T, second: U) -> (T, U) {
    return (first, second)
}

fn find<T: Eq>(items: List<T>, target: T) -> int? {
    for (i, item) in items.enumerate() {
        if item == target {
            return i
        }
    }
    return null
}
```

### 2.3 Type Declarations

#### Class Declaration

```li
class ClassName : SuperClass, Interface1, Interface2 {
    // Fields
    pub let field1: Type1
    priv var field2: Type2

    // Constructor
    fn new(params) -> ClassName { }

    // Methods
    pub fn method(this) -> ReturnType { }
}
```

#### Struct Declaration

```li
struct Point {
    x: float
    y: float

    fn new(x: float, y: float) -> Point {
        return Point { x, y }
    }
}
```

#### Enum Declaration

```li
enum Result<T, E> {
    Ok(T),
    Err(E),
}

enum Direction {
    North,
    South,
    East,
    West,
}
```

#### Interface Declaration

```li
interface Drawable {
    fn draw(this, ctx: GraphicsContext)
    fn bounds(this) -> Rect
}
```

#### Type Alias

```li
type StringList = List<string>
type Callback = fn(int) -> void
type Result<T> = Result<T, Error>
```

---

## 3. Statements

### 3.1 Expression Statements

Any expression followed by a semicolon (or newline) is a statement:

```li
print("Hello")           // Function call
x + 1                    // Expression (value discarded)
object.method()          // Method call
```

### 3.2 Block Statements

Blocks group statements and create a new scope:

```li
{
    let x = 1
    let y = 2
    print(x + y)
}
// x and y not visible here
```

Blocks are expressions that return their last value:

```li
let result = {
    let a = 10
    let b = 20
    a + b           // Block evaluates to 30
}
```

### 3.3 Assignment Statements

```li
var x = 0
x = 10               // Simple assignment
x += 5               // Compound assignment

// Multiple assignment
var a = 0
var b = 0
(a, b) = (1, 2)      // Tuple destructuring

// Chained assignment not allowed
// a = b = c = 0     // ERROR
```

### 3.4 Return Statements

```li
fn example() -> int {
    return 42
}

fn early_return(x: int) -> int {
    if x < 0 {
        return 0     // Early return
    }
    return x * 2
}

// Implicit return (last expression)
fn add(a: int, b: int) -> int {
    a + b            // Implicit return
}
```

### 3.5 Break and Continue

```li
// Break exits the loop
for i in 0..10 {
    if i == 5 {
        break
    }
    print(i)
}

// Continue skips to next iteration
for i in 0..10 {
    if i % 2 == 0 {
        continue
    }
    print(i)    // Only odd numbers
}

// Break with label
outer: for x in 0..10 {
    for y in 0..10 {
        if x * y > 50 {
            break outer
        }
    }
}

// Break with value (in loop expressions)
let result = loop {
    let value = try_get()
    if value > 0 {
        break value
    }
}
```

---

## 4. Expressions

### 4.1 Literal Expressions

```li
42                   // Integer
3.14                 // Float
true                 // Boolean
"hello"              // String
'c'                  // Character
null                 // Null
[1, 2, 3]            // List
{"a": 1}             // Map
(1, "two")           // Tuple
```

### 4.2 Identifier Expressions

```li
variable             // Variable reference
Type.method          // Static method
this.field           // Instance field
super.method()       // Parent method
```

### 4.3 Operator Expressions

#### Arithmetic

```li
a + b                // Addition
a - b                // Subtraction
a * b                // Multiplication
a / b                // Division
a % b                // Remainder
a ** b               // Exponentiation
-a                   // Negation
```

#### Comparison

```li
a == b               // Equal
a != b               // Not equal
a < b                // Less than
a <= b               // Less or equal
a > b                // Greater than
a >= b               // Greater or equal
```

#### Logical

```li
a && b               // Logical AND (short-circuit)
a || b               // Logical OR (short-circuit)
!a                   // Logical NOT
```

#### Bitwise

```li
a & b                // Bitwise AND
a | b                // Bitwise OR
a ^ b                // Bitwise XOR
~a                   // Bitwise NOT
a << n               // Left shift
a >> n               // Right shift (arithmetic)
a >>> n              // Right shift (logical)
```

### 4.4 Call Expressions

```li
// Function call
print("Hello")
add(1, 2)

// Method call
object.method()
list.push(item)

// Chained calls
text.trim().to_lowercase().split(" ")

// Named arguments
create_window(title: "App", width: 800)
```

### 4.5 Index Expressions

```li
list[0]              // List indexing
map["key"]           // Map indexing
array[i][j]          // Multi-dimensional
text[0..5]           // Slice
```

### 4.6 Member Access

```li
object.field         // Field access
Type.static_method   // Static method
module.function      // Module member

// Optional chaining
object?.field        // Returns null if object is null
a?.b?.c              // Chain of optional access
```

### 4.7 Conditional Expressions

#### If Expression

```li
let max = if a > b { a } else { b }

let description = if score >= 90 {
    "Excellent"
} else if score >= 70 {
    "Good"
} else {
    "Needs improvement"
}
```

#### Match Expression

```li
let name = match color {
    Color.Red => "red",
    Color.Green => "green",
    Color.Blue => "blue",
}
```

### 4.8 Object Creation

```li
// Class instantiation
let person = Person.new("Alice", 30)
let person2 = Person {
    name: "Bob",
    age: 25,
}

// Struct instantiation
let point = Point { x: 10.0, y: 20.0 }
let point2 = Point.new(10.0, 20.0)

// With field shorthand
let x = 10.0
let y = 20.0
let point3 = Point { x, y }  // Same as { x: x, y: y }
```

### 4.9 Lambda Expressions

```li
// Full syntax
let add = |a: int, b: int| -> int { return a + b }

// With inference
let double = |x| { x * 2 }

// Expression body
let square = |x: int| x * x

// No parameters
let say_hi = || { print("Hi!") }

// Capturing variables
var count = 0
let increment = || { count += 1 }
```

### 4.10 Range Expressions

```li
0..10                // 0 to 9 (exclusive)
0..=10               // 0 to 10 (inclusive)
..10                 // 0 to 9
10..                 // 10 to infinity
..                   // Full range

// With step
(0..10).step_by(2)   // 0, 2, 4, 6, 8

// In for loops
for i in 0..5 {
    print(i)
}
```

### 4.11 Type Cast Expressions

```li
// Safe cast
let i = f as int           // float to int

// Nullable cast
let dog = animal as Dog?   // Returns null if not Dog

// Type check
if animal is Dog {
    animal.bark()          // Safe to use as Dog
}
```

---

## 5. Control Flow

### 5.1 If Statement

```li
if condition {
    // then branch
}

if condition {
    // then branch
} else {
    // else branch
}

if condition1 {
    // branch 1
} else if condition2 {
    // branch 2
} else {
    // default branch
}
```

### 5.2 If Let (Optional Unwrapping)

```li
if let value = optional {
    // value is unwrapped here
    use(value)
}

if let Some(x) = result {
    // Pattern matched
} else {
    // No match
}

// With additional condition
if let value = optional, value > 0 {
    // value exists and is positive
}
```

### 5.3 Match Statement

```li
match value {
    pattern1 => expression1,
    pattern2 => expression2,
    pattern3 => {
        // Block for multiple statements
        statement1
        statement2
    },
    _ => default_expression,
}
```

#### Match with Guards

```li
match number {
    n if n < 0 => "negative",
    0 => "zero",
    n if n < 10 => "small positive",
    n if n < 100 => "medium",
    _ => "large",
}
```

#### Exhaustiveness

Match expressions must be exhaustive:

```li
enum Option<T> {
    Some(T),
    None,
}

match opt {
    Some(v) => use(v),
    None => default(),
}
// All cases covered

match opt {
    Some(v) => use(v),
}
// ERROR: Non-exhaustive, missing None case
```

### 5.4 While Loop

```li
while condition {
    // body
}

// With break
while true {
    if should_stop {
        break
    }
}

// While let
while let Some(item) = iterator.next() {
    process(item)
}
```

### 5.5 Loop (Infinite)

```li
loop {
    // Runs forever until break
    if done {
        break
    }
}

// Loop as expression
let result = loop {
    let value = try_operation()
    if value.is_ok() {
        break value.unwrap()
    }
}
```

### 5.6 For Loop

```li
// Iterate over collection
for item in collection {
    process(item)
}

// With index
for (index, item) in collection.enumerate() {
    print("${index}: ${item}")
}

// Range-based
for i in 0..10 {
    print(i)
}

// Reverse
for i in (0..10).rev() {
    print(i)
}

// With step
for i in (0..20).step_by(2) {
    print(i)  // 0, 2, 4, ...
}
```

### 5.7 For-In with Destructuring

```li
// Map iteration
for (key, value) in map {
    print("${key}: ${value}")
}

// Tuple list
let points = [(0, 0), (1, 1), (2, 4)]
for (x, y) in points {
    print("(${x}, ${y})")
}
```

---

## 6. Pattern Matching

### 6.1 Pattern Types

#### Literal Patterns

```li
match value {
    0 => "zero",
    1 => "one",
    "hello" => "greeting",
    true => "yes",
    _ => "other",
}
```

#### Binding Patterns

```li
match value {
    x => print(x),     // Binds value to x
}

// With type annotation
match value {
    x: int => print(x),
}
```

#### Wildcard Pattern

```li
match tuple {
    (x, _) => x,       // Ignore second element
    (_, _, z) => z,    // Ignore first two
}
```

#### Tuple Patterns

```li
match pair {
    (0, 0) => "origin",
    (x, 0) => "on x-axis",
    (0, y) => "on y-axis",
    (x, y) => "at (${x}, ${y})",
}
```

#### Struct Patterns

```li
match point {
    Point { x: 0, y: 0 } => "origin",
    Point { x, y: 0 } => "on x-axis at ${x}",
    Point { x: 0, y } => "on y-axis at ${y}",
    Point { x, y } => "at (${x}, ${y})",
}

// Shorthand
match point {
    Point { x, y } if x == y => "on diagonal",
    Point { x, .. } => "x is ${x}",  // Ignore other fields
}
```

#### Enum Patterns

```li
match result {
    Ok(value) => use(value),
    Err(error) => handle(error),
}

match event {
    Event.Click { x, y } => handle_click(x, y),
    Event.KeyPress { key, modifiers: 0 } => handle_key(key),
    Event.KeyPress { key, modifiers } => handle_modified_key(key, modifiers),
    Event.Quit => exit(),
}
```

#### Range Patterns

```li
match char {
    'a'..='z' => "lowercase",
    'A'..='Z' => "uppercase",
    '0'..='9' => "digit",
    _ => "other",
}

match score {
    90..=100 => "A",
    80..=89 => "B",
    70..=79 => "C",
    60..=69 => "D",
    _ => "F",
}
```

#### Or Patterns

```li
match value {
    0 | 1 => "binary digit",
    2 | 4 | 8 | 16 => "power of 2",
    _ => "other",
}
```

### 6.2 Pattern Guards

```li
match point {
    Point { x, y } if x == y => "on diagonal",
    Point { x, y } if x > 0 && y > 0 => "first quadrant",
    Point { x, y } if x < 0 && y > 0 => "second quadrant",
    _ => "other",
}
```

### 6.3 Let Patterns

```li
// Destructuring in let
let (x, y) = get_point()
let Point { x, y } = point
let [first, second, ...rest] = items

// With refutability
let Some(value) = optional else {
    // Handle None case
    return
}
```

### 6.4 @ Bindings

```li
match value {
    // Bind the matched value while also matching pattern
    n @ 1..=10 => print("small number: ${n}"),
    n @ 11..=100 => print("medium number: ${n}"),
    n => print("large number: ${n}"),
}

match event {
    e @ Event.Click { x, y } if x > 100 => {
        log(e)  // Use full event
        handle_click(x, y)
    },
    _ => {},
}
```

---

## 7. Classes and Inheritance

### 7.1 Class Declaration

```li
class ClassName {
    // Fields
    pub let immutable_field: Type
    pub var mutable_field: Type
    priv var _private_field: Type

    // Constructor
    fn new(params) -> ClassName {
        return ClassName {
            immutable_field: value,
            mutable_field: value,
            _private_field: value,
        }
    }

    // Instance methods
    pub fn method(this) -> ReturnType {
        // 'this' is the current instance
    }

    // Mutable methods
    pub fn mutating_method(this mut) {
        this.mutable_field = new_value
    }

    // Static methods
    pub fn static_method() -> Type {
        // No 'this' parameter
    }
}
```

### 7.2 Visibility

```li
pub                  // Public: accessible from anywhere
priv                 // Private: accessible only within the type (default)
```

### 7.3 Constructors

```li
class User {
    pub let name: string
    pub var email: string
    priv var _id: int

    // Primary constructor
    fn new(name: string, email: string) -> User {
        return User {
            name: name,
            email: email,
            _id: generate_id(),
        }
    }

    // Named constructor
    fn anonymous() -> User {
        return User.new("Anonymous", "none@example.com")
    }

    // Constructor with validation
    fn create(name: string, email: string) -> Result<User, string> {
        if !email.contains("@") {
            return Err("Invalid email")
        }
        return Ok(User.new(name, email))
    }
}
```

### 7.4 Inheritance

```li
class Animal {
    pub let name: string

    fn new(name: string) -> Animal {
        return Animal { name: name }
    }

    pub fn speak(this) -> string {
        return "..."
    }
}

class Dog : Animal {
    pub let breed: string

    fn new(name: string, breed: string) -> Dog {
        return Dog {
            ...super.new(name),  // Call parent constructor
            breed: breed,
        }
    }

    // Override parent method
    override fn speak(this) -> string {
        return "Woof!"
    }

    // New method
    pub fn fetch(this) {
        print("${this.name} fetches the ball!")
    }
}
```

### 7.5 Abstract Classes

```li
abstract class Shape {
    // Abstract method (must be implemented)
    abstract fn area(this) -> float
    abstract fn perimeter(this) -> float

    // Concrete method
    fn describe(this) -> string {
        return "Area: ${this.area()}, Perimeter: ${this.perimeter()}"
    }
}

class Circle : Shape {
    pub let radius: float

    fn new(radius: float) -> Circle {
        return Circle { radius: radius }
    }

    override fn area(this) -> float {
        return 3.14159 * this.radius ** 2
    }

    override fn perimeter(this) -> float {
        return 2.0 * 3.14159 * this.radius
    }
}
```

### 7.6 Interface Implementation

```li
interface Comparable {
    fn compare(this, other: Self) -> int
}

interface Printable {
    fn to_string(this) -> string
}

class Person : Comparable, Printable {
    pub let name: string
    pub let age: int

    fn compare(this, other: Person) -> int {
        return this.age - other.age
    }

    fn to_string(this) -> string {
        return "${this.name} (${this.age})"
    }
}
```

### 7.7 Properties (Getters/Setters)

```li
class Temperature {
    priv var _celsius: float

    fn new(celsius: float) -> Temperature {
        return Temperature { _celsius: celsius }
    }

    // Getter
    pub fn celsius(this) -> float {
        return this._celsius
    }

    // Setter
    pub fn set_celsius(this mut, value: float) {
        this._celsius = value
    }

    // Computed property (getter only)
    pub fn fahrenheit(this) -> float {
        return this._celsius * 9.0 / 5.0 + 32.0
    }

    // Computed setter
    pub fn set_fahrenheit(this mut, value: float) {
        this._celsius = (value - 32.0) * 5.0 / 9.0
    }
}

// Usage
let mut temp = Temperature.new(100.0)
print(temp.celsius)      // 100.0
print(temp.fahrenheit)   // 212.0
temp.set_fahrenheit(32.0)
print(temp.celsius)      // 0.0
```

### 7.8 Trait Declarations

Traits define nominally-typed contracts that must be explicitly implemented:

```li
// Simple trait
trait Clone {
    fn clone(self) -> Self
}

// Trait with default method
trait Eq {
    fn eq(self, other: Self) -> bool

    // Default implementation using eq()
    fn ne(self, other: Self) -> bool {
        return !self.eq(other)
    }
}

// Trait with associated type
trait Iterator {
    type Item

    fn next(self mut) -> Self.Item?
    fn has_next(self) -> bool
}

// Generic trait
trait Into<T> {
    fn into(self) -> T
}

// Trait inheritance (supertrait)
trait Ord: Eq {
    fn cmp(self, other: Self) -> Ordering
}

// Trait with multiple supertraits
trait Key: Hash + Eq {
    // Inherits Hash.hash() and Eq.eq()
}

// Public trait
pub trait Serializable {
    fn serialize(self) -> string
    fn deserialize(data: string) -> Result<Self, Error>
}
```

#### Trait Grammar

```
TraitDecl       ::= Visibility? 'trait' Identifier TypeParams?
                    (':' TraitBound ('+'  TraitBound)*)?
                    '{' TraitMember* '}'

TraitMember     ::= TraitMethod | AssociatedType

TraitMethod     ::= Visibility? 'fn' Identifier TypeParams?
                    '(' Parameters? ')' ('->' Type)?
                    (Block | ';')

AssociatedType  ::= 'type' Identifier (':' TraitBound)?

TraitBound      ::= TypePath
```

### 7.9 Impl Blocks

Impl blocks add methods to types. There are two forms:

#### Inherent Impl (Extension Methods)

Add methods directly to a type:

```li
// Extend built-in type
impl string {
    fn is_empty(self) -> bool {
        return self.len() == 0
    }

    fn repeat(self, count: int) -> string {
        var result = ""
        for _ in 0..count {
            result = result + self
        }
        return result
    }
}

// Extend user-defined type
impl Point {
    // Static method (no self)
    fn origin() -> Point {
        return Point { x: 0, y: 0 }
    }

    // Instance method
    fn distance(self, other: Point) -> float {
        let dx = self.x - other.x
        let dy = self.y - other.y
        return sqrt(dx * dx + dy * dy)
    }

    // Mutating method
    fn translate(self mut, dx: float, dy: float) {
        self.x += dx
        self.y += dy
    }
}

// Generic impl
impl<T> List<T> {
    fn first(self) -> T? {
        return if self.len() > 0 { self[0] } else { null }
    }

    fn map<U>(self, f: fn(T) -> U) -> List<U> {
        var result: List<U> = []
        for item in self {
            result.push(f(item))
        }
        return result
    }
}

// Conditional impl (only when T satisfies constraint)
impl<T: Eq> List<T> {
    fn contains(self, value: T) -> bool {
        for item in self {
            if item.eq(value) {
                return true
            }
        }
        return false
    }
}
```

#### Trait Impl

Implement a trait for a type:

```li
// Simple trait impl
impl Clone for Point {
    fn clone(self) -> Point {
        return Point { x: self.x, y: self.y }
    }
}

// Impl with associated type
impl Iterator for Range {
    type Item = int

    fn next(self mut) -> int? {
        if self.current < self.end {
            let value = self.current
            self.current += 1
            return value
        }
        return null
    }

    fn has_next(self) -> bool {
        return self.current < self.end
    }
}

// Generic trait impl
impl<T: Clone> Clone for List<T> {
    fn clone(self) -> List<T> {
        return self.map(|item| item.clone())
    }
}

// Blanket impl (implement for all types satisfying constraint)
impl<T: Debug> ToString for T {
    fn to_string(self) -> string {
        return self.debug_string()
    }
}
```

#### Impl Grammar

```
ImplDecl        ::= 'impl' TypeParams? ImplTarget '{' ImplMember* '}'

ImplTarget      ::= Type                          // Inherent impl
                  | TraitPath 'for' Type          // Trait impl

ImplMember      ::= Visibility? 'fn' Identifier TypeParams?
                    '(' Parameters? ')' ('->' Type)? Block
                  | 'type' Identifier '=' Type

TypeParams      ::= '<' TypeParam (',' TypeParam)* '>'
TypeParam       ::= Identifier (':' TraitBound ('+'  TraitBound)*)?
```

#### Method Resolution

When calling `x.method()`, methods are resolved in order:

1. Inherent methods on the type
2. Methods from trait impls
3. Methods from interfaces (structural)

```li
impl Foo {
    fn bar(self) { print("inherent") }
}

trait T {
    fn bar(self)
}

impl T for Foo {
    fn bar(self) { print("trait") }
}

let f = Foo {}
f.bar()       // Prints "inherent" (inherent methods have priority)
T.bar(f)      // Prints "trait" (explicit trait method call)
```

---

## 8. Error Handling

### 8.1 Result Type

```li
enum Result<T, E> {
    Ok(T),
    Err(E),
}

fn divide(a: int, b: int) -> Result<int, string> {
    if b == 0 {
        return Err("Division by zero")
    }
    return Ok(a / b)
}
```

### 8.2 Using Result

```li
let result = divide(10, 2)

// Pattern matching
match result {
    Ok(value) => print("Result: ${value}"),
    Err(error) => print("Error: ${error}"),
}

// Methods
if result.is_ok() {
    let value = result.unwrap()
}

// With default
let value = result.unwrap_or(0)
let value = result.unwrap_or_else(|| compute_default())

// Expect (panics with message on error)
let value = result.expect("Division should succeed")
```

### 8.3 Propagation Operator (?)

```li
fn read_config() -> Result<Config, Error> {
    let content = fs.read_file("config.json")?
    let parsed = json.parse(content)?
    let config = Config.from_json(parsed)?
    return Ok(config)
}

// Equivalent to:
fn read_config_verbose() -> Result<Config, Error> {
    let content = match fs.read_file("config.json") {
        Ok(c) => c,
        Err(e) => return Err(e),
    }
    // ...
}
```

### 8.4 Try-Catch Blocks

```li
try {
    let file = fs.open("data.txt")
    let content = file.read_all()
    process(content)
} catch IoError as e {
    print("IO Error: ${e.message}")
} catch ParseError as e {
    print("Parse Error: ${e.message}")
} catch {
    print("Unknown error")
} finally {
    cleanup()
}
```

### 8.5 Throw Expression

```li
fn validate(input: string) -> Result<string, ValidationError> {
    if input.is_empty() {
        throw ValidationError.new("Input cannot be empty")
    }
    if input.length > 100 {
        throw ValidationError.new("Input too long")
    }
    return Ok(input.trim())
}
```

### 8.6 Panic

```li
// Panic terminates the program
fn assert_positive(n: int) {
    if n <= 0 {
        panic("Value must be positive, got: ${n}")
    }
}

// Unreachable code
fn process(opt: Option<int>) -> int {
    match opt {
        Some(v) => v,
        None => unreachable("Option should always have value"),
    }
}

// Assertions
assert(x > 0)
assert(x > 0, "x must be positive")
assert_eq(a, b)
assert_ne(a, b)
```

### 8.7 Custom Error Types

```li
class AppError : Error {
    pub let code: int
    pub let message: string
    pub let cause: Error?

    fn new(code: int, message: string, cause: Error? = null) -> AppError {
        return AppError { code, message, cause }
    }

    fn io_error(msg: string) -> AppError {
        return AppError.new(1, msg)
    }

    fn parse_error(msg: string) -> AppError {
        return AppError.new(2, msg)
    }
}

fn load_data() -> Result<Data, AppError> {
    let content = fs.read_file("data.json").map_err(|e| {
        AppError.io_error("Failed to read file: ${e.message}")
    })?

    let data = json.parse(content).map_err(|e| {
        AppError.parse_error("Invalid JSON: ${e.message}")
    })?

    return Ok(data)
}
```

---

## Appendix A: Statement Grammar

```
Statement       ::= Declaration
                  | ExpressionStmt
                  | BlockStmt
                  | IfStmt
                  | MatchStmt
                  | WhileStmt
                  | ForStmt
                  | LoopStmt
                  | ReturnStmt
                  | BreakStmt
                  | ContinueStmt
                  | TryStmt

Declaration     ::= LetDecl | VarDecl | ConstDecl | FnDecl
                  | ClassDecl | StructDecl | EnumDecl
                  | InterfaceDecl | TraitDecl | TypeDecl | ImplDecl

ExpressionStmt  ::= Expression ';'?
BlockStmt       ::= '{' Statement* '}'
IfStmt          ::= 'if' Expression Block ('else' (IfStmt | Block))?
MatchStmt       ::= 'match' Expression '{' MatchArm* '}'
WhileStmt       ::= 'while' Expression Block
ForStmt         ::= 'for' Pattern 'in' Expression Block
LoopStmt        ::= 'loop' Block
ReturnStmt      ::= 'return' Expression? ';'?
BreakStmt       ::= 'break' Label? Expression? ';'?
ContinueStmt    ::= 'continue' Label? ';'?
TryStmt         ::= 'try' Block CatchClause* FinallyClause?

TraitDecl       ::= Visibility? 'trait' Identifier TypeParams?
                    (':' TraitBound ('+' TraitBound)*)?
                    '{' TraitMember* '}'
TraitMember     ::= TraitMethod | AssociatedType
TraitMethod     ::= Visibility? 'fn' Identifier TypeParams?
                    '(' Parameters? ')' ('->' Type)? (Block | ';')
AssociatedType  ::= 'type' Identifier (':' TraitBound)? ';'?

ImplDecl        ::= 'impl' TypeParams? ImplTarget '{' ImplMember* '}'
ImplTarget      ::= Type                          // Inherent impl
                  | TraitPath 'for' Type          // Trait impl
ImplMember      ::= Visibility? 'fn' Identifier TypeParams?
                    '(' Parameters? ')' ('->' Type)? Block
                  | 'type' Identifier '=' Type ';'?

TypeParams      ::= '<' TypeParam (',' TypeParam)* '>'
TypeParam       ::= Identifier (':' TraitBound ('+' TraitBound)*)?
TraitBound      ::= TypePath
```

---

## Appendix B: Expression Grammar

```
Expression      ::= AssignExpr

AssignExpr      ::= OrExpr (AssignOp AssignExpr)?
OrExpr          ::= AndExpr ('||' AndExpr)*
AndExpr         ::= BitOrExpr ('&&' BitOrExpr)*
BitOrExpr       ::= BitXorExpr ('|' BitXorExpr)*
BitXorExpr      ::= BitAndExpr ('^' BitAndExpr)*
BitAndExpr      ::= EqualExpr ('&' EqualExpr)*
EqualExpr       ::= CompareExpr (('==' | '!=') CompareExpr)*
CompareExpr     ::= ShiftExpr (('<' | '<=' | '>' | '>=') ShiftExpr)*
ShiftExpr       ::= AddExpr (('<<' | '>>' | '>>>') AddExpr)*
AddExpr         ::= MulExpr (('+' | '-') MulExpr)*
MulExpr         ::= PowExpr (('*' | '/' | '%') PowExpr)*
PowExpr         ::= UnaryExpr ('**' PowExpr)?
UnaryExpr       ::= ('!' | '-' | '~') UnaryExpr | PostfixExpr
PostfixExpr     ::= PrimaryExpr (CallExpr | IndexExpr | MemberExpr | '!' | '?')*
PrimaryExpr     ::= Literal | Identifier | '(' Expression ')' | IfExpr
                  | MatchExpr | LambdaExpr | ListExpr | MapExpr
```

---

_This document is part of the Lira Language Specification._

/**
 * Sample Lira programs for the playground
 * These programs are tested against the actual compiler.
 */

export const SAMPLE_PROGRAMS = {
  helloWorld: `// Hello World in Lira
println("Hello, Lira!")
println("Welcome to the Lira Playground!")`,

  fibonacci: `// Fibonacci Sequence
// Demonstrates recursion and functions

fn fib(n: int) -> int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

// Print first 10 fibonacci numbers
var i = 0
while i < 10 {
    let result = fib(i)
    println("fib(" + i + ") = " + result)
    i = i + 1
}`,

  arrays: `// Array Operations
// Demonstrates arrays, loops, and iteration

println("=== Array Basics ===")

let numbers = [1, 2, 3, 4, 5]
var sum = 0
for n in numbers {
    sum = sum + n
}
println("Sum: " + sum)

// Array of strings
let fruits = ["apple", "banana", "cherry"]
for fruit in fruits {
    println("Fruit: " + fruit)
}

// Nested arrays
let matrix = [[1, 2], [3, 4], [5, 6]]
var total = 0
for row in matrix {
    for val in row {
        total = total + val
    }
}
println("Matrix total: " + total)

// Break and continue
var even_sum = 0
for n in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
    if n % 2 != 0 {
        continue
    }
    even_sum = even_sum + n
}
println("Even sum: " + even_sum)`,

  patternMatching: `// Pattern Matching
// Demonstrates match expressions

fn grade(score: int) -> string {
    return match score {
        100 => "Perfect!",
        90 => "A",
        80 => "B",
        70 => "C",
        60 => "D",
        _ => "F"
    }
}

println("Testing grades:")
println("100: " + grade(100))
println("90: " + grade(90))
println("80: " + grade(80))
println("75: " + grade(75))
println("50: " + grade(50))

// Variable binding in pattern
fn double_it(x: int) -> int {
    return match x {
        n => n * 2
    }
}

println("double(5) = " + double_it(5))
println("double(21) = " + double_it(21))`,

  enums: `// Enums
// Demonstrates enum definitions and usage

enum Color {
    Red,
    Green,
    Blue
}

// Create enum variants
let red = Color::Red
let green = Color::Green
let blue = Color::Blue

println("Created Color variants")
println("red.__variant: " + red.__variant)
println("green.__variant: " + green.__variant)
println("blue.__variant: " + blue.__variant)

// Compare variants
let c1 = Color::Red
let c2 = Color::Red
let c3 = Color::Blue

println("c1 == c2: " + (c1.__variant == c2.__variant))
println("c1 == c3: " + (c1.__variant == c3.__variant))

enum Status {
    Active,
    Inactive,
    Pending
}

let s = Status::Active
println("Status: " + s.__variant)`,

  structs: `// Structs and Methods
// Demonstrates struct definitions with inline methods

struct Point {
    x: int
    y: int

    fn sum(self) -> int {
        return self.x + self.y
    }

    fn add(self, other: Point) -> Point {
        return Point {
            x: self.x + other.x,
            y: self.y + other.y
        }
    }
}

struct Person {
    name: string
    age: int

    fn greet(self) -> string {
        return "Hello, " + self.name
    }
}

// Create struct instances
let point = Point { x: 10, y: 20 }
println("Point x: " + point.x)
println("Point y: " + point.y)
println("Point sum: " + point.sum())

let person = Person { name: "Alice", age: 30 }
println(person.greet())

// Add two points
let p1 = Point { x: 1, y: 2 }
let p2 = Point { x: 3, y: 4 }
let p3 = p1.add(p2)
println("Added point: (" + p3.x + ", " + p3.y + ")")`,

  closures: `// Closures and Higher-Order Functions
// Demonstrates lambdas and closures

println("=== Basic Lambdas ===")

let double = |x: int| x * 2
println("double(5) = " + double(5))
println("double(21) = " + double(21))

let add = |a: int, b: int| a + b
println("add(3, 4) = " + add(3, 4))

println("=== Higher-Order Functions ===")

fn apply_twice(f: fn(int) -> int, x: int) -> int {
    return f(f(x))
}

println("apply_twice(double, 3) = " + apply_twice(double, 3))

println("=== Closures ===")

fn make_adder(n: int) -> fn(int) -> int {
    return |x: int| x + n
}

let add5 = make_adder(5)
let add10 = make_adder(10)

println("add5(3) = " + add5(3))
println("add10(3) = " + add10(3))

fn make_multiplier(n: int) -> fn(int) -> int {
    return |x: int| x * n
}

let times3 = make_multiplier(3)
let times7 = make_multiplier(7)

println("times3(4) = " + times3(4))
println("times7(4) = " + times7(4))`,

  controlFlow: `// Control Flow
// Demonstrates if/else, while, and loops

println("=== If/Else ===")

fn abs(x: int) -> int {
    if x < 0 {
        return -x
    } else {
        return x
    }
}

println("abs(-5) = " + abs(-5))
println("abs(5) = " + abs(5))

fn max(a: int, b: int) -> int {
    if a > b {
        return a
    }
    return b
}

println("max(3, 7) = " + max(3, 7))

println("=== While Loop ===")

var count = 0
while count < 5 {
    println("Count: " + count)
    count = count + 1
}

println("=== Factorial ===")

fn factorial(n: int) -> int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

println("5! = " + factorial(5))
println("10! = " + factorial(10))`,

  primeNumbers: `// Prime Number Algorithms
// Demonstrates functions, loops, and mathematical operations

fn is_prime(n: int) -> bool {
    if n <= 1 {
        return false
    }
    if n <= 3 {
        return true
    }
    if n % 2 == 0 {
        return false
    }

    var i = 3
    while i * i <= n {
        if n % i == 0 {
            return false
        }
        i = i + 2
    }
    return true
}

// Find all primes up to 50
println("=== Primes up to 50 ===")
var primes = ""
var n = 2
while n <= 50 {
    if is_prime(n) {
        primes = primes + n + " "
    }
    n = n + 1
}
println(primes)

// Count primes up to 100
var count = 0
n = 2
while n <= 100 {
    if is_prime(n) {
        count = count + 1
    }
    n = n + 1
}
println("Number of primes up to 100: " + count)

// Find the nth prime
fn nth_prime(target: int) -> int {
    var found = 0
    var num = 2
    while found < target {
        if is_prime(num) {
            found = found + 1
            if found == target {
                return num
            }
        }
        num = num + 1
    }
    return 0
}

println("10th prime: " + nth_prime(10))
println("25th prime: " + nth_prime(25))`,

  traits: `// Traits and Polymorphism
// Demonstrates trait definitions and implementations

// Define a trait for things that can speak
trait Speak {
    fn speak(self) -> string
}

// Define structs that implement the trait
struct Dog {
    name: string
}

struct Cat {
    name: string
}

struct Parrot {
    name: string
    phrase: string
}

// Implement Speak trait for each struct
impl Speak for Dog {
    fn speak(self) -> string {
        return self.name + " says: Woof!"
    }
}

impl Speak for Cat {
    fn speak(self) -> string {
        return self.name + " says: Meow!"
    }
}

impl Speak for Parrot {
    fn speak(self) -> string {
        return self.name + " says: " + self.phrase
    }
}

fn main() {
    println("=== Animal Sounds ===")

    let dog = Dog { name: "Buddy" }
    let cat = Cat { name: "Whiskers" }
    let parrot = Parrot { name: "Polly", phrase: "Hello there!" }

    println(dog.speak())
    println(cat.speak())
    println(parrot.speak())
}

main()`,

  classInheritance: `// Class Inheritance
// Demonstrates extends keyword and field inheritance

// Base class
class Vehicle {
    brand: string
    year: int
}

// Derived classes inherit parent fields
class Car extends Vehicle {
    doors: int
}

class Motorcycle extends Vehicle {
    engineCC: int
}

class Truck extends Vehicle {
    payload: int
}

fn main() {
    println("=== Vehicle Inventory ===")

    // Create instances with inherited + own fields
    let car = Car {
        brand: "Toyota",
        year: 2023,
        doors: 4
    }

    let bike = Motorcycle {
        brand: "Honda",
        year: 2022,
        engineCC: 650
    }

    let truck = Truck {
        brand: "Ford",
        year: 2021,
        payload: 2000
    }

    // Access inherited fields
    println("Car: " + car.brand + " (" + car.year + "), " + car.doors + " doors")
    println("Motorcycle: " + bike.brand + " (" + bike.year + "), " + bike.engineCC + "cc")
    println("Truck: " + truck.brand + " (" + truck.year + "), " + truck.payload + "kg payload")
}

main()`,

  generics: `// Generics
// Demonstrates generic functions and structs

println("=== Generic Functions ===")

// Generic identity function
fn identity<T>(x: T) -> T {
    return x
}

println("identity(42) = " + identity(42))
println("identity(\"hello\") = " + identity("hello"))
println("identity(true) = " + identity(true))

println("=== Generic Structs ===")

// Generic Box that can hold any type
struct Box<T> {
    value: T
}

let int_box = Box { value: 100 }
let str_box = Box { value: "treasure" }
let bool_box = Box { value: true }

println("int_box.value = " + int_box.value)
println("str_box.value = " + str_box.value)
println("bool_box.value = " + bool_box.value)

println("=== Generic Pair ===")

// Generic Pair with two type parameters
struct Pair<A, B> {
    first: A
    second: B
}

let pair1 = Pair { first: "name", second: 42 }
let pair2 = Pair { first: 3.14, second: true }

println("pair1: (" + pair1.first + ", " + pair1.second + ")")
println("pair2: (" + pair2.first + ", " + pair2.second + ")")`,

  algorithms: `// Classic Algorithms
// Demonstrates sorting, searching, and recursion

println("=== Binary Search ===")

fn binary_search(arr: [int], target: int) -> int {
    var left = 0
    var right = len(arr) - 1

    while left <= right {
        let mid = (left + right) / 2
        if arr[mid] == target {
            return mid
        } else if arr[mid] < target {
            left = mid + 1
        } else {
            right = mid - 1
        }
    }
    return -1  // Not found
}

let numbers = [1, 3, 5, 7, 9, 11, 13, 15, 17, 19]
println("Array: " + numbers)
println("Index of 7: " + binary_search(numbers, 7))
println("Index of 15: " + binary_search(numbers, 15))
println("Index of 6: " + binary_search(numbers, 6))

println("=== GCD (Euclidean Algorithm) ===")

fn gcd(a: int, b: int) -> int {
    if b == 0 {
        return a
    }
    return gcd(b, a % b)
}

println("GCD(48, 18) = " + gcd(48, 18))
println("GCD(100, 25) = " + gcd(100, 25))
println("GCD(17, 13) = " + gcd(17, 13))

println("=== Power Function ===")

fn power(base: int, exp: int) -> int {
    if exp == 0 {
        return 1
    }
    return base * power(base, exp - 1)
}

println("2^10 = " + power(2, 10))
println("3^5 = " + power(3, 5))

println("=== Sum of Array ===")

fn sum_array(arr: [int]) -> int {
    var total = 0
    for x in arr {
        total = total + x
    }
    return total
}

println("Sum of [1,2,3,4,5]: " + sum_array([1, 2, 3, 4, 5]))
println("Sum of [10,20,30]: " + sum_array([10, 20, 30]))`,

  stringOperations: `// String Operations
// Demonstrates string manipulation and operations

println("=== String Concatenation ===")

let hello = "Hello"
let world = "World"
let greeting = hello + ", " + world + "!"
println(greeting)

// Concatenation with numbers
let count = 42
let msg = "The answer is " + count
println(msg)

println("=== String Length ===")

let text = "Lira Programming Language"
println("Text: " + text)
println("Length: " + len(text))

println("=== String Indexing ===")

let word = "HELLO"
println("word[0] = " + word[0])
println("word[1] = " + word[1])
println("word[4] = " + word[4])

println("=== String Comparison ===")

let a = "apple"
let b = "apple"
let c = "banana"

if a == b {
    println("apple == apple: true")
}
if a != c {
    println("apple != banana: true")
}

println("=== String in Pattern Matching ===")

fn describe_fruit(fruit: string) -> string {
    match fruit {
        "apple" => "A red fruit",
        "banana" => "A yellow fruit",
        "orange" => "An orange fruit",
        _ => "Unknown fruit"
    }
}

println(describe_fruit("apple"))
println(describe_fruit("banana"))
println(describe_fruit("kiwi"))

println("=== Building Strings ===")

var result = ""
for i in [1, 2, 3, 4, 5] {
    result = result + i + " "
}
println("Built: " + result)`,

  implBlocks: `// Impl Blocks
// Demonstrates method implementation with impl blocks

struct Counter {
    value: int
}

impl Counter {
    // Static method (no self)
    fn new() -> Counter {
        return Counter { value: 0 }
    }

    // Instance method
    fn get(self) -> int {
        return self.value
    }

    // Method that returns a new instance
    fn increment(self) -> Counter {
        return Counter { value: self.value + 1 }
    }

    fn add(self, amount: int) -> Counter {
        return Counter { value: self.value + amount }
    }
}

fn main() {
    println("=== Counter Demo ===")

    // Create using static method
    let c = Counter.new()
    println("Initial value: " + c.get())

    // Chain increments
    let c2 = c.increment()
    println("After increment: " + c2.get())

    let c3 = c2.increment().increment().increment()
    println("After 3 more increments: " + c3.get())

    // Add arbitrary amount
    let c4 = Counter.new().add(100)
    println("After add(100): " + c4.get())
}

main()`,

  nullCoalescing: `// Null and Optional Values
// Demonstrates null handling and coalescing

println("=== Null Basics ===")

let nothing = null
println("null value: " + nothing)

// Null equality
println("null == null: " + (null == null))
println("null != null: " + (null != null))

println("=== Null Coalescing (??) ===")

// Use ?? to provide default values
let val1 = null ?? 42
println("null ?? 42 = " + val1)

let val2 = 100 ?? 42
println("100 ?? 42 = " + val2)

// Chained coalescing
let a = null
let b = null
let c = 30
let result = a ?? b ?? c
println("null ?? null ?? 30 = " + result)

println("=== Safe Division Pattern ===")

fn safe_divide(x: int, y: int) -> int {
    if y == 0 {
        return 0  // Return 0 for division by zero
    }
    return x / y
}

println("10 / 2 = " + safe_divide(10, 2))
println("10 / 0 = " + safe_divide(10, 0) + " (safe!)")

println("=== Null Checks ===")

let maybe_value = null

if maybe_value == null {
    println("Value is null")
} else {
    println("Value is: " + maybe_value)
}

// Use coalescing for safe access
let display = maybe_value ?? "default"
println("Display: " + display)`,

  recursion: `// Advanced Recursion
// Demonstrates various recursive patterns

println("=== Mutual Recursion ===")

// Two functions that call each other
fn is_even(n: int) -> bool {
    if n == 0 { return true }
    return is_odd(n - 1)
}

fn is_odd(n: int) -> bool {
    if n == 0 { return false }
    return is_even(n - 1)
}

println("is_even(10): " + is_even(10))
println("is_odd(7): " + is_odd(7))
println("is_even(15): " + is_even(15))

println("=== Ackermann Function ===")

// Famous recursive function that grows very fast
fn ackermann(m: int, n: int) -> int {
    if m == 0 {
        return n + 1
    }
    if n == 0 {
        return ackermann(m - 1, 1)
    }
    return ackermann(m - 1, ackermann(m, n - 1))
}

println("ackermann(0, 5) = " + ackermann(0, 5))
println("ackermann(1, 3) = " + ackermann(1, 3))
println("ackermann(2, 3) = " + ackermann(2, 3))
println("ackermann(3, 2) = " + ackermann(3, 2))

println("=== Sum of Digits ===")

fn sum_digits(n: int) -> int {
    if n < 10 {
        return n
    }
    return n % 10 + sum_digits(n / 10)
}

println("sum_digits(12345) = " + sum_digits(12345))
println("sum_digits(9999) = " + sum_digits(9999))

println("=== Reverse Number ===")

fn reverse_helper(n: int, acc: int) -> int {
    if n == 0 {
        return acc
    }
    return reverse_helper(n / 10, acc * 10 + n % 10)
}

fn reverse_num(n: int) -> int {
    return reverse_helper(n, 0)
}

println("reverse(12345) = " + reverse_num(12345))
println("reverse(9876) = " + reverse_num(9876))`,
} as const;

export type SampleProgramName = keyof typeof SAMPLE_PROGRAMS;

export const SAMPLE_PROGRAM_LABELS: Record<SampleProgramName, string> = {
  helloWorld: 'Hello World',
  fibonacci: 'Fibonacci',
  arrays: 'Arrays & Loops',
  patternMatching: 'Pattern Matching',
  enums: 'Enums',
  structs: 'Structs & Methods',
  closures: 'Closures',
  controlFlow: 'Control Flow',
  primeNumbers: 'Prime Numbers',
  traits: 'Traits',
  classInheritance: 'Class Inheritance',
  generics: 'Generics',
  algorithms: 'Algorithms',
  stringOperations: 'String Operations',
  implBlocks: 'Impl Blocks',
  nullCoalescing: 'Null & Coalescing',
  recursion: 'Advanced Recursion',
};

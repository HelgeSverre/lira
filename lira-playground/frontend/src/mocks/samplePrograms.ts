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
};

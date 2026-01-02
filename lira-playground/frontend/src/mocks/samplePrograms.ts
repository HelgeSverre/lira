/**
 * Sample Lira programs for the playground
 */

export const SAMPLE_PROGRAMS = {
  helloWorld: `// Hello World in Lira
fn main() {
    println("Hello, Lira!")
}

main()`,

  fibonacci: `// Fibonacci sequence
fn fib(n: int) -> int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

fn main() {
    for i in 0..10 {
        println("fib(" + i + ") = " + fib(i))
    }
}

main()`,

  fibers: `// Fiber concurrency example
fn worker(id: int, ch: Channel<int>) {
    for i in 0..5 {
        ch <- (id * 10 + i)
        fiber_yield()
    }
}

fn main() {
    let ch = chan(10)

    spawn worker(1, ch)
    spawn worker(2, ch)
    spawn worker(3, ch)

    for _ in 0..15 {
        let val = <-ch
        println("Received: " + val)
    }
}

main()`,

  patternMatching: `// Pattern matching with enums
enum Option {
    None,
    Some(int)
}

fn maybe_double(opt: Option) -> Option {
    match opt {
        Option::None => Option::None
        Option::Some(x) => Option::Some(x * 2)
    }
}

fn main() {
    let a = Option::Some(21)
    let b = Option::None

    match maybe_double(a) {
        Option::Some(n) => println("Result: " + n)
        Option::None => println("No result")
    }

    match maybe_double(b) {
        Option::Some(n) => println("Result: " + n)
        Option::None => println("No result")
    }
}

main()`,

  channels: `// Channel communication
fn producer(ch: Channel<string>) {
    let items = ["apple", "banana", "cherry", "date"]
    for item in items {
        println("Sending: " + item)
        ch <- item
    }
    close(ch)
}

fn consumer(ch: Channel<string>) {
    loop {
        select {
            msg = <-ch => {
                println("Received: " + msg)
            }
            default => {
                println("Channel closed or empty")
                break
            }
        }
    }
}

fn main() {
    let ch = chan(2)

    spawn producer(ch)
    consumer(ch)
}

main()`,

  structs: `// Structs and methods
struct Point {
    x: float,
    y: float
}

impl Point {
    fn new(x: float, y: float) -> Point {
        Point { x: x, y: y }
    }

    fn distance(self, other: Point) -> float {
        let dx = self.x - other.x
        let dy = self.y - other.y
        (dx * dx + dy * dy).sqrt()
    }

    fn to_string(self) -> string {
        "(" + self.x + ", " + self.y + ")"
    }
}

fn main() {
    let p1 = Point::new(0.0, 0.0)
    let p2 = Point::new(3.0, 4.0)

    println("p1 = " + p1.to_string())
    println("p2 = " + p2.to_string())
    println("Distance: " + p1.distance(p2))
}

main()`,

  errorHandling: `// Error handling with Result
fn divide(a: int, b: int) -> Result<float, string> {
    if b == 0 {
        return Err("Division by zero")
    }
    return Ok(a as float / b as float)
}

fn main() {
    let results = [
        divide(10, 2),
        divide(5, 0),
        divide(15, 3)
    ]

    for result in results {
        match result {
            Ok(value) => println("Result: " + value)
            Err(msg) => println("Error: " + msg)
        }
    }
}

main()`,

  closures: `// Closures and higher-order functions
fn make_counter(start: int) -> fn() -> int {
    var count = start
    return || {
        let current = count
        count = count + 1
        current
    }
}

fn map_array(arr: [int], f: fn(int) -> int) -> [int] {
    var result = []
    for item in arr {
        result.push(f(item))
    }
    result
}

fn main() {
    let counter = make_counter(1)
    println(counter())  // 1
    println(counter())  // 2
    println(counter())  // 3

    let numbers = [1, 2, 3, 4, 5]
    let doubled = map_array(numbers, |x| x * 2)
    println("Doubled: " + doubled)
}

main()`,
} as const;

export type SampleProgramName = keyof typeof SAMPLE_PROGRAMS;

export const SAMPLE_PROGRAM_LABELS: Record<SampleProgramName, string> = {
  helloWorld: 'Hello World',
  fibonacci: 'Fibonacci',
  fibers: 'Fibers & Concurrency',
  patternMatching: 'Pattern Matching',
  channels: 'Channels',
  structs: 'Structs & Methods',
  errorHandling: 'Error Handling',
  closures: 'Closures',
};

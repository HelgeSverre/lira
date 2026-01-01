# Lira (Liseth Integrated Language) Specification

## Document Information

| Property           | Value               |
| ------------------ | ------------------- |
| **Version**        | 1.0.0-draft         |
| **Status**         | Draft Specification |
| **Platform**       | macOS, Linux        |
| **Implementation** | Rust                |
| **Last Updated**   | 2025-12             |

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Design Philosophy](#2-design-philosophy)
3. [Language Overview](#3-language-overview)
4. [File Types](#4-file-types)
5. [Toolchain](#5-toolchain)
6. [Quick Start Examples](#6-quick-start-examples)
7. [Runtime Environment](#7-runtime-environment)
8. [Document Map](#8-document-map)

---

## 1. Introduction

### 1.1 What is Lira?

Lira (Liseth Integrated Language) is a modern systems programming language with Go-like fiber concurrency, pattern matching, and a clean syntax. The name "Liseth" derives from the creator's surname. Lira is designed to be expressive yet safe, with a focus on developer productivity and runtime efficiency.

### 1.2 Design Goals

Lira was designed with the following primary goals:

1. **Developer Productivity**: Modern syntax inspired by Dart, TypeScript, and Go for rapid development
2. **Safety**: Static typing with inference, null safety, and automatic memory management
3. **Performance**: Efficient bytecode VM with low-overhead green threads (fibers)
4. **Concurrency**: Built-in support for fibers and channels for concurrent programming
5. **Declarative UI**: Separate `.liui` format for building user interfaces with minimal boilerplate

### 1.3 Target Audience

Lira is designed for developers building:

- System utilities and CLI tools
- Concurrent applications and services
- Background services and daemons
- Applications requiring lightweight concurrency

---

## 2. Design Philosophy

### 2.1 Simplicity Over Complexity

Lira favors clear, readable syntax over clever shortcuts. When multiple approaches are possible, the simpler one is preferred. The language avoids hidden magic and implicit behavior that could surprise developers.

### 2.2 Static Types with Dynamic Feel

While Lira is statically typed, extensive type inference makes it feel lightweight. Explicit type annotations are optional in most contexts, appearing only where they add clarity or are necessary for disambiguation.

```li
// Type inference - compiler knows these types
let name = "Alice"           // string
let count = 42               // int
let items = [1, 2, 3]        // List<int>

// Explicit when helpful
let config: Map<string, any> = load_config()
```

### 2.3 Explicit Over Implicit

Lira makes program behavior explicit:

- No implicit type coercion (except safe numeric widening)
- No null without explicit `?` optional type
- No implicit returns (must use `return` keyword)
- No hidden control flow (no exceptions from operators)

### 2.4 Concurrency as a First-Class Concept

Modern applications are concurrent. Lira treats concurrency as fundamental, providing lightweight green threads (fibers) and channels as built-in language features, not library add-ons.

### 2.5 Separation of Logic and UI

Following the QML model, Lira separates application logic (`.li` files) from user interface declarations (`.liui` files). This separation enables:

- Designers and developers to work independently
- UI hot-reloading during development
- Clear data flow between state and presentation

---

## 3. Language Overview

### 3.1 Type System

Lira uses a strong, static type system with full type inference:

| Category        | Types                                                                    |
| --------------- | ------------------------------------------------------------------------ |
| **Primitives**  | `int`, `float`, `bool`, `string`, `char`, `void`                         |
| **Integers**    | `int8`, `int16`, `int32`, `int64`, `uint8`, `uint16`, `uint32`, `uint64` |
| **Collections** | `List<T>`, `Map<K,V>`, `Set<T>`, tuples `(T, U, ...)`                    |
| **Optionals**   | `T?` (nullable), unwrap with `?`, `??`, `!`                              |
| **User Types**  | `class`, `struct`, `enum`, `interface`                                   |
| **Functions**   | `fn(Args) -> Return`                                                     |

### 3.2 Memory Management

Lira uses **automatic reference counting (ARC)** with cycle detection:

- Objects are automatically deallocated when their reference count reaches zero
- A background cycle detector handles circular references
- No manual memory management required
- Predictable, low-latency deallocation (no GC pauses)

### 3.3 Concurrency Model

Lira provides **green threads (fibers)** with **channel-based communication**:

```li
// Spawn a fiber
spawn {
    let result = compute_heavy_task()
    channel.send(result)
}

// Receive from channel
let value = channel.receive()

// Select from multiple channels
select {
    msg = <-messages => handle(msg),
    <-timeout(1000) => print("Timed out"),
}
```

### 3.4 Error Handling

Lira uses the `Result<T, E>` type for recoverable errors:

```li
fn read_file(path: string) -> Result<string, IoError> {
    let file = fs.open(path)?  // Propagate error with ?
    let content = file.read_all()?
    return Ok(content)
}

// Usage
match read_file("config.json") {
    Ok(content) => parse(content),
    Err(error) => log_error(error),
}
```

### 3.5 Declarative UI

The `.liui` format provides declarative UI definitions:

```liui
// counter.liui
import state from "./counter.li"

Window {
    title: "Counter"

    VBox {
        Label { text: ${"Count: " + state.count} }
        Button {
            text: "Increment"
            onClick: () => { state.count += 1 }
        }
    }
}
```

---

## 4. File Types

### 4.1 Source Files (.li)

Lira source files contain application logic, type definitions, and functions.

```
my_app/
├── src/
│   ├── main.li          # Entry point
│   ├── models/
│   │   ├── user.li
│   │   └── post.li
│   └── utils/
│       └── helpers.li
```

### 4.2 UI Definition Files (.liui)

Lira UI files contain declarative user interface definitions in a JSON-like syntax.

```
my_app/
├── src/
│   └── main.li
├── ui/
│   ├── main_window.liui
│   ├── components/
│   │   ├── header.liui
│   │   └── sidebar.liui
```

### 4.3 Compiled Bytecode (.lic)

Compiled Lira bytecode files are executed by the Lira VM.

```
build/
├── my_app.lic           # Compiled bytecode
└── my_app.lic.debug     # Debug symbols (optional)
```

### 4.4 Package Manifest (li.toml)

Package configuration file defining dependencies and build settings.

```toml
[package]
name = "my_app"
version = "1.0.0"
entry = "src/main.li"

[dependencies]
std = "1.0"
gui = "1.0"

[build]
optimization = "release"
debug_info = true
```

---

## 5. Toolchain

### 5.1 Compiler (lirac)

The Lira compiler transforms source files into bytecode:

```bash
# Compile a single file
lirac compile src/main.li -o build/main.lic

# Compile a project
lirac build

# Compile with optimization
lirac build --release
```

### 5.2 Virtual Machine (liravm)

The Lira VM executes compiled bytecode:

```bash
# Run a compiled program
liravm run build/main.lic

# Run with debugging
liravm run --debug build/main.lic
```

### 5.3 Runner (lir)

Convenience tool that compiles and runs in one step:

```bash
# Compile and run
lir src/main.li

# Start REPL
lir
```

### 5.4 Formatter (lifmt)

Automatic code formatter for consistent style:

```bash
# Format a file
lifmt src/main.li

# Format entire project
lifmt --recursive src/
```

### 5.5 Documentation Generator (lidoc)

Generates documentation from doc comments:

```bash
# Generate HTML documentation
lidoc generate --output docs/

# Serve documentation locally
lidoc serve --port 8080
```

### 5.6 Package Manager (lipkg)

Manages dependencies and packages:

```bash
# Initialize new project
lipkg init my_app

# Add dependency
lipkg add http

# Install all dependencies
lipkg install

# Publish package
lipkg publish
```

---

## 6. Quick Start Examples

### 6.1 Hello World

```li
// hello.li

fn main() {
    print("Hello, World!")
}
```

### 6.2 Variables and Types

```li
fn main() {
    // Immutable binding
    let name = "Lira"
    let version = 1.0

    // Mutable binding
    var counter = 0
    counter += 1

    // Explicit types
    let values: List<int> = [1, 2, 3, 4, 5]

    // Constants
    const MAX_SIZE = 1024

    print("${name} v${version}")
}
```

### 6.3 Functions

```li
// Function with parameters and return type
fn add(a: int, b: int) -> int {
    return a + b
}

// Expression body (short form)
fn square(x: int) -> int => x * x

// Default parameters
fn greet(name: string, greeting: string = "Hello") -> string {
    return "${greeting}, ${name}!"
}

// Generic function
fn first<T>(items: List<T>) -> T? {
    return if items.length > 0 { items[0] } else { null }
}

fn main() {
    print(add(2, 3))           // 5
    print(square(4))           // 16
    print(greet("World"))      // Hello, World!
    print(first([1, 2, 3]))    // 1
}
```

### 6.4 Classes and Structs

```li
// Value type (copied on assignment)
struct Point {
    x: float
    y: float

    fn distance_to(this, other: Point) -> float {
        let dx = this.x - other.x
        let dy = this.y - other.y
        return (dx * dx + dy * dy).sqrt()
    }
}

// Reference type (shared reference)
class Person {
    pub let name: string
    pub var age: int

    fn new(name: string, age: int) -> Person {
        return Person { name: name, age: age }
    }

    pub fn greet(this) -> string {
        return "Hi, I'm ${this.name}!"
    }
}

fn main() {
    let p1 = Point { x: 0.0, y: 0.0 }
    let p2 = Point { x: 3.0, y: 4.0 }
    print(p1.distance_to(p2))  // 5.0

    let person = Person.new("Alice", 30)
    print(person.greet())      // Hi, I'm Alice!
}
```

### 6.5 Pattern Matching

```li
enum Shape {
    Circle(radius: float),
    Rectangle(width: float, height: float),
    Triangle(base: float, height: float),
}

fn area(shape: Shape) -> float {
    match shape {
        Shape.Circle(r) => 3.14159 * r * r,
        Shape.Rectangle(w, h) => w * h,
        Shape.Triangle(b, h) => 0.5 * b * h,
    }
}

fn main() {
    let shapes = [
        Shape.Circle(5.0),
        Shape.Rectangle(4.0, 6.0),
        Shape.Triangle(3.0, 4.0),
    ]

    for shape in shapes {
        print("Area: ${area(shape)}")
    }
}
```

### 6.6 Concurrency

```li
fn main() {
    // Create a channel
    let results = Channel<int>.new()

    // Spawn worker fibers
    for i in 1..=5 {
        spawn {
            // Simulate work
            sleep(100 * i)
            results.send(i * i)
        }
    }

    // Collect results
    var total = 0
    for _ in 1..=5 {
        total += results.receive()
    }

    print("Sum of squares: ${total}")  // 55
}
```

### 6.7 GUI Application

```li
// counter.li
import gui.core.{App, Window}
import gui.widgets.{Button, Label, VBox}

fn main() {
    let app = App.new("Counter")
    var count = 0

    let window = Window.new(
        title: "Counter App",
        width: 300,
        height: 200,
    )

    let label = Label.new("Count: 0")
    let inc_btn = Button.new("+")
    let dec_btn = Button.new("-")

    inc_btn.on_click(|| {
        count += 1
        label.set_text("Count: ${count}")
    })

    dec_btn.on_click(|| {
        count -= 1
        label.set_text("Count: ${count}")
    })

    let layout = VBox.new([label, inc_btn, dec_btn])
    window.set_content(layout)
    window.show()

    app.run()
}
```

### 6.8 GUI with Lira UI

**counter.li:**

```li
// State module
export var count = 0

export fn increment() {
    count += 1
}

export fn decrement() {
    count -= 1
}
```

**counter.liui:**

```liui
import { count, increment, decrement } from "./counter.li"

Window {
    title: "Counter App"
    width: 300
    height: 200

    VBox {
        spacing: 16
        padding: 24
        align: Alignment.Center

        Label {
            text: ${"Count: " + count}
            style: {
                fontSize: 32
                fontWeight: FontWeight.Bold
            }
        }

        HBox {
            spacing: 12

            Button {
                text: "-"
                width: 60
                onClick: decrement
            }

            Button {
                text: "+"
                width: 60
                primary: true
                onClick: increment
            }
        }
    }
}
```

---

## 7. Runtime Environment

### 7.1 Host Primitives

Lira applications interact with the host operating system through a set of built-in primitives implemented in the VM:

| Primitive     | Purpose                          |
| ------------- | -------------------------------- |
| `file_open`   | Open file                        |
| `file_close`  | Close file descriptor            |
| `file_read`   | Read from file                   |
| `file_write`  | Write to file                    |
| `file_exists` | Check file existence             |
| `time_ms`     | Get current time in milliseconds |
| `sleep`       | Sleep for milliseconds           |
| `env_get`     | Get environment variable         |
| `env_args`    | Get command line arguments       |

### 7.2 Memory Model

The Lira VM manages memory with:

- Per-fiber stacks (64KB default)
- Shared heap for objects
- Reference counting with cycle detection
- Automatic garbage collection

### 7.3 Execution Model

Lira applications run in the Lira VM (`liravm`):

- Main fiber is the entry point
- Green threads are user-space scheduled
- Channels enable safe communication between fibers
- Cooperative scheduling with explicit yields

---

## 8. Document Map

This specification is organized into the following documents:

### Core Language

| Document                    | Description                              |
| --------------------------- | ---------------------------------------- |
| **00-lira-overview.md**     | This document - overview and quick start |
| **01-lexical-structure.md** | Tokens, keywords, literals, operators    |
| **02-type-system.md**       | Types, generics, inference rules         |
| **03-syntax-constructs.md** | Statements, expressions, control flow    |
| **04-concurrency.md**       | Fibers, channels, synchronization        |
| **05-module-system.md**     | Imports, exports, packages               |

### Virtual Machine

| Document                  | Description                         |
| ------------------------- | ----------------------------------- |
| **10-bytecode-format.md** | .lic file format specification      |
| **11-instruction-set.md** | Complete opcode reference           |
| **12-vm-runtime.md**      | Execution model, data structures    |
| **13-memory-model.md**    | Reference counting, cycle detection |

### Declarative UI

| Document               | Description                   |
| ---------------------- | ----------------------------- |
| **20-liui-format.md**  | .liui syntax and semantics    |
| **21-liui-widgets.md** | Widget catalog and properties |

### Implementation

| Document                        | Description             |
| ------------------------------- | ----------------------- |
| **30-standard-library.md**      | std.\* module reference |
| **40-compiler-architecture.md** | Compiler design         |
| **50-tooling.md**               | CLI tools reference     |

---

## Appendix A: Reserved Keywords

```
abstract    as          async       await       bool
break       case        catch       char        class
const       continue    default     else        enum
export      extends     false       finally     float
fn          for         if          impl        import
in          int         interface   is          let
loop        match       mod         mut         new
null        override    priv        pub         return
select      send        spawn       static      string
struct      super       this        throw       true
try         type        uint        var         void
when        while
```

---

## Appendix B: Operators by Precedence

| Precedence  | Operators                    | Associativity |
| ----------- | ---------------------------- | ------------- |
| 1 (highest) | `()` `[]` `.` `?.`           | Left          |
| 2           | `!` `-` (unary) `~`          | Right         |
| 3           | `**`                         | Right         |
| 4           | `*` `/` `%`                  | Left          |
| 5           | `+` `-`                      | Left          |
| 6           | `<<` `>>` `>>>`              | Left          |
| 7           | `<` `<=` `>` `>=`            | Left          |
| 8           | `==` `!=`                    | Left          |
| 9           | `&`                          | Left          |
| 10          | `^`                          | Left          |
| 11          | `\|`                         | Left          |
| 12          | `&&`                         | Left          |
| 13          | `\|\|`                       | Left          |
| 14          | `??` `?:`                    | Right         |
| 15          | `=` `+=` `-=` `*=` `/=` etc. | Right         |
| 16 (lowest) | `=>`                         | Right         |

---

## Appendix C: Version History

| Version     | Date    | Changes                     |
| ----------- | ------- | --------------------------- |
| 1.0.0-draft | 2025-01 | Initial specification draft |

---

_This document is part of the Lira Language Specification._

# Lira Testing Specification

## Document Information

| Property        | Value               |
| --------------- | ------------------- |
| **Document ID** | 51-testing          |
| **Version**     | 1.0.0-draft         |
| **Status**      | Draft Specification |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Test Framework (litest)](#2-test-framework-litest)
3. [Unit Testing](#3-unit-testing)
4. [Host-Mode VM](#4-host-mode-vm)
5. [Mocking Framework](#5-mocking-framework)
6. [GUI Testing](#6-gui-testing)
7. [Integration Testing](#7-integration-testing)
8. [System Testing](#8-system-testing)
9. [CI/CD Workflow](#9-cicd-workflow)
10. [Coverage & Profiling](#10-coverage--profiling)

---

## 1. Overview

### 1.1 Testing Philosophy

Lira testing follows a layered approach that enables rapid development iteration while ensuring full system compatibility:

```
┌─────────────────────────────────────────────────────────────────────┐
│                      TESTING PYRAMID                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│                         ┌─────────┐                                  │
│                        /  System   \         Full stack tests       │
│                       /   Tests     \        End-to-end, slower     │
│                      ┌───────────────┐                               │
│                     /  Integration    \      Host-mode VM + mocks   │
│                    /      Tests        \     Mocked syscalls        │
│                   ┌─────────────────────┐                            │
│                  /      Unit Tests       \   Pure bytecode          │
│                 /   (No OS Dependencies)  \  Fast, isolated         │
│                └───────────────────────────┘                         │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 Testing Modes

| Mode       | Environment | Speed | Coverage | Use Case                   |
| ---------- | ----------- | ----- | -------- | -------------------------- |
| **Unit**   | Any host    | < 1s  | Logic    | Pure functions, algorithms |
| **Host**   | macOS/Linux | 1-10s | I/O      | Integration with mocks     |
| **System** | Full stack  | 10s+  | Full     | End-to-end validation      |

### 1.3 What Runs Where

```
┌─────────────────────────────────────────────────────────────────────┐
│                    EXECUTION ENVIRONMENT                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐│
│  │              RUNS ON ANY HOST (No Mocking)                      ││
│  ├─────────────────────────────────────────────────────────────────┤│
│  │  • Arithmetic & math operations                                 ││
│  │  • String manipulation                                          ││
│  │  • Collections (List, Map, Set)                                 ││
│  │  • Control flow (if, match, loops)                              ││
│  │  • Pattern matching                                             ││
│  │  • Pure functions                                               ││
│  │  • Fiber spawning & channels (userspace)                        ││
│  │  • Synchronization primitives (Mutex, Semaphore)                ││
│  └─────────────────────────────────────────────────────────────────┘│
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐│
│  │              REQUIRES HOST-MODE MOCKING                         ││
│  ├─────────────────────────────────────────────────────────────────┤│
│  │  • File I/O (std.fs)                      → MockFS              ││
│  │  • Console I/O (print, read_line)         → MockIO              ││
│  │  • Time functions (sleep, get_time)       → MockClock           ││
│  │  • Process control (exit, spawn)          → MockProcess         ││
│  │  • Window creation (gui.Window)           → MockWindow          ││
│  │  • Event handling (gui events)            → MockEventQueue      ││
│  │  • Shared memory (surfaces)               → MockSharedMem       ││
│  └─────────────────────────────────────────────────────────────────┘│
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐│
│  │              REQUIRES FULL SYSTEM TEST                          ││
│  ├─────────────────────────────────────────────────────────────────┤│
│  │  • Full graphics rendering                                      ││
│  │  • Hardware input devices                                       ││
│  │  • Multi-process IPC                                            ││
│  │  • Real filesystem persistence                                  ││
│  │  • System-level features                                        ││
│  └─────────────────────────────────────────────────────────────────┘│
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.4 Development Workflow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    DEVELOPMENT LOOP                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│   1. Write code (.li/.liui)                                         │
│              │                                                       │
│              ▼                                                       │
│   2. lirac check              ─────── Type checking (instant)         │
│              │                                                       │
│              ▼                                                       │
│   3. lirac test               ─────── Unit tests (< 5s)               │
│              │                                                       │
│              ▼                                                       │
│   4. lirac test --integration ─────── Host-mode tests (< 30s)         │
│              │                                                       │
│              ▼                                                       │
│   5. lirac run --host         ─────── Interactive testing (host)      │
│              │                                                       │
│              ▼                                                       │
│   6. make test-qemu         ─────── System tests (CI/nightly)       │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 2. Test Framework (litest)

### 2.1 Test Attributes

```li
import testing

/// Basic test function
#[test]
fn test_addition() {
    assert_eq(2 + 2, 4)
}

/// Test with custom name
#[test(name = "Subtraction works correctly")]
fn test_subtraction() {
    assert_eq(5 - 3, 2)
}

/// Test expected to panic
#[test(should_panic)]
fn test_divide_by_zero() {
    let _ = 1 / 0
}

/// Test expected to panic with specific message
#[test(should_panic = "division by zero")]
fn test_divide_by_zero_message() {
    let _ = 1 / 0
}

/// Ignored test (skipped unless explicitly run)
#[test]
#[ignore]
fn test_slow_operation() {
    // This test is slow
}

/// Ignored with reason
#[test]
#[ignore(reason = "Requires network access")]
fn test_network_call() {
    // ...
}

/// Test with timeout
#[test(timeout = 5000)]  // 5 seconds
fn test_with_timeout() {
    // Must complete within 5 seconds
}

/// Async test (fiber-based)
#[test(async)]
fn test_async_channel() {
    let ch = Channel<int>.new()
    spawn { ch.send(42) }
    assert_eq(ch.receive(), 42)
}
```

### 2.2 Assertions

```li
import testing.{assert, assert_eq, assert_ne, assert_true, assert_false}
import testing.{assert_some, assert_none, assert_ok, assert_err}
import testing.{assert_matches, assert_panic}

// Basic assertions
assert(condition)                      // Assert condition is true
assert(condition, "Custom message")    // With message

// Equality
assert_eq(actual, expected)            // actual == expected
assert_ne(actual, expected)            // actual != expected

// Boolean
assert_true(value)                     // value == true
assert_false(value)                    // value == false

// Option<T>
assert_some(option)                    // option.is_some()
assert_some(option, expected)          // option == Some(expected)
assert_none(option)                    // option.is_none()

// Result<T, E>
assert_ok(result)                      // result.is_ok()
assert_ok(result, expected)            // result == Ok(expected)
assert_err(result)                     // result.is_err()
assert_err(result, expected)           // result == Err(expected)

// Pattern matching
assert_matches(value, Pattern::Variant { field: _ })

// Panic assertion
assert_panic(|| { panic("error") })
assert_panic(|| { panic("error") }, "error")  // Match message

// Approximate equality (floats)
assert_approx_eq(3.14159, 3.14, epsilon: 0.01)

// Collection assertions
assert_contains(list, element)
assert_empty(collection)
assert_len(collection, expected_len)
```

### 2.3 Test Organization

```li
// tests/math_test.li

import testing

/// Test module for math operations
mod math_tests {
    #[test]
    fn test_add() {
        assert_eq(add(2, 3), 5)
    }

    #[test]
    fn test_multiply() {
        assert_eq(multiply(3, 4), 12)
    }

    /// Nested module for edge cases
    mod edge_cases {
        #[test]
        fn test_add_zero() {
            assert_eq(add(0, 5), 5)
        }

        #[test]
        fn test_add_negative() {
            assert_eq(add(-1, 1), 0)
        }
    }
}
```

### 2.4 Setup and Teardown

```li
import testing

mod database_tests {
    var db: Database?

    /// Called before each test in this module
    #[before_each]
    fn setup() {
        db = Database.in_memory()
        db!.execute("CREATE TABLE users (id INT, name TEXT)")
    }

    /// Called after each test in this module
    #[after_each]
    fn teardown() {
        db!.close()
        db = null
    }

    /// Called once before all tests in this module
    #[before_all]
    fn global_setup() {
        // Initialize test environment
    }

    /// Called once after all tests in this module
    #[after_all]
    fn global_teardown() {
        // Cleanup test environment
    }

    #[test]
    fn test_insert() {
        db!.execute("INSERT INTO users VALUES (1, 'Alice')")
        let count = db!.query_one("SELECT COUNT(*) FROM users")
        assert_eq(count, 1)
    }
}
```

### 2.5 Test Runner Commands

```bash
# Run all tests
lirac test

# Run tests in specific file
lirac test tests/math_test.li

# Run tests matching pattern
lirac test --filter "math"
lirac test --filter "test_add*"

# Run specific test
lirac test --filter "math_tests::test_add"

# Run ignored tests
lirac test --include-ignored

# Run only ignored tests
lirac test --ignored

# Run tests with verbose output
lirac test --verbose

# Run tests and show stdout
lirac test --nocapture

# Run tests in parallel (default)
lirac test --jobs 4

# Run tests sequentially
lirac test --jobs 1

# Run integration tests only
lirac test --integration

# Run with specific test mode
lirac test --mode unit         # Unit tests only (default)
lirac test --mode host         # Host-mode with mocks
lirac test --mode system       # Full system (requires QEMU)
```

### 2.6 Test Output

```
$ lirac test

Running tests in my_project

   Compiling my_project v0.1.0
     Running tests/math_test.li

running 6 tests
test math_tests::test_add ... ok
test math_tests::test_multiply ... ok
test math_tests::edge_cases::test_add_zero ... ok
test math_tests::edge_cases::test_add_negative ... ok
test string_tests::test_concat ... ok
test string_tests::test_split ... FAILED

failures:

---- string_tests::test_split ----
thread 'test' panicked at 'assertion failed: `(left == right)`
  left: `["a", "b", "c"]`,
 right: `["a", "b"]`', tests/string_test.li:15:5

failures:
    string_tests::test_split

test result: FAILED. 5 passed; 1 failed; 0 ignored; 0 measured

error: test failed
```

---

## 3. Unit Testing

### 3.1 Pure Function Testing

Pure functions (no I/O, no side effects) can be tested directly without any mocking:

```li
// src/math.li
pub fn factorial(n: int) -> int {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

pub fn fibonacci(n: int) -> int {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2)
    }
}

// tests/math_test.li
import testing
import src.math.{factorial, fibonacci}

#[test]
fn test_factorial() {
    assert_eq(factorial(0), 1)
    assert_eq(factorial(1), 1)
    assert_eq(factorial(5), 120)
    assert_eq(factorial(10), 3628800)
}

#[test]
fn test_fibonacci() {
    assert_eq(fibonacci(0), 0)
    assert_eq(fibonacci(1), 1)
    assert_eq(fibonacci(10), 55)
}
```

### 3.2 Collection Testing

```li
import testing

#[test]
fn test_list_operations() {
    var list = List<int>.new()

    list.push(1)
    list.push(2)
    list.push(3)

    assert_eq(list.len(), 3)
    assert_eq(list[0], 1)
    assert_eq(list.pop(), Some(3))
    assert_eq(list.len(), 2)
}

#[test]
fn test_map_operations() {
    var map = Map<string, int>.new()

    map.insert("one", 1)
    map.insert("two", 2)

    assert_eq(map.get("one"), Some(1))
    assert_eq(map.get("three"), None)
    assert(map.contains_key("two"))
}
```

### 3.3 Fiber and Channel Testing

Fibers and channels are pure userspace constructs and require no mocking:

```li
import testing

#[test(async)]
fn test_unbuffered_channel() {
    let ch = Channel<int>.new()

    spawn {
        ch.send(42)
    }

    let value = ch.receive()
    assert_eq(value, 42)
}

#[test(async)]
fn test_buffered_channel() {
    let ch = Channel<int>.buffered(2)

    // Can send without blocking (buffer has space)
    ch.send(1)
    ch.send(2)

    assert_eq(ch.receive(), 1)
    assert_eq(ch.receive(), 2)
}

#[test(async)]
fn test_select() {
    let ch1 = Channel<int>.new()
    let ch2 = Channel<string>.new()

    spawn { ch1.send(42) }
    spawn { ch2.send("hello") }

    var got_int = false
    var got_str = false

    for _ in 0..2 {
        select {
            n = <-ch1 => { got_int = true; assert_eq(n, 42) },
            s = <-ch2 => { got_str = true; assert_eq(s, "hello") },
        }
    }

    assert(got_int && got_str)
}

#[test(async)]
fn test_worker_pool() {
    let jobs = Channel<int>.buffered(10)
    let results = Channel<int>.buffered(10)

    // Spawn 3 workers
    for _ in 0..3 {
        spawn {
            for job in jobs {
                results.send(job * 2)
            }
        }
    }

    // Send jobs
    for i in 1..=5 {
        jobs.send(i)
    }
    jobs.close()

    // Collect results
    var sum = 0
    for _ in 0..5 {
        sum += results.receive()
    }

    assert_eq(sum, 2 + 4 + 6 + 8 + 10)
}
```

### 3.4 Property-Based Testing

```li
import testing
import testing.property.{forall, Gen}

#[test]
fn test_list_reverse_twice() {
    forall(Gen.list(Gen.int())) |list| {
        let reversed = list.reverse().reverse()
        assert_eq(reversed, list)
    }
}

#[test]
fn test_sort_is_sorted() {
    forall(Gen.list(Gen.int())) |list| {
        let sorted = list.sorted()
        for i in 0..(sorted.len() - 1) {
            assert(sorted[i] <= sorted[i + 1])
        }
    }
}

#[test]
fn test_string_concat_length() {
    forall(Gen.string(), Gen.string()) |a, b| {
        assert_eq((a + b).len(), a.len() + b.len())
    }
}
```

---

## 4. Host-Mode VM

### 4.1 Overview

The host-mode VM (`liravm --host`) allows running Lira applications on macOS/Linux by intercepting syscalls and providing mock implementations:

```
┌─────────────────────────────────────────────────────────────────────┐
│                       HOST-MODE ARCHITECTURE                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│   ┌─────────────────────────────────────────────────────────────┐   │
│   │                    Lira Application                       │   │
│   └─────────────────────────────────────────────────────────────┘   │
│                              │                                       │
│                              ▼                                       │
│   ┌─────────────────────────────────────────────────────────────┐   │
│   │                   Lira Standard Library                   │   │
│   │              (std.fs, std.io, gui.*, etc.)                   │   │
│   └─────────────────────────────────────────────────────────────┘   │
│                              │                                       │
│                              ▼                                       │
│   ┌─────────────────────────────────────────────────────────────┐   │
│   │                    SYSCALL Instruction                       │   │
│   │                     (Opcode 0xE8)                            │   │
│   └─────────────────────────────────────────────────────────────┘   │
│                              │                                       │
│            ┌─────────────────┴─────────────────┐                     │
│            │                                   │                     │
│            ▼                                   ▼                     │
│   ┌─────────────────┐               ┌─────────────────┐             │
│   │   Native OS     │               │   Host-Mode VM  │             │
│   │     Layer       │               │   Syscall Layer │             │
│   │                 │               │                 │             │
│   │  • Real FS      │               │  • MockFS       │             │
│   │  • Real GUI     │               │  • MockWindow   │             │
│   │  • Real IPC     │               │  • MockIPC      │             │
│   └─────────────────┘               └─────────────────┘             │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 4.2 Syscall Interception

When running in host mode, the VM intercepts `SYSCALL` instructions and routes them to mock handlers:

```rust
// VM syscall dispatch (conceptual)
fn execute_syscall(&mut self, syscall_num: u16) -> Value {
    if self.host_mode {
        self.host_syscall_handler.handle(syscall_num, &self.stack)
    } else {
        // Real native syscall
        unsafe { syscall(syscall_num, args...) }
    }
}
```

### 4.3 Syscall Mapping

| Syscall              | Number | Host-Mode Behavior                 |
| -------------------- | ------ | ---------------------------------- |
| `sys_exit`           | 0x0000 | Record exit code, terminate fiber  |
| `sys_fork`           | 0x0001 | Not supported in host mode (error) |
| `sys_read`           | 0x0022 | Read from MockFS file handle       |
| `sys_write`          | 0x0023 | Write to MockFS or capture stdout  |
| `sys_open`           | 0x0020 | Open MockFS file, return handle    |
| `sys_close`          | 0x0021 | Close MockFS handle                |
| `sys_get_time`       | 0x0080 | Return MockClock time              |
| `sys_sleep`          | 0x0081 | Advance MockClock, no real sleep   |
| `sys_create_window`  | 0x00A8 | Create MockWindow, return handle   |
| `sys_destroy_window` | 0x00A9 | Destroy MockWindow                 |
| `sys_get_event`      | 0x00AD | Return event from MockEventQueue   |
| `sys_send`           | 0x0050 | Route to MockIPC                   |
| `sys_receive`        | 0x0051 | Route to MockIPC                   |

### 4.4 Command Line Usage

```bash
# Run application in host mode
liravm --host app.lic

# Run with mock filesystem initialized from directory
liravm --host --mock-fs ./test-data app.lic

# Run with event file (pre-recorded input events)
liravm --host --events events.json app.lic

# Run headless (no window creation)
liravm --host --headless app.lic

# Run with mock time starting at specific timestamp
liravm --host --mock-time "2024-01-01T00:00:00Z" app.lic

# Combine options
liravm --host --headless --mock-fs ./fixtures app.lic
```

### 4.5 Host-Mode Limitations

| Feature          | Supported | Notes                                  |
| ---------------- | --------- | -------------------------------------- |
| File I/O         | ✓         | Via MockFS (in-memory or real host FS) |
| Console I/O      | ✓         | Via host stdin/stdout                  |
| Time/Sleep       | ✓         | Via MockClock (controllable)           |
| GUI Windows      | ✓         | Via MockWindow (headless)              |
| GUI Events       | ✓         | Via MockEventQueue (synthetic)         |
| Rendering        | Partial   | Capture pixel buffer, no display       |
| Process Fork     | ✗         | Not supported                          |
| Multi-Process    | ✗         | Single process only                    |
| Hardware Drivers | ✗         | Not applicable                         |
| Raw Syscalls     | ✗         | All intercepted                        |

---

## 5. Mocking Framework

### 5.1 MockFS (Filesystem)

```li
import testing.mock.MockFS

#[test]
fn test_file_read() {
    // Create mock filesystem
    let fs = MockFS.new()

    // Pre-populate with test data
    fs.write_file("/test.txt", "Hello, World!")
    fs.create_dir("/data")
    fs.write_file("/data/config.json", "{\"key\": \"value\"}")

    // Install mock (replaces syscalls for this test)
    testing.install_mock(fs)
    defer { testing.remove_mock(fs) }

    // Now std.fs operations use the mock
    let content = File.read_string("/test.txt")
    assert_eq(content, Ok("Hello, World!"))

    let files = Directory.list("/data")
    assert_eq(files.unwrap().len(), 1)
}

#[test]
fn test_file_write() {
    let fs = MockFS.new()
    testing.install_mock(fs)
    defer { testing.remove_mock(fs) }

    // Write file
    let file = File.create("/output.txt").unwrap()
    file.write("Test output")
    file.close()

    // Verify using mock
    assert_eq(fs.read_file("/output.txt"), "Test output")
}
```

### 5.2 MockClock (Time)

```li
import testing.mock.MockClock

#[test]
fn test_timeout() {
    let clock = MockClock.new(Instant.from_secs(0))
    testing.install_mock(clock)
    defer { testing.remove_mock(clock) }

    let start = Instant.now()

    // Advance mock time by 5 seconds (no real delay)
    clock.advance(Duration.seconds(5))

    let elapsed = Instant.now().duration_since(start)
    assert_eq(elapsed.as_secs(), 5)
}

#[test]
fn test_sleep_is_instant() {
    let clock = MockClock.new(Instant.from_secs(0))
    clock.set_auto_advance(true)  // Auto-advance on sleep
    testing.install_mock(clock)
    defer { testing.remove_mock(clock) }

    let start = Instant.now()

    // This returns immediately but advances mock clock
    std.time.sleep(Duration.seconds(10))

    let elapsed = Instant.now().duration_since(start)
    assert_eq(elapsed.as_secs(), 10)
}
```

### 5.3 MockWindow (GUI)

```li
import testing.mock.{MockWindow, MockEventQueue}

#[test]
fn test_window_creation() {
    let windows = MockWindow.manager()
    testing.install_mock(windows)
    defer { testing.remove_mock(windows) }

    // Create window (uses mock)
    let window = Window.new(title: "Test", width: 800, height: 600)

    // Verify window was created
    assert_eq(windows.window_count(), 1)
    assert_eq(windows.get_title(window.handle()), "Test")
}

#[test]
fn test_button_click() {
    let windows = MockWindow.manager()
    let events = MockEventQueue.new()
    testing.install_mock(windows)
    testing.install_mock(events)
    defer {
        testing.remove_mock(windows)
        testing.remove_mock(events)
    }

    var clicked = false

    let window = Window.new(title: "Test", width: 400, height: 300)
    let button = Button.new("Click Me")
    button.on_click(|| { clicked = true })
    window.add(button)
    window.show()

    // Inject click event at button position
    events.push(MouseEvent {
        type: MouseDown,
        x: 200,  // Center of button
        y: 150,
        button: Left,
    })
    events.push(MouseEvent {
        type: MouseUp,
        x: 200,
        y: 150,
        button: Left,
    })

    // Process events
    window.process_events()

    assert(clicked)
}
```

### 5.4 MockIO (Console)

```li
import testing.mock.MockIO

#[test]
fn test_console_output() {
    let io = MockIO.new()
    testing.install_mock(io)
    defer { testing.remove_mock(io) }

    print("Hello, ")
    print("World!")
    println("")

    assert_eq(io.stdout(), "Hello, World!\n")
}

#[test]
fn test_console_input() {
    let io = MockIO.new()
    io.set_stdin("Alice\n42\n")
    testing.install_mock(io)
    defer { testing.remove_mock(io) }

    let name = read_line()
    let age = read_line().parse::<int>()

    assert_eq(name, "Alice")
    assert_eq(age, Ok(42))
}
```

### 5.5 Mock Context

For tests requiring multiple mocks, use `MockContext`:

```li
import testing.mock.{MockContext, MockFS, MockClock, MockIO}

#[test]
fn test_with_context() {
    let ctx = MockContext.new()
        .with_fs(MockFS.from_dir("./fixtures"))
        .with_clock(MockClock.at("2024-06-15T10:00:00Z"))
        .with_io(MockIO.new())
        .build()

    testing.run_with_context(ctx) {
        // All operations in this block use mocks
        let content = File.read_string("/fixtures/test.txt")
        let now = DateTime.now()
        print("Testing...")

        assert(content.is_ok())
        assert_eq(now.year(), 2024)
        assert_eq(ctx.io().stdout(), "Testing...")
    }
}
```

---

## 6. GUI Testing

### 6.1 Widget Unit Testing

Test individual widgets without a window:

```li
import testing
import gui.widgets.Button

#[test]
fn test_button_properties() {
    let button = Button.new("Submit")

    assert_eq(button.text(), "Submit")
    assert(button.enabled())
    assert(button.visible())

    button.set_enabled(false)
    assert(!button.enabled())
}

#[test]
fn test_button_click_callback() {
    let button = Button.new("Click")
    var click_count = 0

    button.on_click(|| { click_count += 1 })

    // Simulate clicks
    button.trigger_click()
    button.trigger_click()

    assert_eq(click_count, 2)
}
```

### 6.2 Layout Testing

Test layout calculations without rendering:

```li
import testing
import gui.widgets.{VBox, Label, Button}
import gui.layout.{Size, Rect}

#[test]
fn test_vbox_layout() {
    let container = VBox.new()
    container.set_spacing(10)
    container.set_padding(20)

    container.add(Label.new("Title"))
    container.add(Button.new("OK"))

    // Measure with constraints
    let size = container.measure(Size { width: 200, height: 400 })

    // Perform layout
    container.layout(Rect { x: 0, y: 0, width: 200, height: size.height })

    // Verify child positions
    let title = container.children()[0]
    let button = container.children()[1]

    assert_eq(title.bounds().x, 20)  // Padding
    assert_eq(title.bounds().y, 20)  // Padding
    assert_eq(button.bounds().y, title.bounds().bottom() + 10)  // Spacing
}
```

### 6.3 Event Simulation

```li
import testing
import testing.mock.{MockWindow, MockEventQueue}
import gui.events.{MouseEvent, KeyEvent, Modifiers}

#[test]
fn test_keyboard_input() {
    let events = MockEventQueue.new()
    testing.install_mock(events)
    defer { testing.remove_mock(events) }

    let input = TextField.new()
    var final_text = ""
    input.on_change(|text| { final_text = text })

    // Simulate typing "Hello"
    for char in "Hello".chars() {
        events.push(KeyEvent {
            type: KeyDown,
            key: Key.from_char(char),
            modifiers: Modifiers.none(),
        })
        events.push(KeyEvent {
            type: KeyUp,
            key: Key.from_char(char),
            modifiers: Modifiers.none(),
        })
    }

    // Process all events
    while let Some(event) = events.poll() {
        input.handle_event(event)
    }

    assert_eq(input.text(), "Hello")
    assert_eq(final_text, "Hello")
}

#[test]
fn test_modifier_keys() {
    let events = MockEventQueue.new()
    testing.install_mock(events)
    defer { testing.remove_mock(events) }

    let input = TextField.new()
    input.set_text("Hello, World!")
    input.select_all()

    var copied = false
    input.on_copy(|| { copied = true })

    // Simulate Ctrl+C
    events.push(KeyEvent {
        type: KeyDown,
        key: Key.C,
        modifiers: Modifiers.ctrl(),
    })

    input.handle_event(events.poll().unwrap())

    assert(copied)
}
```

### 6.4 Snapshot Testing

Capture and compare rendered output:

```li
import testing
import testing.snapshot.{capture_widget, assert_snapshot}

#[test]
fn test_button_render() {
    let button = Button.new("Click Me")
    button.set_style(ButtonStyle.primary())

    // Capture rendered pixels
    let snapshot = capture_widget(button, Size { width: 120, height: 40 })

    // Compare against stored snapshot
    // Creates snapshot on first run, compares on subsequent runs
    assert_snapshot(snapshot, "button_primary")
}

#[test]
fn test_form_layout() {
    let form = VBox.new()
    form.add(Label.new("Username:"))
    form.add(TextField.new())
    form.add(Label.new("Password:"))
    form.add(TextField.new())
    form.add(Button.new("Login"))

    let snapshot = capture_widget(form, Size { width: 300, height: 200 })
    assert_snapshot(snapshot, "login_form")
}
```

Snapshot management:

```bash
# Update all snapshots
lirac test --update-snapshots

# Update specific snapshot
lirac test --update-snapshots --filter "test_button_render"

# Review snapshot differences
lirac test --review-snapshots
```

### 6.5 Lira UI Component Testing

```li
import testing
import testing.liui.{render_component, query}

#[test]
fn test_counter_component() {
    // Load and render component
    let component = render_component("src/ui/counter.liui")

    // Query elements
    let label = query.by_id(component, "count-label")
    let increment = query.by_id(component, "increment-btn")

    assert_eq(label.text(), "Count: 0")

    // Simulate click
    increment.trigger_click()

    assert_eq(label.text(), "Count: 1")
}

#[test]
fn test_reactive_binding() {
    // Create state
    let state = #{ count: 0, name: "Alice" }

    // Render with state
    let component = render_component("src/ui/greeting.liui", state)

    let greeting = query.by_id(component, "greeting")
    assert_eq(greeting.text(), "Hello, Alice!")

    // Update state
    state.name = "Bob"
    component.update()

    assert_eq(greeting.text(), "Hello, Bob!")
}
```

---

## 7. Integration Testing

### 7.1 Integration Test Setup

Integration tests run in host mode with mocks:

```li
// tests/integration/file_processing_test.li

import testing
import testing.mock.{MockFS, MockIO}

#[test(integration)]
fn test_csv_processor() {
    // Setup mock filesystem with test data
    let fs = MockFS.new()
    fs.write_file("/input.csv", "name,age\nAlice,30\nBob,25")

    let io = MockIO.new()

    testing.install_mock(fs)
    testing.install_mock(io)
    defer {
        testing.remove_mock(fs)
        testing.remove_mock(io)
    }

    // Run the application logic
    process_csv("/input.csv", "/output.json")

    // Verify output
    let output = fs.read_file("/output.json")
    assert(output.contains("Alice"))
    assert(output.contains("30"))

    // Verify console output
    assert(io.stdout().contains("Processed 2 records"))
}
```

### 7.2 GUI Integration Tests

```li
// tests/integration/app_flow_test.li

import testing
import testing.mock.{MockContext, MockFS, MockWindow, MockEventQueue}

#[test(integration)]
fn test_full_login_flow() {
    let ctx = MockContext.new()
        .with_fs(MockFS.new())
        .with_window(MockWindow.manager())
        .with_events(MockEventQueue.new())
        .build()

    testing.run_with_context(ctx) {
        // Start application
        let app = App.new("TestApp")
        let main_window = app.main_window()

        // Find login form elements
        let username_field = main_window.find_by_id("username")
        let password_field = main_window.find_by_id("password")
        let login_button = main_window.find_by_id("login-btn")

        // Enter credentials
        username_field.set_text("testuser")
        password_field.set_text("password123")

        // Click login
        login_button.trigger_click()
        app.process_events()

        // Verify navigation to dashboard
        let current_view = main_window.current_view()
        assert_eq(current_view.id(), "dashboard")

        // Verify welcome message
        let welcome = main_window.find_by_id("welcome-msg")
        assert(welcome.text().contains("testuser"))
    }
}
```

### 7.3 Async Integration Tests

```li
#[test(integration, async)]
fn test_background_job_processing() {
    let fs = MockFS.new()
    fs.write_file("/jobs/job1.txt", "task1")
    fs.write_file("/jobs/job2.txt", "task2")

    testing.install_mock(fs)
    defer { testing.remove_mock(fs) }

    let results = Channel<Result<string, Error>>.new()

    // Start worker
    spawn {
        let worker = JobWorker.new("/jobs")
        worker.process_all(results)
    }

    // Collect results
    var completed = 0
    for _ in 0..2 {
        let result = results.receive()
        assert(result.is_ok())
        completed += 1
    }

    assert_eq(completed, 2)
}
```

---

## 8. System Testing

### 8.1 QEMU Test Environment

System tests run in a full test environment:

```makefile
# Makefile

test-system:
	$(MAKE) -C kernel
	$(MAKE) -C livm
	./scripts/build-test-image.sh
	./scripts/run-qemu-tests.sh

run-qemu-tests.sh:
	#!/bin/bash
	qemu-system-x86_64 \
		-kernel build/test-kernel \
		-initrd build/test-initrd.img \
		-append "test_mode=1" \
		-nographic \
		-serial mon:stdio \
		-no-reboot \
		| tee test-output.log

	# Check for test pass/fail in output
	grep -q "ALL TESTS PASSED" test-output.log
```

### 8.2 System Test Structure

```li
// tests/system/full_app_test.li

import testing

#[test(system)]
fn test_window_creation() {
    // This runs in system test mode
    let window = Window.new(
        title: "System Test",
        width: 640,
        height: 480,
    )

    assert(window.is_visible())

    // Draw something
    window.fill_rect(Rect { x: 10, y: 10, width: 100, height: 100 }, Color.RED)
    window.present()

    // Wait for vsync
    window.wait_vsync()

    window.close()
}

#[test(system)]
fn test_file_persistence() {
    // Write to real filesystem
    let file = File.create("/tmp/test_output.txt").unwrap()
    file.write_string("Hello from system test!")
    file.close()

    // Read back
    let content = File.read_string("/tmp/test_output.txt").unwrap()
    assert_eq(content, "Hello from system test!")

    // Cleanup
    File.delete("/tmp/test_output.txt")
}
```

### 8.3 Automated Screenshot Verification

```li
#[test(system)]
fn test_ui_rendering() {
    let window = Window.new(title: "UI Test", width: 400, height: 300)

    // Render UI
    let label = Label.new("Hello, World!")
    let button = Button.new("Click Me")

    window.add(VBox.new([label, button]))
    window.present()

    // Capture framebuffer
    let screenshot = window.capture_screenshot()

    // Save for manual review or compare
    screenshot.save("/tmp/ui_test_screenshot.png")

    // Or compare against reference
    let reference = Image.load("/tests/references/ui_test.png")
    assert(screenshot.matches(reference, tolerance: 0.01))
}
```

---

## 9. CI/CD Workflow

### 9.1 CI Pipeline Configuration

```yaml
# .github/workflows/ci.yml

name: Lira CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  check:
    name: Type Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Lira toolchain
        run: ./scripts/install-toolchain.sh
      - name: Type check
        run: lirac check

  unit-tests:
    name: Unit Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Lira toolchain
        run: ./scripts/install-toolchain.sh
      - name: Run unit tests
        run: lirac test --mode unit
      - name: Upload coverage
        run: lirac test --coverage --output coverage.lcov

  integration-tests:
    name: Integration Tests
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - name: Install Lira toolchain
        run: ./scripts/install-toolchain.sh
      - name: Run integration tests
        run: lirac test --mode host --integration

  system-tests:
    name: System Tests
    runs-on: ubuntu-latest
    needs: integration-tests
    steps:
      - uses: actions/checkout@v4
      - name: Install dependencies
        run: |
          sudo apt-get install qemu-system-x86
          ./scripts/install-toolchain.sh
      - name: Build project
        run: make all
      - name: Run system tests
        run: make test-system
        timeout-minutes: 10

  format-check:
    name: Format Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Lira toolchain
        run: ./scripts/install-toolchain.sh
      - name: Check formatting
        run: lifmt --check .
```

### 9.2 Local CI Script

```bash
#!/bin/bash
# scripts/ci-local.sh - Run full CI locally

set -e

echo "=== Type Check ==="
lirac check

echo "=== Format Check ==="
lifmt --check .

echo "=== Unit Tests ==="
lirac test --mode unit

echo "=== Integration Tests ==="
lirac test --mode host --integration

echo "=== Build ==="
lirac build --release

echo "=== All checks passed! ==="
```

### 9.3 Pre-commit Hook

```bash
#!/bin/bash
# .git/hooks/pre-commit

# Format check
if ! lifmt --check .; then
    echo "Error: Code is not formatted. Run 'lifmt -w .'"
    exit 1
fi

# Type check
if ! lirac check; then
    echo "Error: Type check failed"
    exit 1
fi

# Quick unit tests
if ! lirac test --mode unit --filter "quick"; then
    echo "Error: Quick tests failed"
    exit 1
fi
```

---

## 10. Coverage & Profiling

### 10.1 Code Coverage

```bash
# Generate coverage report
lirac test --coverage

# Output formats
lirac test --coverage --format html -o coverage/
lirac test --coverage --format lcov -o coverage.lcov
lirac test --coverage --format json -o coverage.json

# View coverage summary
lirac test --coverage --summary
#   src/math.li         95.2%
#   src/parser.li       87.3%
#   src/vm/executor.li  72.1%
#   ─────────────────────────
#   Total               84.8%

# Coverage thresholds (fail if below)
lirac test --coverage --min-coverage 80
```

### 10.2 Coverage Report

```
$ lirac test --coverage --summary

Code Coverage Report
====================

File                        Lines    Covered   Percent
────────────────────────────────────────────────────────
src/lib.li                  150      142       94.7%
src/parser/lexer.li         320      298       93.1%
src/parser/parser.li        450      387       86.0%
src/vm/executor.li          280      195       69.6%
src/vm/gc.li                120       89       74.2%
src/gui/widgets/button.li    85       82       96.5%
src/gui/widgets/label.li     45       45      100.0%
────────────────────────────────────────────────────────
Total                      1450     1238       85.4%

Uncovered lines:
  src/vm/executor.li:142-148  - Error handling branch
  src/vm/executor.li:201-210  - Rare opcode path
  src/vm/gc.li:89-95          - Cycle detection edge case
```

### 10.3 Test Profiling

```bash
# Profile test execution time
lirac test --profile

# Output:
#   Slowest tests:
#     test_large_file_processing  2.34s
#     test_complex_layout         1.12s
#     test_channel_stress         0.89s
#
#   Total: 4.82s (47 tests)

# Profile specific test
lirac test --filter "test_large_file" --profile --flame-graph profile.svg
```

### 10.4 Benchmarking

```li
import testing.bench.{benchmark, Bencher}

#[bench]
fn bench_fibonacci(b: &mut Bencher) {
    b.iter(|| {
        fibonacci(20)
    })
}

#[bench]
fn bench_list_push(b: &mut Bencher) {
    b.iter(|| {
        var list = List<int>.new()
        for i in 0..1000 {
            list.push(i)
        }
    })
}

#[bench]
fn bench_map_lookup(b: &mut Bencher) {
    let map = Map<string, int>.new()
    for i in 0..1000 {
        map.insert(i.to_string(), i)
    }

    b.iter(|| {
        for i in 0..1000 {
            let _ = map.get(i.to_string())
        }
    })
}
```

Running benchmarks:

```bash
$ lirac bench

running 3 benchmarks
bench_fibonacci      ... bench:    12,450 ns/iter (+/- 523)
bench_list_push      ... bench:    45,230 ns/iter (+/- 1,024)
bench_map_lookup     ... bench:   234,120 ns/iter (+/- 5,432)

test result: ok. 0 failed; 3 measured
```

---

## Appendix A: Test Directory Structure

```
project/
├── src/
│   ├── lib.li
│   ├── main.li
│   └── ...
├── ui/
│   └── *.liui
├── tests/
│   ├── unit/              # Unit tests (no mocks)
│   │   ├── math_test.li
│   │   └── parser_test.li
│   ├── integration/       # Integration tests (host-mode)
│   │   ├── file_io_test.li
│   │   └── gui_flow_test.li
│   ├── system/            # System tests (QEMU)
│   │   └── full_app_test.li
│   ├── fixtures/          # Test data files
│   │   ├── sample.csv
│   │   └── config.json
│   └── snapshots/         # UI snapshots
│       ├── button_primary.png
│       └── login_form.png
├── benches/               # Benchmarks
│   └── perf_bench.li
└── li.toml
```

---

## Appendix B: Test Configuration

```toml
# li.toml

[package]
name = "my_app"
version = "1.0.0"

[test]
# Default test mode
mode = "unit"

# Test timeout (ms)
timeout = 30000

# Parallel test execution
parallel = true
jobs = 4

# Coverage settings
[test.coverage]
enabled = true
min_coverage = 80
exclude = ["tests/*", "benches/*"]

# Integration test settings
[test.integration]
mock_fs_root = "tests/fixtures"
headless = true

# System test settings
[test.system]
qemu_memory = "512M"
timeout = 300000  # 5 minutes

# Snapshot settings
[test.snapshots]
directory = "tests/snapshots"
update_on_missing = true
```

---

## Appendix C: Mock API Reference

### MockFS

```li
class MockFS {
    /// Create empty mock filesystem
    static fn new() -> MockFS

    /// Create from host directory (copies files)
    static fn from_dir(path: string) -> MockFS

    /// Write file content
    fn write_file(&mut self, path: string, content: string)

    /// Read file content
    fn read_file(&self, path: string) -> string

    /// Check if file exists
    fn exists(&self, path: string) -> bool

    /// Create directory
    fn create_dir(&mut self, path: string)

    /// List directory contents
    fn list_dir(&self, path: string) -> List<string>

    /// Delete file or directory
    fn delete(&mut self, path: string)

    /// Get all operations performed
    fn operations(&self) -> List<FSOperation>
}
```

### MockClock

```li
class MockClock {
    /// Create at specific time
    static fn new(time: Instant) -> MockClock

    /// Create at ISO timestamp
    static fn at(iso: string) -> MockClock

    /// Advance time by duration
    fn advance(&mut self, duration: Duration)

    /// Set current time
    fn set(&mut self, time: Instant)

    /// Auto-advance on sleep calls
    fn set_auto_advance(&mut self, enabled: bool)

    /// Get current mock time
    fn now(&self) -> Instant
}
```

### MockWindow

```li
class MockWindowManager {
    /// Create mock window manager
    static fn new() -> MockWindowManager

    /// Get number of windows created
    fn window_count(&self) -> int

    /// Get window by handle
    fn get_window(&self, handle: WindowHandle) -> MockWindow?

    /// Get window title
    fn get_title(&self, handle: WindowHandle) -> string

    /// Get window size
    fn get_size(&self, handle: WindowHandle) -> Size

    /// Capture window contents
    fn capture(&self, handle: WindowHandle) -> PixelBuffer
}
```

### MockEventQueue

```li
class MockEventQueue {
    /// Create empty event queue
    static fn new() -> MockEventQueue

    /// Push event to queue
    fn push(&mut self, event: Event)

    /// Push mouse event
    fn push_mouse(&mut self, type: MouseEventType, x: int, y: int)

    /// Push key event
    fn push_key(&mut self, type: KeyEventType, key: Key, modifiers: Modifiers)

    /// Poll next event
    fn poll(&mut self) -> Event?

    /// Clear all events
    fn clear(&mut self)

    /// Load events from JSON file
    static fn from_file(path: string) -> MockEventQueue
}
```

---

_This document is part of the Lira Language Specification._

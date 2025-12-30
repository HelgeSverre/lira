# Lira Concurrency Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 04-concurrency |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **Prerequisites** | 00-03 (core language specs) |

---

## Table of Contents

1. [Concurrency Model](#1-concurrency-model)
2. [Fibers (Green Threads)](#2-fibers-green-threads)
3. [Channels](#3-channels)
4. [Select Statement](#4-select-statement)
5. [Synchronization Primitives](#5-synchronization-primitives)
6. [Async/Await](#6-asyncawait)
7. [Concurrency Patterns](#7-concurrency-patterns)

---

## 1. Concurrency Model

### 1.1 Overview

Lira uses **green threads (fibers)** with **channel-based communication** as its primary concurrency model. This design is inspired by Go's goroutines and CSP (Communicating Sequential Processes).

Key characteristics:
- **Lightweight**: Fibers use ~8KB stack (vs ~1MB for OS threads)
- **Cooperatively scheduled**: Fibers yield at specific points
- **No shared mutable state**: Communication via channels
- **M:N threading**: Many fibers mapped to fewer OS threads

### 1.2 Design Philosophy

Lira follows the principle:

> **Do not communicate by sharing memory; share memory by communicating.**

Fibers communicate through channels rather than shared mutable state, eliminating data races by design.

### 1.3 Scheduling Model

```
┌─────────────────────────────────────────────────────────────┐
│                     Lira Process                          │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   Fiber Scheduler                    │    │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │    │
│  │  │ Fiber 1 │ │ Fiber 2 │ │ Fiber 3 │ │ Fiber N │   │    │
│  │  │ (Ready) │ │(Running)│ │(Blocked)│ │ (Ready) │   │    │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘   │    │
│  │                    ↓                                │    │
│  │             Ready Queue                             │    │
│  └─────────────────────────────────────────────────────┘    │
│                         ↓                                    │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              OS Thread (host process)                │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

Fibers yield control at:
- `spawn` (creating new fiber)
- Channel `send` / `receive` (blocking)
- `select` statement
- Explicit `yield` call
- System calls (I/O operations)

---

## 2. Fibers (Green Threads)

### 2.1 Spawn Expression

The `spawn` keyword creates a new fiber:

```li
spawn {
    // Code runs concurrently
    print("Hello from fiber!")
}
```

#### Spawn Returns Fiber Handle

```li
let handle = spawn {
    heavy_computation()
    return 42
}

// Wait for completion
let result = handle.join()  // Blocks until fiber completes
print(result)  // 42
```

#### Spawn with Captured Variables

```li
let data = load_data()

spawn {
    // 'data' is captured by reference
    process(data)
}

// Explicit capture list
var counter = 0
spawn [counter] {
    // 'counter' is captured by copy
    print(counter)
}
```

### 2.2 Fiber Type

```li
// Fiber handle type
type Fiber<T> = struct {
    id: FiberId
    state: FiberState
}

enum FiberState {
    Ready,
    Running,
    Blocked,
    Completed,
    Failed,
}
```

### 2.3 Fiber Operations

```li
let fiber = spawn {
    do_work()
    return result
}

// Join: wait for completion and get result
let result: T = fiber.join()

// Join with timeout
let result: T? = fiber.join_timeout(Duration.seconds(5))

// Check state
if fiber.is_completed() {
    let result = fiber.result()
}

// Cancel (cooperative)
fiber.cancel()
if fiber.is_cancelled() {
    // Handle cancellation
}
```

### 2.4 Named Fibers

```li
// Named fiber for debugging
spawn "data-processor" {
    process_data()
}

spawn "http-handler-${request_id}" {
    handle_request(request)
}
```

### 2.5 Fiber-Local Storage

```li
// Thread-local equivalent for fibers
fiber_local var request_id: string = ""
fiber_local var logger: Logger = Logger.default()

spawn {
    request_id = generate_id()
    logger = Logger.new(request_id)
    // Use fiber-local values
}
```

### 2.6 Yield

Explicitly yield control to the scheduler:

```li
fn long_computation() {
    for i in 0..1_000_000 {
        if i % 1000 == 0 {
            yield  // Let other fibers run
        }
        compute(i)
    }
}
```

---

## 3. Channels

### 3.1 Channel Creation

```li
// Unbuffered channel (rendezvous)
let ch = Channel<int>.new()

// Buffered channel
let ch = Channel<int>.buffered(10)  // Buffer size 10

// Typed channel
let messages: Channel<Message> = Channel.new()
```

### 3.2 Send and Receive

#### Send

```li
let ch = Channel<int>.new()

// Send blocks until receiver is ready (unbuffered)
ch.send(42)

// Or using arrow syntax
42 -> ch

// Try send (non-blocking)
let sent = ch.try_send(42)  // Returns bool

// Send with timeout
let sent = ch.send_timeout(42, Duration.seconds(1))
```

#### Receive

```li
// Receive blocks until value available
let value = ch.receive()

// Or using arrow syntax
let value = <-ch

// Try receive (non-blocking)
let value: int? = ch.try_receive()

// Receive with timeout
let value: int? = ch.receive_timeout(Duration.seconds(1))
```

### 3.3 Channel Iteration

```li
// Iterate until channel is closed
for value in channel {
    process(value)
}

// Equivalent to:
loop {
    match channel.receive() {
        Some(value) => process(value),
        None => break,  // Channel closed
    }
}
```

### 3.4 Closing Channels

```li
let ch = Channel<int>.new()

// Close the channel
ch.close()

// Check if closed
if ch.is_closed() {
    // No more values will be sent
}

// Receive from closed channel
let value = ch.receive()  // Returns null after close

// Send to closed channel
ch.send(42)  // Panics!
```

### 3.5 Channel Semantics

| Operation | Unbuffered | Buffered (not full) | Buffered (full) | Closed |
|-----------|------------|---------------------|-----------------|--------|
| `send` | Block until receiver | Immediate | Block until space | Panic |
| `receive` | Block until sender | Immediate | Block until value | Return null |
| `try_send` | false | true | false | Panic |
| `try_receive` | null | value | value | null |

### 3.6 Multiple Producers / Multiple Consumers

Channels support MPMC (Multiple Producer, Multiple Consumer):

```li
let work = Channel<Task>.buffered(100)
let results = Channel<Result>.buffered(100)

// Multiple producers
for i in 0..3 {
    spawn {
        for task in get_tasks() {
            work.send(task)
        }
    }
}

// Multiple consumers
for i in 0..5 {
    spawn {
        for task in work {
            let result = process(task)
            results.send(result)
        }
    }
}
```

---

## 4. Select Statement

### 4.1 Basic Select

The `select` statement waits on multiple channel operations:

```li
select {
    value = <-channel1 => {
        print("Received from channel1: ${value}")
    },
    value = <-channel2 => {
        print("Received from channel2: ${value}")
    },
}
```

### 4.2 Select with Send

```li
select {
    <-quit => {
        print("Quit signal received")
        break
    },
    42 -> output => {
        print("Sent 42")
    },
    msg = <-input => {
        handle(msg)
    },
}
```

### 4.3 Default Case

```li
select {
    msg = <-channel => {
        handle(msg)
    },
    _ => {
        // Non-blocking: runs if no channel ready
        do_other_work()
    },
}
```

### 4.4 Timeout

```li
select {
    result = <-results => {
        use(result)
    },
    _ = <-timeout(Duration.seconds(5)) => {
        print("Timed out!")
    },
}
```

### 4.5 Select in Loop

```li
loop {
    select {
        msg = <-messages => {
            handle_message(msg)
        },
        <-quit => {
            print("Shutting down")
            break
        },
        _ = <-ticker(Duration.seconds(1)) => {
            print("Tick")
        },
    }
}
```

### 4.6 Select Expression

```li
let result = select {
    a = <-channel_a => a * 2,
    b = <-channel_b => b + 1,
    _ => 0,  // Default
}
```

---

## 5. Synchronization Primitives

### 5.1 Mutex

```li
import std.sync.Mutex

let counter = Mutex<int>.new(0)

spawn {
    // Lock and access
    let guard = counter.lock()
    guard.value += 1
    // Automatically unlocks when guard goes out of scope
}

// Or with explicit scope
{
    let guard = counter.lock()
    guard.value += 1
}

// Try lock (non-blocking)
if let guard = counter.try_lock() {
    guard.value += 1
}
```

### 5.2 RwLock (Read-Write Lock)

```li
import std.sync.RwLock

let data = RwLock<Config>.new(config)

// Multiple readers allowed
spawn {
    let guard = data.read()
    print(guard.value.setting)
}

// Only one writer, excludes readers
spawn {
    let guard = data.write()
    guard.value.setting = "new value"
}
```

### 5.3 Semaphore

```li
import std.sync.Semaphore

let sem = Semaphore.new(3)  // Allow 3 concurrent accesses

spawn {
    sem.acquire()  // Wait for permit
    // Do work (max 3 concurrent)
    sem.release()
}

// With timeout
if sem.try_acquire_timeout(Duration.seconds(1)) {
    // Got permit
    sem.release()
}
```

### 5.4 WaitGroup

```li
import std.sync.WaitGroup

let wg = WaitGroup.new()

for i in 0..10 {
    wg.add(1)
    spawn {
        do_work(i)
        wg.done()
    }
}

wg.wait()  // Block until all done
print("All work completed")
```

### 5.5 Once

```li
import std.sync.Once

let init_once = Once.new()
var config: Config? = null

fn get_config() -> Config {
    init_once.run(|| {
        config = load_config()
    })
    return config!
}
```

### 5.6 Condition Variable

```li
import std.sync.{Mutex, Condvar}

let mutex = Mutex<Queue<int>>.new(Queue.new())
let not_empty = Condvar.new()

// Producer
spawn {
    let guard = mutex.lock()
    guard.value.push(item)
    not_empty.notify_one()
}

// Consumer
spawn {
    let guard = mutex.lock()
    while guard.value.is_empty() {
        guard = not_empty.wait(guard)
    }
    let item = guard.value.pop()
}
```

### 5.6 Atomic Types

```li
import std.sync.atomic.{AtomicInt, AtomicBool, AtomicRef}

let counter = AtomicInt.new(0)
counter.fetch_add(1)
counter.fetch_sub(1)
let value = counter.load()
counter.store(10)
counter.compare_exchange(10, 20)

let flag = AtomicBool.new(false)
flag.store(true)

let shared = AtomicRef<Data>.new(data)
let current = shared.load()
shared.store(new_data)
```

---

## 6. Async/Await

### 6.1 Async Functions

For I/O-bound operations, Lira supports async/await:

```li
async fn fetch_url(url: string) -> Result<string, Error> {
    let response = await http.get(url)
    return response.body
}

async fn main() {
    let content = await fetch_url("https://example.com")
    print(content)
}
```

### 6.2 Awaiting Futures

```li
// Single await
let result = await async_operation()

// Concurrent await (parallel execution)
let (a, b, c) = await (
    fetch("url1"),
    fetch("url2"),
    fetch("url3"),
)

// Race (first to complete)
let first = await race(
    fetch("primary"),
    fetch("backup"),
)

// All (wait for all, fail if any fails)
let results = await all([
    task1(),
    task2(),
    task3(),
])
```

### 6.3 Async and Fibers

Async functions run within fibers:

```li
spawn async {
    let data = await fetch_data()
    process(data)
}

// Mixing sync and async
fn main() {
    let fiber = spawn async {
        await do_async_work()
    }
    fiber.join()
}
```

### 6.4 Async Iteration

```li
async fn process_stream() {
    let stream = await open_stream()

    await for item in stream {
        process(item)
    }
}
```

---

## 7. Concurrency Patterns

### 7.1 Worker Pool

```li
fn worker_pool<T, R>(
    work: Channel<T>,
    results: Channel<R>,
    num_workers: int,
    process: fn(T) -> R,
) {
    for _ in 0..num_workers {
        spawn {
            for item in work {
                let result = process(item)
                results.send(result)
            }
        }
    }
}

// Usage
let work = Channel<Task>.buffered(100)
let results = Channel<Result>.buffered(100)

worker_pool(work, results, 4, |task| task.process())

// Send work
for task in tasks {
    work.send(task)
}
work.close()

// Collect results
for result in results {
    handle(result)
}
```

### 7.2 Pipeline

```li
fn pipeline<A, B, C>(
    source: Channel<A>,
    stage1: fn(A) -> B,
    stage2: fn(B) -> C,
) -> Channel<C> {
    let mid = Channel<B>.buffered(10)
    let out = Channel<C>.buffered(10)

    spawn {
        for item in source {
            mid.send(stage1(item))
        }
        mid.close()
    }

    spawn {
        for item in mid {
            out.send(stage2(item))
        }
        out.close()
    }

    return out
}

// Usage
let input = Channel<string>.new()
let output = pipeline(input, parse, transform)
```

### 7.3 Fan-Out / Fan-In

```li
fn fan_out<T>(input: Channel<T>, n: int) -> List<Channel<T>> {
    let outputs: List<Channel<T>> = []
    for _ in 0..n {
        outputs.push(Channel.buffered(10))
    }

    spawn {
        var i = 0
        for item in input {
            outputs[i % n].send(item)
            i += 1
        }
        for out in outputs {
            out.close()
        }
    }

    return outputs
}

fn fan_in<T>(inputs: List<Channel<T>>) -> Channel<T> {
    let output = Channel<T>.buffered(10)

    for input in inputs {
        spawn {
            for item in input {
                output.send(item)
            }
        }
    }

    // Close output when all inputs done
    spawn {
        let wg = WaitGroup.new()
        for _ in inputs {
            wg.add(1)
        }
        // ... wait logic
        wg.wait()
        output.close()
    }

    return output
}
```

### 7.4 Rate Limiter

```li
class RateLimiter {
    priv ticker: Channel<void>
    priv tokens: Channel<void>

    fn new(rate: int, per: Duration) -> RateLimiter {
        let interval = per / rate
        let tokens = Channel<void>.buffered(rate)
        let ticker = Channel<void>.new()

        // Pre-fill tokens
        for _ in 0..rate {
            tokens.send(())
        }

        // Refill at interval
        spawn {
            loop {
                sleep(interval)
                tokens.try_send(())
            }
        }

        return RateLimiter { ticker, tokens }
    }

    fn acquire(this) {
        this.tokens.receive()
    }

    fn try_acquire(this) -> bool {
        return this.tokens.try_receive() != null
    }
}

// Usage
let limiter = RateLimiter.new(10, Duration.seconds(1))

spawn {
    limiter.acquire()  // Block until token available
    do_rate_limited_work()
}
```

### 7.5 Circuit Breaker

```li
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

class CircuitBreaker {
    priv var state: CircuitState = CircuitState.Closed
    priv var failures: int = 0
    priv let threshold: int
    priv let reset_timeout: Duration
    priv var last_failure: Instant?
    priv mutex: Mutex<void>

    fn new(threshold: int, reset_timeout: Duration) -> CircuitBreaker {
        return CircuitBreaker {
            state: CircuitState.Closed,
            failures: 0,
            threshold,
            reset_timeout,
            last_failure: null,
            mutex: Mutex.new(()),
        }
    }

    fn call<T>(this, operation: fn() -> Result<T, Error>) -> Result<T, Error> {
        let _guard = this.mutex.lock()

        match this.state {
            CircuitState.Open => {
                if this.should_attempt() {
                    this.state = CircuitState.HalfOpen
                } else {
                    return Err(Error.new("Circuit breaker open"))
                }
            },
            _ => {},
        }

        match operation() {
            Ok(result) => {
                this.on_success()
                return Ok(result)
            },
            Err(error) => {
                this.on_failure()
                return Err(error)
            },
        }
    }

    priv fn on_success(this) {
        this.failures = 0
        this.state = CircuitState.Closed
    }

    priv fn on_failure(this) {
        this.failures += 1
        this.last_failure = Instant.now()

        if this.failures >= this.threshold {
            this.state = CircuitState.Open
        }
    }

    priv fn should_attempt(this) -> bool {
        if let last = this.last_failure {
            return last.elapsed() >= this.reset_timeout
        }
        return true
    }
}
```

### 7.6 Timeout Pattern

```li
fn with_timeout<T>(duration: Duration, operation: fn() -> T) -> T? {
    let result_ch = Channel<T>.new()
    let timeout_ch = timeout(duration)

    spawn {
        let result = operation()
        result_ch.try_send(result)
    }

    select {
        result = <-result_ch => Some(result),
        _ = <-timeout_ch => null,
    }
}

// Usage
let result = with_timeout(Duration.seconds(5), || {
    slow_operation()
})

match result {
    Some(value) => use(value),
    None => print("Operation timed out"),
}
```

---

## Appendix A: Built-in Concurrency Functions

```li
// Time utilities
fn sleep(duration: Duration)
fn timeout(duration: Duration) -> Channel<void>
fn ticker(interval: Duration) -> Channel<void>

// Current fiber
fn current_fiber() -> Fiber<void>
fn fiber_id() -> FiberId
fn yield()

// Channel utilities
fn merge<T>(channels: List<Channel<T>>) -> Channel<T>
fn broadcast<T>(source: Channel<T>, count: int) -> List<Channel<T>>
```

---

## Appendix B: Concurrency Types

```li
// From std.sync
Mutex<T>
RwLock<T>
Semaphore
WaitGroup
Once
Condvar

// From std.sync.atomic
AtomicBool
AtomicInt
AtomicUint
AtomicRef<T>

// From std.channel
Channel<T>

// Duration
Duration.nanoseconds(n)
Duration.microseconds(n)
Duration.milliseconds(n)
Duration.seconds(n)
Duration.minutes(n)
Duration.hours(n)
```

---

*This document is part of the Lira Language Specification.*

# Lira Concurrency Specification

## Document Information

| Property          | Value                       |
| ----------------- | --------------------------- |
| **Document ID**   | 04-concurrency              |
| **Version**       | 1.0.0-draft                 |
| **Status**        | Draft Specification         |
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

> **Implementation status (as of this branch).** What actually works today:
> `spawn <function-call>` (e.g. `spawn worker(ch)`) schedules a fiber, and
> coordination is done with **channels** (`chan`, `send`, `recv`) and `select`
> (see §3–§4). The richer fiber-handle API described in §2.1–§2.5 below —
> `spawn { block }`, `handle.join()` / `join_timeout()`, `fiber.result()`,
> `fiber.is_completed()`, `fiber.cancel()` / `is_cancelled()`, named fibers, and
> fiber-local storage — is **NOT implemented**; treat those subsections as a
> design sketch. To wait for spawned work, use a channel or `std.sync.WaitGroup`
> (§5). `fiber_yield()` (§2.6) and deadlock detection do work.

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

| Operation     | Unbuffered           | Buffered (not full) | Buffered (full)   | Closed      |
| ------------- | -------------------- | ------------------- | ----------------- | ----------- |
| `send`        | Block until receiver | Immediate           | Block until space | Panic       |
| `receive`     | Block until sender   | Immediate           | Block until value | Return null |
| `try_send`    | false                | true                | false             | Panic       |
| `try_receive` | null                 | value               | value             | null        |

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

## 5. Synchronization Primitives (`std.sync`)

The `std.sync` module ships **VM-honest** synchronization primitives built on
top of the fiber/channel runtime. Import it with:

```li
import std.sync
```

> **Why these APIs look different from Go/Rust:** Lira has **no user-level
> Drop/RAII**. The conventional "lock guard that auto-unlocks when it goes out
> of scope" cannot be expressed. Every primitive in `std.sync` is therefore
> built on channels and uses an **explicit** API (or a bracketed closure
> helper). See [§5.5 Not Yet Implemented](#55-not-yet-implemented--planned) for
> the primitives that were intentionally left out because they require Drop or
> have no meaning on a cooperative single-threaded VM.

All primitives are implemented in `stdlib/sync.li` as ordinary Lira structs over
channels — there are no special compiler intrinsics involved.

### 5.1 Mutex (`IntMutex` / `StringMutex`)

A mutex is backed by a **capacity-1 channel** seeded with the protected value.
The value sitting in the channel means *unlocked*; taking it out (`lock`) means
*locked*. Because only one value fits, only one fiber can hold it at a time, so
there are no lost updates.

Because there is no RAII, **`unlock` is explicit** — the caller is responsible
for putting a value back:

```li
import std.sync

let m = new_int_mutex(0)

// Explicit lock / unlock: take the value out, put a new value back.
let v = m.lock()       // blocks until the value is available
m.unlock(v + 1)        // releases the lock, storing the new value
```

The recommended idiom is the bracketed **`with`** form, which guarantees the
lock is released:

```li
// lock, apply the closure to the current value, unlock the result
m.with(|x: int| x + 1)
```

A `StringMutex` works identically over a `string` value:

```li
let name = new_string_mutex("")
name.with(|s: string| s + "!")
```

| Constructor                 | Methods                                          |
| --------------------------- | ------------------------------------------------ |
| `new_int_mutex(initial)`    | `lock() -> int`, `unlock(int)`, `with(fn)`       |
| `new_string_mutex(initial)` | `lock() -> string`, `unlock(string)`, `with(fn)` |

> **Note:** `try_lock()` and a generic `Mutex<T>` are not yet implemented — see
> [§5.5](#55-not-yet-implemented--planned). Concrete `IntMutex` / `StringMutex`
> are provided instead of a generic `Mutex<T>`.

### 5.2 WaitGroup

A `WaitGroup` is a **done-token channel**: each worker sends one token when it
finishes, and the coordinator calls `wait(n)` to receive exactly `n` tokens.
This is simpler and more deterministic than a hidden shared counter (which would
itself need a mutex).

Because there is no hidden counter, the coordinator **passes the expected worker
count explicitly** to `wait`:

```li
import std.sync

fn worker(wg: WaitGroup, i: int) {
    do_work(i)
    wg.done()          // each worker signals exactly once on exit
}

fn main() {
    let wg = new_wait_group()
    let n = 10

    var i = 0
    while i < n {
        spawn worker(wg, i)
        i = i + 1
    }

    wg.wait(n)         // blocks until n workers have signalled done
    println("All work completed")
}
```

| Constructor        | Methods                          |
| ------------------ | -------------------------------- |
| `new_wait_group()` | `done()`, `wait(n)` (blocks)     |

> **Deviation from Go:** there is no separate `add()` — you do not increment a
> counter ahead of time. Instead you tell `wait` how many `done()` signals to
> expect. The backing channel is buffered (capacity 64), so pick a worker count
> below that, or drain the group as you go.

### 5.3 Semaphore

A `Semaphore` is a channel **pre-seeded with `n` permits**. `acquire` removes a
permit (blocking when none remain); `release` returns one. This bounds the
number of fibers in a critical section to `n`, while still allowing real overlap
up to that bound.

```li
import std.sync

let sem = new_semaphore(3)  // allow 3 concurrent fibers

fn worker(sem: Semaphore) {
    sem.acquire()           // blocks until a permit is free
    // ... at most 3 fibers run this section concurrently ...
    sem.release()           // return the permit
}
```

| Constructor         | Methods                  |
| ------------------- | ------------------------ |
| `new_semaphore(n)`  | `acquire()`, `release()` |

> **Note:** there is no `try_acquire` / `try_acquire_timeout` yet.

### 5.4 Worked Example

This program (from `examples/sync_mutex_waitgroup.li`) proves real mutual
exclusion and that `wait` blocks — two fibers each increment a shared
`IntMutex` 1000 times and the total is exactly 2000:

```li
import std.sync

fn worker(m: IntMutex, wg: WaitGroup, iters: int) {
    var i = 0
    while i < iters {
        let v = m.lock()
        m.unlock(v + 1)
        i = i + 1
    }
    wg.done()
}

fn main() {
    let m = new_int_mutex(0)
    let wg = new_wait_group()
    let iters = 1000

    spawn worker(m, wg, iters)
    spawn worker(m, wg, iters)

    wg.wait(2)

    let total = m.lock()
    m.unlock(total)
    println("total: " + total)        // total: 2000
    println("expected: " + (2 * iters))
}
```

### 5.5 Not Yet Implemented / Planned

The following primitives appeared in earlier drafts of this spec but are **not
implemented**. They are listed here honestly so nobody depends on APIs that do
not exist. Do not treat any of the following as working:

| Primitive                          | Status                | Reason                                                                                 |
| ---------------------------------- | --------------------- | -------------------------------------------------------------------------------------- |
| **RAII guard Mutex** (auto-unlock) | Not implementable     | Lira has no Drop/RAII; a guard cannot run code on scope exit. Use `with()` / `unlock`. |
| **`try_lock()`**                   | Planned               | Needs an `Option`/sentinel return to be ergonomic.                                     |
| **Generic `Mutex<T>`**             | Planned               | Needs a checker `type_params` patch + generic-constructor fix. Use `IntMutex` etc.     |
| **`RwLock<T>`** (read/write)       | Not implementable yet | Read/write guards have the same Drop problem as the Mutex guard.                        |
| **`Condvar`** (`wait(guard)`)      | Not implementable yet | Depends on the nonexistent lock guard.                                                  |
| **`Once`** (run-once closure)      | Not needed yet        | Trivial under cooperative scheduling; not provided.                                    |
| **Atomics** (`AtomicInt`, …)       | Not meaningful        | A cooperative, single-threaded, shared-heap VM has no data races to guard against — a plain field read/modify/write between yield points is already atomic. Use an `IntMutex` if you need mutual exclusion across blocking points. |

If you reach for one of these, the idiomatic replacement is almost always
**channels + fibers + `select`** directly, or an `IntMutex` / `Semaphore`.

---

## 6. Async/Await — Not Implemented (use fibers + channels)

> **Lira does not have `async`/`await`, and it is not the recommended
> concurrency model.** Lira's concurrency is **green-threaded (Go-style)**:
> you express concurrency with `spawn`, channels (`chan` / `send` / `recv`),
> and `select`. Fibers block on I/O and channel operations transparently and
> cooperatively yield to the scheduler, so there is no "function colour"
> (sync vs async) split and no separate `Future` type to await.

### 6.1 Status

The `async` and `await` keywords are reserved as lexer tokens only. They are
**not parsed, type-checked, compiled, or executed.** Likewise the following are
**not implemented** and should not be treated as working APIs:

- `async fn` / `async { ... }` blocks
- `await` expressions
- `Future<T>` values
- concurrent-await tuple syntax (`await (a, b, c)`)
- `race(...)`, `all([...])`
- `await for item in stream` async iteration

There is no async state machine generation; nothing in the compiler lowers these
forms.

### 6.2 Do This Instead

Anything you would reach for `async`/`await` to do is expressed directly with
fibers and channels. The fiber blocks while it waits; other fibers keep running.

Instead of awaiting a single async call:

```li
// NOT: let content = await fetch_url(url)
// Spawn the work and receive the result over a channel.
let result = chan(1)
spawn fetch_into(url, result)   // worker calls send(result, body)
let content = recv(result)      // blocks this fiber until the body arrives
```

Instead of `race(primary, backup)` (first to complete wins):

```li
let a = chan(1)
let b = chan(1)
spawn fetch_into(primary, a)
spawn fetch_into(backup, b)

let first = select {
    v = <-a => v,
    v = <-b => v,
}
```

Instead of `all([t1, t2, t3])` (wait for all), use a `WaitGroup` or collect each
result from a channel — see [§5.2 WaitGroup](#52-waitgroup) and the worker-pool
pattern in [§7.1](#71-worker-pool).

Instead of `await for item in stream`, range over a channel until it closes:

```li
loop {
    match recv(stream) {
        Some(item) => process(item),
        None => break,        // channel closed
    }
}
```

> **Summary:** treat the fiber + channel + `select` model as the only
> concurrency model. There is no async runtime to opt into.

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
// From std.sync (IMPLEMENTED — see §5)
IntMutex          // new_int_mutex(initial)
StringMutex       // new_string_mutex(initial)
WaitGroup         // new_wait_group()
Semaphore         // new_semaphore(n)

// Planned / not implemented (see §5.5) — do NOT depend on these:
//   Mutex<T> (generic), RwLock<T>, Once, Condvar,
//   AtomicBool, AtomicInt, AtomicUint, AtomicRef<T>

// Channels are a built-in, created with chan(n) / chan():
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

_This document is part of the Lira Language Specification._

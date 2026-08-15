# Native Backend (`lira-codegen`)

Lira has two backends behind one front end. `lirac::codegen` lowers the checked
AST to bytecode for `liravm`; `lira-codegen` lowers the same AST to machine code
through [Cranelift](https://cranelift.dev/).

```
source.li
    │
    ├── lexer → parser → checker            (crates/lirac, shared)
    │
    ├── codegen.rs ──────────► .lic bytecode ──► liravm      `lira run`
    │
    └── lira-codegen/lower.rs ► Cranelift IR ──► machine code
                                                  ├─ in-memory  `lira jit`
                                                  └─ object + link `lira build`
```

```bash
lira build hello.li -o hello   # standalone native executable
./hello
lira jit hello.li              # compile to native code and run in-process
lira run hello.li              # bytecode VM (complete implementation)
```

The native backend is **partial by design**. Anything it cannot lower is
reported as an error naming the construct, and the program still runs under
`lira run`. It never falls back to a slow path or guesses.

## Why static typing changes the code that comes out

The bytecode VM stores every value as a tagged `Value`, so `a + b` is a dispatch
on two tags, an unbox, an add, and a rebox. The checker has already proved both
operands are `int`, so native code does not repeat any of that work:

| Lira | Machine code |
|---|---|
| `a + b` on `int` | one `iadd` on two 64-bit registers |
| `pt.x` | one `load` at a constant byte offset |
| `f + 1.0` on `float` | one `fadd` in an FP register |
| `match shape { Circle(r) => ... }` | load the discriminant, compare, load the payload slot |

No tag bits, no NaN boxing, no guard branches. `fib(30)` runs roughly 70× faster
natively than on the VM.

## Value representation

Registers (`src/abi.rs`):

| Lira type | Register |
|---|---|
| `int`, `int8`…`int64`, `uint8`…`uint64`, `char` | `i64` |
| `float` | `f64` |
| `bool` | `i8` |
| `string`, arrays, structs, enums, channels | pointer |

Narrow integers widen to `i64` in registers but keep their natural width in
memory, so a struct field declared `int32` really is four bytes.

A `char` is an integer code point, matching the bytecode VM, which has no
separate character value and prints `'a'` as `97`.

## Memory layout

Every heap object starts with the same 16-byte header from
`runtime/lira_rt.h`, so the runtime can treat any pointer uniformly:

```c
typedef struct { uint32_t kind; uint32_t flags; int64_t rc; } LiraHeader;
```

`src/layout.rs` computes the rest at compile time:

```
struct Point { x: int, y: int }

  0 ┌──────────────┐
    │ LiraHeader   │  16 bytes
 16 ├──────────────┤
    │ x: int       │  pt.x  →  load.i64 [base + 16]
 24 ├──────────────┤
    │ y: int       │  pt.y  →  load.i64 [base + 24]
 32 └──────────────┘
```

Fields are laid out in declaration order with natural C alignment. Aggregate
fields are pointers, so recursive and mutually recursive types need no special
handling and declaration order does not matter.

Enums are the header, an `i64` discriminant, then uniform 8-byte payload slots
sized to the widest variant:

```
enum Shape { Dot, Circle(float), Rect(int, int) }

  0 │ LiraHeader │
 16 │ tag: i64   │   0 = Dot, 1 = Circle, 2 = Rect
 24 │ slot 0     │   Circle's radius, Rect's width
 32 │ slot 1     │   Rect's height
```

String literals need no runtime construction at all: the complete object, header
included, is emitted into read-only data with a negative refcount marking it
immortal.

## Closures

A function value is a heap object: a code pointer, a capture count, then one
8-byte cell per captured value.

```
let add5 = make_adder(5)

  0 │ LiraHeader  │
 16 │ code        │──► lira__lambda__0(env, x)  { return x + env.captures[0] }
 24 │ count = 1   │
 32 │ capture: 5  │
```

Every function value's code takes its own closure as the first argument, whether
or not it captures anything, so a lambda and a named function are called through
one path. A named function used as a value — `apply_twice(double, 3)` — gets a
wrapper that ignores the environment, and its closure object is emitted into
read-only data with a relocation, so taking a function's value costs nothing at
run time.

Captures are copied by value when the closure is built, matching the bytecode
VM's `MakeClosure`. That is what makes `make_adder(5)` work: the frame it was
built in is gone by the time `add5` runs.

Free variables are found with a proper scope walk rather than by collecting every
identifier and subtracting the bound ones, so a name shadowed partway through the
body is not captured.

One sharp edge worth knowing about: an indirect call's signature and the lifted
function's signature are derived from the same recorded `Type::Function`. They
have to be — Cranelift cannot check an indirect call, so a mismatch returns
whatever happened to be in the return register rather than failing to compile.

## Optionals and Result

`string?` is a `string` that may be null — the pointer already has a spare
value. `int?` does not: every bit pattern of an `i64` is a valid `int`. Those
wrap their payload in a one-slot heap box, and null means none.

```
int?    →  null  |  ┌ header ┐
                    │ slot   │  the int
                    └────────┘
string? →  null  |  the string pointer itself
```

`T` flows into `T?` implicitly (boxing where needed) and back out where the
checker has established the value is present; unwrapping a null reports rather
than reading through it. `a ?? b` yields the unwrapped type, so `get_null() ?? 0`
is an `int`.

`Result<T, E>` is a tag and one payload slot, but unlike a user enum its payload
types come from the `Result<T, E>` at each use rather than from one shared
declaration. That is why `Result::Ok(x)` takes its type from the context it is
returned into.

`expr?` propagates both: an absent optional returns null from the enclosing
function, and an `Err` is handed back to the caller unchanged.

## Fibers: the interesting part

The bytecode VM can suspend a fiber by saving an instruction pointer, because
its call frames live in a heap vector. Native code has no such luxury — call
frames live on the machine stack and are addressed through SP.

Lira uses **stackful fibers with an assembly context switch**, the approach Go,
Lua coroutines and Boost.Context take. Each fiber gets its own 256 KB `mmap`'d
stack with a guard page at the low end, so a stack overflow faults instead of
silently trampling another fiber's data.

```
  Fiber A stack          Fiber B stack           Scheduler stack
 ┌──────────────┐       ┌──────────────┐        ┌──────────────┐
 │ native frames│       │ native frames│        │ run queue    │
 └──────┬───────┘       └──────▲───────┘        └──────▲───────┘
        │  lira_ctx_switch(&a->sp, sched_sp)           │
        └─────────────────────────────────────────────►┘
                    save callee-saved regs, swap SP
```

`runtime/lira_ctx.S` is the whole mechanism:

- **x86-64 SysV**: push `rbp rbx r12-r15`, store SP, load the other SP, pop, `ret`.
- **AArch64 AAPCS64**: save `x19-x28`, `x29`, `x30` and `d8-d15` in a 160-byte
  frame, swap SP, restore, `ret` into the saved `x30`.

Caller-saved registers need no attention: the C compiler already treats a call
to `lira_ctx_switch` as clobbering them.

A new fiber's stack is pre-loaded with the frame the switch expects to pop,
whose return address is a small assembly trampoline. The trampoline moves the
fiber pointer — parked in a callee-saved slot — into the first argument register
and calls into C, which never comes back.

`spawn f(a, b)` needs one more piece: the scheduler can only start a fiber from
a `void(*)(void*)`. So the arguments are evaluated at the spawn site, boxed into
a heap cell, and unpacked by a thunk Cranelift generates for that call site.

Channels are cooperative and lock-free by construction — everything runs on one
OS thread and only moves state at an explicit switch point. `send` hands its
value straight to a waiting receiver, which is what makes an unbuffered channel
a true rendezvous. When no fiber can run and some are still blocked, the
scheduler reports a deadlock rather than hanging.

## The runtime

`liblira_rt` is C plus the two assembly context switches, compiled by this
crate's build script. It is linked into the compiler binary (so the JIT can
resolve `lira_rt_*` symbols in-process) and embedded verbatim with
`include_bytes!` (so `lira build` can hand it to the system linker without a
separate runtime install).

Generated `main` is two lines: hand the entry point to `lira_rt_boot`, return
what it reports. The entry point runs as fiber 0, which is why the top level can
block on a channel.

`src/runtime.rs` is the single source of truth for the ABI on the Rust side;
`tests/native.rs` and a unit test in `src/jit.rs` check that the codegen table,
the JIT symbol table and the C header stay in step.

## Type information the checker does not record

The checker skips the bodies of methods declared inside `struct`, `class` and
`impl` blocks — it only records member references there for the LSP. Bytecode
does not care, because it is dynamically typed at run time. Native code does, so
`lower.rs` carries a small structural inference pass that runs off the names in
scope, the struct and enum layouts, and the declared signatures. The same pass
recovers types the checker erases to `any`, such as an enum payload bound by
`Option::Some(x)`.

## Built-ins

Around 80 of the language's built-in functions are implemented natively in
`liblira_rt`: the math library, character-indexed string operations, time,
randomness, the environment, files and the filesystem, base64 and URL encoding,
MD5/SHA-1/SHA-256/SHA-512, UUIDs, and TCP/DNS.

`sqrt`, `abs`, `floor`, `ceil`, `trunc`, `is_nan`, `is_infinite` and `is_finite`
never reach the runtime at all — they lower to single Cranelift instructions.

Two invariants keep this honest:

- A unit test builds the checker's own environment and compares every built-in's
  parameter and return types against the table in `src/runtime.rs`. A signature
  that drifts is a failing test, not a wrong answer at run time.
- Every lowered value is checked against the machine type its Lira type implies
  before it is used. This is what catches a built-in and the checker disagreeing
  about a result type — the failure mode is an `i64` read as an `f64`, which no
  other check would notice. It found two real mis-compiles when it was added.

A user function shadows a built-in of the same name, because the checker
resolves the call that way: `examples/stdlib_demo.li` defines its own `abs`.

Still missing: `json_*` (needs a dynamic value representation), `regex_*` (needs
an engine) and `http_*`.

## What is supported

Functions, methods, `impl` blocks (including `impl int`, `impl string` and
`impl [int]`, so the standard library's methods on built-in types work), static
methods, recursion and mutual recursion, named arguments and defaults, type
aliases, top-level globals; `if`/`else`, `while`, `loop`, `for` over arrays and
ranges, ranges as values, `break`/`continue`, blocks as expressions;
`match` with literal, range, wildcard, binding, or, struct and enum-constructor
patterns, plus guards; structs with nested and narrow fields; enums with
payloads and `__enum`/`__variant` reflection; tuples, tuple patterns and
destructuring `let`; lambdas, closures with captures, and functions as values;
optionals including boxed scalar optionals, `??`, `?.` and `?`; `Result<T, E>`
with typed payloads;
arrays with indexing, assignment, `push`, `pop` and `len`; strings with
concatenation, interpolation, comparison and `len`; `spawn`, `chan`, `send`,
`recv`, `close`, `fiber_yield`, `fiber_id`.

Of the repository's 124 examples, 86 compile natively and produce byte-identical
output to the bytecode VM. Six more compile and run correctly but cannot be
compared byte-for-byte: they print timestamps, random UUIDs, or `env_args`,
which differ between an interpreted script and a compiled binary by nature. The
rest use constructs listed below and are declined with a reason.

46 examples are pinned as regression tests in `tests/parity.rs`, and every
example that type-checks is required to either compile or be declined cleanly —
an internal error fails the suite.

## What is not supported yet

| Not lowered | Notes |
|---|---|
| Maps | Needs a hashed representation in the runtime |
| Class inheritance | Needs prefixed layouts and a vtable |
| Generics | Type-erased in the VM; native wants monomorphisation |
| `select` | Needs multi-channel parking |
| `json_*`, `regex_*`, `http_*` | Need a dynamic value, a regex engine and an HTTP client |
| String indexing | Needs a decision on byte vs. character indexing |

Two more limits worth knowing:

- **Nothing is reclaimed.** Allocations come from `malloc` and are never freed.
  The `rc` field is in the header for the ARC scheme the VM uses, but the
  backend does not yet emit retain/release pairs.
- **`lira jit` runs one program per process.** The runtime's scheduler state is
  process-global and single-threaded.

## Differences from the bytecode VM

- `spawn` really runs the fiber. The VM only executes spawned fibers in fiber
  mode, so `examples/spawn_expression.li` prints one extra line natively.
- `pop` on an empty array reports a runtime error instead of returning `null`,
  because `T?` has no native representation for scalar `T`.
- `print` terminates the line, matching the VM's `Print` opcode, which always
  appends a newline. The runtime keeps newline-free entry points for when that
  is fixed.
- A function whose body ends in a bare expression returns it. The VM returns
  null instead, so `examples/null_and_optionals.li` differs — the native result
  is the one the example's own comments expect.

## Platform support

x86-64 and AArch64 on Linux and macOS. 64-bit and little-endian only. The link
step shells out to `cc`; set `LIRA_CC` to override.

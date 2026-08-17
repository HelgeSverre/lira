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
                                                  ├─ isolated worker  `lira jit`
                                                  └─ object + link `lira build`
```

```bash
lira build hello.li -o hello   # standalone native executable
./hello
lira jit hello.li              # compile to native code in an isolated worker
lira run hello.li              # bytecode VM (complete implementation)
```

The native backend lowers the checked AST to standalone machine code. A source
construct that is rejected by native lowering produces a diagnostic naming the
construct; it is never silently mis-compiled or routed through an unspecified
fallback. The bytecode path remains available through `lira run`.

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

No tag bits, no NaN boxing, no guard branches. This representation is intended
to reduce dispatch overhead; an accurate release-build benchmark is pending,
so this document makes no speedup claim.

## Value representation

Registers (`src/abi.rs`):

| Lira type | Register |
|---|---|
| `int`, `int8`…`int64`, `uint8`…`uint64`, `char` | `i64` |
| `float` | `f64` |
| `bool` | `i8` |
| `string`, arrays, structs, enums, interfaces, `any`, channels | pointer |

Narrow integers widen to `i64` in registers but keep their natural width in
memory, so a struct field declared `int32` really is four bytes.

A `char` is an integer code point, matching the bytecode VM, which has no
separate character value and prints `'a'` as `97`.

## Interface ABI and witness dispatch

Interface values are implemented in the native runtime; they are not rejected
at the lowering boundary. `runtime/lira_rt.h` fixes these layouts:

```c
typedef struct {
    const LiraStr *name;
    const LiraStr *signature;
} LiraInterfaceMethod;                 // 16 bytes

typedef struct {
    uint64_t method_count;
    const LiraInterfaceMethod *methods;
} LiraInterfaceSpec;                    // 16 bytes

typedef struct {
    const LiraInterfaceSpec *spec;
    uint32_t payload_kind;              // ref, i64, f64, or i8
    uint32_t method_count;
    void (*method_slots[])(void);       // trailing erased function slots
} LiraInterfaceWitness;

typedef struct {
    LiraHeader hdr;
    uint64_t payload;
    const LiraInterfaceWitness *witness;
} LiraInterface;                        // 32 bytes
```

`layout.rs` emits one immutable `LiraInterfaceSpec` per declaration. For each
concrete source/target pair, `lower.rs` emits an immutable witness containing
the target spec, payload representation, and one slot per method. Each slot is
filled with a generated, typed Cranelift thunk. A thunk extracts the payload
with `lira_rt_interface_payload`, converts arguments/results to the checked
method representations, and calls the concrete implementation. Interface-to-
interface assignments use forwarding thunks. String `len` and array `len`,
`push`, and `pop` use native intrinsic thunks.

`lira_rt_interface_new` validates the bounded metadata and allocates the
managed interface object. `lira_rt_interface_is` compares the target method
spec structurally; it does not rely on a nominal interface-name tag. Method
slot lookup and payload extraction fail closed when the witness, method count,
or payload kind is invalid. `Any` boxing preserves an interface's spec and
witness (`lira_rt_any_box_interface`); unboxing to a target interface checks
the target descriptor. Lowering can also recover a checked, finite set of
concrete interface witnesses from erased `Any` values, including raw strings,
arrays, objects, and supported scalar forms when their source type is
unambiguous.

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

## Resource containment

The native runtime places independent hard ceilings on its managed heap and
live fiber count:

- GC-managed objects plus array, map, and channel backing storage are limited
  to 256 MiB per process.
- At most 512 fibers may be live at once. Each fiber has a 256 KiB guarded
  stack, so runaway `spawn` cannot create an unbounded number of mappings.
- `LIRA_NATIVE_MEMORY_LIMIT_BYTES` and `LIRA_NATIVE_MAX_FIBERS` may lower those
  limits for a sandbox or test. Zero, malformed values, overflow, and attempts
  to raise the compiled limits are runtime errors.

These runtime budgets do not make a standalone AOT executable a sandbox. In
particular, host-library allocations outside the accounted heap and arbitrary
filesystem, device, or network access require containment by the process that
launches the executable.

Native integration tests add a second boundary around generated programs: AOT
executables and JIT runs execute in child process groups with a wall deadline,
CPU limits, bounded output capture, kill-and-reap cleanup, and a cross-process
execution lock. On Linux the test/process supervisor applies an address-space
limit; on macOS it samples the group's physical memory reactively rather than
providing a hard OS address-space ceiling. The stdout/stderr caps bound captured
output, but do not sandbox arbitrary filesystem or device I/O. Recovery tests
that require several JIT modules to share one runtime execute the complete
sequence inside the same bounded child.

The public `lira jit` command runs the generated program in a private worker
process. The worker has its own process group, CPU and wall-clock deadline,
memory and output ceilings, and is always killed and reaped on a limit breach.
The library `jit_run()` API has the same fail-closed behavior: it requires
`LIRA_JIT_WORKER` to name an executable worker. Trusted embedders may opt into
the explicitly named `jit_run_in_process()` API, but it has no process-level
deadline and remains uncontained, so it must be isolated by the host.
`jit_run_isolated()` is the direct library entry point for a caller that already
has a trusted worker executable. The process-group boundary contains generated
Lira code; it is not a security sandbox for a hostile worker binary that
deliberately changes its process group.

## Classes

A class instance carries a pointer to its virtual method table between the
header and its fields. A child's fields are laid out after its parent's, so a
`Dog*` reads correctly as an `Animal*`, and the vtable keeps the parent's slot
indices — an `override` changes whose code fills a slot, not where the slot is.

```
class Dog extends Animal        Dog's vtable
  0 │ LiraHeader │              ┌──────────────┐
 16 │ vtable     │─────────────►│ speak → Dog  │  slot 0, overridden
 24 │ name       │  (Animal's)  │ describe →   │  slot 1, inherited
 32 │ breed      │  (Dog's)     │      Animal  │
```

That is what makes `describe()`, declared only on `Animal`, reach a `Puppy`'s
`speak()`: the static type fixes the slot, the instance supplies the
implementation. `super.method()` skips the table and calls the parent's code
directly, which is the whole point of writing it.

Classes are laid out parents-first regardless of declaration order, and a class
extending something that is not a class in the program is reported rather than
laid out with its inherited fields silently missing.

## Generics

The VM erases generics to a uniform tagged value. Native code has no such
representation, so a generic is monomorphised: one copy per concrete type
argument set, each with its own layout, under a mangled name.

```
fn identity<T>(x: T) -> T      identity(42)      → identity$int
struct Box<T> { value: T }     Box { value: 1 }  → Box$int, laid out as a struct
enum Opt<T> { Some(T), None }  Opt::Some(42)     → Opt$int, tag plus an int slot
```

The backend resolves bindings from the concrete uses it sees: arguments at a
call site, the values assigned to literal fields, and the payload used to
construct a variant. `foo::<int>(x)` names its instantiation outright. This
also covers inline generic methods and methods with their own type parameters;
their method-level bindings are derived from the call site alongside the
receiver's bindings.

Instantiations are a worklist, since one can demand another, and each is
recorded so a generic that calls itself terminates rather than unfolding
forever. A generic type named in a signature — `fn describe(o: Opt<int>)` — is
built at declaration time, so a function can take one before anything has
constructed it.

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

## Maps

A map is a string-keyed open-addressing hash table in the runtime, with the same
uniform 8-byte cells arrays use for values. String keys match the language's
map representation, and `len` works on a native map.

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

`select` tries its arms in source order with the non-blocking channel
operations. A `_` arm catches a pass that finds nothing ready. Without one, the
fiber yields and tries again — and the runtime reports a deadlock if a whole
sweep of the run queue goes by with no successful channel operation, so a select
that can never become ready fails loudly instead of spinning forever.

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

## Type information at the lowering boundary

The checker records expression and method-body types in semantic side tables,
and native lowering consumes those facts where they are available. `lower.rs`
also carries a small structural inference fallback for contexts whose source
type is deliberately erased to `any`, such as an enum payload bound by
`Option::Some(x)`. The fallback is not a substitute for checking: it only
selects a native representation after the shared checker has accepted the
program.

## Built-ins

Native `liblira_rt` coverage includes the math library, character-indexed string
operations, time,
randomness, the environment, files and the filesystem, base64 and URL encoding,
MD5/SHA-1/SHA-256/SHA-512, UUIDs, TCP/DNS, JSON, regular expressions, and HTTP.

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

## What is supported

Functions, methods, `impl` blocks (including `impl int`, `impl string` and
`impl [int]`, so the standard library's methods on built-in types work), static
methods, recursion and mutual recursion, named arguments and defaults, type
aliases, top-level globals; `if`/`else`, `while`, `loop`, `for` over arrays,
ranges, tuples, strings, and `Any`, ranges as values, `break`/`continue`, blocks
as expressions; `match` with literal, range, wildcard, binding, or, struct and
enum-constructor patterns, plus guards; structs with nested and narrow fields;
enums with payloads and `__enum`/`__variant` reflection; tuples, tuple patterns
and destructuring `let`; lambdas, closures with captures, and functions as
values; optionals including boxed scalar optionals, `??`, `?.` and `?`;
`Result<T, E>` with typed payloads; string-keyed maps; `select` values, with and
without a default arm; classes with inheritance, virtual dispatch, `override`
and `super`; generic functions, structs, enums, impls, and inline generic
methods, monomorphised; arrays with indexing, assignment, `push`, `pop` and
`len`; strings with concatenation, interpolation, comparison, Unicode scalar
indexing, and `len`; `spawn`, `chan`, `send`, `recv`, `close`, `fiber_yield`,
`fiber_id`; and `Any` values with exact type descriptors.

The bounded exhaustive test
`every_frontend_valid_example_executes_on_vm_aot_and_jit_and_matches_directives`
recursively discovers files under `examples/` and `tests/samples/`. It
rejects the two fixtures marked as expected compile errors and executes every
other frontend-valid source through bounded VM, AOT, and JIT runs. The local
crawler fixture is hermetic, including TCP connect coverage.

## Current native-lowering boundaries

| Not lowered | Notes |
|---|---|
| Heterogeneous arrays | Use an explicitly erased element type such as `[any]` when heterogeneous values are required |
| An unconstrained `[]` | Nothing pins the element type; the error says to annotate |
| Generic methods called through an interface receiver | `generic methods on interfaces are not lowered yet`; call-site type arguments are not instantiated through an interface witness |
| Interface methods with `void` parameters | Native witness generation rejects these because the Cranelift/native ABI needs a concrete parameter representation |
| Flow narrowing after `is` | The shared checker does not refine a binding inside the true branch, so branch-only members are rejected before either backend |

`Any`-typed `is Interface` checks and checked `Any`-to-interface casts are
lowered. Native descriptors retain exact array element and interface identity;
raw integer-family values can recover a custom witness only when the finite
checker-approved conformer set is unambiguous. The bytecode VM still loses
exact custom primitive and array element identity after some values pass
through `Any`, so the corresponding exact-descriptor regressions are
native-only rather than VM-parity claims.

One more limit worth knowing: **`lira jit` runs one program per process.** The
runtime's scheduler state is process-global and single-threaded.

`tcp_connect` performs blocking system name resolution on the native I/O pool.
The socket connect attempt has a deadline, but POSIX `getaddrinfo` has no
portable cancellation deadline. Isolated JIT and the test launcher bound the
whole worker process; a standalone AOT executable must be launched under an
external wall-time policy when untrusted hostnames are possible.

## Memory management

Heap objects are reclaimed by a conservative tracing collector rather than
remaining allocated for the lifetime of the process. The collector scans
managed objects and runtime/fiber roots, and generated top-level globals
register root slots so a global reference remains live. It handles cycles and
reclaims unreachable native objects while preserving the hard native heap and
fiber budgets described above.

## Platform support

x86-64 and AArch64 on Linux and macOS. 64-bit and little-endian only. The link
step shells out to `cc`; set `LIRA_CC` to override.

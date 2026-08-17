# Lira VM Runtime

The bytecode VM is implemented by `crates/liravm`. It loads the sequential
version-2 `.lic` format, executes one opcode at a time, and uses a Rust value
enum rather than NaN-boxed 64-bit words.

## VM state

`VM` owns the loaded `Program`, an operand `Vec<Value>`, a flat locals vector,
call frames, an instruction pointer, runtime/syscall context, captured output,
and the fiber scheduler. A call frame records the callee offset, return address,
locals base/count, operand-stack base, and optional closure captures. The
interpreter checks stack and bytecode bounds before every read/pop and reports
runtime errors with optional source location and function-name stack data from
the `.lic` debug tables.

The terminal `Halt` opcode is `0xff` and returns success. `sys_exit` can request
another exit code through the runtime syscall path. It is not an illegal-opcode
sentinel.

## Runtime values

The authoritative representation is the Rust enum `liravm::value::Value`:

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Rc<String>),
    Array(Gc<GcCell<Vec<Value>>>),
    Tuple(Gc<GcCell<TupleData>>),
    Struct(Gc<GcCell<HashMap<String, Value>>>),
    Object(Gc<GcCell<HashMap<String, Value>>>),
    Interface(Gc<InterfaceData>),
    Function(usize),
    Closure(Gc<ClosureData>),
    Fiber(u64),
    Channel(u64),
}
```

`Rc<String>` is used for interned/immutable strings. Cyclic-capable aggregates,
closures, and interface data use `gc::Gc`; their tracing implementation follows
reachable `Value` edges. `Struct` and `Tuple` are value-semantic at compiler
copy boundaries (`CopyValue` recursively copies them). Arrays, objects,
closures, channels, fibers, and strings retain their runtime identity/handle
semantics.

`TupleData` contains an element vector and a private initialization cursor.
`NewTuple` plus ordered `TupleSet` can fill a tuple; completed tuples are sealed
and cannot be mutated through `ArraySet`.

## Interface values and witness dispatch

An interface value is `Value::Interface(Gc<InterfaceData>)`:

```rust
pub struct InterfaceData {
    pub receiver: Value,
    pub methods: HashMap<String, InterfaceMethod>,
}

pub enum InterfaceMethod {
    Value(Value),
    Intrinsic(InterfaceIntrinsic),
}
```

The receiver is retained separately from the method witness map. `Value`
methods are bytecode `Function`/`Closure` values; intrinsic witnesses currently
cover string `len` and array `len`, `push`, and `pop`. `InterfaceBox` constructs
this object from its inline witness encoding, optionally recursively copying a
struct receiver. `InterfaceCall` finds the named witness, prepends
`InterfaceData.receiver` as the implicit receiver, and either enters the
function/closure or performs the intrinsic operation. Missing methods,
non-callable fields, invalid witness kinds, and wrong intrinsic arguments are
runtime errors.

`TypeIs` uses coarse runtime IDs: `0` null, `1` bool, `2` int, `3` float,
`4` string, `5` array, `6` object/struct, `7` function/closure, `8` tuple,
`9` channel, and `10` interface. `InterfaceIs` is a separate inline structural
method-witness query; it does not require a nominal runtime type descriptor.

## Heap and resource boundaries

Arrays and tuples are bounded before allocation and growth. The current VM cap
is 16 MiB of backing storage per collection, measured using the Rust `Value`
representation. This is a per-collection guard, not a process-wide heap quota.
The output capture path also bounds one rendered value at 8 MiB and retained
captured output at 8 MiB/100,000 lines.

`gc::Gc` traces arrays, tuples, structs, objects, interfaces, and closures.
`Collect` forces a collection at an interpreter dispatch boundary; automatic
collection is additionally driven after a bounded allocation interval. The VM
does not manually expose object headers, refcounts, NaN tags, or raw pointers
as part of the language value ABI.

## Fibers and channels

The scheduler provides cooperative green fibers. `Spawn` creates a fiber at a
16-bit code offset with an 8-bit argument count; `Yield` parks the current
fiber in fiber mode (and is a no-op in single-fiber mode). Channels are scheduler
IDs with bounded-capacity send/receive queues. Blocking send/receive saves the
current VM stack, locals, instruction pointer, and call stack, then resumes a
ready fiber. `ChanRecv` returns a value and an open/closed boolean. A closed
channel drains buffered values before reporting closed; a scheduler deadlock is
reported rather than spinning forever. `Select` uses the scheduler's
deterministic readiness arbiter and supports receive, send, and default arms.

## System calls

`Syscall` is opcode `0xfe` followed by exactly one `u8` syscall number. The
runtime dispatch function therefore accepts `u8`, not `u16`; the number is not
the opcode itself. Arguments are ordinary stack `Value`s, popped in reverse
push order, checked against each syscall's expected types, and results are
pushed as `Value`s. Syscall families cover process exit/output, files,
environment, time/randomness, encoding/hashes, JSON, regex, HTTP, networking,
and other host services exposed by `Runtime`. Unknown numbers and invalid
argument types produce runtime errors.

## Output and errors

`Print` and `Println` render one `Value` through its bounded `Display`
implementation; `Println` appends one newline. `Assert` accepts only
`Value::Bool(true)` and rejects false/non-boolean values. Runtime failures are
returned as strings by legacy APIs or as `RuntimeError { message, line, column,
stack }` by structured APIs.

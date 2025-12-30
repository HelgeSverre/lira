# Lira VM Runtime Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 12-vm-runtime |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |
| **Prerequisites** | 10-bytecode-format, 11-instruction-set |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Value Representation](#2-value-representation)
3. [Object Model](#3-object-model)
4. [Call Stack](#4-call-stack)
5. [Fiber System](#5-fiber-system)
6. [Channel Implementation](#6-channel-implementation)
7. [Exception Handling](#7-exception-handling)
8. [Execution Engine](#8-execution-engine)
9. [Runtime Services](#9-runtime-services)

---

## 1. Overview

### 1.1 VM Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    LI-LANG VM ARCHITECTURE                       │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   EXECUTION ENGINE                         │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │ Interpreter │  │   JIT (?)   │  │   Debugger  │       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   FIBER SCHEDULER                          │  │
│  │  [Fiber₀] [Fiber₁] [Fiber₂] ... [FiberN]                  │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    MEMORY MANAGER                          │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │    Heap     │  │ Ref Counter │  │   Cycles    │       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   RUNTIME SERVICES                         │  │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐      │  │
│  │  │ Strings │  │  Types  │  │ Modules │  │ Syscall │      │  │
│  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘      │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Design Principles

1. **Efficiency**: Minimal overhead for common operations
2. **Safety**: Memory-safe execution, bounds checking
3. **Concurrency**: First-class fiber and channel support
4. **Debuggability**: Rich debugging information available

### 1.3 Implementation Language

The VM is implemented in Rust for:
- Memory safety without GC
- Zero-cost abstractions
- Direct syscall access
- Excellent performance

---

## 2. Value Representation

### 2.1 NaN-Boxing Overview

All values are represented as 64-bit quantities using NaN-boxing:

```rust
/// A Lira value (64 bits)
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Value(u64);
```

IEEE 754 double-precision floats use a specific bit pattern for NaN:
- Sign bit: 1 bit
- Exponent: 11 bits (all 1s for NaN)
- Mantissa: 52 bits (non-zero for NaN)

We use the "quiet NaN" space to encode non-float values:

```
┌─────────────────────────────────────────────────────────────────┐
│                    NaN-BOXING BIT LAYOUT                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Float64 (normal):                                               │
│  ┌─┬───────────┬────────────────────────────────────────────────┐│
│  │S│ Exponent  │                  Mantissa                       ││
│  │ │ (11 bits) │                 (52 bits)                       ││
│  └─┴───────────┴────────────────────────────────────────────────┘│
│                                                                  │
│  Tagged Value (uses quiet NaN space):                            │
│  ┌─┬───────────┬───┬────────┬───────────────────────────────────┐│
│  │0│11111111111│ 1 │  Tag   │              Payload               ││
│  │ │  (NaN)    │QNaN│(4 bits)│            (48 bits)              ││
│  └─┴───────────┴───┴────────┴───────────────────────────────────┘│
│                                                                  │
│  Pointer (48-bit address):                                       │
│  ┌─┬───────────┬───┬────────┬───────────────────────────────────┐│
│  │0│11111111111│ 1 │  0000  │         48-bit pointer             ││
│  └─┴───────────┴───┴────────┴───────────────────────────────────┘│
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Tag Values

```rust
const QNAN: u64         = 0x7FF8_0000_0000_0000;
const TAG_MASK: u64     = 0xFFFF_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Tag definitions (4 bits after quiet NaN marker)
const TAG_POINTER: u64  = 0x7FF8_0000_0000_0000; // 0000
const TAG_INT32: u64    = 0x7FF8_0001_0000_0000; // 0001
const TAG_BOOL: u64     = 0x7FF8_0002_0000_0000; // 0010
const TAG_NULL: u64     = 0x7FF8_0003_0000_0000; // 0011
const TAG_CHAR: u64     = 0x7FF8_0004_0000_0000; // 0100
const TAG_UNDEFINED: u64= 0x7FF8_0005_0000_0000; // 0101
const TAG_INT64_PTR: u64= 0x7FF8_0006_0000_0000; // 0110 (boxed)
const TAG_SPECIAL: u64  = 0x7FF8_000F_0000_0000; // 1111

// Special values
const VALUE_NULL: u64   = TAG_NULL;
const VALUE_TRUE: u64   = TAG_BOOL | 1;
const VALUE_FALSE: u64  = TAG_BOOL | 0;
const VALUE_UNDEFINED: u64 = TAG_UNDEFINED;
```

### 2.3 Value Operations

```rust
impl Value {
    /// Check if value is a float
    #[inline]
    pub fn is_float(self) -> bool {
        (self.0 & QNAN) != QNAN
    }

    /// Check if value is a pointer
    #[inline]
    pub fn is_pointer(self) -> bool {
        (self.0 & TAG_MASK) == TAG_POINTER
    }

    /// Check if value is null
    #[inline]
    pub fn is_null(self) -> bool {
        self.0 == VALUE_NULL
    }

    /// Check if value is a boolean
    #[inline]
    pub fn is_bool(self) -> bool {
        (self.0 & TAG_MASK) == TAG_BOOL
    }

    /// Check if value is an int32
    #[inline]
    pub fn is_int(self) -> bool {
        (self.0 & TAG_MASK) == TAG_INT32
    }

    /// Check if value is a character
    #[inline]
    pub fn is_char(self) -> bool {
        (self.0 & TAG_MASK) == TAG_CHAR
    }

    /// Create float value
    #[inline]
    pub fn from_float(f: f64) -> Self {
        Value(f.to_bits())
    }

    /// Extract float value
    #[inline]
    pub fn as_float(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Create int32 value
    #[inline]
    pub fn from_int(i: i32) -> Self {
        Value(TAG_INT32 | (i as u32 as u64))
    }

    /// Extract int32 value
    #[inline]
    pub fn as_int(self) -> i32 {
        (self.0 & 0xFFFFFFFF) as u32 as i32
    }

    /// Create pointer value
    #[inline]
    pub fn from_pointer(ptr: *mut Object) -> Self {
        Value(TAG_POINTER | (ptr as u64 & PAYLOAD_MASK))
    }

    /// Extract pointer value
    #[inline]
    pub fn as_pointer<T>(self) -> *mut T {
        (self.0 & PAYLOAD_MASK) as *mut T
    }

    /// Create boolean value
    #[inline]
    pub fn from_bool(b: bool) -> Self {
        Value(if b { VALUE_TRUE } else { VALUE_FALSE })
    }

    /// Extract boolean value
    #[inline]
    pub fn as_bool(self) -> bool {
        self.0 == VALUE_TRUE
    }

    /// Create character value
    #[inline]
    pub fn from_char(c: char) -> Self {
        Value(TAG_CHAR | (c as u64))
    }

    /// Extract character value
    #[inline]
    pub fn as_char(self) -> char {
        char::from_u32((self.0 & 0x1FFFFF) as u32).unwrap_or('\u{FFFD}')
    }

    /// Create null value
    #[inline]
    pub fn null() -> Self {
        Value(VALUE_NULL)
    }
}
```

### 2.4 Type Checking at Runtime

```rust
impl Value {
    /// Get runtime type tag
    pub fn type_tag(self) -> TypeTag {
        if self.is_float() {
            TypeTag::Float64
        } else if self.is_pointer() {
            // Read object header for actual type
            let obj = unsafe { &*self.as_pointer::<Object>() };
            obj.header.type_tag
        } else if self.is_int() {
            TypeTag::Int32
        } else if self.is_bool() {
            TypeTag::Bool
        } else if self.is_null() {
            TypeTag::Null
        } else if self.is_char() {
            TypeTag::Char
        } else {
            TypeTag::Unknown
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypeTag {
    Null = 0,
    Bool = 1,
    Int32 = 2,
    Int64 = 3,
    Float32 = 4,
    Float64 = 5,
    Char = 6,
    String = 7,
    Array = 8,
    Object = 9,
    Function = 10,
    Closure = 11,
    Fiber = 12,
    Channel = 13,
    Unknown = 255,
}
```

---

## 3. Object Model

### 3.1 Object Header

Every heap-allocated object has a 16-byte header:

```rust
#[repr(C)]
struct ObjectHeader {
    /// Reference count (63 bits) + color flag (1 bit)
    refcount_and_color: u64,

    /// Type information pointer
    type_info: *const TypeInfo,
}

impl ObjectHeader {
    const COLOR_MASK: u64 = 1;
    const REFCOUNT_MASK: u64 = !1;

    #[inline]
    pub fn refcount(&self) -> u64 {
        self.refcount_and_color >> 1
    }

    #[inline]
    pub fn increment_refcount(&mut self) {
        self.refcount_and_color += 2; // Add 2 to skip color bit
    }

    #[inline]
    pub fn decrement_refcount(&mut self) -> u64 {
        self.refcount_and_color -= 2;
        self.refcount()
    }

    #[inline]
    pub fn color(&self) -> GCColor {
        if self.refcount_and_color & Self::COLOR_MASK == 0 {
            GCColor::White
        } else {
            GCColor::Gray
        }
    }

    #[inline]
    pub fn set_color(&mut self, color: GCColor) {
        match color {
            GCColor::White => self.refcount_and_color &= Self::REFCOUNT_MASK,
            GCColor::Gray => self.refcount_and_color |= Self::COLOR_MASK,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GCColor {
    White, // Not visited
    Gray,  // In cycle detection queue
}
```

### 3.2 Type Information

```rust
#[repr(C)]
struct TypeInfo {
    /// Type name (for debugging)
    name: *const u8,
    name_len: u32,

    /// Type tag
    tag: TypeTag,

    /// Size of instance (excluding header)
    instance_size: u32,

    /// Number of reference fields
    ref_field_count: u16,

    /// Offsets to reference fields (for GC tracing)
    ref_field_offsets: *const u16,

    /// Virtual method table
    vtable: *const VTableEntry,
    vtable_len: u16,

    /// Flags
    flags: TypeFlags,

    /// Parent type (for inheritance)
    parent: *const TypeInfo,

    /// Interface list
    interfaces: *const *const TypeInfo,
    interface_count: u16,
}

#[repr(transparent)]
struct TypeFlags(u16);

impl TypeFlags {
    const ABSTRACT: u16     = 1 << 0;
    const FINAL: u16        = 1 << 1;
    const INTERFACE: u16    = 1 << 2;
    const PRIMITIVE: u16    = 1 << 3;
    const ARRAY: u16        = 1 << 4;
    const BUILTIN: u16      = 1 << 5;
}

#[repr(C)]
struct VTableEntry {
    /// Method name hash
    name_hash: u32,
    /// Function pointer
    func: *const u8,
}
```

### 3.3 Object Layout

```
┌─────────────────────────────────────────────────────────────────┐
│                      OBJECT LAYOUT                               │
├─────────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                 OBJECT HEADER (16 bytes)                   │  │
│  │  ┌─────────────────────┐  ┌─────────────────────┐         │  │
│  │  │ refcount + color    │  │   type_info ptr     │         │  │
│  │  │     (8 bytes)       │  │    (8 bytes)        │         │  │
│  │  └─────────────────────┘  └─────────────────────┘         │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    INSTANCE DATA                           │  │
│  │  Field layout depends on type:                            │  │
│  │  - Primitives: inline values                              │  │
│  │  - References: Value (8 bytes each)                       │  │
│  │  - Alignment: 8-byte aligned                              │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.4 String Object

```rust
#[repr(C)]
struct StringObject {
    header: ObjectHeader,
    /// Length in bytes
    len: u32,
    /// Hash (cached, 0 if not computed)
    hash: u32,
    /// UTF-8 data follows (inline)
    data: [u8; 0],
}

impl StringObject {
    pub fn alloc(vm: &mut VM, s: &str) -> Value {
        let size = size_of::<StringObject>() + s.len();
        let ptr = vm.heap.alloc(size, &STRING_TYPE);
        unsafe {
            let obj = &mut *ptr.cast::<StringObject>();
            obj.len = s.len() as u32;
            obj.hash = 0;
            ptr::copy_nonoverlapping(
                s.as_ptr(),
                obj.data.as_mut_ptr(),
                s.len()
            );
        }
        Value::from_pointer(ptr)
    }

    pub fn as_str(&self) -> &str {
        unsafe {
            let bytes = slice::from_raw_parts(
                self.data.as_ptr(),
                self.len as usize
            );
            str::from_utf8_unchecked(bytes)
        }
    }

    pub fn hash(&mut self) -> u32 {
        if self.hash == 0 {
            self.hash = fnv1a_hash(self.as_str().as_bytes());
            if self.hash == 0 {
                self.hash = 1; // Avoid rehashing
            }
        }
        self.hash
    }
}
```

### 3.5 Array Object

```rust
#[repr(C)]
struct ArrayObject {
    header: ObjectHeader,
    /// Number of elements
    len: u32,
    /// Capacity (for dynamic arrays)
    capacity: u32,
    /// Element type info
    elem_type: *const TypeInfo,
    /// Elements follow (inline)
    data: [Value; 0],
}

impl ArrayObject {
    pub fn alloc(vm: &mut VM, elem_type: &TypeInfo, len: usize) -> Value {
        let size = size_of::<ArrayObject>() + len * size_of::<Value>();
        let ptr = vm.heap.alloc(size, &ARRAY_TYPE);
        unsafe {
            let obj = &mut *ptr.cast::<ArrayObject>();
            obj.len = len as u32;
            obj.capacity = len as u32;
            obj.elem_type = elem_type;
            // Zero-initialize elements
            ptr::write_bytes(obj.data.as_mut_ptr(), 0, len);
        }
        Value::from_pointer(ptr)
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<Value> {
        if index < self.len as usize {
            Some(unsafe { *self.data.as_ptr().add(index) })
        } else {
            None
        }
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: Value) -> bool {
        if index < self.len as usize {
            unsafe { *self.data.as_mut_ptr().add(index) = value };
            true
        } else {
            false
        }
    }
}
```

### 3.6 Closure Object

```rust
#[repr(C)]
struct ClosureObject {
    header: ObjectHeader,
    /// Function index
    func_index: u32,
    /// Number of captured variables
    capture_count: u16,
    /// Padding
    _padding: u16,
    /// Captured values (inline)
    captures: [Value; 0],
}

impl ClosureObject {
    pub fn alloc(vm: &mut VM, func_index: u32, captures: &[Value]) -> Value {
        let size = size_of::<ClosureObject>() + captures.len() * size_of::<Value>();
        let ptr = vm.heap.alloc(size, &CLOSURE_TYPE);
        unsafe {
            let obj = &mut *ptr.cast::<ClosureObject>();
            obj.func_index = func_index;
            obj.capture_count = captures.len() as u16;
            ptr::copy_nonoverlapping(
                captures.as_ptr(),
                obj.captures.as_mut_ptr(),
                captures.len()
            );
        }
        Value::from_pointer(ptr)
    }
}
```

---

## 4. Call Stack

### 4.1 Call Frame Structure

```rust
#[repr(C)]
struct CallFrame {
    /// Return address (instruction pointer in caller)
    return_pc: u32,

    /// Caller's frame pointer (index in frame stack)
    caller_fp: u32,

    /// Function being executed
    function: *const FunctionDef,

    /// Base of local variables in value stack
    locals_base: u32,

    /// Base of operand stack in value stack
    stack_base: u32,

    /// Exception handler chain (index, 0xFFFFFFFF if none)
    exception_handler: u32,

    /// Flags
    flags: FrameFlags,
}

#[repr(transparent)]
struct FrameFlags(u8);

impl FrameFlags {
    const NATIVE: u8    = 1 << 0; // Native function call
    const TAILCALL: u8  = 1 << 1; // Tail call optimization active
    const CATCH: u8     = 1 << 2; // Has active exception handler
}
```

### 4.2 Frame Stack

```rust
struct FrameStack {
    /// Array of call frames
    frames: Vec<CallFrame>,

    /// Current frame pointer (index into frames)
    fp: usize,

    /// Maximum stack depth
    max_depth: usize,
}

impl FrameStack {
    pub fn new(max_depth: usize) -> Self {
        Self {
            frames: Vec::with_capacity(256),
            fp: 0,
            max_depth,
        }
    }

    pub fn push(&mut self, frame: CallFrame) -> Result<(), VMError> {
        if self.frames.len() >= self.max_depth {
            return Err(VMError::StackOverflow);
        }
        self.frames.push(frame);
        self.fp = self.frames.len() - 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<CallFrame> {
        let frame = self.frames.pop()?;
        self.fp = frame.caller_fp as usize;
        Some(frame)
    }

    pub fn current(&self) -> &CallFrame {
        &self.frames[self.fp]
    }

    pub fn current_mut(&mut self) -> &mut CallFrame {
        &mut self.frames[self.fp]
    }
}
```

### 4.3 Value Stack

```rust
struct ValueStack {
    /// Array of values
    values: Vec<Value>,

    /// Stack pointer (index of next free slot)
    sp: usize,

    /// Maximum stack size
    max_size: usize,
}

impl ValueStack {
    pub fn new(max_size: usize) -> Self {
        Self {
            values: vec![Value::null(); 1024],
            sp: 0,
            max_size,
        }
    }

    #[inline]
    pub fn push(&mut self, value: Value) -> Result<(), VMError> {
        if self.sp >= self.max_size {
            return Err(VMError::StackOverflow);
        }
        if self.sp >= self.values.len() {
            self.values.resize(self.values.len() * 2, Value::null());
        }
        self.values[self.sp] = value;
        self.sp += 1;
        Ok(())
    }

    #[inline]
    pub fn pop(&mut self) -> Value {
        debug_assert!(self.sp > 0);
        self.sp -= 1;
        self.values[self.sp]
    }

    #[inline]
    pub fn peek(&self, offset: usize) -> Value {
        self.values[self.sp - 1 - offset]
    }

    #[inline]
    pub fn get_local(&self, base: usize, index: usize) -> Value {
        self.values[base + index]
    }

    #[inline]
    pub fn set_local(&mut self, base: usize, index: usize, value: Value) {
        self.values[base + index] = value;
    }
}
```

---

## 5. Fiber System

### 5.1 Fiber Structure

```rust
#[repr(C)]
struct Fiber {
    header: ObjectHeader,

    /// Fiber state
    state: FiberState,

    /// Fiber ID (unique)
    id: u64,

    /// Call frame stack
    frames: FrameStack,

    /// Value stack
    stack: ValueStack,

    /// Current instruction pointer
    pc: u32,

    /// Current function
    function: *const FunctionDef,

    /// Result value (when completed)
    result: Value,

    /// Error value (when failed)
    error: Value,

    /// Parent fiber (for structured concurrency)
    parent: *mut Fiber,

    /// Children fibers
    children: Vec<*mut Fiber>,

    /// Blocked on (channel, mutex, etc.)
    blocked_on: *mut dyn Blocker,

    /// Wake-up time (for sleep)
    wake_time: Option<Instant>,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    Created = 0,
    Running = 1,
    Suspended = 2,
    Blocked = 3,
    Completed = 4,
    Failed = 5,
    Cancelled = 6,
}
```

### 5.2 Fiber Scheduler

```rust
struct Scheduler {
    /// Ready queue (round-robin)
    ready_queue: VecDeque<*mut Fiber>,

    /// Blocked fibers (waiting on I/O, channels, etc.)
    blocked: HashSet<*mut Fiber>,

    /// Sleeping fibers (sorted by wake time)
    sleeping: BinaryHeap<SleepEntry>,

    /// Currently running fiber
    current: *mut Fiber,

    /// Main fiber
    main_fiber: *mut Fiber,

    /// Next fiber ID
    next_id: u64,

    /// Total fiber count
    fiber_count: usize,
}

struct SleepEntry {
    fiber: *mut Fiber,
    wake_time: Instant,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            blocked: HashSet::new(),
            sleeping: BinaryHeap::new(),
            current: ptr::null_mut(),
            main_fiber: ptr::null_mut(),
            next_id: 1,
            fiber_count: 0,
        }
    }

    /// Spawn a new fiber
    pub fn spawn(&mut self, vm: &mut VM, func_index: u32, args: &[Value]) -> *mut Fiber {
        let fiber = Fiber::new(self.next_id, func_index, args);
        self.next_id += 1;
        self.fiber_count += 1;

        let ptr = Box::into_raw(Box::new(fiber));
        self.ready_queue.push_back(ptr);
        ptr
    }

    /// Yield current fiber
    pub fn yield_current(&mut self) {
        if !self.current.is_null() {
            unsafe {
                (*self.current).state = FiberState::Suspended;
            }
            self.ready_queue.push_back(self.current);
        }
    }

    /// Block current fiber
    pub fn block_current(&mut self, blocker: *mut dyn Blocker) {
        if !self.current.is_null() {
            unsafe {
                (*self.current).state = FiberState::Blocked;
                (*self.current).blocked_on = blocker;
            }
            self.blocked.insert(self.current);
        }
    }

    /// Wake a blocked fiber
    pub fn wake(&mut self, fiber: *mut Fiber) {
        if self.blocked.remove(&fiber) {
            unsafe {
                (*fiber).state = FiberState::Suspended;
                (*fiber).blocked_on = ptr::null_mut();
            }
            self.ready_queue.push_back(fiber);
        }
    }

    /// Sleep current fiber
    pub fn sleep_current(&mut self, duration: Duration) {
        if !self.current.is_null() {
            let wake_time = Instant::now() + duration;
            unsafe {
                (*self.current).state = FiberState::Blocked;
                (*self.current).wake_time = Some(wake_time);
            }
            self.sleeping.push(SleepEntry {
                fiber: self.current,
                wake_time,
            });
        }
    }

    /// Select next fiber to run
    pub fn schedule(&mut self) -> Option<*mut Fiber> {
        // Wake sleeping fibers
        let now = Instant::now();
        while let Some(entry) = self.sleeping.peek() {
            if entry.wake_time <= now {
                let entry = self.sleeping.pop().unwrap();
                unsafe {
                    (*entry.fiber).state = FiberState::Suspended;
                    (*entry.fiber).wake_time = None;
                }
                self.ready_queue.push_back(entry.fiber);
            } else {
                break;
            }
        }

        // Get next ready fiber
        self.ready_queue.pop_front()
    }

    /// Run the scheduler
    pub fn run(&mut self, vm: &mut VM) -> Result<Value, VMError> {
        while let Some(fiber) = self.schedule() {
            self.current = fiber;
            unsafe {
                (*fiber).state = FiberState::Running;
            }

            // Execute fiber until it yields, blocks, or completes
            match vm.execute_fiber(fiber) {
                Ok(FiberResult::Yield) => {
                    self.yield_current();
                }
                Ok(FiberResult::Complete(value)) => {
                    unsafe {
                        (*fiber).state = FiberState::Completed;
                        (*fiber).result = value;
                    }
                    self.fiber_count -= 1;

                    // Wake parent if waiting
                    if !unsafe { (*fiber).parent.is_null() } {
                        self.wake(unsafe { (*fiber).parent });
                    }

                    if fiber == self.main_fiber {
                        return Ok(value);
                    }
                }
                Ok(FiberResult::Block(blocker)) => {
                    self.block_current(blocker);
                }
                Err(e) => {
                    unsafe {
                        (*fiber).state = FiberState::Failed;
                        (*fiber).error = e.to_value(vm);
                    }
                    self.fiber_count -= 1;

                    if fiber == self.main_fiber {
                        return Err(e);
                    }
                }
            }

            self.current = ptr::null_mut();
        }

        // No more fibers
        Ok(Value::null())
    }
}
```

### 5.3 Fiber Result

```rust
pub enum FiberResult {
    /// Fiber yielded voluntarily
    Yield,
    /// Fiber completed with value
    Complete(Value),
    /// Fiber blocked on something
    Block(*mut dyn Blocker),
}

/// Trait for things a fiber can block on
pub trait Blocker {
    /// Check if unblocked
    fn is_ready(&self) -> bool;
    /// Get result when unblocked
    fn take_result(&mut self) -> Option<Value>;
}
```

---

## 6. Channel Implementation

### 6.1 Channel Structure

```rust
#[repr(C)]
struct Channel {
    header: ObjectHeader,

    /// Buffer capacity (0 = unbuffered)
    capacity: u32,

    /// Current buffer size
    size: u32,

    /// Read index
    read_idx: u32,

    /// Write index
    write_idx: u32,

    /// Is channel closed?
    closed: bool,

    /// Waiting senders
    send_waiters: Vec<*mut Fiber>,

    /// Waiting receivers
    recv_waiters: Vec<*mut Fiber>,

    /// Pending send value (for unbuffered)
    pending_send: Option<Value>,

    /// Buffer (for buffered channels)
    buffer: Vec<Value>,
}

impl Channel {
    pub fn new(capacity: usize) -> Self {
        Self {
            header: ObjectHeader::new(&CHANNEL_TYPE),
            capacity: capacity as u32,
            size: 0,
            read_idx: 0,
            write_idx: 0,
            closed: false,
            send_waiters: Vec::new(),
            recv_waiters: Vec::new(),
            pending_send: None,
            buffer: if capacity > 0 {
                vec![Value::null(); capacity]
            } else {
                Vec::new()
            },
        }
    }

    pub fn is_unbuffered(&self) -> bool {
        self.capacity == 0
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}
```

### 6.2 Channel Operations

```rust
impl Channel {
    /// Try to send without blocking
    pub fn try_send(&mut self, value: Value, scheduler: &mut Scheduler) -> SendResult {
        if self.closed {
            return SendResult::Closed;
        }

        if self.is_unbuffered() {
            // Unbuffered: need a receiver waiting
            if let Some(receiver) = self.recv_waiters.pop() {
                // Direct handoff
                unsafe {
                    (*receiver).result = value;
                }
                scheduler.wake(receiver);
                return SendResult::Ok;
            }
            return SendResult::WouldBlock;
        }

        // Buffered: check if space available
        if (self.size as usize) < self.buffer.len() {
            self.buffer[self.write_idx as usize] = value;
            self.write_idx = (self.write_idx + 1) % self.capacity;
            self.size += 1;

            // Wake a waiting receiver if any
            if let Some(receiver) = self.recv_waiters.pop() {
                scheduler.wake(receiver);
            }

            return SendResult::Ok;
        }

        SendResult::WouldBlock
    }

    /// Send with blocking
    pub fn send(
        &mut self,
        value: Value,
        fiber: *mut Fiber,
        scheduler: &mut Scheduler
    ) -> SendResult {
        match self.try_send(value, scheduler) {
            SendResult::WouldBlock => {
                // Store pending value and block
                if self.is_unbuffered() {
                    self.pending_send = Some(value);
                }
                self.send_waiters.push(fiber);
                scheduler.block_current(self as *mut _ as *mut dyn Blocker);
                SendResult::Blocked
            }
            result => result,
        }
    }

    /// Try to receive without blocking
    pub fn try_recv(&mut self, scheduler: &mut Scheduler) -> RecvResult {
        if self.is_unbuffered() {
            // Unbuffered: check for pending send
            if let Some(value) = self.pending_send.take() {
                // Wake the sender
                if let Some(sender) = self.send_waiters.pop() {
                    scheduler.wake(sender);
                }
                return RecvResult::Ok(value);
            }
            if self.closed {
                return RecvResult::Closed;
            }
            return RecvResult::WouldBlock;
        }

        // Buffered: check buffer
        if self.size > 0 {
            let value = self.buffer[self.read_idx as usize];
            self.buffer[self.read_idx as usize] = Value::null();
            self.read_idx = (self.read_idx + 1) % self.capacity;
            self.size -= 1;

            // Wake a waiting sender if any
            if let Some(sender) = self.send_waiters.pop() {
                scheduler.wake(sender);
            }

            return RecvResult::Ok(value);
        }

        if self.closed {
            return RecvResult::Closed;
        }

        RecvResult::WouldBlock
    }

    /// Receive with blocking
    pub fn recv(
        &mut self,
        fiber: *mut Fiber,
        scheduler: &mut Scheduler
    ) -> RecvResult {
        match self.try_recv(scheduler) {
            RecvResult::WouldBlock => {
                self.recv_waiters.push(fiber);
                scheduler.block_current(self as *mut _ as *mut dyn Blocker);
                RecvResult::Blocked
            }
            result => result,
        }
    }

    /// Close the channel
    pub fn close(&mut self, scheduler: &mut Scheduler) {
        self.closed = true;

        // Wake all waiters
        for sender in self.send_waiters.drain(..) {
            scheduler.wake(sender);
        }
        for receiver in self.recv_waiters.drain(..) {
            scheduler.wake(receiver);
        }
    }
}

pub enum SendResult {
    Ok,
    WouldBlock,
    Blocked,
    Closed,
}

pub enum RecvResult {
    Ok(Value),
    WouldBlock,
    Blocked,
    Closed,
}
```

### 6.3 Select Implementation

```rust
struct SelectCase {
    channel: *mut Channel,
    op: SelectOp,
    value: Value, // For send operations
}

enum SelectOp {
    Send,
    Recv,
}

fn select(
    cases: &[SelectCase],
    has_default: bool,
    fiber: *mut Fiber,
    scheduler: &mut Scheduler,
) -> SelectResult {
    // First pass: try non-blocking operations
    let mut ready_indices: Vec<usize> = Vec::new();

    for (i, case) in cases.iter().enumerate() {
        let channel = unsafe { &mut *case.channel };

        match case.op {
            SelectOp::Send => {
                if channel.try_send(case.value, scheduler) == SendResult::Ok {
                    ready_indices.push(i);
                }
            }
            SelectOp::Recv => {
                if let RecvResult::Ok(value) = channel.try_recv(scheduler) {
                    return SelectResult::Ready(i, Some(value));
                }
            }
        }
    }

    // If any sends succeeded, pick one randomly
    if !ready_indices.is_empty() {
        let idx = ready_indices[random() % ready_indices.len()];
        return SelectResult::Ready(idx, None);
    }

    // If has default, use it
    if has_default {
        return SelectResult::Default;
    }

    // Block on all channels
    for case in cases {
        let channel = unsafe { &mut *case.channel };
        match case.op {
            SelectOp::Send => channel.send_waiters.push(fiber),
            SelectOp::Recv => channel.recv_waiters.push(fiber),
        }
    }

    scheduler.block_current(ptr::null_mut()); // Special select blocking
    SelectResult::Blocked
}

enum SelectResult {
    Ready(usize, Option<Value>),
    Default,
    Blocked,
}
```

---

## 7. Exception Handling

### 7.1 Exception Structure

```rust
#[repr(C)]
struct Exception {
    header: ObjectHeader,
    /// Exception type
    exception_type: *const TypeInfo,
    /// Error message
    message: Value, // String
    /// Stack trace
    stack_trace: Value, // Array of StackFrame
    /// Cause (chained exception)
    cause: Value, // Exception or null
}

#[repr(C)]
struct StackTraceEntry {
    /// Function name
    function_name: Value, // String
    /// File name
    file_name: Value, // String
    /// Line number
    line: u32,
    /// Column number
    column: u32,
}
```

### 7.2 Exception Handler Table

```rust
struct ExceptionHandlerTable {
    handlers: Vec<ExceptionHandler>,
}

struct ExceptionHandler {
    /// Start of protected region (bytecode offset)
    try_start: u32,
    /// End of protected region
    try_end: u32,
    /// Handler entry point
    handler_pc: u32,
    /// Caught exception type (null = catch all)
    catch_type: *const TypeInfo,
}

impl ExceptionHandlerTable {
    pub fn find_handler(
        &self,
        pc: u32,
        exception_type: *const TypeInfo
    ) -> Option<&ExceptionHandler> {
        for handler in &self.handlers {
            if pc >= handler.try_start && pc < handler.try_end {
                if handler.catch_type.is_null() ||
                   is_subtype(exception_type, handler.catch_type) {
                    return Some(handler);
                }
            }
        }
        None
    }
}
```

### 7.3 Exception Unwinding

```rust
impl VM {
    fn throw_exception(&mut self, exception: Value) -> Result<(), VMError> {
        let exc_obj = unsafe { &*exception.as_pointer::<Exception>() };

        // Build stack trace
        self.build_stack_trace(exc_obj);

        // Unwind stack looking for handler
        loop {
            let frame = self.frames.current();
            let function = unsafe { &*frame.function };

            // Check for handler in current frame
            if let Some(handler) = function.exception_table.find_handler(
                self.pc,
                exc_obj.exception_type
            ) {
                // Found handler
                self.pc = handler.handler_pc;
                self.stack.sp = frame.stack_base as usize;
                self.stack.push(exception)?;
                return Ok(());
            }

            // Pop frame
            if let Some(popped) = self.frames.pop() {
                self.pc = popped.return_pc;
            } else {
                // No handler found - propagate to runtime
                return Err(VMError::UnhandledException(exception));
            }
        }
    }

    fn build_stack_trace(&mut self, exception: &mut Exception) {
        let mut entries = Vec::new();

        for frame in self.frames.frames.iter().rev() {
            let function = unsafe { &*frame.function };

            let entry = StackTraceEntry {
                function_name: self.intern_string(function.name()),
                file_name: self.get_source_file(frame),
                line: self.get_line_number(frame),
                column: self.get_column_number(frame),
            };
            entries.push(entry);
        }

        exception.stack_trace = self.create_array(&entries);
    }
}
```

---

## 8. Execution Engine

### 8.1 Main Interpreter Loop

```rust
impl VM {
    pub fn execute(&mut self) -> Result<Value, VMError> {
        loop {
            let opcode = self.fetch_u8();

            match opcode {
                // Stack operations
                0x00 => { /* NOP */ }
                0x01 => { self.stack.pop(); }
                0x03 => {
                    let v = self.stack.peek(0);
                    self.stack.push(v)?;
                }
                0x07 => {
                    let b = self.stack.pop();
                    let a = self.stack.pop();
                    self.stack.push(b)?;
                    self.stack.push(a)?;
                }

                // Local variables
                0x10 => {
                    let idx = self.fetch_u8() as usize;
                    let base = self.frames.current().locals_base as usize;
                    let v = self.stack.get_local(base, idx);
                    self.stack.push(v)?;
                }
                0x16 => {
                    let idx = self.fetch_u8() as usize;
                    let base = self.frames.current().locals_base as usize;
                    let v = self.stack.pop();
                    self.stack.set_local(base, idx, v);
                }

                // Constants
                0x20 => self.stack.push(Value::null())?,
                0x21 => self.stack.push(Value::from_bool(true))?,
                0x22 => self.stack.push(Value::from_bool(false))?,
                0x23 => self.stack.push(Value::from_int(0))?,
                0x24 => self.stack.push(Value::from_int(1))?,
                0x29 => {
                    let v = self.fetch_i8() as i32;
                    self.stack.push(Value::from_int(v))?;
                }
                0x2B => {
                    let idx = self.fetch_u8() as usize;
                    let v = self.module.const_pool.get(idx);
                    self.stack.push(v)?;
                }

                // Integer arithmetic
                0x30 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    self.stack.push(Value::from_int(a.wrapping_add(b)))?;
                }
                0x31 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    self.stack.push(Value::from_int(a.wrapping_sub(b)))?;
                }
                0x32 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    self.stack.push(Value::from_int(a.wrapping_mul(b)))?;
                }
                0x33 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    if b == 0 {
                        return Err(VMError::DivisionByZero);
                    }
                    self.stack.push(Value::from_int(a / b))?;
                }

                // Float arithmetic
                0x46 => {
                    let b = self.stack.pop().as_float();
                    let a = self.stack.pop().as_float();
                    self.stack.push(Value::from_float(a + b))?;
                }

                // Comparisons
                0x60 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    self.stack.push(Value::from_bool(a == b))?;
                }
                0x62 => {
                    let b = self.stack.pop().as_int();
                    let a = self.stack.pop().as_int();
                    self.stack.push(Value::from_bool(a < b))?;
                }

                // Control flow
                0x80 => {
                    let offset = self.fetch_i16();
                    self.pc = (self.pc as i32 + offset as i32) as u32;
                }
                0x82 => {
                    let offset = self.fetch_i16();
                    let cond = self.stack.pop().as_bool();
                    if cond {
                        self.pc = (self.pc as i32 + offset as i32) as u32;
                    }
                }
                0x83 => {
                    let offset = self.fetch_i16();
                    let cond = self.stack.pop().as_bool();
                    if !cond {
                        self.pc = (self.pc as i32 + offset as i32) as u32;
                    }
                }

                // Function calls
                0x90 => {
                    let func_idx = self.fetch_u16() as usize;
                    self.invoke(func_idx)?;
                }
                0x96 => {
                    let value = self.stack.pop();
                    if let Some(frame) = self.frames.pop() {
                        self.pc = frame.return_pc;
                        // Clean up locals
                        self.stack.sp = frame.locals_base as usize;
                        self.stack.push(value)?;

                        if self.frames.frames.is_empty() {
                            return Ok(value);
                        }
                    }
                }
                0x97 => {
                    if let Some(frame) = self.frames.pop() {
                        self.pc = frame.return_pc;
                        self.stack.sp = frame.locals_base as usize;

                        if self.frames.frames.is_empty() {
                            return Ok(Value::null());
                        }
                    }
                }

                // Object operations
                0xA0 => {
                    let type_idx = self.fetch_u16() as usize;
                    let obj = self.allocate_object(type_idx)?;
                    self.stack.push(obj)?;
                }
                0xA1 => {
                    let field_idx = self.fetch_u16() as usize;
                    let obj = self.stack.pop();
                    if obj.is_null() {
                        return Err(VMError::NullPointer);
                    }
                    let value = self.get_field(obj, field_idx);
                    self.stack.push(value)?;
                }
                0xA2 => {
                    let field_idx = self.fetch_u16() as usize;
                    let value = self.stack.pop();
                    let obj = self.stack.pop();
                    if obj.is_null() {
                        return Err(VMError::NullPointer);
                    }
                    self.set_field(obj, field_idx, value);
                }

                // Array operations
                0xB0 => {
                    let elem_type = self.fetch_u8();
                    let len = self.stack.pop().as_int() as usize;
                    let arr = self.allocate_primitive_array(elem_type, len)?;
                    self.stack.push(arr)?;
                }
                0xB3 => {
                    let idx = self.stack.pop().as_int() as usize;
                    let arr = self.stack.pop();
                    let value = self.array_get(arr, idx)?;
                    self.stack.push(value)?;
                }
                0xB4 => {
                    let value = self.stack.pop();
                    let idx = self.stack.pop().as_int() as usize;
                    let arr = self.stack.pop();
                    self.array_set(arr, idx, value)?;
                }

                // Reference counting
                0xD0 => {
                    let obj = self.stack.peek(0);
                    if obj.is_pointer() {
                        self.inc_ref(obj);
                    }
                }
                0xD1 => {
                    let obj = self.stack.pop();
                    if obj.is_pointer() {
                        self.dec_ref(obj)?;
                    }
                }

                // Channel operations
                0xD8 => {
                    let capacity = self.stack.pop().as_int() as usize;
                    let ch = self.allocate_channel(capacity)?;
                    self.stack.push(ch)?;
                }
                0xD9 => {
                    let value = self.stack.pop();
                    let ch = self.stack.pop();
                    self.channel_send(ch, value)?;
                }
                0xDA => {
                    let ch = self.stack.pop();
                    let value = self.channel_recv(ch)?;
                    self.stack.push(value)?;
                }

                // Fiber operations
                0xE0 => {
                    let func_idx = self.fetch_u16();
                    let fiber = self.spawn_fiber(func_idx)?;
                    self.stack.push(fiber)?;
                }
                0xE1 => {
                    return Ok(Value::null()); // Yield
                }
                0xE3 => {
                    let fiber = self.stack.pop();
                    let result = self.join_fiber(fiber)?;
                    self.stack.push(result)?;
                }

                // Syscall
                0xE8 => {
                    let syscall_num = self.fetch_u16();
                    let result = self.syscall(syscall_num)?;
                    self.stack.push(result)?;
                }

                // Exception handling
                0xF0 => {
                    let exception = self.stack.pop();
                    self.throw_exception(exception)?;
                }

                // Miscellaneous
                0xF4 => {
                    self.breakpoint()?;
                }

                0xFF => {
                    return Err(VMError::IllegalInstruction);
                }

                _ => {
                    return Err(VMError::UnknownOpcode(opcode));
                }
            }
        }
    }

    #[inline]
    fn fetch_u8(&mut self) -> u8 {
        let v = self.bytecode[self.pc as usize];
        self.pc += 1;
        v
    }

    #[inline]
    fn fetch_i8(&mut self) -> i8 {
        self.fetch_u8() as i8
    }

    #[inline]
    fn fetch_u16(&mut self) -> u16 {
        let lo = self.fetch_u8() as u16;
        let hi = self.fetch_u8() as u16;
        lo | (hi << 8)
    }

    #[inline]
    fn fetch_i16(&mut self) -> i16 {
        self.fetch_u16() as i16
    }

    #[inline]
    fn fetch_u32(&mut self) -> u32 {
        let a = self.fetch_u8() as u32;
        let b = self.fetch_u8() as u32;
        let c = self.fetch_u8() as u32;
        let d = self.fetch_u8() as u32;
        a | (b << 8) | (c << 16) | (d << 24)
    }
}
```

### 8.2 Function Invocation

```rust
impl VM {
    fn invoke(&mut self, func_idx: usize) -> Result<(), VMError> {
        let function = &self.module.functions[func_idx];

        // Calculate argument count from signature
        let arg_count = function.param_count as usize;

        // Save current state
        let return_pc = self.pc;
        let caller_fp = self.frames.fp as u32;

        // Set up new frame
        let locals_base = self.stack.sp - arg_count;
        let stack_base = locals_base + function.local_count as usize;

        // Extend stack for locals
        for _ in arg_count..function.local_count as usize {
            self.stack.push(Value::null())?;
        }

        let frame = CallFrame {
            return_pc,
            caller_fp,
            function,
            locals_base: locals_base as u32,
            stack_base: stack_base as u32,
            exception_handler: 0xFFFFFFFF,
            flags: FrameFlags(0),
        };

        self.frames.push(frame)?;
        self.pc = function.code_offset;

        Ok(())
    }
}
```

---

## 9. Runtime Services

### 9.1 String Interning

```rust
struct StringPool {
    /// Interned strings (hash -> string object)
    interned: HashMap<u64, Value>,
}

impl StringPool {
    pub fn intern(&mut self, vm: &mut VM, s: &str) -> Value {
        let hash = fnv1a_hash(s.as_bytes());

        if let Some(&value) = self.interned.get(&hash) {
            return value;
        }

        let string = StringObject::alloc(vm, s);
        self.interned.insert(hash, string);
        string
    }

    pub fn get(&self, hash: u64) -> Option<Value> {
        self.interned.get(&hash).copied()
    }
}

fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
```

### 9.2 Type Registry

```rust
struct TypeRegistry {
    /// All registered types
    types: Vec<Box<TypeInfo>>,

    /// Type name to index
    name_index: HashMap<String, usize>,

    /// Built-in types
    pub int_type: usize,
    pub float_type: usize,
    pub bool_type: usize,
    pub string_type: usize,
    pub array_type: usize,
    pub object_type: usize,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            types: Vec::new(),
            name_index: HashMap::new(),
            int_type: 0,
            float_type: 0,
            bool_type: 0,
            string_type: 0,
            array_type: 0,
            object_type: 0,
        };

        // Register built-in types
        registry.int_type = registry.register_builtin("int", TypeTag::Int32);
        registry.float_type = registry.register_builtin("float", TypeTag::Float64);
        registry.bool_type = registry.register_builtin("bool", TypeTag::Bool);
        registry.string_type = registry.register_builtin("string", TypeTag::String);
        registry.array_type = registry.register_builtin("Array", TypeTag::Array);
        registry.object_type = registry.register_builtin("Object", TypeTag::Object);

        registry
    }

    fn register_builtin(&mut self, name: &str, tag: TypeTag) -> usize {
        let type_info = Box::new(TypeInfo {
            name: name.as_ptr(),
            name_len: name.len() as u32,
            tag,
            instance_size: 0,
            ref_field_count: 0,
            ref_field_offsets: ptr::null(),
            vtable: ptr::null(),
            vtable_len: 0,
            flags: TypeFlags(TypeFlags::BUILTIN | TypeFlags::PRIMITIVE),
            parent: ptr::null(),
            interfaces: ptr::null(),
            interface_count: 0,
        });

        let idx = self.types.len();
        self.types.push(type_info);
        self.name_index.insert(name.to_string(), idx);
        idx
    }

    pub fn register(&mut self, type_info: TypeInfo) -> usize {
        let name = unsafe {
            let slice = slice::from_raw_parts(type_info.name, type_info.name_len as usize);
            String::from_utf8_lossy(slice).to_string()
        };

        let idx = self.types.len();
        self.types.push(Box::new(type_info));
        self.name_index.insert(name, idx);
        idx
    }

    pub fn get(&self, idx: usize) -> &TypeInfo {
        &self.types[idx]
    }

    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        self.name_index.get(name).copied()
    }
}
```

### 9.3 Module Loader

```rust
struct ModuleLoader {
    /// Loaded modules
    modules: HashMap<String, Module>,

    /// Module search paths
    search_paths: Vec<PathBuf>,
}

impl ModuleLoader {
    pub fn load(&mut self, name: &str) -> Result<&Module, VMError> {
        if let Some(module) = self.modules.get(name) {
            return Ok(module);
        }

        // Find module file
        let path = self.find_module(name)?;

        // Read and parse
        let data = fs::read(&path)?;
        let module = Module::parse(&data)?;

        // Validate
        module.validate()?;

        // Register
        self.modules.insert(name.to_string(), module);
        Ok(self.modules.get(name).unwrap())
    }

    fn find_module(&self, name: &str) -> Result<PathBuf, VMError> {
        let filename = format!("{}.lic", name.replace('.', "/"));

        for base in &self.search_paths {
            let path = base.join(&filename);
            if path.exists() {
                return Ok(path);
            }
        }

        Err(VMError::ModuleNotFound(name.to_string()))
    }
}
```

### 9.4 Syscall Interface

```rust
impl VM {
    /// Syscall handler for host primitives
    pub fn syscall(&mut self, num: u16) -> Result<Value, VMError> {
        match num {
            // sys_exit (0)
            0x0000 => {
                let code = self.stack.pop().as_int();
                Err(VMError::Exit(code))
            }

            // sys_read (34 = 0x22)
            0x0022 => {
                let len = self.stack.pop().as_int() as usize;
                let buf = self.stack.pop();
                let fd = self.stack.pop().as_int() as u32;

                let buf_ptr = unsafe { &mut *buf.as_pointer::<ArrayObject>() };
                let slice = unsafe {
                    slice::from_raw_parts_mut(
                        buf_ptr.data.as_mut_ptr() as *mut u8,
                        len
                    )
                };

                let result = unsafe {
                    syscall3(SYS_READ, fd as usize, slice.as_ptr() as usize, len)
                };

                Ok(Value::from_int(result as i32))
            }

            // sys_write (35 = 0x23)
            0x0023 => {
                let len = self.stack.pop().as_int() as usize;
                let buf = self.stack.pop();
                let fd = self.stack.pop().as_int() as u32;

                let buf_ptr = unsafe { &*buf.as_pointer::<ArrayObject>() };
                let slice = unsafe {
                    slice::from_raw_parts(
                        buf_ptr.data.as_ptr() as *const u8,
                        len
                    )
                };

                let result = unsafe {
                    syscall3(SYS_WRITE, fd as usize, slice.as_ptr() as usize, len)
                };

                Ok(Value::from_int(result as i32))
            }

            // sys_create_window (168 = 0xA8)
            0x00A8 => {
                let height = self.stack.pop().as_int() as u32;
                let width = self.stack.pop().as_int() as u32;
                let title = self.stack.pop();

                let title_str = unsafe { &*title.as_pointer::<StringObject>() };

                let result = unsafe {
                    syscall4(
                        SYS_CREATE_WINDOW,
                        title_str.data.as_ptr() as usize,
                        title_str.len as usize,
                        width as usize,
                        height as usize
                    )
                };

                Ok(Value::from_int(result as i32))
            }

            // sys_get_event (173 = 0xAD)
            0x00AD => {
                let event_buf = self.stack.pop();

                let buf_ptr = unsafe { &mut *event_buf.as_pointer::<ArrayObject>() };
                let event_ptr = buf_ptr.data.as_mut_ptr() as *mut u8;

                let result = unsafe {
                    syscall2(SYS_GET_EVENT, event_ptr as usize, 64)
                };

                Ok(Value::from_int(result as i32))
            }

            _ => Err(VMError::UnknownSyscall(num)),
        }
    }
}
```

---

## Appendix A: VM Error Types

```rust
#[derive(Debug)]
pub enum VMError {
    // Execution errors
    StackOverflow,
    StackUnderflow,
    DivisionByZero,
    NullPointer,
    IndexOutOfBounds { index: usize, length: usize },
    TypeMismatch { expected: TypeTag, found: TypeTag },
    IllegalInstruction,
    UnknownOpcode(u8),

    // Exception handling
    UnhandledException(Value),

    // Memory errors
    OutOfMemory,
    InvalidPointer,

    // Channel errors
    ChannelClosed,
    ChannelFull,

    // Fiber errors
    FiberNotFound,
    FiberAlreadyRunning,

    // Module errors
    ModuleNotFound(String),
    InvalidModule(String),

    // Syscall errors
    UnknownSyscall(u16),
    SyscallFailed { num: u16, error: i32 },

    // Other
    Exit(i32),
}
```

---

## Appendix B: Performance Considerations

### B.1 Opcode Dispatch

The interpreter uses a computed goto dispatch (where available) or switch dispatch:

```rust
// Threaded code dispatch (fastest)
#[cfg(feature = "threaded")]
macro_rules! dispatch {
    ($vm:expr, $opcode:expr) => {
        unsafe { goto *DISPATCH_TABLE[$opcode as usize] }
    };
}

// Switch dispatch (portable)
#[cfg(not(feature = "threaded"))]
macro_rules! dispatch {
    ($vm:expr, $opcode:expr) => {
        continue
    };
}
```

### B.2 Inline Caching

For virtual method calls, inline caching improves performance:

```rust
struct InlineCache {
    type_info: *const TypeInfo,
    method_offset: u32,
}

impl VM {
    fn invoke_virtual_cached(
        &mut self,
        cache: &mut InlineCache,
        method_idx: u16,
    ) -> Result<(), VMError> {
        let receiver = self.stack.peek(/* arg_count */);
        let obj = unsafe { &*receiver.as_pointer::<Object>() };
        let type_info = obj.header.type_info;

        // Check cache
        if cache.type_info == type_info {
            // Cache hit
            let func_ptr = unsafe {
                (*type_info).vtable.add(cache.method_offset as usize)
            };
            return self.invoke_direct(func_ptr);
        }

        // Cache miss - lookup and update
        let offset = self.vtable_lookup(type_info, method_idx)?;
        cache.type_info = type_info;
        cache.method_offset = offset;

        let func_ptr = unsafe {
            (*type_info).vtable.add(offset as usize)
        };
        self.invoke_direct(func_ptr)
    }
}
```

### B.3 Stack Caching

Top-of-stack values can be cached in registers:

```rust
// Pseudo-code for stack caching
struct CachedStack {
    tos: Value,      // Top of stack (cached)
    tos_valid: bool, // Is TOS cached?
    stack: ValueStack,
}
```

---

*This document is part of the Lira Language Specification.*

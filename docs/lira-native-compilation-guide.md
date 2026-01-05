# Lira Native Compilation Guide

## Purpose

This document provides comprehensive context for implementing LLVM and WASM compilation backends for the Lira programming language. It covers foundational concepts, Lira's current architecture, and practical implementation guidance.

---

## Part 1: Foundational Concepts

### 1.1 The Compilation/Execution Spectrum

Understanding the terminology is critical before implementation:

| Term | Definition | Example |
|------|------------|---------|
| **Source Interpreter** | Walks AST directly, executes as it goes | Early Bash, some Lisps |
| **Bytecode Interpreter** | Executes a compact instruction format | CPython, Lua, **Lira (current)** |
| **JIT Compiler** | Interprets first, compiles hot paths on-the-fly | V8, HotSpot JVM, LuaJIT |
| **AOT Compiler** | Translates everything to machine code before execution | C, Rust, Go, **Lira (goal)** |

### 1.2 Key Terms Defined

**Intermediate Representation (IR)**
A data structure or code format between source and final output. It's an abstraction the compiler works with internally for analysis and optimization. Not necessarily executable.

Examples: LLVM IR, SSA form, an AST, three-address code.

**Bytecode**
A *specific kind* of IR designed to be executed (usually by a VM). It's a compact, portable instruction set. All bytecode is IR, but not all IR is bytecode.

Examples: JVM bytecode, Python `.pyc`, Lira `.lic` files.

**Code Generator (Codegen)**
The compiler phase that takes an IR and produces output — machine code, bytecode, or another IR. It's the "backend" of a compiler.

**Virtual Machine (VM)**
A program that executes bytecode. It's a software CPU — reads bytecode instructions and performs the corresponding operations. Contains a fetch-decode-execute loop.

**Interpreter**
A program that executes code by reading and acting on it directly. A bytecode interpreter (like Lira's VM) interprets bytecode. A source interpreter would walk the AST directly.

**Runtime**
Everything your program needs at execution time that isn't your code itself:
- Memory management / GC
- Standard library implementations
- Type information / reflection
- Exception handling machinery
- FFI bridges
- Thread/fiber scheduling

**Critical distinction:** A VM *contains* a runtime, but a runtime doesn't require a VM.

| Language | VM? | Runtime? |
|----------|-----|----------|
| Java | Yes (JVM) | Yes (JRE includes VM + stdlib + classloader) |
| Go | No (compiles to native) | Yes (GC, goroutine scheduler) |
| Rust | No | Minimal (no GC) |
| Lira (current) | Yes (liravm) | Yes (embedded in VM) |
| Lira (goal) | No | Yes (linked library) |

---

## Part 2: Lira's Current Architecture

### 2.1 Compilation Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                        COMPILE TIME                         │
├─────────────────────────────────────────────────────────────┤
│  source.li                                                  │
│      ↓                                                      │
│  Lexer (lexer.rs) → tokens                                  │
│      ↓                                                      │
│  Parser (parser.rs) → AST                                   │
│      ↓                                                      │
│  Type Checker (checker.rs) → TypedProgram                   │
│      ↓                                                      │
│  Code Generator (codegen.rs) → bytecode (.lic)              │
│                                                             │
│  This pipeline is the COMPILER (lirac)                      │
└─────────────────────────────────────────────────────────────┘
                            ↓
                     bytecode file (.lic)
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                         RUN TIME                            │
├─────────────────────────────────────────────────────────────┤
│  liravm loads bytecode                                      │
│      ↓                                                      │
│  VM loop (vm.rs) ← THIS IS THE INTERPRETER                  │
│      │                                                      │
│      ├── Calls into runtime (runtime.rs) for syscalls       │
│      ├── Manages fibers (fiber.rs)                          │
│      └── Handles values (value.rs)                          │
│                                                             │
│  This is the RUNTIME (interpreter + support code)           │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 The Interpreter Loop

The VM in `vm.rs` is a bytecode interpreter. Conceptually:

```rust
loop {
    let opcode = self.fetch();  // Read instruction at IP
    match opcode {
        OP_ADD => {
            let b = self.pop();
            let a = self.pop();
            self.push(a + b);
        }
        OP_CALL => { /* set up call frame, jump */ }
        OP_PRINT => { /* call into runtime */ }
        OP_SPAWN => { /* create fiber */ }
        // ... 52 more opcodes (56 total)
    }
    self.ip += 1;
}
```

This is interpretation: read instruction, do what it says, advance, repeat.

### 2.3 Current Crate Structure

```
crates/
  lira-core/     # Shared types, opcodes (56 defined in opcode.rs)
  lirac/         # Compiler frontend
    ├── lexer.rs
    ├── parser.rs
    ├── ast.rs        # AST definitions (494 lines)
    ├── checker.rs    # Type checker (6600+ lines)
    └── codegen.rs    # Bytecode generator (4200+ lines, monolithic)
  liravm/        # Virtual machine / interpreter
    ├── vm.rs         # Main interpreter loop
    ├── fiber.rs      # Fiber concurrency implementation
    ├── runtime.rs    # 193 syscalls
    ├── value.rs      # Runtime value types (uses Rc<RefCell<>>)
    └── memory.rs     # Memory management
```

### 2.4 Current Limitations for Multi-Backend Support

1. **No separate IR** — `TypedProgram` is just an alias for `Program` (raw AST)
2. **No backend abstraction** — `CodeGenerator` directly emits `Vec<u8>` bytecode
3. **Tightly coupled** — Emission, constant pooling, and jump patching are interleaved
4. **Type erasure** — Generics use runtime type erasure, not monomorphization

### 2.5 Runtime Features Requiring Native Implementation

| Feature | Complexity | Current Location |
|---------|------------|------------------|
| Fiber concurrency | Very High | `fiber.rs`, `vm.rs` |
| Channels | High | `fiber.rs` |
| Closures | Medium | `value.rs`, `vm.rs` |
| Reference counting | Medium | `value.rs` (Rc<RefCell<>>) |
| 193 syscalls | High | `runtime.rs` |
| Pattern matching | Low | Already in codegen |

---

## Part 3: Target Architecture (LLVM Backend)

### 3.1 What LLVM Provides

LLVM is compiler infrastructure. You generate LLVM IR, it handles platform-specific concerns:

```
Lira Compiler → LLVM IR → LLVM → x86_64 / ARM64 / WASM / etc.
                            ↓
                  Optimization passes
                  Register allocation
                  Instruction selection
                  Platform ABI handling
                  Debug info generation
```

**You get "for free":**
- Cross-compilation (build on Mac, target Windows)
- Optimizations (-O0 through -O3)
- Debug info (DWARF, CodeView)
- Native performance
- Multiple targets from one IR

**You still must implement:**
- Runtime library (syscalls, memory management)
- Fiber stack switching (platform-specific assembly)
- Reference counting emission
- Closure representation

### 3.2 LLVM IR Example

```llvm
define i64 @factorial(i64 %n) {
entry:
    %is_base = icmp eq i64 %n, 0
    br i1 %is_base, label %base, label %recurse

base:
    ret i64 1

recurse:
    %n_minus_1 = sub i64 %n, 1
    %sub_result = call i64 @factorial(i64 %n_minus_1)
    %result = mul i64 %n, %sub_result
    ret i64 %result
}
```

### 3.3 Proposed Crate Structure

```
crates/
  lira-core/           # Shared types (existing, unchanged)
  lirac/               # Compiler frontend (existing, modified)
  liravm/              # VM (existing, unchanged)
  lira-ir/             # NEW: Lira Intermediate Representation
  lira-backend-bc/     # NEW: Bytecode backend (extracted from codegen.rs)
  lira-backend-llvm/   # NEW: LLVM backend
  lira-runtime/        # NEW: Native runtime library
```

### 3.4 New Compilation Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                     COMPILE TIME                            │
├─────────────────────────────────────────────────────────────┤
│  source.li                                                  │
│      ↓                                                      │
│  Lexer → Parser → Type Checker → TypedProgram               │
│      ↓                                                      │
│  LIR Lowering (NEW)                                         │
│      ↓                                                      │
│  Lira IR (explicit CFG, desugared constructs)               │
│      ↓                                                      │
│  ┌─────────────┬─────────────┬─────────────┐                │
│  │ BC Backend  │ LLVM Backend│ WASM Backend│                │
│  │ (existing)  │ (new)       │ (future)    │                │
│  └─────────────┴─────────────┴─────────────┘                │
│        ↓              ↓              ↓                      │
│    .lic file     native binary   .wasm file                 │
└─────────────────────────────────────────────────────────────┘
```

### 3.5 Lira IR Design

The IR sits between type checking and code generation:

```rust
// lira-ir/src/lib.rs

pub struct Module {
    pub functions: Vec<Function>,
    pub structs: Vec<StructDef>,
    pub globals: Vec<Global>,
}

pub struct Function {
    pub name: String,
    pub signature: FunctionSignature,
    pub body: Option<FunctionBody>,  // None for extern
    pub captures: Vec<CaptureInfo>,  // For closures
    pub is_fiber_entry: bool,        // Can be spawned
}

pub struct FunctionBody {
    pub blocks: Vec<BasicBlock>,     // Control flow graph
    pub locals: Vec<LocalDef>,
    pub temps: Vec<TempDef>,
}

pub struct BasicBlock {
    pub label: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

pub enum Instruction {
    // Arithmetic
    BinOp { dest: TempId, op: BinOpKind, left: Operand, right: Operand },
    UnaryOp { dest: TempId, op: UnaryOpKind, operand: Operand },
    
    // Memory
    Alloc { dest: TempId, ty: Type },
    Load { dest: TempId, ptr: Operand },
    Store { ptr: Operand, value: Operand },
    
    // Calls
    Call { dest: Option<TempId>, func: Operand, args: Vec<Operand> },
    
    // Fiber primitives (backends decide how to implement)
    Spawn { dest: TempId, func: FuncId, args: Vec<Operand> },
    Yield,
    ChannelSend { channel: Operand, value: Operand },
    ChannelRecv { dest: TempId, channel: Operand },
    
    // Reference counting
    IncRef { ptr: Operand },
    DecRef { ptr: Operand },
}

pub enum Terminator {
    Return(Option<Operand>),
    Branch(BlockId),
    CondBranch { cond: Operand, then_block: BlockId, else_block: BlockId },
    Switch { value: Operand, cases: Vec<(i64, BlockId)>, default: BlockId },
}
```

**Benefits of LIR:**
- Explicit control flow (basic blocks, no implicit fallthrough)
- Desugared pattern matching (becomes switches/branches)
- Explicit closure captures
- Fiber operations as first-class instructions
- Backend-agnostic

### 3.6 Backend Trait

```rust
pub trait Backend {
    type Error: std::error::Error;
    type Output;

    fn compile(&mut self, module: &lir::Module, options: &CompileOptions) -> Result<Self::Output, Self::Error>;
    fn name(&self) -> &'static str;
    fn supports_feature(&self, feature: BackendFeature) -> bool;
}

pub enum BackendFeature {
    Fibers,
    Closures,
    Channels,
    FileIO,
    Networking,
}
```

### 3.7 Type Mapping (Lira → LLVM)

| Lira Type | LLVM Type | Notes |
|-----------|-----------|-------|
| `int` | `i64` | |
| `float` | `f64` | |
| `bool` | `i1` | |
| `string` | `ptr` | Pointer to runtime LiraString struct |
| `[T]` | `ptr` | Pointer to `{ i64 len, i64 cap, ptr data }` |
| `struct Foo` | `%Foo = type { ... }` | Named struct |
| `fn(...) -> T` | Function pointer or closure | See below |
| `fiber` | `ptr` | Opaque pointer to runtime fiber |
| `channel<T>` | `ptr` | Opaque pointer to runtime channel |

### 3.8 Closure Compilation

Closures become fat pointers (function pointer + environment pointer):

```llvm
; Closure type: { fn_ptr, env_ptr }
%closure = type { ptr, ptr }

; Creating a closure that captures variable x
%env = call ptr @lira_alloc(i64 8)        ; Allocate space for captures
store i64 %x, ptr %env                     ; Store captured value
%closure_val = insertvalue %closure undef, ptr @my_closure_fn, 0
%closure_val2 = insertvalue %closure %closure_val, ptr %env, 1

; Calling a closure
%fn_ptr = extractvalue %closure %closure_val, 0
%env_ptr = extractvalue %closure %closure_val, 1
%result = call i64 %fn_ptr(ptr %env_ptr, i64 %arg)  ; env is implicit first arg
```

### 3.9 LLVM Backend Sketch

Using `inkwell` (safe Rust LLVM bindings):

```rust
// lira-backend-llvm/src/lib.rs

use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;

pub struct LlvmBackend<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    values: HashMap<TempId, BasicValueEnum<'ctx>>,
    functions: HashMap<String, FunctionValue<'ctx>>,
}

impl<'ctx> LlvmBackend<'ctx> {
    pub fn compile_module(&mut self, lir: &lir::Module) -> Result<(), LlvmError> {
        // First pass: declare all functions
        for func in &lir.functions {
            self.declare_function(func)?;
        }
        
        // Second pass: compile function bodies
        for func in &lir.functions {
            if let Some(body) = &func.body {
                self.compile_function(func, body)?;
            }
        }
        
        Ok(())
    }
    
    fn compile_function(&mut self, func: &lir::Function, body: &lir::FunctionBody) -> Result<(), LlvmError> {
        let llvm_func = self.functions[&func.name];
        
        // Create basic blocks
        let blocks: HashMap<BlockId, BasicBlock> = body.blocks.iter()
            .map(|b| (b.label, self.context.append_basic_block(llvm_func, &b.label.to_string())))
            .collect();
        
        // Compile each block
        for block in &body.blocks {
            self.builder.position_at_end(blocks[&block.label]);
            
            for instr in &block.instructions {
                self.compile_instruction(instr)?;
            }
            
            self.compile_terminator(&block.terminator, &blocks)?;
        }
        
        Ok(())
    }
    
    fn compile_instruction(&mut self, instr: &lir::Instruction) -> Result<(), LlvmError> {
        match instr {
            lir::Instruction::BinOp { dest, op, left, right } => {
                let l = self.get_value(left);
                let r = self.get_value(right);
                let result = match op {
                    BinOpKind::Add => self.builder.build_int_add(l.into_int_value(), r.into_int_value(), "add"),
                    BinOpKind::Sub => self.builder.build_int_sub(l.into_int_value(), r.into_int_value(), "sub"),
                    // ... other ops
                };
                self.values.insert(*dest, result.into());
            }
            
            lir::Instruction::Call { dest, func, args } => {
                let callee = self.get_function_value(func);
                let arg_vals: Vec<_> = args.iter()
                    .map(|a| self.get_value(a).into())
                    .collect();
                let result = self.builder.build_call(callee, &arg_vals, "call");
                if let Some(d) = dest {
                    self.values.insert(*d, result.try_as_basic_value().left().unwrap());
                }
            }
            
            lir::Instruction::Spawn { dest, func, args } => {
                // Emit call to runtime fiber spawn
                let spawn_fn = self.module.get_function("lira_fiber_spawn").unwrap();
                // ... package function pointer and args
                let fiber_id = self.builder.build_call(spawn_fn, &[/* ... */], "spawn");
                self.values.insert(*dest, fiber_id.try_as_basic_value().left().unwrap());
            }
            
            lir::Instruction::Yield => {
                let yield_fn = self.module.get_function("lira_fiber_yield").unwrap();
                self.builder.build_call(yield_fn, &[], "yield");
            }
            
            lir::Instruction::IncRef { ptr } => {
                let inc_ref_fn = self.module.get_function("lira_inc_ref").unwrap();
                let p = self.get_value(ptr);
                self.builder.build_call(inc_ref_fn, &[p.into()], "");
            }
            
            // ... other instructions
        }
        Ok(())
    }
}
```

---

## Part 4: Native Runtime Library

### 4.1 Overview

The runtime is a Rust library compiled to a static library (`.a`) that gets linked into every Lira binary. It provides:

- Memory allocation and reference counting
- String and array operations
- Fiber scheduler and stack switching
- Channel implementation
- All 193 syscalls

### 4.2 C ABI Interface

```rust
// lira-runtime/src/lib.rs

// ============ Memory Management ============

#[no_mangle]
pub extern "C" fn lira_alloc(size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, 8).unwrap();
    unsafe { alloc::alloc(layout) }
}

#[no_mangle]
pub extern "C" fn lira_free(ptr: *mut u8, size: usize) {
    let layout = Layout::from_size_align(size, 8).unwrap();
    unsafe { alloc::dealloc(ptr, layout) }
}

#[no_mangle]
pub extern "C" fn lira_inc_ref(ptr: *mut RefCountedHeader) {
    if !ptr.is_null() {
        unsafe { (*ptr).ref_count += 1; }
    }
}

#[no_mangle]
pub extern "C" fn lira_dec_ref(ptr: *mut RefCountedHeader) -> bool {
    if ptr.is_null() { return false; }
    unsafe {
        (*ptr).ref_count -= 1;
        if (*ptr).ref_count == 0 {
            // Run destructor, free memory
            return true;
        }
    }
    false
}

// ============ Strings ============

#[repr(C)]
pub struct LiraString {
    header: RefCountedHeader,
    len: usize,
    data: [u8; 0],  // Flexible array member
}

#[no_mangle]
pub extern "C" fn lira_string_new(data: *const u8, len: usize) -> *mut LiraString {
    // Allocate, copy data, return pointer
}

#[no_mangle]
pub extern "C" fn lira_string_concat(a: *const LiraString, b: *const LiraString) -> *mut LiraString {
    // Allocate new string, copy both, return
}

// ============ Arrays ============

#[repr(C)]
pub struct LiraArray {
    header: RefCountedHeader,
    len: usize,
    cap: usize,
    data: *mut u8,
}

#[no_mangle]
pub extern "C" fn lira_array_new(elem_size: usize, initial_cap: usize) -> *mut LiraArray { }

#[no_mangle]
pub extern "C" fn lira_array_push(arr: *mut LiraArray, elem: *const u8, elem_size: usize) { }

#[no_mangle]
pub extern "C" fn lira_array_get(arr: *const LiraArray, index: usize, elem_size: usize) -> *const u8 { }

// ============ Fibers ============

#[no_mangle]
pub extern "C" fn lira_fiber_spawn(func: extern "C" fn(*mut u8), arg: *mut u8) -> u64 {
    SCHEDULER.with(|s| s.borrow_mut().spawn(func, arg))
}

#[no_mangle]
pub extern "C" fn lira_fiber_yield() {
    SCHEDULER.with(|s| s.borrow_mut().yield_current())
}

#[no_mangle]
pub extern "C" fn lira_fiber_run_scheduler() {
    SCHEDULER.with(|s| s.borrow_mut().run())
}

// ============ Channels ============

#[no_mangle]
pub extern "C" fn lira_channel_new() -> *mut Channel { }

#[no_mangle]
pub extern "C" fn lira_channel_send(chan: *mut Channel, val: *mut u8) { }

#[no_mangle]
pub extern "C" fn lira_channel_recv(chan: *mut Channel) -> *mut u8 { }

// ============ I/O (subset of 193 syscalls) ============

#[no_mangle]
pub extern "C" fn lira_print(s: *const LiraString) {
    let s = unsafe { &*s };
    print!("{}", s.as_str());
}

#[no_mangle]
pub extern "C" fn lira_println(s: *const LiraString) {
    let s = unsafe { &*s };
    println!("{}", s.as_str());
}

#[no_mangle]
pub extern "C" fn lira_fs_read_file(path: *const LiraString) -> *mut LiraString { }

#[no_mangle]
pub extern "C" fn lira_fs_write_file(path: *const LiraString, contents: *const LiraString) -> bool { }

// ... remaining syscalls
```

### 4.3 Fiber Stack Switching

Native fibers require platform-specific assembly for context switching. Each fiber has its own stack, and switching requires saving/restoring registers and swapping stack pointers.

```rust
// lira-runtime/src/fiber/context.rs

#[repr(C)]
pub struct FiberContext {
    // Saved registers (platform-specific)
    rsp: u64,  // Stack pointer
    rbp: u64,  // Frame pointer
    rbx: u64,  // Callee-saved
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}

extern "C" {
    fn fiber_switch(from: *mut FiberContext, to: *const FiberContext);
    fn fiber_init(ctx: *mut FiberContext, stack_top: *mut u8, entry: extern "C" fn(*mut u8), arg: *mut u8);
}
```

x86_64 assembly (Linux/macOS):

```nasm
; lira-runtime/src/fiber/switch_x86_64.s

.global fiber_switch
fiber_switch:
    ; Save callee-saved registers to current context
    mov [rdi + 0],  rsp
    mov [rdi + 8],  rbp
    mov [rdi + 16], rbx
    mov [rdi + 24], r12
    mov [rdi + 32], r13
    mov [rdi + 40], r14
    mov [rdi + 48], r15
    
    ; Load callee-saved registers from target context
    mov rsp, [rsi + 0]
    mov rbp, [rsi + 8]
    mov rbx, [rsi + 16]
    mov r12, [rsi + 24]
    mov r13, [rsi + 32]
    mov r14, [rsi + 40]
    mov r15, [rsi + 48]
    
    ret

.global fiber_init
fiber_init:
    ; rdi = context ptr
    ; rsi = stack top
    ; rdx = entry function
    ; rcx = arg
    
    ; Set up initial stack frame
    sub rsi, 8
    mov qword ptr [rsi], offset fiber_entry_trampoline
    
    ; Save to context
    mov [rdi + 0], rsi   ; rsp
    mov [rdi + 8], rsi   ; rbp (arbitrary, will be set by callee)
    mov [rdi + 24], rdx  ; entry fn in r12
    mov [rdi + 32], rcx  ; arg in r13
    
    ret

fiber_entry_trampoline:
    ; Called when fiber starts
    mov rdi, r13         ; arg
    call r12             ; entry(arg)
    ; Fiber finished, yield back to scheduler
    call lira_fiber_exit
```

ARM64 assembly (macOS M-series, Linux ARM):

```nasm
; lira-runtime/src/fiber/switch_aarch64.s

.global fiber_switch
fiber_switch:
    ; Save callee-saved registers
    stp x19, x20, [x0, #0]
    stp x21, x22, [x0, #16]
    stp x23, x24, [x0, #32]
    stp x25, x26, [x0, #48]
    stp x27, x28, [x0, #64]
    stp x29, x30, [x0, #80]  ; fp, lr
    mov x9, sp
    str x9, [x0, #96]        ; sp
    
    ; Load from target context
    ldp x19, x20, [x1, #0]
    ldp x21, x22, [x1, #16]
    ldp x23, x24, [x1, #32]
    ldp x25, x26, [x1, #48]
    ldp x27, x28, [x1, #64]
    ldp x29, x30, [x1, #80]
    ldr x9, [x1, #96]
    mov sp, x9
    
    ret
```

---

## Part 5: WASM Considerations

### 5.1 WASM via LLVM/Emscripten

Once the LLVM backend exists, WASM becomes achievable via Emscripten:

```
Lira → LIR → LLVM IR → Emscripten → WASM + JS glue
```

Pros:
- Reuses LLVM backend work
- Mature tooling
- Asyncify available for fibers

Cons:
- Larger output (includes libc, Asyncify overhead)
- Complex toolchain
- Less control over output

### 5.2 The Fiber Problem in WASM

WASM has no native stack switching. When you call a function, it runs to completion. Options:

| Approach | Status | Code Bloat | Performance |
|----------|--------|------------|-------------|
| **Asyncify** | Available now | 2-3x | Moderate overhead |
| CPS Transform | Complex | High | Variable |
| Stack Switching | Phase 2 proposal | None | Native |
| No fibers | Subset only | None | Best |

**Asyncify** transforms synchronous code to be resumable:
1. Mark yield points (spawn, channel ops, yield)
2. On yield: unwind stack, save locals to heap, return to JS
3. On resume: replay call stack with saved state

**Fiber-free subset**: For programs without `spawn`, compile directly without fiber runtime. Static analysis can detect fiber usage.

### 5.3 Browser Syscall Mapping

| Category | Web API | Notes |
|----------|---------|-------|
| Console | console.log | Direct |
| Time | Date.now(), performance.now() | Direct |
| HTTP | fetch() | **Async — needs Asyncify** |
| Crypto | crypto.subtle | **Async** |
| File I/O | IndexedDB or File API | Limited, async |
| TCP | WebSocket only | No raw sockets |
| Regex | JavaScript RegExp | Via JS interop |

A browser-compatible Lira would need compile-time detection of unsupported syscalls.

---

## Part 6: Implementation Plan

### Phase 1: Infrastructure (2-3 weeks)

- [ ] Create `lira-ir` crate with LIR types
- [ ] Implement AST → LIR lowering in `lirac`
- [ ] Extract bytecode backend to `lira-backend-bc`
- [ ] Implement `Backend` trait
- [ ] Verify bytecode output unchanged

### Phase 2: LLVM Backend Core (3-4 weeks)

- [ ] Create `lira-backend-llvm` with `inkwell`
- [ ] Implement type translation
- [ ] Implement expression/statement compilation
- [ ] Basic function compilation (no closures, no fibers)
- [ ] Runtime stubs (panic on fiber operations)

### Phase 3: Native Runtime (2-3 weeks)

- [ ] Create `lira-runtime` crate
- [ ] Implement memory management (alloc, ref counting)
- [ ] Implement string/array types
- [ ] Implement core syscalls (print, basic I/O)
- [ ] Linking infrastructure (compile Lira → object file → link with runtime)

### Phase 4: Full Feature Support (4-5 weeks)

- [ ] Closure compilation (fat pointers)
- [ ] Platform assembly for fiber switching (x86_64, ARM64)
- [ ] Fiber scheduler in runtime
- [ ] Channel implementation
- [ ] Select statement support

### Phase 5: WASM via Emscripten (3-4 weeks)

- [ ] Configure Emscripten toolchain
- [ ] Asyncify integration for fibers
- [ ] JavaScript glue for browser syscalls
- [ ] Fiber-free compilation option

### Phase 6: Polish (2-3 weeks)

- [ ] LLVM optimization passes (-O1, -O2, -O3)
- [ ] Debug info (DWARF)
- [ ] Cross-compilation support
- [ ] Documentation

**Total estimated effort: 4-6 months**

---

## Part 7: Key Files Reference

| File | Purpose | Relevance |
|------|---------|-----------|
| `crates/lirac/src/codegen.rs` | Current bytecode generator | Pattern for backends, 4200+ lines |
| `crates/lirac/src/ast.rs` | AST definitions | Input for LIR lowering |
| `crates/lirac/src/checker.rs` | Type information | Needed for LIR type info |
| `crates/liravm/src/fiber.rs` | Fiber semantics | Reference for native implementation |
| `crates/liravm/src/runtime.rs` | 193 syscalls | Must reimplement for native |
| `crates/liravm/src/value.rs` | Runtime value types | Memory layout reference |
| `crates/lira-core/src/opcode.rs` | 56 VM opcodes | Semantic reference |

---

## Part 8: Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Fiber complexity | High | Implement fiber-free subset first |
| LLVM API stability | Medium | Pin to LLVM 17 |
| Asyncify overhead | Medium | Offer fiber-free WASM target |
| Platform assembly | Medium | Start with x86_64 + ARM64 only |
| Syscall parity | Low | Document unsupported syscalls per target |
| LLVM binary size | Medium | Consider Cranelift for dev builds |

---

## Part 9: Alternatives Considered

### Cranelift Instead of LLVM

Pros: Faster compilation, pure Rust, simpler integration
Cons: Less optimization, smaller ecosystem

Verdict: Consider for JIT or fast dev builds. Use LLVM for release/AOT.

### Compile VM to WASM (Quick Path)

```bash
# Compile liravm itself to WASM
cargo build --target wasm32-unknown-unknown -p liravm
```

Pros: Works today, minimal effort
Cons: Interpreter overhead in WASM (~10-50x slower than native)

Verdict: Good for quick prototype, not production target.

### Remove Fibers from Language

Pros: Massively simplifies all backends
Cons: Breaks existing programs, loses differentiating feature

Verdict: Not recommended. Fiber-free compilation subset is viable alternative.

---

## Appendix A: Build Output Example

After implementation, the build flow:

```bash
# Compile to native
$ lirac hello.li -o hello
$ ./hello
Hello from Lira!

$ file hello
hello: Mach-O 64-bit executable arm64

# Cross-compile
$ lirac --target x86_64-unknown-linux-gnu hello.li -o hello-linux

# Compile to WASM
$ lirac --target wasm32 hello.li -o hello.wasm

# Compile to bytecode (existing behavior)
$ lirac --target bytecode hello.li -o hello.lic
$ liravm hello.lic
```

---

## Appendix B: Glossary

| Term | Definition |
|------|------------|
| **AOT** | Ahead-Of-Time compilation — translate to machine code before execution |
| **ABI** | Application Binary Interface — calling conventions, data layout |
| **Asyncify** | WASM transformation for simulating stack switching |
| **Basic Block** | Sequence of instructions with single entry/exit point |
| **Bytecode** | Compact instruction format for VM execution |
| **CFG** | Control Flow Graph — basic blocks connected by branches |
| **Codegen** | Code generator — compiler backend |
| **Fat Pointer** | Pointer bundled with metadata (e.g., closure = fn_ptr + env_ptr) |
| **IR** | Intermediate Representation |
| **JIT** | Just-In-Time compilation — compile during execution |
| **LIR** | Lira Intermediate Representation (proposed) |
| **LLVM IR** | LLVM's intermediate representation |
| **Monomorphization** | Generating specialized code for each generic instantiation |
| **SSA** | Single Static Assignment — IR form where each variable assigned once |
| **VM** | Virtual Machine — software that executes bytecode |

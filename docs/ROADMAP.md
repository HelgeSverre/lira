# Lira Implementation Roadmap

**Version**: 3.0
**Last Updated**: 2025-12-30
**Status**: Phase 7 In Progress - Advanced Features

---

## Overview

This roadmap tracks the implementation of Lira, a modern systems programming language with Go-like fiber concurrency. Development follows a **host-first** strategy: the compiler and VM are developed and tested on macOS/Linux.

**Total Estimated Tasks**: ~70 discrete implementation tasks
**Major Phases**: 7 phases from lexer to advanced features
**Test Coverage**: ~30% of language features (expanding)

---

## Table of Contents

1. [Phase 0: Project Setup](#phase-0-project-setup) - COMPLETE
2. [Phase 1: Lexer & Parser](#phase-1-lexer--parser) - COMPLETE
3. [Phase 2: Type System](#phase-2-type-system) - COMPLETE
4. [Phase 3: Bytecode Generation](#phase-3-bytecode-generation) - COMPLETE
5. [Phase 4: VM Core](#phase-4-vm-core) - COMPLETE
6. [Phase 5: Fiber Runtime](#phase-5-fiber-runtime) - COMPLETE
7. [Phase 6: Standard Library](#phase-6-standard-library) - COMPLETE
8. [Phase 7: Advanced Features](#phase-7-advanced-features) - IN PROGRESS

---

## Phase 0: Project Setup

**Goal**: Establish project structure and build system.
**Status**: COMPLETE

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T0.1 | Create lira directory | Set up lira/ project | [x] |
| T0.2 | Create lira-core crate | Shared types between compiler and VM | [x] |
| T0.3 | Create lirac crate | Compiler crate with CLI | [x] |
| T0.4 | Create liravm crate | VM crate with CLI | [x] |
| T0.5 | Add to workspace | Update root Cargo.toml | [x] |
| T0.6 | Add justfile commands | li-build, li-test, li-run | [x] |
| T0.7 | Verify cargo check | All crates compile | [x] |

---

## Phase 1: Lexer & Parser

**Goal**: Parse Lira source code into an AST.
**Depends On**: Phase 0
**Spec Reference**: `01-lexical-structure.md`, `03-syntax-constructs.md`
**Status**: COMPLETE

### 1.1 Lexer

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T1.1 | Implement whitespace/comments | Skip spaces, //, /* */ | [x] |
| T1.2 | Implement identifiers/keywords | Keyword lookup table | [x] |
| T1.3 | Implement number literals | int, float, hex, binary | [x] |
| T1.4 | Implement string literals | Escape sequences, interpolation | [x] |
| T1.5 | Implement operators | All operators from spec | [x] |
| T1.6 | Implement delimiters | (), {}, [], etc. | [x] |
| T1.7 | Error reporting | Line/column in error messages | [x] |
| T1.8 | Test lexer | Comprehensive token tests | [x] |

### 1.2 Parser

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T1.10 | Parse expressions | Literals, binary, unary, calls | [x] |
| T1.11 | Parse statements | let, var, return, if, while, for | [x] |
| T1.12 | Parse functions | fn declarations, parameters | [x] |
| T1.13 | Parse classes/structs | class, struct, fields, methods | [x] |
| T1.14 | Parse enums | enum with variants | [x] |
| T1.15 | Parse imports | import statements | [x] |
| T1.16 | Parse match | match expressions with patterns | [x] |
| T1.17 | Operator precedence | Pratt parser or precedence climbing | [x] |
| T1.18 | Error recovery | Continue parsing after errors | [x] |
| T1.19 | Test parser | AST verification tests | [x] |

---

## Phase 2: Type System

**Goal**: Implement type checking and inference.
**Depends On**: Phase 1
**Spec Reference**: `02-type-system.md`
**Status**: COMPLETE

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T2.1 | Primitive types | int, float, bool, string, char | [x] |
| T2.2 | Integer variants | int8-64, uint8-64 | [x] |
| T2.3 | Optional types | T? with null safety | [x] |
| T2.4 | Generic types | List<T>, Map<K,V> | [x] |
| T2.5 | Function types | fn(A, B) -> C | [x] |
| T2.6 | Tuple types | (A, B, C) | [x] |
| T2.7 | Type inference | Hindley-Milner style | [x] |
| T2.8 | Type checking | Validate assignments, calls | [x] |
| T2.9 | Struct/class types | Field access, methods | [x] |
| T2.10 | Enum types | Variant checking, exhaustiveness | [x] |
| T2.11 | Generic constraints | where clauses (inline bounds supported) | [x] |
| T2.12 | Type errors | Clear, helpful error messages | [x] |
| T2.13 | Test type checker | Type system edge cases | [x] |

---

## Phase 3: Bytecode Generation

**Goal**: Generate executable bytecode from typed AST.
**Depends On**: Phase 2
**Spec Reference**: `10-bytecode-format.md`, `11-instruction-set.md`
**Status**: COMPLETE

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T3.1 | Define opcodes | All VM instructions | [x] |
| T3.2 | Bytecode header | Magic, version, metadata | [x] |
| T3.3 | Constant pool | Encode literals | [x] |
| T3.4 | Function table | Function metadata, entry points | [x] |
| T3.5 | Generate expressions | Arithmetic, comparisons | [x] |
| T3.6 | Generate control flow | if, while, for, match | [x] |
| T3.7 | Generate function calls | Call/return sequences | [x] |
| T3.8 | Generate object ops | Field access, method calls | [x] |
| T3.9 | Local variables | Stack slot allocation | [x] |
| T3.10 | Write .lic files | Binary output format | [x] |
| T3.11 | Debug info | Line number mapping | [x] |
| T3.12 | Test code generation | Bytecode verification | [x] |
| T3.13 | Compound assignments | +=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>= | [x] |
| T3.14 | Increment/decrement | ++x, x++, --x, x-- | [x] |
| T3.15 | Null coalescing | ?? operator | [x] |

---

## Phase 4: VM Core

**Goal**: Execute bytecode in the virtual machine.
**Depends On**: Phase 3
**Spec Reference**: `12-vm-runtime.md`, `13-memory-model.md`
**Status**: COMPLETE

### 4.1 Interpreter

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T4.1 | Load bytecode | Parse .lic files | [x] |
| T4.2 | Stack operations | push, pop, dup | [x] |
| T4.3 | Arithmetic ops | add, sub, mul, div, mod | [x] |
| T4.4 | Comparison ops | eq, ne, lt, le, gt, ge | [x] |
| T4.5 | Logical ops | and, or, not | [x] |
| T4.6 | Control flow | jump, jump_if_true/false | [x] |
| T4.7 | Function calls | call, return | [x] |
| T4.8 | Local variables | load_local, store_local | [x] |
| T4.9 | Object operations | get_field, set_field, new | [x] |
| T4.10 | Array operations | new_array, get, set, len | [x] |
| T4.11 | Test interpreter | Hello world execution | [x] |

### 4.2 Memory Management

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T4.20 | Object allocation | Heap management | [x] |
| T4.21 | Reference counting | Using Rc<RefCell<>> | [x] |
| T4.22 | Cycle detection | Mark-and-sweep for cycles | [x] |
| T4.23 | String interning | Deduplicate strings | [x] |
| T4.24 | Test memory | Allocation/deallocation tests | [x] |

---

## Phase 5: Fiber Runtime

**Goal**: Implement green threads and channels.
**Depends On**: Phase 4
**Spec Reference**: `04-concurrency.md`
**Status**: COMPLETE

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T5.1 | Fiber structure | Stack, locals, IP per fiber | [x] |
| T5.2 | Fiber scheduler | Round-robin scheduling | [x] |
| T5.3 | spawn instruction | Create new fiber | [x] |
| T5.4 | yield instruction | Cooperative yielding | [x] |
| T5.5 | Channel creation | Buffered and unbuffered | [x] |
| T5.6 | Channel send | Blocking send | [x] |
| T5.7 | Channel receive | Blocking receive | [x] |
| T5.8 | select statement | Multiple channel wait | [x] |
| T5.9 | Fiber-local storage | Per-fiber data | [x] |
| T5.10 | Test concurrency | Spawn, channel tests | [x] |

---

## Phase 6: Standard Library

**Goal**: Implement platform-agnostic standard library with host primitives.
**Depends On**: Phase 5
**Spec Reference**: `30-standard-library.md`
**Status**: COMPLETE

### 6.1 Host Primitives (Rust Layer)

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T6.1 | Syscall extension | Extended syscall mechanism for host primitives | [x] |
| T6.2 | File I/O primitives | file_open, file_read, file_write, file_close, file_exists, file_size | [x] |
| T6.3 | Time primitives | time_ms, sleep | [x] |
| T6.4 | System primitives | env_get, env_args | [x] |
| T6.5 | Console primitives | input (read line via syscall) | [x] |

### 6.2 Core Types (Lira Layer)

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T6.10 | stdlib directory | Create lira/stdlib/ | [x] |
| T6.11 | std.core | abs, min, max, clamp, array utilities | [x] |
| T6.12 | std.io | Debug, assert, timing utilities | [x] |
| T6.13 | std.fs | read_file, write_file, append_file, exists, size | [x] |
| T6.14 | std.strings | String utilities (split, join, trim, etc.) | [x] |
| T6.15 | std.math | sqrt, sin, cos, tan, log, exp, pow, etc. | [x] |
| T6.16 | std.time | timestamp, sleep, format, parse | [x] |
| T6.17 | std.collections | List, Map, Set enhanced methods | [x] |
| T6.18 | std.path | Path manipulation utilities | [x] |
| T6.19 | std.hash | MD5, SHA1, SHA256, SHA512 | [x] |
| T6.20 | std.json | JSON parse/stringify | [x] |
| T6.21 | std.url | URL parsing and encoding | [x] |
| T6.22 | std.http | HTTP client (get, post, request) | [x] |
| T6.23 | std.regex | Regular expression matching | [x] |
| T6.24 | std.uuid | UUID generation (v4, v7) | [x] |
| T6.25 | std.env | Environment variables | [x] |
| T6.26 | std.os | OS info and operations | [x] |
| T6.27 | std.net | Network utilities | [x] |
| T6.28 | std.random | Random number generation | [x] |
| T6.29 | std.log | Logging utilities | [x] |
| T6.30 | std.test | Testing framework | [x] |

### 6.3 Module System

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T6.40 | Module loader | ModuleLoader for resolving imports | [x] |
| T6.41 | Import parsing | import std.fs, import std.io.{File} | [x] |
| T6.42 | Stdlib resolution | Resolve std.* to stdlib/ directory | [x] |
| T6.43 | Multi-file compilation | Merge imported modules into AST | [x] |
| T6.44 | Circular import detection | Error on circular dependencies | [x] |

---

## Phase 7: Advanced Features

**Goal**: Complete remaining language features.
**Depends On**: Phase 6
**Status**: IN PROGRESS (19 tasks complete, 5 pending)

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T7.1 | Array type annotations | `[int]` in function params | [x] |
| T7.2 | Forward references | Mutual recursion support | [x] |
| T7.3 | Module system | import/export, namespaces | [x] (moved to Phase 6.3) |
| T7.4 | Generic syntax | `fn foo<T>(x: T) -> T` parsing | [x] |
| T7.5 | Generic constraint checking | Reject ops on unconstrained type params | [x] |
| T7.6 | Generic monomorphization | Generate specialized code per concrete type | [x] |
| T7.7 | Enum variant access | `Color::Red` syntax | [x] |
| T7.8 | Classes with inheritance | extends, super, method override | [x] |
| T7.9 | Enums with data | Enum variants with associated values | [x] |
| T7.10 | Trait parsing | Parse `trait Name { fn method(self) }` | [x] |
| T7.11 | Impl block parsing | Parse `impl Type { }` and `impl Trait for Type { }` | [x] |
| T7.12 | Self type and receiver | `self`, `self mut`, `Self` in impl blocks | [x] |
| T7.13 | Impl type checking | Register methods from impl blocks, resolve calls | [x] |
| T7.14 | Trait impl checking | Verify trait implementations are complete | [x] |
| T7.15 | Method dispatch codegen | Generate code for `receiver.method()` calls | [x] |
| T7.16 | Inherent method lookup | Resolve methods defined directly on types | [x] |
| T7.17 | Trait method lookup | Resolve methods from trait impls | [x] |
| T7.18 | Error propagation | ? operator, Result handling | [x] |
| T7.19 | Generic trait bounds | where T: Eq + Hash | [x] |
| T7.20 | Default parameters | fn foo(x: int = 0) | [x] |
| T7.21 | Named arguments | foo(name: "bar", value: 42) | [x] |
| T7.22 | Destructuring | let (a, b) = tuple, let { x, y } = struct | [x] |
| T7.23 | Range expressions | 1..10, 1..=10 | [x] |
| T7.24 | Type expressions | `x as int`, `x is int`, `x?.field` | [x] |

---

## Dependency Graph

```
Phase 0 (Setup)
    │
    ▼
Phase 1 (Lexer & Parser)
    │
    ▼
Phase 2 (Type System)
    │
    ▼
Phase 3 (Bytecode Generation)
    │
    ▼
Phase 4 (VM Core)
    │
    ▼
Phase 5 (Fiber Runtime)
    │
    ▼
Phase 6 (Standard Library)
    │
    ▼
Phase 7 (Advanced Features)  ←── CURRENT
    │
    ▼
Production-Ready Language
```

---

## Example Applications

Working examples are available in `examples/`:

### Core Language

| Example | Description | Status |
|---------|-------------|--------|
| hello.li | Basic output | [x] Working |
| fibonacci.li | Recursive functions | [x] Working |
| factorial.li | Recursive + iterative | [x] Working |
| prime_checker.li | Functions with loops | [x] Working |
| control_flow.li | If/else, while, FizzBuzz | [x] Working |

### Data Structures

| Example | Description | Status |
|---------|-------------|--------|
| array_ops.li | Array creation and access | [x] Working |
| structs.li | Struct definitions | [x] Working |
| nested_structures.li | Nested arrays and structs | [x] Working |
| string_ops.li | String manipulation | [x] Working |

### Operators

| Example | Description | Status |
|---------|-------------|--------|
| operator_comprehensive.li | All operators tested | [x] Working |
| arithmetic_edge_cases.li | Edge cases for math | [x] Working |
| compound_assign.li | +=, -=, *=, etc. | [x] Working |
| bitwise_ops.li | Bitwise operations | [x] Working |

### Functions & Patterns

| Example | Description | Status |
|---------|-------------|--------|
| lambda.li | Closures and lambdas | [x] Working |
| pattern_match.li | Match expressions | [x] Working |
| pattern_guards.li | Match with guards | [x] Working |
| named_arguments.li | Named function arguments | [x] Working |

### Concurrency

| Example | Description | Status |
|---------|-------------|--------|
| channel_basic.li | Channel creation | [x] Working |
| fiber_basic.li | Fiber syntax demo | [x] Working |
| select_basic.li | Select statement | [x] Working |

### Types

| Example | Description | Status |
|---------|-------------|--------|
| integer_types.li | Sized integer types | [x] Working |
| null_and_optionals.li | Null and ?? operator | [x] Working |
| generics_basic.li | Generic functions and structs | [x] Working |
| type_expressions.li | Type casting and checking | [x] Working |
| range_expressions.li | Range creation 1..10 | [x] Working |

All examples compile with `lirac` and run with `liravm` on macOS/Linux.

---

## Milestone Definitions

| Milestone | Criteria | Status |
|-----------|----------|--------|
| **M1: Parser Works** | Can parse valid Lira files to AST | ✓ Complete |
| **M2: Type Checking** | Type errors detected, inference works | ✓ Complete |
| **M3: Hello World** | `print("Hello")` compiles and runs | ✓ Complete |
| **M4: Functions** | User-defined functions work | ✓ Complete |
| **M5: Closures** | Lambda expressions with captures | ✓ Complete |
| **M6: Fibers** | Concurrent programs run | ✓ Complete |
| **M7: Stdlib** | Basic stdlib available | ✓ Complete |
| **M8: Production** | Full language feature set | In Progress |

---

## Build Commands

```bash
# Build compiler and VM
just build

# Build release
just release

# Run tests
just test

# Compile and run a .li file
just run path/to/file.li

# Type check code
just check

# Lint code
just clippy

# Format code
just fmt
```

---

## Statistics

| Metric | Count |
|--------|-------|
| Total tasks | ~85 |
| Completed | ~74 |
| In Progress | ~4 |
| Pending | ~7 |
| Compiler LOC | ~15,900 |
| VM LOC | ~5,600 |
| Core LOC | ~350 |
| Stdlib modules | 21 |
| Example files | 86 |

---

## References

- `00-lira-overview.md` - Language overview
- `01-lexical-structure.md` - Tokens and keywords
- `02-type-system.md` - Types and generics
- `03-syntax-constructs.md` - Statements and expressions
- `04-concurrency.md` - Fibers and channels
- `05-module-system.md` - Imports and packages
- `10-bytecode-format.md` - .lic file format
- `11-instruction-set.md` - VM opcodes
- `12-vm-runtime.md` - Execution model
- `13-memory-model.md` - ARC and memory
- `30-standard-library.md` - Standard library spec

---

*This roadmap tracks the development of Lira as a standalone systems programming language.*

---

## Phase 8: Developer Tooling

**Goal**: Provide first-class IDE support and developer tools.
**Depends On**: Phase 7
**Status**: IN PROGRESS

### 8.1 Tree-sitter Grammar

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T8.1 | Tree-sitter grammar | Create grammar.js for Lira syntax | [x] |
| T8.2 | Highlight queries | Write highlights.scm for syntax highlighting | [x] |
| T8.3 | Fold queries | Write folds.scm for code folding | [x] |
| T8.4 | Locals queries | Write locals.scm for scope tracking | [x] |
| T8.5 | Test corpus | Create test cases for parser validation | [x] |

### 8.2 Language Server (LSP)

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T8.6 | LSP scaffold | Basic server with tower-lsp | [x] |
| T8.7 | Diagnostics | Report errors from type checker | [x] |
| T8.8 | Completion | Keyword and symbol completion | [x] |
| T8.9 | Hover | Type info and documentation on hover | [x] |
| T8.10 | Go to definition | Navigate to symbol definitions | [x] |
| T8.11 | Find references | Find all references to a symbol | [ ] |
| T8.12 | Document symbols | Outline view for files | [x] |
| T8.13 | Semantic tokens | Enhanced syntax highlighting | [ ] |

### 8.3 Editor Extensions

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T8.14 | VS Code extension | Syntax + LSP client for VS Code | [ ] |
| T8.15 | JetBrains plugin | Syntax + LSP for IntelliJ/CLion | [ ] |
| T8.16 | Neovim config | Tree-sitter + LSP configuration | [ ] |

### 8.4 Developer Tools

| ID | Task | Description | Status |
|----|------|-------------|--------|
| T8.17 | lira-fmt | Code formatter | [ ] |
| T8.18 | lira-doc | Documentation generator | [ ] |


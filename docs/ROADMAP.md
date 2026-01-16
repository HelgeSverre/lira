# Lira Implementation Roadmap

**Version**: 3.2
**Last Updated**: 2026-01-16
**Status**: Phase 8 In Progress - Developer Tooling

---

## Overview

This roadmap tracks the implementation of Lira, a modern systems programming language with Go-like fiber concurrency. Development follows a **host-first** strategy: the compiler and VM are developed and tested on macOS/Linux.

**Total Estimated Tasks**: ~190 discrete implementation tasks
**Major Phases**: 22 phases from lexer to production-ready
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
8. [Phase 7: Advanced Features](#phase-7-advanced-features) - COMPLETE
9. [Phase 8: Developer Tooling](#phase-8-developer-tooling) - IN PROGRESS (LSP: 20 features complete)
10. [Phase 9: Developer Experience & Debugging](#phase-9-developer-experience--debugging) - IN PROGRESS (2/9)
11. [Phase 10: Performance & Profiling](#phase-10-performance--profiling) - PLANNED
12. [Phase 11: Package Ecosystem](#phase-11-package-ecosystem) - PLANNED
13. [Phase 12: Native Compilation](#phase-12-native-compilation) - PLANNED
14. [Phase 13: Interoperability](#phase-13-interoperability) - PLANNED
15. [Phase 14: Self-Hosting](#phase-14-self-hosting) - PLANNED
16. [Phase 15: Advanced Language Features](#phase-15-advanced-language-features) - PLANNED
17. [Phase 16: Community & Ecosystem](#phase-16-community--ecosystem) - PLANNED
18. [Phase 17: Testing Framework](#phase-17-testing-framework) - PLANNED
19. [Phase 18: Standard Library Expansions](#phase-18-standard-library-expansions) - PLANNED
20. [Phase 19: Concurrency Enhancements](#phase-19-concurrency-enhancements) - PLANNED
21. [Phase 20: Windows Support](#phase-20-windows-support) - PLANNED
22. [Phase 21: Language Specification](#phase-21-language-specification) - PLANNED

---

## Phase 0: Project Setup

**Goal**: Establish project structure and build system.
**Status**: COMPLETE

| ID   | Task                   | Description                          | Status |
| ---- | ---------------------- | ------------------------------------ | ------ |
| T0.1 | Create lira directory  | Set up lira/ project                 | [x]    |
| T0.2 | Create lira-core crate | Shared types between compiler and VM | [x]    |
| T0.3 | Create lirac crate     | Compiler crate with CLI              | [x]    |
| T0.4 | Create liravm crate    | VM crate with CLI                    | [x]    |
| T0.5 | Add to workspace       | Update root Cargo.toml               | [x]    |
| T0.6 | Add justfile commands  | li-build, li-test, li-run            | [x]    |
| T0.7 | Verify cargo check     | All crates compile                   | [x]    |

---

## Phase 1: Lexer & Parser

**Goal**: Parse Lira source code into an AST.
**Depends On**: Phase 0
**Spec Reference**: `01-lexical-structure.md`, `03-syntax-constructs.md`
**Status**: COMPLETE

### 1.1 Lexer

| ID   | Task                           | Description                     | Status |
| ---- | ------------------------------ | ------------------------------- | ------ |
| T1.1 | Implement whitespace/comments  | Skip spaces, //, /\* \*/        | [x]    |
| T1.2 | Implement identifiers/keywords | Keyword lookup table            | [x]    |
| T1.3 | Implement number literals      | int, float, hex, binary         | [x]    |
| T1.4 | Implement string literals      | Escape sequences, interpolation | [x]    |
| T1.5 | Implement operators            | All operators from spec         | [x]    |
| T1.6 | Implement delimiters           | (), {}, [], etc.                | [x]    |
| T1.7 | Error reporting                | Line/column in error messages   | [x]    |
| T1.8 | Test lexer                     | Comprehensive token tests       | [x]    |

### 1.2 Parser

| ID    | Task                  | Description                         | Status |
| ----- | --------------------- | ----------------------------------- | ------ |
| T1.10 | Parse expressions     | Literals, binary, unary, calls      | [x]    |
| T1.11 | Parse statements      | let, var, return, if, while, for    | [x]    |
| T1.12 | Parse functions       | fn declarations, parameters         | [x]    |
| T1.13 | Parse classes/structs | class, struct, fields, methods      | [x]    |
| T1.14 | Parse enums           | enum with variants                  | [x]    |
| T1.15 | Parse imports         | import statements                   | [x]    |
| T1.16 | Parse match           | match expressions with patterns     | [x]    |
| T1.17 | Operator precedence   | Pratt parser or precedence climbing | [x]    |
| T1.18 | Error recovery        | Continue parsing after errors       | [x]    |
| T1.19 | Test parser           | AST verification tests              | [x]    |

---

## Phase 2: Type System

**Goal**: Implement type checking and inference.
**Depends On**: Phase 1
**Spec Reference**: `02-type-system.md`
**Status**: COMPLETE

| ID    | Task                | Description                             | Status |
| ----- | ------------------- | --------------------------------------- | ------ |
| T2.1  | Primitive types     | int, float, bool, string, char          | [x]    |
| T2.2  | Integer variants    | int8-64, uint8-64                       | [x]    |
| T2.3  | Optional types      | T? with null safety                     | [x]    |
| T2.4  | Generic types       | List<T>, Map<K,V>                       | [x]    |
| T2.5  | Function types      | fn(A, B) -> C                           | [x]    |
| T2.6  | Tuple types         | (A, B, C)                               | [x]    |
| T2.7  | Type inference      | Hindley-Milner style                    | [x]    |
| T2.8  | Type checking       | Validate assignments, calls             | [x]    |
| T2.9  | Struct/class types  | Field access, methods                   | [x]    |
| T2.10 | Enum types          | Variant checking, exhaustiveness        | [x]    |
| T2.11 | Generic constraints | where clauses (inline bounds supported) | [x]    |
| T2.12 | Type errors         | Clear, helpful error messages           | [x]    |
| T2.13 | Test type checker   | Type system edge cases                  | [x]    |

---

## Phase 3: Bytecode Generation

**Goal**: Generate executable bytecode from typed AST.
**Depends On**: Phase 2
**Spec Reference**: `10-bytecode-format.md`, `11-instruction-set.md`
**Status**: COMPLETE

| ID    | Task                    | Description                     | Status          |
| ----- | ----------------------- | ------------------------------- | --------------- | --- |
| T3.1  | Define opcodes          | All VM instructions             | [x]             |
| T3.2  | Bytecode header         | Magic, version, metadata        | [x]             |
| T3.3  | Constant pool           | Encode literals                 | [x]             |
| T3.4  | Function table          | Function metadata, entry points | [x]             |
| T3.5  | Generate expressions    | Arithmetic, comparisons         | [x]             |
| T3.6  | Generate control flow   | if, while, for, match           | [x]             |
| T3.7  | Generate function calls | Call/return sequences           | [x]             |
| T3.8  | Generate object ops     | Field access, method calls      | [x]             |
| T3.9  | Local variables         | Stack slot allocation           | [x]             |
| T3.10 | Write .lic files        | Binary output format            | [x]             |
| T3.11 | Debug info              | Line number mapping             | [x]             |
| T3.12 | Test code generation    | Bytecode verification           | [x]             |
| T3.13 | Compound assignments    | +=, -=, \*=, /=, %=, &=,        | =, ^=, <<=, >>= | [x] |
| T3.14 | Increment/decrement     | ++x, x++, --x, x--              | [x]             |
| T3.15 | Null coalescing         | ?? operator                     | [x]             |

---

## Phase 4: VM Core

**Goal**: Execute bytecode in the virtual machine.
**Depends On**: Phase 3
**Spec Reference**: `12-vm-runtime.md`, `13-memory-model.md`
**Status**: COMPLETE

### 4.1 Interpreter

| ID    | Task              | Description               | Status |
| ----- | ----------------- | ------------------------- | ------ |
| T4.1  | Load bytecode     | Parse .lic files          | [x]    |
| T4.2  | Stack operations  | push, pop, dup            | [x]    |
| T4.3  | Arithmetic ops    | add, sub, mul, div, mod   | [x]    |
| T4.4  | Comparison ops    | eq, ne, lt, le, gt, ge    | [x]    |
| T4.5  | Logical ops       | and, or, not              | [x]    |
| T4.6  | Control flow      | jump, jump_if_true/false  | [x]    |
| T4.7  | Function calls    | call, return              | [x]    |
| T4.8  | Local variables   | load_local, store_local   | [x]    |
| T4.9  | Object operations | get_field, set_field, new | [x]    |
| T4.10 | Array operations  | new_array, get, set, len  | [x]    |
| T4.11 | Test interpreter  | Hello world execution     | [x]    |

### 4.2 Memory Management

| ID    | Task               | Description                   | Status |
| ----- | ------------------ | ----------------------------- | ------ |
| T4.20 | Object allocation  | Heap management               | [x]    |
| T4.21 | Reference counting | Using Rc<RefCell<>>           | [x]    |
| T4.22 | Cycle detection    | Mark-and-sweep for cycles     | [x]    |
| T4.23 | String interning   | Deduplicate strings           | [x]    |
| T4.24 | Test memory        | Allocation/deallocation tests | [x]    |

---

## Phase 5: Fiber Runtime

**Goal**: Implement green threads and channels.
**Depends On**: Phase 4
**Spec Reference**: `04-concurrency.md`
**Status**: COMPLETE

| ID    | Task                | Description                 | Status |
| ----- | ------------------- | --------------------------- | ------ |
| T5.1  | Fiber structure     | Stack, locals, IP per fiber | [x]    |
| T5.2  | Fiber scheduler     | Round-robin scheduling      | [x]    |
| T5.3  | spawn instruction   | Create new fiber            | [x]    |
| T5.4  | yield instruction   | Cooperative yielding        | [x]    |
| T5.5  | Channel creation    | Buffered and unbuffered     | [x]    |
| T5.6  | Channel send        | Blocking send               | [x]    |
| T5.7  | Channel receive     | Blocking receive            | [x]    |
| T5.8  | select statement    | Multiple channel wait       | [x]    |
| T5.9  | Fiber-local storage | Per-fiber data              | [x]    |
| T5.10 | Test concurrency    | Spawn, channel tests        | [x]    |

---

## Phase 6: Standard Library

**Goal**: Implement platform-agnostic standard library with host primitives.
**Depends On**: Phase 5
**Spec Reference**: `30-standard-library.md`
**Status**: COMPLETE

### 6.1 Host Primitives (Rust Layer)

| ID   | Task                | Description                                                          | Status |
| ---- | ------------------- | -------------------------------------------------------------------- | ------ |
| T6.1 | Syscall extension   | Extended syscall mechanism for host primitives                       | [x]    |
| T6.2 | File I/O primitives | file_open, file_read, file_write, file_close, file_exists, file_size | [x]    |
| T6.3 | Time primitives     | time_ms, sleep                                                       | [x]    |
| T6.4 | System primitives   | env_get, env_args                                                    | [x]    |
| T6.5 | Console primitives  | input (read line via syscall)                                        | [x]    |

### 6.2 Core Types (Lira Layer)

| ID    | Task             | Description                                      | Status |
| ----- | ---------------- | ------------------------------------------------ | ------ |
| T6.10 | stdlib directory | Create lira/stdlib/                              | [x]    |
| T6.11 | std.core         | abs, min, max, clamp, array utilities            | [x]    |
| T6.12 | std.io           | Debug, assert, timing utilities                  | [x]    |
| T6.13 | std.fs           | read_file, write_file, append_file, exists, size | [x]    |
| T6.14 | std.strings      | String utilities (split, join, trim, etc.)       | [x]    |
| T6.15 | std.math         | sqrt, sin, cos, tan, log, exp, pow, etc.         | [x]    |
| T6.16 | std.time         | timestamp, sleep, format, parse                  | [x]    |
| T6.17 | std.collections  | List, Map, Set enhanced methods                  | [x]    |
| T6.18 | std.path         | Path manipulation utilities                      | [x]    |
| T6.19 | std.hash         | MD5, SHA1, SHA256, SHA512                        | [x]    |
| T6.20 | std.json         | JSON parse/stringify                             | [x]    |
| T6.21 | std.url          | URL parsing and encoding                         | [x]    |
| T6.22 | std.http         | HTTP client (get, post, request)                 | [x]    |
| T6.23 | std.regex        | Regular expression matching                      | [x]    |
| T6.24 | std.uuid         | UUID generation (v4, v7)                         | [x]    |
| T6.25 | std.env          | Environment variables                            | [x]    |
| T6.26 | std.os           | OS info and operations                           | [x]    |
| T6.27 | std.net          | Network utilities                                | [x]    |
| T6.28 | std.random       | Random number generation                         | [x]    |
| T6.29 | std.log          | Logging utilities                                | [x]    |
| T6.30 | std.test         | Testing framework                                | [x]    |

### 6.3 Module System

| ID    | Task                      | Description                         | Status |
| ----- | ------------------------- | ----------------------------------- | ------ |
| T6.40 | Module loader             | ModuleLoader for resolving imports  | [x]    |
| T6.41 | Import parsing            | import std.fs, import std.io.{File} | [x]    |
| T6.42 | Stdlib resolution         | Resolve std.\* to stdlib/ directory | [x]    |
| T6.43 | Multi-file compilation    | Merge imported modules into AST     | [x]    |
| T6.44 | Circular import detection | Error on circular dependencies      | [x]    |

---

## Phase 7: Advanced Features

**Goal**: Complete remaining language features.
**Depends On**: Phase 6
**Status**: COMPLETE

| ID    | Task                        | Description                                         | Status                   |
| ----- | --------------------------- | --------------------------------------------------- | ------------------------ |
| T7.1  | Array type annotations      | `[int]` in function params                          | [x]                      |
| T7.2  | Forward references          | Mutual recursion support                            | [x]                      |
| T7.3  | Module system               | import/export, namespaces                           | [x] (moved to Phase 6.3) |
| T7.4  | Generic syntax              | `fn foo<T>(x: T) -> T` parsing                      | [x]                      |
| T7.5  | Generic constraint checking | Reject ops on unconstrained type params             | [x]                      |
| T7.6  | Generic monomorphization    | Generate specialized code per concrete type         | [x]                      |
| T7.7  | Enum variant access         | `Color::Red` syntax                                 | [x]                      |
| T7.8  | Classes with inheritance    | extends, super, method override                     | [x]                      |
| T7.9  | Enums with data             | Enum variants with associated values                | [x]                      |
| T7.10 | Trait parsing               | Parse `trait Name { fn method(self) }`              | [x]                      |
| T7.11 | Impl block parsing          | Parse `impl Type { }` and `impl Trait for Type { }` | [x]                      |
| T7.12 | Self type and receiver      | `self`, `self mut`, `Self` in impl blocks           | [x]                      |
| T7.13 | Impl type checking          | Register methods from impl blocks, resolve calls    | [x]                      |
| T7.14 | Trait impl checking         | Verify trait implementations are complete           | [x]                      |
| T7.15 | Method dispatch codegen     | Generate code for `receiver.method()` calls         | [x]                      |
| T7.16 | Inherent method lookup      | Resolve methods defined directly on types           | [x]                      |
| T7.17 | Trait method lookup         | Resolve methods from trait impls                    | [x]                      |
| T7.18 | Error propagation           | ? operator, Result handling                         | [x]                      |
| T7.19 | Generic trait bounds        | where T: Eq + Hash                                  | [x]                      |
| T7.20 | Default parameters          | fn foo(x: int = 0)                                  | [x]                      |
| T7.21 | Named arguments             | foo(name: "bar", value: 42)                         | [x]                      |
| T7.22 | Destructuring               | let (a, b) = tuple, let { x, y } = struct           | [x]                      |
| T7.23 | Range expressions           | 1..10, 1..=10                                       | [x]                      |
| T7.24 | Type expressions            | `x as int`, `x is int`, `x?.field`                  | [x]                      |

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
Phase 4 (VM Core) ────────────────────────────┐
    │                                          │
    ▼                                          ▼
Phase 5 (Fiber Runtime) ──────────────┐    Phase 20 (Windows)
    │                                  │
    ├──────────────────────────────────┤
    ▼                                  ▼
Phase 6 (Standard Library)     Phase 19 (Concurrency+)
    │
    ▼
Phase 7 (Advanced Features)  ←── CURRENT ─────┐
    │                                          │
    ▼                                          ▼
Phase 8 (Developer Tooling)  ←── CURRENT   Phase 21 (Language Spec)
    │
    ▼
Phase 9 (Dev Experience)
    │
    ▼
Phase 10 (Performance)
    │
    ▼
Phase 11 (Package Ecosystem) ─────────────────┐
    │                                          │
    ▼                                          ▼
Phase 12 (Native Compilation)         Phase 17 (Testing)
    │
    ▼
Phase 13 (Interoperability) ──────────────────┐
    │                                          │
    ▼                                          ▼
Phase 14 (Self-Hosting)               Phase 18 (Stdlib+)
    │
    ▼
Phase 15 (Metaprogramming)
    │
    ├────────────────────┐
    ▼                    ▼
Phase 16 (Community)    Production-Ready Language
```

---

## Example Applications

Working examples are available in `examples/`:

### Core Language

| Example          | Description              | Status      |
| ---------------- | ------------------------ | ----------- |
| hello.li         | Basic output             | [x] Working |
| fibonacci.li     | Recursive functions      | [x] Working |
| factorial.li     | Recursive + iterative    | [x] Working |
| prime_checker.li | Functions with loops     | [x] Working |
| control_flow.li  | If/else, while, FizzBuzz | [x] Working |

### Data Structures

| Example              | Description               | Status      |
| -------------------- | ------------------------- | ----------- |
| array_ops.li         | Array creation and access | [x] Working |
| structs.li           | Struct definitions        | [x] Working |
| nested_structures.li | Nested arrays and structs | [x] Working |
| string_ops.li        | String manipulation       | [x] Working |

### Operators

| Example                   | Description          | Status      |
| ------------------------- | -------------------- | ----------- |
| operator_comprehensive.li | All operators tested | [x] Working |
| arithmetic_edge_cases.li  | Edge cases for math  | [x] Working |
| compound_assign.li        | +=, -=, \*=, etc.    | [x] Working |
| bitwise_ops.li            | Bitwise operations   | [x] Working |

### Functions & Patterns

| Example            | Description              | Status      |
| ------------------ | ------------------------ | ----------- |
| lambda.li          | Closures and lambdas     | [x] Working |
| pattern_match.li   | Match expressions        | [x] Working |
| pattern_guards.li  | Match with guards        | [x] Working |
| named_arguments.li | Named function arguments | [x] Working |

### Concurrency

| Example          | Description       | Status      |
| ---------------- | ----------------- | ----------- |
| channel_basic.li | Channel creation  | [x] Working |
| fiber_basic.li   | Fiber syntax demo | [x] Working |
| select_basic.li  | Select statement  | [x] Working |

### Types

| Example               | Description                   | Status      |
| --------------------- | ----------------------------- | ----------- |
| integer_types.li      | Sized integer types           | [x] Working |
| null_and_optionals.li | Null and ?? operator          | [x] Working |
| generics_basic.li     | Generic functions and structs | [x] Working |
| type_expressions.li   | Type casting and checking     | [x] Working |
| range_expressions.li  | Range creation 1..10          | [x] Working |

All examples compile with `lirac` and run with `liravm` on macOS/Linux.

---

## Milestone Definitions

| Milestone             | Criteria                              | Status      |
| --------------------- | ------------------------------------- | ----------- |
| **M1: Parser Works**  | Can parse valid Lira files to AST     | ✓ Complete  |
| **M2: Type Checking** | Type errors detected, inference works | ✓ Complete  |
| **M3: Hello World**   | `print("Hello")` compiles and runs    | ✓ Complete  |
| **M4: Functions**     | User-defined functions work           | ✓ Complete  |
| **M5: Closures**      | Lambda expressions with captures      | ✓ Complete  |
| **M6: Fibers**        | Concurrent programs run               | ✓ Complete  |
| **M7: Stdlib**        | Basic stdlib available                | ✓ Complete  |
| **M8: Production**    | Full language feature set             | In Progress |

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

| Metric           | Count   |
| ---------------- | ------- |
| Total tasks      | ~193    |
| Completed        | ~82     |
| In Progress      | ~2      |
| Pending          | ~109    |
| Compiler LOC     | ~15,900 |
| VM LOC           | ~5,600  |
| Core LOC         | ~350    |
| LSP LOC          | ~3,200  |
| Stdlib modules   | 21      |
| LSP features     | 20      |
| Editor plugins   | 6       |
| Example files  | 86      |
| Major Phases   | 22      |

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

_This roadmap tracks the development of Lira as a standalone systems programming language._

---

## Phase 8: Developer Tooling

**Goal**: Provide first-class IDE support and developer tools.
**Depends On**: Phase 7
**Status**: IN PROGRESS

### 8.1 Tree-sitter Grammar

| ID   | Task                | Description                                  | Status |
| ---- | ------------------- | -------------------------------------------- | ------ |
| T8.1 | Tree-sitter grammar | Create grammar.js for Lira syntax            | [x]    |
| T8.2 | Highlight queries   | Write highlights.scm for syntax highlighting | [x]    |
| T8.3 | Fold queries        | Write folds.scm for code folding             | [x]    |
| T8.4 | Locals queries      | Write locals.scm for scope tracking          | [x]    |
| T8.5 | Test corpus         | Create test cases for parser validation      | [x]    |

### 8.2 Language Server (LSP)

| ID    | Task             | Description                                 | Status |
| ----- | ---------------- | ------------------------------------------- | ------ |
| T8.6  | LSP scaffold     | Basic server with tower-lsp                 | [x]    |
| T8.7  | Diagnostics      | Report errors from type checker             | [x]    |
| T8.8  | Completion       | Keyword and symbol completion               | [x]    |
| T8.9  | Hover            | Type info and documentation on hover        | [x]    |
| T8.10 | Go to definition | Navigate to symbol definitions              | [x]    |
| T8.11 | Find references  | Find all references to a symbol             | [x]    |
| T8.12 | Document symbols | Outline view for files                      | [x]    |
| T8.13 | Semantic tokens  | Enhanced syntax highlighting                | [x]    |
| T8.23 | Inlay hints      | Show inferred types inline in editor        | [x]    |
| T8.24 | Code actions     | Quick fixes and refactoring suggestions     | [x]    |
| T8.25 | Rename refactor  | Rename symbols across files                 | [x]    |
| T8.26 | Call hierarchy   | Show callers and callees of functions       | [x]    |
| T8.27 | Signature help   | Parameter hints while typing function calls | [x]    |
| T8.35 | Folding ranges      | Code folding for functions, blocks, imports | [x]    |
| T8.36 | Document links      | Clickable import paths to navigate to files | [x]    |
| T8.37 | User completions    | Complete user-defined symbols (fn, struct)  | [x]    |
| T8.38 | Document highlight  | Highlight all occurrences of symbol         | [x]    |
| T8.39 | Selection range     | Smart expand/shrink selection               | [x]    |
| T8.40 | Workspace symbols   | Search symbols across all files             | [x]    |
| T8.41 | Type definition     | Go to type definition of symbol             | [x]    |

### 8.3 Editor Extensions

| ID    | Task              | Description                      | Status |
| ----- | ----------------- | -------------------------------- | ------ |
| T8.14 | VS Code extension | Syntax + LSP client for VS Code  | [x]    |
| T8.15 | JetBrains plugin  | Syntax via TextMate grammar      | [x]    |
| T8.16 | Vim/Neovim plugin | Syntax highlighting + LSP config | [x]    |
| T8.17 | Zed extension     | Tree-sitter grammar + LSP        | [x]    |
| T8.18 | Helix config      | Tree-sitter + LSP configuration  | [x]    |

### 8.4 Developer Tools

| ID    | Task                           | Description                                              | Status |
| ----- | ------------------------------ | -------------------------------------------------------- | ------ |
| T8.19 | lira-fmt                       | Code formatter                                           | [ ]    |
| T8.20 | lira-doc                       | Documentation generator                                  | [x]    |
| T8.21 | lira-doc - mdbook generation   | Generate mdbook structure from doc comments              | [x]    |
| T8.22 | lira-doc - mdbook enhancements | Custom theme for mdbook doc site and syntax highlighting | [x]    |

### 8.5 Zed Advanced Features

| ID    | Task                 | Description                                               | Status |
| ----- | -------------------- | --------------------------------------------------------- | ------ |
| T8.28 | Zed DAP integration  | Debug adapter protocol for step debugging in Zed          | [ ]    |
| T8.29 | Zed slash commands   | `/lira-run`, `/lira-check`, `/lira-docs` for AI assistant | [ ]    |
| T8.30 | Zed context server   | Provide Lira semantic context to AI assistant             | [ ]    |
| T8.31 | Zed docs indexing    | Index stdlib for `/docs` slash command                    | [ ]    |
| T8.32 | Zed build tasks      | Predefined compile/run task templates                     | [ ]    |
| T8.33 | Zed LSP auto-install | Automatic lira-lsp download and configuration             | [ ]    |
| T8.34 | Zed file icons       | Custom icons for `.li` and `.lic` files (32x32 SVG)       | [ ]    |

---

## Phase 9: Developer Experience & Debugging

**Goal**: Improve developer workflow with interactive tools and debugging support.
**Depends On**: Phase 8
**Status**: IN PROGRESS (2/9 complete)

| ID   | Task                    | Description                                                         | Status |
| ---- | ----------------------- | ------------------------------------------------------------------- | ------ |
| T9.1 | REPL                    | Interactive interpreter for quick experimentation (`lira repl`)     | [ ]    |
| T9.2 | AST Dump Flag           | `lira ast file.li` to output parsed AST as JSON                     | [x]    |
| T9.3 | Bytecode Disassembler   | `lira disasm file.lic` for human-readable bytecode inspection       | [x]    |
| T9.4 | Debug Symbols           | Emit DWARF-like debug info, source mapping for stack traces         | [ ]    |
| T9.5 | Step Debugger           | Interactive debugger with breakpoints, step-in/over, var inspection | [ ]    |
| T9.6 | Watch Mode              | `lira watch` - auto-recompile on file changes                       | [ ]    |
| T9.7 | Error Suggestions       | "Did you mean X?" suggestions for typos and common mistakes         | [ ]    |
| T9.8 | Incremental Compilation | Only recompile changed files and their dependents                   | [ ]    |
| T9.9 | Compilation Caching     | Cache compiled artifacts across sessions                            | [ ]    |

---

## Phase 10: Performance & Profiling

**Goal**: Validate and optimize runtime performance.
**Depends On**: Phase 9
**Status**: PLANNED

| ID     | Task                      | Description                                                  | Status |
| ------ | ------------------------- | ------------------------------------------------------------ | ------ |
| T10.1  | Benchmark Suite           | Standardized benchmarks (fibonacci, primes, sorting, I/O)    | [ ]    |
| T10.2  | Cross-Language Benchmarks | Compare against Go, Rust, Python, Lua, JavaScript, Ruby      | [ ]    |
| T10.3  | Runtime Profiler          | CPU time profiling with flame graphs (`liravm --profile`)    | [ ]    |
| T10.4  | Memory Profiler           | Heap allocation tracking, leak detection, ARC cycle analysis | [ ]    |
| T10.5  | Code Coverage             | Test coverage reports with line-level granularity            | [ ]    |
| T10.6  | Dead Code Elimination     | Remove unreachable code during compilation                   | [ ]    |
| T10.7  | Constant Folding          | Evaluate constant expressions at compile time                | [ ]    |
| T10.8  | Tail Call Optimization    | Optimize recursive tail calls to prevent stack overflow      | [ ]    |
| T10.9  | Escape Analysis           | Determine stack vs heap allocation for better performance    | [ ]    |
| T10.10 | Function Inlining         | Inline small functions to reduce call overhead               | [ ]    |
| T10.11 | Parallel Compilation      | Multi-threaded compiler for faster builds                    | [ ]    |

---

## Phase 11: Package Ecosystem

**Goal**: Enable dependency management and community package sharing.
**Depends On**: Phase 10
**Status**: PLANNED

| ID     | Task             | Description                                                  | Status |
| ------ | ---------------- | ------------------------------------------------------------ | ------ |
| T11.1  | Package Manifest | `lira.toml` for project metadata, dependencies, build config | [ ]    |
| T11.2  | Package Manager  | `lira add`, `lira install`, dependency resolution            | [ ]    |
| T11.3  | Package Registry | Central registry for publishing/discovering packages         | [ ]    |
| T11.4  | Build System     | `lira build`, `lira test`, `lira run` unified CLI            | [ ]    |
| T11.5  | Lockfiles        | `lira.lock` for reproducible builds                          | [ ]    |
| T11.6  | Vendoring        | `lira vendor` to copy dependencies locally                   | [ ]    |
| T11.7  | Feature Flags    | Optional package features (`[features]` in lira.toml)        | [ ]    |
| T11.8  | Security Audit   | `lira audit` to check dependencies for vulnerabilities       | [ ]    |
| T11.9  | License Checking | Verify dependency licenses are compatible                    | [ ]    |
| T11.10 | Version Manager  | `liraup` - install/manage multiple Lira versions (like rustup) | [ ]    |

---

## Phase 12: Native Compilation

**Goal**: Compile to native machine code for production performance.
**Depends On**: Phase 11
**Status**: PLANNED

| ID    | Task                 | Description                                      | Status |
| ----- | -------------------- | ------------------------------------------------ | ------ |
| T12.1 | LLVM Backend         | Compile Lira AST → LLVM IR → native machine code | [ ]    |
| T12.2 | Native Binary Output | Produce standalone executables (no VM required)  | [ ]    |
| T12.3 | Cross-Compilation    | Target different architectures (x86_64, ARM64)   | [ ]    |
| T12.4 | WebAssembly Target   | Compile to WASM for browser/edge deployment      | [ ]    |

---

## Phase 13: Interoperability

**Goal**: Enable integration with existing ecosystems.
**Depends On**: Phase 12
**Status**: PLANNED

| ID    | Task              | Description                                                        | Status |
| ----- | ----------------- | ------------------------------------------------------------------ | ------ |
| T13.1 | C FFI             | Call C libraries, define extern functions, handle ABI              | [ ]    |
| T13.2 | Embedded Runtime  | Embed Lira VM in Rust/C applications as a scripting engine         | [ ]    |
| T13.3 | Async I/O         | Non-blocking I/O integrated with fiber scheduler (io_uring/kqueue) | [ ]    |
| T13.4 | Sandboxing        | Restrict runtime capabilities (file access, network, etc.)         | [ ]    |
| T13.5 | Signal Handling   | Handle OS signals (SIGINT, SIGTERM, etc.)                          | [ ]    |
| T13.6 | Graceful Shutdown | Clean fiber termination on shutdown signals                        | [ ]    |
| T13.7 | Process Spawning  | Spawn and manage child processes                                   | [ ]    |

---

## Phase 14: Self-Hosting

**Goal**: Prove the language by implementing its own toolchain.
**Depends On**: Phase 13
**Status**: PLANNED

| ID    | Task                 | Description                              | Status |
| ----- | -------------------- | ---------------------------------------- | ------ |
| T14.1 | Self-Hosted Compiler | Rewrite lirac in Lira itself (bootstrap) | [ ]    |
| T14.2 | Self-Hosted VM       | Rewrite liravm in Lira (stretch goal)    | [ ]    |

---

## Phase 15: Advanced Language Features

**Goal**: Add powerful metaprogramming and type system extensions.
**Depends On**: Phase 14
**Status**: PLANNED

| ID    | Task                | Description                                               | Status |
| ----- | ------------------- | --------------------------------------------------------- | ------ |
| T15.1 | Macro System        | Compile-time metaprogramming (hygienic macros)            | [ ]    |
| T15.2 | Comptime Evaluation | Compile-time function execution (`const fn` / `comptime`) | [ ]    |
| T15.3 | Effect System       | Track side effects in type system (IO, exceptions)        | [ ]    |

---

## Phase 16: Community & Ecosystem

**Goal**: Build community adoption and learning resources.
**Depends On**: Phase 9+
**Status**: PLANNED

| ID    | Task              | Description                                  | Status |
| ----- | ----------------- | -------------------------------------------- | ------ |
| T16.1 | Online Playground | Web-based REPL (like Go Playground)          | [ ]    |
| T16.2 | Language Tutorial | Interactive "Tour of Lira" guide             | [ ]    |
| T16.3 | Static Analyzer   | Linter with configurable rules (`lira lint`) | [ ]    |
| T16.4 | JIT Compilation   | Hot-path optimization with tracing JIT       | [ ]    |

---

## Phase 17: Testing Framework

**Goal**: Comprehensive testing support for Lira applications.
**Depends On**: Phase 11
**Status**: PLANNED

| ID    | Task                 | Description                                         | Status |
| ----- | -------------------- | --------------------------------------------------- | ------ |
| T17.1 | Inline Tests         | `#[test]` attribute for tests in source files       | [ ]    |
| T17.2 | Doc Tests            | Run code examples in doc comments as tests          | [ ]    |
| T17.3 | Snapshot Testing     | Compare output against saved snapshots              | [ ]    |
| T17.4 | Property-Based Tests | Generate random inputs to find edge cases (fuzzing) | [ ]    |
| T17.5 | Mocking Framework    | Mock functions and types for isolation testing      | [ ]    |
| T17.6 | Test Fixtures        | Setup/teardown helpers for test suites              | [ ]    |
| T17.7 | Parallel Test Runner | Run tests concurrently for faster feedback          | [ ]    |

---

## Phase 18: Standard Library Expansions

**Goal**: Extend stdlib with production-ready modules.
**Depends On**: Phase 6, Phase 13
**Status**: PLANNED

| ID     | Task              | Description                                         | Status |
| ------ | ----------------- | --------------------------------------------------- | ------ |
| T18.1  | std.sql           | Database drivers (SQLite, PostgreSQL, MySQL)        | [ ]    |
| T18.2  | std.cli           | Argument parsing, help generation, subcommands      | [ ]    |
| T18.3  | std.compress      | Compression (gzip, zstd, brotli)                    | [ ]    |
| T18.4  | std.websocket     | WebSocket client and server                         | [ ]    |
| T18.5  | std.template      | Template engine for text generation                 | [ ]    |
| T18.6  | std.csv           | CSV parsing and generation                          | [ ]    |
| T18.7  | std.yaml          | YAML parsing and generation                         | [ ]    |
| T18.8  | std.toml          | TOML parsing and generation                         | [ ]    |
| T18.9  | std.image         | Basic image manipulation (resize, crop, format)     | [ ]    |
| T18.10 | std.crypto        | Encryption (AES, RSA), signatures, key management   | [ ]    |
| T18.11 | std.tls           | TLS/SSL support for secure connections              | [ ]    |
| T18.12 | std.log (structured) | Structured logging with JSON output, log levels  | [ ]    |

---

## Phase 19: Concurrency Enhancements

**Goal**: Advanced concurrency patterns and primitives.
**Depends On**: Phase 5
**Status**: PLANNED

| ID    | Task                    | Description                                          | Status |
| ----- | ----------------------- | ---------------------------------------------------- | ------ |
| T19.1 | Channel Timeouts        | `recv_timeout`, `send_timeout` with deadlines        | [ ]    |
| T19.2 | Context Cancellation    | Propagate cancellation through fiber hierarchies     | [ ]    |
| T19.3 | Structured Concurrency  | Nurseries/scopes - child fibers tied to parent scope | [ ]    |
| T19.4 | Select Default          | Non-blocking select with default case                | [ ]    |
| T19.5 | Broadcast Channels      | One-to-many message distribution                     | [ ]    |
| T19.6 | Work Stealing Scheduler | Improved fiber scheduling for multi-core             | [ ]    |
| T19.7 | Fiber Pools             | Reusable fiber pools for high-throughput workloads   | [ ]    |
| T19.8 | Deadlock Detection      | Runtime detection of channel deadlocks               | [ ]    |

---

## Phase 20: Windows Support

**Goal**: Full Windows platform support for Lira toolchain and runtime.
**Depends On**: Phase 4, Phase 6
**Status**: PLANNED

| ID    | Task                  | Description                                            | Status |
| ----- | --------------------- | ------------------------------------------------------ | ------ |
| T20.1 | Windows CI            | GitHub Actions Windows build and test pipeline         | [ ]    |
| T20.2 | Windows syscalls      | File I/O, process, networking via Windows API          | [ ]    |
| T20.3 | Windows paths         | Handle backslashes, drive letters, UNC paths           | [ ]    |
| T20.4 | Windows console       | ANSI colors, terminal handling on Windows              | [ ]    |
| T20.5 | Windows installer     | MSI or exe installer for Lira toolchain                | [ ]    |
| T20.6 | Chocolatey package    | `choco install lira` distribution                      | [ ]    |
| T20.7 | winget package        | Windows Package Manager distribution                   | [ ]    |
| T20.8 | Windows fiber support | Fiber scheduling compatible with Windows threading     | [ ]    |

---

## Phase 21: Language Specification

**Goal**: Formal language specification for standardization and correctness.
**Depends On**: Phase 7
**Status**: IN PROGRESS

| ID     | Task                    | Description                                              | Status |
| ------ | ----------------------- | -------------------------------------------------------- | ------ |
| T21.1  | Specification format    | Choose format (EBNF + prose, like Go/Rust spec)          | [x]    |
| T21.2  | Lexical specification   | Formal grammar for tokens, keywords, literals            | [x]    |
| T21.3  | Syntax specification    | Complete EBNF grammar for all constructs                 | [x]    |
| T21.4  | Type system spec        | Formal type rules, inference algorithm                   | [x]    |
| T21.5  | Semantics spec          | Operational semantics for expressions/statements         | [x]    |
| T21.6  | Memory model spec       | ARC semantics, object lifecycle, ordering guarantees     | [x]    |
| T21.7  | Concurrency spec        | Fiber scheduling, channel semantics, memory ordering     | [x]    |
| T21.8  | Standard library spec   | API contracts, behavior guarantees for stdlib            | [ ]    |
| T21.9  | Versioning policy       | Semantic versioning, backwards compatibility rules       | [ ]    |
| T21.10 | Specification website   | Published spec at docs.lira-lang.org/spec                | [ ]    |
| T21.11 | Spec validation tool    | lira-spec crate for conformance testing                  | [x]    |
| T21.12 | Tree-sitter comparison  | Compare EBNF spec with tree-sitter grammar               | [x]    |
| T21.13 | Name normalization      | Normalize rule names across spec/tree-sitter/parser      | [ ]    |
| T21.14 | Keyword audit           | Sync keywords across lexer, spec, and tree-sitter        | [ ]    |
| T21.15 | Semantic tests          | Add runtime behavior conformance tests                   | [ ]    |
| T21.16 | Sync strategy doc       | Document source-of-truth hierarchy (SPECIFICATION_SYNC)  | [x]    |

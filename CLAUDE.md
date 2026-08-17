# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lira is a systems programming language with Go-like fiber concurrency, pattern matching, and strong typing. It compiles to bytecode and runs on a custom virtual machine.

## Build Commands

```bash
just build          # Build compiler, VM and native backend (debug)
just release        # Build in release mode
just test           # Run all tests (unit + integration)
just test-verbose   # Run tests with output
just run <file.li>  # Compile and run a .li file on the bytecode VM
just build-native <file.li> <out>  # Compile a .li file to a native executable
just jit <file.li>  # Compile to native code and run it in an isolated worker
just check          # Type check without building
just clippy         # Run Rust linter
just fmt            # Format Rust code
just lsp            # Run LSP server (for testing)
```

**Manual build (without just):**

```bash
cargo build --package lirac --package liravm --package lira-codegen
cargo nextest run --workspace
```

**Run a single test:**

```bash
cargo nextest run --package lirac --test integration -- <test_name>
cargo nextest run --package liravm -- <test_name>
```

## Architecture

The project is organized as a Cargo workspace with six main crates:

### `lira-core` - Shared types

- `opcode.rs` - VM instruction definitions
- `bytecode.rs` - Bytecode format structures

### `lirac` - Compiler (source → bytecode)

Compilation pipeline in order:

1. `lexer.rs` - Tokenization
2. `parser.rs` - AST construction
3. `checker.rs` - Type checking and inference
4. `codegen.rs` - Bytecode generation
5. `module_loader.rs` - Import resolution

Entry points:

- `compile_file()` / `compile_with_imports()` - Full compilation
- `check_file()` / `check_with_imports()` - Type checking only

### `liravm` - Virtual Machine (executes bytecode)

- `vm.rs` - Bytecode interpreter (main execution loop)
- `fiber.rs` - Green thread scheduler
- `runtime.rs` - Built-in functions and syscalls
- `memory.rs` - ARC-based heap management
- `value.rs` - Runtime value types

Entry points:

- `run_file()` / `run()` - Execute bytecode
- `run_with_capture()` - Execute with output capture (for testing)

### `lira-codegen` - Native backend (source → machine code)

Lowers the same checked AST to native code via Cranelift, as an alternative to
the bytecode VM.

- `layout.rs` - Struct/enum memory layout (offsets, alignment)
- `abi.rs` - Lira types → Cranelift machine types
- `lower.rs` - AST → Cranelift IR
- `jit.rs` / `aot.rs` - JIT lowering / standalone executables
- `runtime/` - `liblira_rt`: allocator, strings, arrays, fiber scheduler,
  channels, and the x86-64/AArch64 context switch

Entry points:

- `build_native()` - Compile to a standalone executable (`lira build`)
- `jit_run()` - Compile and run in an isolated worker (`lira jit`)
- `jit_run_in_process()` - Trusted-only in-process JIT API
- `jit_run_isolated()` - Explicit isolated-worker API

The backend is deliberately partial: constructs it cannot lower are reported as
errors rather than mis-compiled, and those programs still run under `lira run`.
See `docs/60-native-backend.md`.

### `lira-lsp` - Language Server Protocol

LSP implementation using tower-lsp for IDE features.

### `lira-doc` - Documentation Generator

Generates Markdown documentation from Lira source files.

- `extractor.rs` - Extracts doc comments and declarations
- `generator.rs` - Produces Markdown output
- `types.rs` - Documentation model types

## Testing

Integration tests use directive comments in `.li` source files:

- `// @expect: <output>` - Expect exact output line
- `// @expect-contains: <text>` - Output contains text
- `// @expect-error` - Expect compilation failure
- `// @skip` - Skip test

Example files in `examples/` serve as both documentation and test cases.

## Key Directories

- `stdlib/` - Standard library modules (`.li` files)
- `examples/` - 87 example programs
- `docs/` - Language specifications
- `editors/` - IDE/editor integrations (VS Code, Vim, Zed, Helix, IntelliJ)

## Language File Extension

Lira source files use `.li` extension. Compiled bytecode uses `.lic`.

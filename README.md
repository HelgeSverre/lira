# Lira

[![Rust](https://img.shields.io/badge/rust-1.94%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE.md)
[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)]()

A modern systems programming language with Go-like fiber concurrency, pattern matching, and a clean syntax. Runs on a custom bytecode VM, or compiles straight to a native executable.

## Features

- **Fiber-based concurrency** — lightweight green threads with channels
- **Pattern matching** — with guards and destructuring
- **Strong typing** — with type inference and generics
- **Clean syntax** — inspired by Rust, Go, and Swift
- **Fast iteration** — bytecode compilation for quick development cycles
- **Native compilation** — a Cranelift backend that emits standalone executables

## Quick Start

### Prerequisites

- **Rust** 1.94+ ([install](https://rustup.rs/))
- **just** command runner ([install](https://github.com/casey/just#installation))

### Setup

```bash
git clone https://github.com/HelgeSverre/lira.git
cd lira

# Build the compiler and VM
just build

# Run the hello world example
just run examples/hello.li

# ...or compile it to a native executable
just build-native examples/hello.li hello && ./hello
```

### Hello World

```lira
// hello.li
fn main() {
    println("Hello, Lira!")
}
```

### Two backends

The same front end feeds two backends. The bytecode VM is the reference
interpreter and starts instantly; the Cranelift backend emits unboxed machine
code and a standalone binary.

```bash
lira run hello.li              # bytecode VM
lira build hello.li -o hello   # standalone native executable
lira jit hello.li              # native code, compiled and run in an isolated worker
```

Because Lira is statically typed, native code carries no tags: an `int` is an
`i64` register, `pt.x` is a load at a constant offset, and a `match` over an enum
is a compare against the discriminant. Fibers keep working — each one gets its
own guarded stack and switches through a hand-written context switch, so
`spawn` and channels behave the same as on the VM.

Native coverage is checked by
`every_frontend_valid_example_executes_on_vm_aot_and_jit_and_matches_directives`.
That bounded exhaustive test recursively discovers 133 files under
`examples/` and `tests/samples/`, rejects the two fixtures marked as expected
compile errors, and executes every other frontend-valid source through bounded
VM, AOT, and JIT runs. The crawler fixture is hermetic, including its TCP
connect path. See [docs/60-native-backend.md](docs/60-native-backend.md) for
the backend architecture and resource boundaries.

## Development

### Commands

| Command                            | Description                                  |
| ---------------------------------- | -------------------------------------------- |
| `just build`                       | Build compiler, VM and native backend (debug) |
| `just release`                     | Build in release mode                        |
| `just test`                        | Run all tests                                |
| `just test-verbose`                | Run tests with output                        |
| `just run <file>`                  | Compile and run a `.li` file on the VM       |
| `just build-native <file> <out>`   | Compile a `.li` file to a native executable  |
| `just jit <file>`                  | Compile to native code and run it in an isolated worker |
| `just check`                       | Type check without building                  |
| `just clippy`                      | Run Rust linter                              |
| `just fmt`                         | Format Rust code                             |
| `just clean`                       | Clean build artifacts                        |

### Manual Build (without just)

```bash
# Build
cargo build --package lirac --package liravm

# Compile a file
cargo run --package lirac -- compile examples/hello.li -o /tmp/hello.lic

# Run bytecode
cargo run --package liravm -- run /tmp/hello.lic

# Run tests
cargo test --workspace
```

### CLI Tools

**lirac** — The Lira compiler

```bash
lirac compile <file.li> [-o output.lic]   # Compile to bytecode
lirac check <file.li>                      # Type check only
lirac --version                            # Show version
```

**liravm** — The Lira virtual machine

```bash
liravm run <file.lic>      # Execute bytecode
liravm run-debug <file>    # Run with debug output
liravm --version           # Show version
```

**lira-lsp** — The Lira language server

```bash
lira-lsp                   # Start LSP server (stdio)
```

**LSP Features:**
- Diagnostics (syntax and type errors)
- Completion (keywords, builtins, user symbols)
- Hover (type info and documentation)
- Go to definition
- Find references
- Document symbols (outline)
- Semantic highlighting
- Signature help (parameter hints)
- Folding ranges
- Document links (clickable imports)
- Inlay hints (inline type annotations)
- Rename symbol
- Call hierarchy (callers/callees)
- Code actions (quick fixes, refactoring)

**lira-doc** — The Lira documentation generator

```bash
lira-doc <file.li>         # Generate Markdown docs for a file
lira-doc stdlib/           # Generate docs for a directory
```

## Editor Support

| Editor   | Extension                              | Syntax | LSP | Tree-sitter | Notes |
| -------- | -------------------------------------- | :----: | :-: | :---------: | ----- |
| VS Code  | [vscode-lira](editors/vscode-lira)     | ✓      | ✓   | —           | Full LSP client |
| Zed      | [zed-lira](editors/zed-lira)           | ✓      | ✓   | ✓           | Auto-configured |
| Neovim   | [vim-lira](editors/vim-lira)           | ✓      | ✓   | ✓           | Multiple LSP options |
| Helix    | [helix-lira](editors/helix-lira)       | ✓      | ✓   | ✓           | Full integration |
| Vim      | [vim-lira](editors/vim-lira)           | ✓      | ✓   | —           | ALE/vim-lsp support |
| IntelliJ | [intellij-lira](editors/intellij-lira) | ✓      | ✓*  | —           | Via LSP4IJ plugin |

*Requires plugin installation - see extension README for setup instructions.

Install with `just <editor>-install` (e.g., `just nvim-install`).

## Project Structure

```
lira/
├── crates/
│   ├── lirac/          # Compiler (lexer, parser, checker, bytecode codegen)
│   ├── liravm/         # Virtual machine (interpreter, fibers, runtime)
│   ├── lira-codegen/   # Native backend (Cranelift, fiber runtime, linker)
│   ├── lira-core/      # Shared types & opcodes
│   ├── lira-lsp/       # Language server (LSP)
│   └── lira-doc/       # Documentation generator
├── editors/            # Editor extensions
│   ├── tree-sitter-lira/   # Tree-sitter grammar
│   ├── vscode-lira/        # VS Code extension
│   ├── vim-lira/           # Vim/Neovim plugin
│   ├── zed-lira/           # Zed extension
│   ├── helix-lira/         # Helix config
│   └── intellij-lira/      # IntelliJ plugin
├── stdlib/             # Standard library (20 modules)
├── examples/           # 87 example programs
├── docs/               # Language specifications
└── justfile            # Build commands
```

## Language Overview

```lira
// Variables and types
let name: string = "Lira"
var count = 0                    // type inferred

// Functions
fn greet(name: string) -> string {
    return "Hello, " + name
}

// Structs with methods
struct Point { x: int, y: int }

impl Point {
    fn distance(self) -> float {
        return sqrt(self.x * self.x + self.y * self.y)
    }
}

// Pattern matching
match value {
    0 => "zero",
    n if n < 0 => "negative",
    _ => "positive"
}

// Fiber concurrency
let ch = chan(1)
spawn { send(ch, compute()) }
let result = recv(ch)

// Imports
import std.fs.{read_file, write_file}
```

## Documentation

| Document                                        | Description                |
| ----------------------------------------------- | -------------------------- |
| [Language Overview](docs/00-lira-overview.md)   | Introduction to Lira       |
| [Type System](docs/02-type-system.md)           | Types, generics, inference |
| [Concurrency](docs/04-concurrency.md)           | Fibers, channels, select   |
| [Standard Library](docs/30-standard-library.md) | Stdlib reference           |
| [Bytecode Format](docs/10-bytecode-format.md)   | `.lic` file specification  |
| [Roadmap](docs/ROADMAP.md)                      | Development progress       |

## Status

**Current Phase**: Developer Tooling (Phase 8)

| Component               | Status                          |
| ----------------------- | ------------------------------- |
| Lexer & Parser          | Complete                        |
| Type System             | Complete                        |
| Bytecode Compiler       | Complete                        |
| VM Core                 | Complete                        |
| Fiber Runtime           | Complete                        |
| Standard Library        | Complete (21 modules)           |
| Language Server         | Complete (20 features)          |
| Documentation Generator | Complete                        |
| Editor Extensions       | Complete (6 editors)            |
| Code Formatter          | Planned                         |
| Debugger                | Planned                         |

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing`)
3. Run tests (`just test`)
4. Run linter (`just clippy`)
5. Commit changes (`git commit -m 'Add amazing feature'`)
6. Push to branch (`git push origin feature/amazing`)
7. Open a Pull Request

## License

MIT License — see [LICENSE](LICENSE.md) for details.

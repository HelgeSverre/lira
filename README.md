# Lira

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE.md)
[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)]()

A modern systems programming language with Go-like fiber concurrency, pattern matching, and a clean syntax. Compiles to bytecode and runs on a custom VM.

## Features

- **Fiber-based concurrency** — lightweight green threads with channels
- **Pattern matching** — with guards and destructuring
- **Strong typing** — with type inference and generics
- **Clean syntax** — inspired by Rust, Go, and Swift
- **Fast iteration** — bytecode compilation for quick development cycles

## Quick Start

### Prerequisites

- **Rust** 1.70+ ([install](https://rustup.rs/))
- **just** command runner ([install](https://github.com/casey/just#installation))

### Setup

```bash
git clone https://github.com/HelgeSverre/lira.git
cd lira

# Build the compiler and VM
just build

# Run the hello world example
just run examples/hello.li
```

### Hello World

```lira
// hello.li
fn main() {
    println("Hello, Lira!")
}
```

## Development

### Commands

| Command             | Description                   |
| ------------------- | ----------------------------- |
| `just build`        | Build compiler and VM (debug) |
| `just release`      | Build in release mode         |
| `just test`         | Run all tests                 |
| `just test-verbose` | Run tests with output         |
| `just run <file>`   | Compile and run a `.li` file  |
| `just check`        | Type check without building   |
| `just clippy`       | Run Rust linter               |
| `just fmt`          | Format Rust code              |
| `just clean`        | Clean build artifacts         |

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

**lira-doc** — The Lira documentation generator

```bash
lira-doc <file.li>         # Generate Markdown docs for a file
lira-doc stdlib/           # Generate docs for a directory
```

## Editor Support

| Editor   | Extension                              | Features              |
| -------- | -------------------------------------- | --------------------- |
| VS Code  | [vscode-lira](editors/vscode-lira)     | Syntax, LSP, snippets |
| Zed      | [zed-lira](editors/zed-lira)           | Syntax, tree-sitter   |
| Neovim   | [vim-lira](editors/vim-lira)           | Syntax, LSP config    |
| Vim      | [vim-lira](editors/vim-lira)           | Syntax highlighting   |
| Helix    | [helix-lira](editors/helix-lira)       | Syntax, LSP config    |
| IntelliJ | [intellij-lira](editors/intellij-lira) | Syntax (TextMate)     |

Install with `just <editor>-install` (e.g., `just nvim-install`).

## Project Structure

```
lira/
├── crates/
│   ├── lirac/          # Compiler (lexer, parser, checker, codegen)
│   ├── liravm/         # Virtual machine (interpreter, fibers, runtime)
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

**Current Phase**: Developer Tooling

| Component               | Status                |
| ----------------------- | --------------------- |
| Lexer & Parser          | Complete              |
| Type System             | Complete              |
| Bytecode Compiler       | Complete              |
| VM Core                 | Complete              |
| Fiber Runtime           | Complete              |
| Standard Library        | Complete (20 modules) |
| Language Server         | Complete              |
| Documentation Generator | Complete              |
| Editor Extensions       | Complete              |

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

# Lira REPL Specification

## Document Information

| Property        | Value               |
| --------------- | ------------------- |
| **Document ID** | 60-lira-repl        |
| **Version**     | 0.1.0-draft         |
| **Status**      | Draft Specification |

---

## Table of Contents

1. [Overview](#1-overview)
2. [Current Status](#2-current-status)
3. [Core Features](#3-core-features)
4. [Command Reference](#4-command-reference)
5. [Help System](#5-help-system)
6. [Implementation Notes](#6-implementation-notes)
7. [Roadmap](#7-roadmap)

---

## 1. Overview

### 1.1 Purpose

The Lira REPL (Read-Eval-Print Loop) provides an interactive environment for:

- Exploring the language interactively
- Testing code snippets quickly
- Learning Lira syntax and features
- Debugging and introspection
- Prototyping before writing full programs

### 1.2 Design Goals

1. **Immediate feedback** - Execute code and see results instantly
2. **Discoverability** - Built-in help for language features, types, and stdlib
3. **Session persistence** - Variables and functions persist during a session
4. **Familiar UX** - Standard terminal editing (arrows, history, tab completion)
5. **Introspection** - Inspect types, AST, and bytecode for learning

### 1.3 Invocation

```bash
# Start interactive REPL
lira repl

# Future: evaluate expression directly
lira eval "1 + 2 * 3"
```

---

## 2. Current Status

> **Note**: The REPL is currently under development. This section documents the current limitations.

### 2.1 What Works

- Basic input/output loop
- Multi-line input with brace tracking (`>` and `..` prompts)
- `:quit`, `:help` commands
- Expression evaluation

### 2.2 Current Limitations

| Limitation | Description |
|------------|-------------|
| **No state persistence** | Variables and functions do not persist between inputs |
| **No line editing** | Cannot use arrow keys to navigate/edit input |
| **No history** | Cannot recall previous commands with up/down arrows |
| **Limited help** | Only shows basic usage, no language reference |
| **No tab completion** | Cannot complete identifiers or keywords |

### 2.3 Example of Current Behavior

```
$ lira repl
Lira REPL v0.1.0
Type :quit to exit, :help for help

> let a = 3
> a
Error: 2:1: Undefined variable: a    // Variables don't persist!
```

---

## 3. Core Features

### 3.1 Session State Persistence

Variables, functions, and type definitions should persist within a session:

```
>>> let x = 42
>>> x * 2
84

>>> fn double(n: int) -> int { n * 2 }
>>> double(x)
84

>>> struct Point { x: int, y: int }
>>> let p = Point { x: 10, y: 20 }
>>> p.x + p.y
30
```

**Implementation approach**: Accumulate top-level declarations and prepend to each evaluation.

### 3.2 Line Editing

Full terminal line editing support:

| Key | Action |
|-----|--------|
| `←` `→` | Move cursor left/right |
| `Ctrl+A` | Move to beginning of line |
| `Ctrl+E` | Move to end of line |
| `Ctrl+W` | Delete word before cursor |
| `Ctrl+U` | Delete to beginning of line |
| `Ctrl+K` | Delete to end of line |
| `Backspace` | Delete character before cursor |
| `Delete` | Delete character at cursor |

### 3.3 Command History

```
>>> let x = 1    // Execute some commands
>>> let y = 2
>>> x + y
3
>>> ↑            // Press up arrow
>>> x + y        // Previous command recalled
>>> ↑
>>> let y = 2    // Earlier command
```

History should:
- Persist across sessions (saved to `~/.lira_history`)
- Support `Ctrl+R` for reverse search
- Be accessible via `:history` command

### 3.4 Multi-line Input

Automatic continuation for incomplete expressions:

```
>>> fn greet(name: string) -> string {
...     return "Hello, " + name + "!"
... }
>>> greet("World")
"Hello, World!"

>>> let numbers = [
...     1, 2, 3,
...     4, 5, 6,
... ]
```

Detection triggers:
- Unclosed braces `{`, `[`, `(`
- Trailing operators (`+`, `-`, `*`, etc.)
- Unclosed string literals

### 3.5 Tab Completion

Complete identifiers, keywords, and stdlib:

```
>>> pri⇥
>>> print             // Completes to 'print'

>>> std.str⇥
>>> std.strings       // Completes stdlib module

>>> ma⇥
>>> match             // Completes keyword
```

Completion sources:
1. Session-defined identifiers (variables, functions, types)
2. Keywords and operators
3. Standard library modules and functions
4. Built-in types and methods

---

## 4. Command Reference

All REPL commands are prefixed with `:`.

### 4.1 Session Commands

| Command | Description |
|---------|-------------|
| `:quit`, `:q` | Exit the REPL |
| `:reset` | Clear session state (variables, functions) |
| `:clear` | Clear the screen |
| `:history` | Show command history |
| `:save <file>` | Save session history to file |
| `:load <file>` | Load and execute a Lira file |

### 4.2 Help Commands

| Command | Description |
|---------|-------------|
| `:help`, `:h` | Show general help |
| `:help <topic>` | Show help for topic |
| `:help keywords` | List all keywords |
| `:help types` | List built-in types |
| `:help operators` | Show operator precedence |

See [Section 5: Help System](#5-help-system) for details.

### 4.3 Introspection Commands

| Command | Description |
|---------|-------------|
| `:type <expr>` | Show the type of an expression |
| `:ast <expr>` | Show the AST of an expression |
| `:bytecode <expr>` | Show generated bytecode |
| `:env` | Show all defined variables and their types |

Examples:

```
>>> :type 1 + 2
int

>>> :type fn(x: int) -> int { x * 2 }
fn(int) -> int

>>> :env
x: int = 42
greet: fn(string) -> string
Point: struct { x: int, y: int }
```

### 4.4 Debugging Commands

| Command | Description |
|---------|-------------|
| `:time <expr>` | Measure execution time |
| `:trace <expr>` | Execute with trace output |

```
>>> :time fibonacci(30)
832040
Time: 12.3ms

>>> :trace 1 + 2 * 3
  PUSH_INT 1
  PUSH_INT 2
  PUSH_INT 3
  MUL
  ADD
7
```

---

## 5. Help System

### 5.1 Overview

The REPL help system provides instant access to language documentation. The same data source should be reusable for:

- REPL `:help` command
- Man pages (`man lira-keywords`)
- Website/API documentation
- LSP hover information

### 5.2 Help Topics

#### Keywords

```
>>> :help fn
fn - Function Declaration

Declares a named function with typed parameters and return type.

Syntax:
    fn name(param: Type, ...) -> ReturnType {
        body
    }

Examples:
    fn add(a: int, b: int) -> int {
        return a + b
    }

    fn greet(name: string) {
        println("Hello, " + name)
    }

See also: return, closure
```

#### Types

```
>>> :help string
string - String Type

An immutable sequence of UTF-8 characters.

Methods:
    len() -> int              Length in characters
    is_empty() -> bool        Check if empty
    contains(s: string) -> bool
    starts_with(s: string) -> bool
    ends_with(s: string) -> bool
    to_upper() -> string
    to_lower() -> string
    trim() -> string
    split(sep: string) -> [string]
    ...

Examples:
    let s = "Hello, World!"
    println(s.len())         // 13
    println(s.to_upper())    // "HELLO, WORLD!"

See also: char, std.strings
```

#### Standard Library Modules

```
>>> :help std.io
std.io - Input/Output Module

Functions:
    print(value: any)         Print without newline
    println(value: any)       Print with newline
    eprint(value: any)        Print to stderr
    eprintln(value: any)      Print to stderr with newline
    read_line() -> string     Read line from stdin

Examples:
    import std.io

    println("Enter your name:")
    let name = read_line()
    println("Hello, " + name)
```

### 5.3 Help Data Format

Help data should be stored in a structured format (e.g., TOML, YAML, or embedded Rust structs):

```toml
# help/keywords/fn.toml
[keyword]
name = "fn"
category = "declaration"
brief = "Function Declaration"
description = """
Declares a named function with typed parameters and return type.
"""

[syntax]
grammar = "fn name(param: Type, ...) -> ReturnType { body }"

[[examples]]
code = """
fn add(a: int, b: int) -> int {
    return a + b
}
"""
description = "Simple function with return value"

[see_also]
keywords = ["return", "closure"]
```

### 5.4 Centralized Help Crate

Create `crates/lira-help/` to centralize language documentation:

```
crates/lira-help/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Public API
│   ├── keywords.rs      # Keyword documentation
│   ├── types.rs         # Type documentation
│   ├── operators.rs     # Operator documentation
│   └── stdlib.rs        # Stdlib documentation
└── data/
    ├── keywords/        # Keyword help files
    ├── types/           # Type help files
    └── operators/       # Operator help files
```

This crate can be used by:
- REPL (`:help` command)
- `lira-doc` (documentation generator)
- `lira-lsp` (hover information)
- Website generator

---

## 6. Implementation Notes

### 6.1 Recommended Dependencies

```toml
[dependencies]
rustyline = "14"         # Line editing, history, completion
rustyline-derive = "0.10" # Helper macros
```

### 6.2 State Persistence Strategy

To persist session state, accumulate declarations:

```rust
struct ReplState {
    /// Accumulated top-level declarations
    declarations: String,
    /// Variables and their types (for :env command)
    variables: HashMap<String, String>,
    /// History for :history command
    history: Vec<String>,
}

impl ReplState {
    fn eval(&mut self, input: &str) -> Result<String, Error> {
        // Build program: declarations + input wrapped in main
        let program = format!(
            "{}\nfn __repl_main() {{ {} }}\n__repl_main()",
            self.declarations,
            input
        );

        // Compile and run
        let bytecode = lirac::compile(&program)?;
        let (_, output) = liravm::run_with_capture(&bytecode)?;

        // If input was a declaration, add it to state
        if is_declaration(input) {
            self.declarations.push_str(input);
            self.declarations.push('\n');
        }

        Ok(output.join("\n"))
    }
}
```

### 6.3 Declaration Detection

Detect declarations to persist:

```rust
fn is_declaration(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.starts_with("let ") ||
    trimmed.starts_with("const ") ||
    trimmed.starts_with("fn ") ||
    trimmed.starts_with("struct ") ||
    trimmed.starts_with("class ") ||
    trimmed.starts_with("enum ") ||
    trimmed.starts_with("type ") ||
    trimmed.starts_with("interface ") ||
    trimmed.starts_with("trait ") ||
    trimmed.starts_with("impl ")
}
```

### 6.4 Rustyline Integration

```rust
use rustyline::{Editor, Result};
use rustyline::error::ReadlineError;

fn repl() -> Result<()> {
    let mut rl = Editor::<()>::new()?;
    rl.load_history(".lira_history").ok();

    let mut state = ReplState::new();

    loop {
        let prompt = if state.continuation { "... " } else { ">>> " };
        match rl.readline(prompt) {
            Ok(line) => {
                rl.add_history_entry(&line);
                // Process line...
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                state.cancel_continuation();
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    rl.save_history(".lira_history")?;
    Ok(())
}
```

---

## 7. Roadmap

### Phase 1: Essential Features

| Feature | Priority | Complexity |
|---------|----------|------------|
| Session state persistence | High | Medium |
| Line editing (rustyline) | High | Low |
| Command history | High | Low |
| Multi-line input | High | Low (partial exists) |

### Phase 2: Help System

| Feature | Priority | Complexity |
|---------|----------|------------|
| `:help` basic topics | High | Medium |
| Keyword documentation | High | Medium |
| Type documentation | Medium | Medium |
| Stdlib documentation | Medium | High |
| Centralized help crate | Medium | High |

### Phase 3: Introspection

| Feature | Priority | Complexity |
|---------|----------|------------|
| `:type` command | High | Low |
| `:env` command | Medium | Low |
| `:ast` command | Low | Medium |
| `:bytecode` command | Low | Medium |

### Phase 4: Polish

| Feature | Priority | Complexity |
|---------|----------|------------|
| Tab completion | Medium | Medium |
| Syntax highlighting | Low | High |
| `:time` benchmarking | Low | Low |
| `:edit` external editor | Low | Medium |
| Configuration file | Low | Low |

---

## Appendix A: Comparison with Other REPLs

| Feature | Python | Node.js | Lira (Planned) |
|---------|--------|---------|----------------|
| State persistence | Yes | Yes | Yes |
| Line editing | Yes | Yes | Yes |
| History | Yes | Yes | Yes |
| Tab completion | Yes | Yes | Yes |
| Multi-line | Yes | Yes | Yes |
| Type inspection | Limited | Limited | Strong |
| AST/bytecode view | No | No | Yes |
| Integrated help | Yes | No | Yes |

---

## Appendix B: Configuration

Future: `~/.config/lira/repl.toml`

```toml
[repl]
# Prompt customization
prompt = ">>> "
continuation_prompt = "... "

# History
history_file = "~/.lira_history"
history_size = 1000

# Behavior
auto_indent = true
multiline_paste = true

# Display
show_types = false     # Auto-show type of result
syntax_highlight = true
```

---

_This document is part of the Lira Language Specification._

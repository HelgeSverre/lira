# Lira Tooling Specification

## Document Information

| Property | Value |
|----------|-------|
| **Document ID** | 50-tooling |
| **Version** | 1.0.0-draft |
| **Status** | Draft Specification |

---

## Table of Contents

1. [Overview](#1-overview)
2. [lirac - Compiler](#2-lirac---compiler)
3. [liravm - Virtual Machine](#3-liravm---virtual-machine)
4. [lir - REPL/Runner](#4-lir---replrunner)
5. [lifmt - Formatter](#5-lifmt---formatter)
6. [lidoc - Documentation Generator](#6-lidoc---documentation-generator)
7. [lipkg - Package Manager](#7-lipkg---package-manager)
8. [IDE Support](#8-ide-support)

---

## 1. Overview

### 1.1 Tool Suite

```
┌─────────────────────────────────────────────────────────────────┐
│                    LI-LANG TOOL SUITE                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                     DEVELOPMENT TOOLS                       ││
│  │                                                             ││
│  │  lirac      Compiler - Compile .li/.liui to .lic bytecode   ││
│  │  liravm     VM - Execute .lic bytecode                       ││
│  │  lir      REPL - Interactive Lira shell                 ││
│  │  lifmt    Formatter - Format Lira source code           ││
│  │  lidoc    Docs - Generate documentation                    ││
│  │  lipkg    Package - Package manager                        ││
│  │                                                             ││
│  └─────────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                      IDE INTEGRATION                        ││
│  │                                                             ││
│  │  li-lsp   Language Server Protocol implementation          ││
│  │  li-dap   Debug Adapter Protocol implementation            ││
│  │                                                             ││
│  └─────────────────────────────────────────────────────────────┘│
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Installation

Lira tools are available after building:

```bash
# All tools available in /usr/bin
/usr/bin/lic
/usr/bin/livm
/usr/bin/lir
/usr/bin/lifmt
/usr/bin/lidoc
/usr/bin/lipkg
```

---

## 2. lirac - Compiler

### 2.1 Basic Usage

```bash
# Compile a single file
lirac build src/main.li

# Compile and output to specific location
lirac build src/main.li -o build/app.lic

# Compile a project (uses li.toml)
lirac build

# Check for errors without compiling
lirac check

# Check a specific file
lirac check src/module.li
```

### 2.2 Command Reference

```
lirac - Lira Compiler

USAGE:
    lirac <COMMAND> [OPTIONS]

COMMANDS:
    build       Compile source files to bytecode
    check       Check for errors without producing output
    run         Compile and run immediately
    clean       Remove build artifacts
    init        Create a new Lira project
    new         Create a new Lira project in a new directory

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information
    -v, --verbose    Enable verbose output
    -q, --quiet      Suppress output except errors
```

### 2.3 Build Command

```bash
lirac build [OPTIONS] [FILES...]

OPTIONS:
    -o, --output <PATH>      Output file or directory
    --release                Build with optimizations
    --debug                  Include debug information (default)
    --target <TARGET>        Target platform (macos-x86_64, linux-x86_64)
    --lib                    Build as library
    --features <FEATURES>    Comma-separated feature flags
    -j, --jobs <N>           Number of parallel jobs

EXAMPLES:
    # Build project in release mode
    lirac build --release

    # Build with specific features
    lirac build --features gui,network

    # Build specific file
    lirac build src/main.li -o app.lic
```

### 2.4 Check Command

```bash
lirac check [OPTIONS] [FILES...]

OPTIONS:
    --all-targets    Check all targets (lib, bin, tests, examples)
    --tests          Check test files
    --lib            Check library only

EXAMPLES:
    # Check entire project
    lirac check

    # Check including tests
    lirac check --tests
```

### 2.5 Run Command

```bash
lirac run [OPTIONS] [-- ARGS...]

OPTIONS:
    --release        Run with optimizations
    --example <NAME> Run an example
    --bin <NAME>     Run a specific binary

EXAMPLES:
    # Run main.li
    lirac run

    # Run with arguments
    lirac run -- arg1 arg2

    # Run an example
    lirac run --example counter
```

### 2.6 Project Initialization

```bash
# Create new project in current directory
lirac init

# Create new project in new directory
lirac new my_app

# Create library project
lirac new my_lib --lib

# Create GUI application
lirac new my_gui --template gui
```

Generated project structure:

```
my_app/
├── li.toml
├── src/
│   └── main.li
├── ui/
│   └── main.liui
└── .gitignore
```

---

## 3. liravm - Virtual Machine

### 3.1 Basic Usage

```bash
# Run bytecode file
liravm app.lic

# Run with arguments
liravm app.lic -- arg1 arg2

# Run with debug output
liravm --debug app.lic
```

### 3.2 Command Reference

```
liravm - Lira Virtual Machine

USAGE:
    liravm [OPTIONS] <FILE> [-- ARGS...]

OPTIONS:
    -h, --help           Print help information
    -V, --version        Print version information
    --debug              Enable debug mode
    --trace              Trace execution
    --profile            Enable profiling
    --heap-size <SIZE>   Set heap size (e.g., 256M)
    --stack-size <SIZE>  Set stack size (e.g., 1M)

EXAMPLES:
    liravm app.lic
    liravm --heap-size 512M app.lic
    liravm --debug app.lic
```

### 3.3 Debug Mode

```bash
# Start in debug mode
liravm --debug app.lic

# Debug commands (at debug prompt)
> break main.li:42          # Set breakpoint
> continue                  # Continue execution
> step                      # Step one instruction
> next                      # Step over function calls
> print variable            # Print variable value
> stack                     # Show call stack
> locals                    # Show local variables
> quit                      # Exit debugger
```

### 3.4 Profiling

```bash
# Run with profiler
liravm --profile app.lic

# Output:
#   Total time: 1.234s
#   Functions:
#     process_data   45.2%  0.557s
#     parse_input    32.1%  0.396s
#     main           12.3%  0.152s
#     ...
```

---

## 4. lir - REPL/Runner

### 4.1 Basic Usage

```bash
# Start interactive REPL
lir

# Run a file
lir run script.li

# Evaluate expression
lir eval "1 + 2 * 3"
```

### 4.2 REPL Session

```
$ lir
Lira REPL v1.0.0
Type :help for help, :quit to exit.

>>> let x = 42
>>> x * 2
84

>>> fn greet(name: string) -> string {
...     return "Hello, " + name + "!"
... }

>>> greet("World")
"Hello, World!"

>>> :type greet
fn(string) -> string

>>> :quit
```

### 4.3 REPL Commands

```
REPL Commands (prefix with :)

:help              Show this help
:quit              Exit REPL
:clear             Clear screen
:reset             Reset REPL state
:type <expr>       Show type of expression
:ast <expr>        Show AST of expression
:bytecode <expr>   Show bytecode of expression
:load <file>       Load and execute file
:save <file>       Save session to file
:history           Show command history
```

### 4.4 Script Mode

```bash
# Run script file
lir run script.li

# With arguments
lir run script.li -- arg1 arg2

# Watch mode (re-run on file changes)
lir run --watch script.li
```

---

## 5. lifmt - Formatter

### 5.1 Basic Usage

```bash
# Format a file (output to stdout)
lifmt src/main.li

# Format a file in place
lifmt -w src/main.li

# Format all files in directory
lifmt -w src/

# Check formatting without changes
lifmt --check src/
```

### 5.2 Command Reference

```
lifmt - Lira Formatter

USAGE:
    lifmt [OPTIONS] [FILES...]

OPTIONS:
    -h, --help           Print help information
    -w, --write          Write changes to files
    --check              Check if files are formatted
    --diff               Show diff of changes
    --config <FILE>      Use custom config file
    --stdin              Read from stdin
    --stdin-filepath     Path to use for stdin

EXAMPLES:
    lifmt src/main.li           # Print formatted output
    lifmt -w src/               # Format all files in src/
    lifmt --check .             # Check all files
    lifmt --diff src/main.li    # Show diff
```

### 5.3 Configuration

Create `.lifmt.toml` in project root:

```toml
# .lifmt.toml

# Indentation
indent_style = "space"  # "space" or "tab"
indent_width = 4

# Line length
max_line_length = 100

# Braces
brace_style = "same_line"  # "same_line" or "next_line"

# Trailing commas
trailing_comma = true

# Blank lines
max_blank_lines = 1
blank_line_before_fn = true

# Imports
sort_imports = true
group_imports = true  # Separate std, external, local

# Strings
prefer_single_quotes = false
```

### 5.4 Output Examples

Before:
```li
fn   calculate(x:int,y:int)->int{
let result=x+y
return result}
```

After:
```li
fn calculate(x: int, y: int) -> int {
    let result = x + y
    return result
}
```

---

## 6. lidoc - Documentation Generator

### 6.1 Basic Usage

```bash
# Generate docs for project
lidoc

# Generate docs for specific module
lidoc src/lib.li

# Open docs in browser
lidoc --open

# Output to specific directory
lidoc -o docs/api/
```

### 6.2 Command Reference

```
lidoc - Lira Documentation Generator

USAGE:
    lidoc [OPTIONS] [FILES...]

OPTIONS:
    -h, --help           Print help information
    -o, --output <DIR>   Output directory (default: ./docs)
    --open               Open in browser after generating
    --format <FORMAT>    Output format: html, markdown
    --private            Include private items
    --no-deps            Don't document dependencies
    --theme <THEME>      Documentation theme

EXAMPLES:
    lidoc                    # Generate docs for project
    lidoc -o api-docs/       # Custom output directory
    lidoc --format markdown  # Generate markdown
```

### 6.3 Documentation Comments

```li
/// A user account in the system.
///
/// Users can have multiple roles and permissions.
///
/// # Example
/// ```li
/// let user = User.new("alice", "alice@example.com")
/// user.add_role(Role.Admin)
/// ```
pub class User {
    /// The unique username
    pub let username: string

    /// Email address for notifications
    pub let email: string

    /// Create a new user
    ///
    /// # Arguments
    /// * `username` - The unique username
    /// * `email` - Email address
    ///
    /// # Returns
    /// A new User instance
    pub static fn new(username: string, email: string) -> User {
        // ...
    }
}
```

### 6.4 Generated Documentation

The documentation includes:
- Module overview and organization
- Type definitions with fields and methods
- Function signatures with descriptions
- Example code blocks (runnable)
- Cross-references and links
- Search functionality

---

## 7. lipkg - Package Manager

### 7.1 Basic Usage

```bash
# Initialize a new package
lipkg init

# Add a dependency
lipkg add json

# Add with specific version
lipkg add http@1.2.0

# Remove a dependency
lipkg remove json

# Update dependencies
lipkg update

# Install dependencies
lipkg install
```

### 7.2 Command Reference

```
lipkg - Lira Package Manager

USAGE:
    lipkg <COMMAND> [OPTIONS]

COMMANDS:
    init        Initialize a new package
    add         Add a dependency
    remove      Remove a dependency
    update      Update dependencies
    install     Install dependencies
    publish     Publish package to registry
    search      Search for packages
    info        Show package information

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information
```

### 7.3 Package Manifest

`li.toml`:

```toml
[package]
name = "my_app"
version = "1.0.0"
description = "My Lira application"
authors = ["Author Name <author@example.com>"]
license = "MIT"

[dependencies]
json = "1.2"
http = { version = "0.5", features = ["tls"] }

[dev-dependencies]
test = "1.0"
```

### 7.4 Package Commands

```bash
# Search for packages
lipkg search json
#   json       1.2.3   JSON parsing and serialization
#   json-rpc   0.5.0   JSON-RPC protocol implementation

# Show package info
lipkg info json
#   json v1.2.3
#   JSON parsing and serialization for Lira
#
#   Homepage: https://...
#   Repository: https://...
#   License: MIT
#   Dependencies: none

# Publish package
lipkg publish
#   Publishing my_app v1.0.0
#   Uploaded successfully!
```

---

## 8. IDE Support

### 8.1 Language Server (li-lsp)

```bash
# Start language server
li-lsp

# With specific options
li-lsp --stdio
li-lsp --tcp --port 9999
```

Features:
- Code completion
- Go to definition
- Find references
- Hover information
- Diagnostics (errors/warnings)
- Code actions (quick fixes)
- Formatting
- Rename symbol
- Document symbols
- Workspace symbols

### 8.2 Debug Adapter (li-dap)

```bash
# Start debug adapter
li-dap

# Options
li-dap --stdio
li-dap --port 9998
```

Features:
- Breakpoints (line, conditional, function)
- Step in/out/over
- Variable inspection
- Call stack
- Watch expressions
- Exception breakpoints

### 8.3 Editor Configuration

#### VS Code

Extension `lira`:

```json
// settings.json
{
    "lira.format.onSave": true,
    "lira.check.onSave": true,
    "lira.lsp.path": "/usr/bin/li-lsp"
}
```

#### Vim/Neovim

```vim
" .vimrc
Plug 'lira/lira.vim'

" Format on save
autocmd BufWritePre *.li :LiFormat
```

#### Emacs

```elisp
;; init.el
(require 'li-mode)
(add-hook 'li-mode-hook 'lsp)
```

---

## Appendix A: Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 10 | Syntax error |
| 11 | Type error |
| 12 | Resolution error |
| 20 | Runtime error |
| 21 | VM error |
| 30 | I/O error |
| 31 | Network error |

---

## Appendix B: Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `LI_HOME` | Lira installation directory | `/usr/lib/li` |
| `LI_PATH` | Module search path | `.:/usr/lib/li/std` |
| `LI_DEBUG` | Enable debug output | `0` |
| `LI_COLOR` | Enable colored output | `auto` |
| `LI_HEAP_SIZE` | Default heap size | `256M` |
| `LI_STACK_SIZE` | Default stack size | `1M` |

---

## Appendix C: Configuration Files

### Global Configuration

`~/.li/config.toml`:

```toml
[build]
jobs = 4
color = "auto"

[format]
style = "default"

[package]
registry = "https://packages.helge.io"
```

### Project Configuration

`li.toml` (see Package Manifest above)

---

*This document is part of the Lira Language Specification.*

# Lira Module System Specification

## Document Information

| Property          | Value                       |
| ----------------- | --------------------------- |
| **Document ID**   | 05-module-system            |
| **Version**       | 1.0.0-draft                 |
| **Status**        | Draft Specification         |
| **Prerequisites** | 00-04 (core language specs) |

---

## Table of Contents

1. [Module System Overview](#1-module-system-overview)
2. [Modules](#2-modules)
3. [Imports](#3-imports)
4. [Exports](#4-exports)
5. [Packages](#5-packages)
6. [Standard Library](#6-standard-library)
7. [Build System Integration](#7-build-system-integration)

---

## 1. Module System Overview

### 1.1 Design Goals

Lira's module system is designed for:

1. **Encapsulation**: Hide implementation details
2. **Reusability**: Share code between files and projects
3. **Clarity**: Explicit imports/exports, no implicit globals
4. **Scalability**: Support large codebases with many files

### 1.2 Module Hierarchy

```
Package (my_app)
├── Module (src/main.li)
├── Module (src/lib.li)
└── Submodule (src/utils/)
    ├── Module (src/utils/mod.li)
    ├── Module (src/utils/string.li)
    └── Module (src/utils/math.li)
```

### 1.3 Visibility Levels

| Visibility      | Keyword    | Accessible From        |
| --------------- | ---------- | ---------------------- |
| Private         | (default)  | Same file only         |
| Package-private | `internal` | Same package           |
| Public          | `pub`      | Anywhere (if exported) |

---

## 2. Modules

### 2.1 File as Module

Each `.li` file is a module. The module name is derived from the file path:

```
src/
├── main.li           # Module: main
├── utils.li          # Module: utils
└── models/
    ├── mod.li        # Module: models
    ├── user.li       # Module: models.user
    └── post.li       # Module: models.post
```

### 2.2 Module Declaration

Optional explicit module declaration:

```li
// src/utils/string.li
mod utils.string

// Declarations follow
pub fn trim(s: string) -> string {
    // ...
}
```

If omitted, the module name is inferred from the file path.

### 2.3 Module Hierarchy

Create submodules with directories:

```
src/utils/
├── mod.li            # Module root (optional)
├── string.li         # Submodule
└── math.li           # Submodule
```

**mod.li** (module root):

```li
// src/utils/mod.li
// Re-export submodules
pub mod string
pub mod math

// Additional utilities
pub fn helper() { }
```

### 2.4 Inline Modules

Define submodules within a file:

```li
// main.li
mod helpers {
    pub fn format(n: int) -> string {
        return "${n}"
    }
}

fn main() {
    let s = helpers.format(42)
}
```

---

## 3. Imports

### 3.1 Basic Import

```li
// Import entire module
import std.io

fn main() {
    std.io.print("Hello")
}
```

### 3.2 Import with Alias

```li
// Import with alias
import std.collections as col

fn main() {
    let list: col.List<int> = []
}
```

### 3.3 Import Specific Items

```li
// Import specific items
import std.io.{print, println}

fn main() {
    print("Hello")
    println("World")
}
```

### 3.4 Import All (Glob Import)

```li
// Import all public items (use sparingly)
import std.io.*

fn main() {
    print("Hello")  // Directly available
}
```

### 3.5 Nested Imports

```li
// Multiple imports from same base
import std.{
    io.{print, println},
    collections.{List, Map},
    string.trim,
}
```

### 3.6 Relative Imports

```li
// Import from same package
import .utils           // ./utils.li
import .models.user     // ./models/user.li
import ..shared         // ../shared.li
import ...common        // ../../common.li
```

### 3.7 Import Renaming

```li
// Rename specific imports
import std.collections.List as ArrayList
import std.collections.Map as HashMap

let list: ArrayList<int> = []
let map: HashMap<string, int> = {}
```

### 3.8 Conditional Imports

```li
// Platform-specific imports
#[cfg(target = "macos")]
import darwin.gui

#[cfg(target = "test")]
import std.test.{assert, mock}
```

---

## 4. Exports

### 4.1 Public Declarations

Use `pub` to make declarations public:

```li
// Public function
pub fn public_function() { }

// Private function (default)
fn private_function() { }

// Public type
pub class PublicClass { }

// Public constant
pub const VERSION = "1.0.0"
```

### 4.2 Export Keyword

Explicit export for module interface:

```li
// utils.li

// Internal implementation
fn internal_helper() { }

// Exported functions
export fn public_api() {
    internal_helper()
}

export class Config {
    // ...
}
```

### 4.3 Re-exports

Re-export items from other modules:

```li
// lib.li - Package entry point

// Re-export from submodules
pub use models.User
pub use models.Post
pub use utils.{format, parse}

// Re-export with rename
pub use internal.Detail as PublicName

// Re-export everything from submodule
pub use models.*
```

### 4.4 Visibility Modifiers

```li
class MyClass {
    pub let public_field: int         // Accessible anywhere
    internal let package_field: int    // Same package only
    let private_field: int             // Same file only
    priv let explicit_private: int     // Explicit private
}
```

### 4.5 Export Groups

```li
// Group exports at end of file
export {
    User,
    Post,
    Comment,
    create_user,
    delete_user,
}
```

---

## 5. Packages

### 5.1 Package Structure

```
my_package/
├── li.toml              # Package manifest
├── src/
│   ├── lib.li           # Library entry point
│   ├── main.li          # Executable entry point (optional)
│   └── ...
├── tests/
│   ├── test_utils.li
│   └── ...
├── examples/
│   └── demo.li
└── docs/
    └── README.md
```

### 5.2 Package Manifest (li.toml)

```toml
[package]
name = "my_app"
version = "1.0.0"
description = "My Lira application"
authors = ["Author Name <author@example.com>"]
license = "MIT"

# Entry points
[entry]
main = "src/main.li"      # Executable
lib = "src/lib.li"        # Library

[dependencies]
std = "1.0"
http = "0.5.0"
json = { version = "1.2", features = ["pretty"] }

# Git dependency
utils = { git = "https://github.com/user/utils.git", tag = "v1.0.0" }

# Local dependency
local_lib = { path = "../local_lib" }

[dev-dependencies]
test_framework = "1.0"
mock = "0.3"

[build]
optimization = "release"
target = "macos-x86_64"

[features]
default = ["std"]
gui = ["native-gui"]
network = ["http", "websocket"]
```

### 5.3 Dependency Resolution

Dependencies are resolved from:

1. **Local path**: `{ path = "../local_lib" }`
2. **Git repository**: `{ git = "...", tag = "..." }`
3. **Package registry**: `"1.0.0"` (version string)

Version syntax:

- `"1.0.0"` - Exact version
- `"^1.0"` - Compatible with 1.x
- `"~1.0"` - Approximately 1.0.x
- `">=1.0, <2.0"` - Range

### 5.4 Workspaces

For multi-package projects:

```toml
# workspace/li.toml
[workspace]
members = [
    "core",
    "cli",
    "gui",
]

[workspace.dependencies]
# Shared dependencies
serde = "1.0"
```

Individual packages reference workspace:

```toml
# workspace/core/li.toml
[package]
name = "core"
version = "1.0.0"

[dependencies]
serde = { workspace = true }
```

---

## 6. Standard Library

### 6.1 Standard Library Structure

```
std/
├── core/           # Fundamental types (auto-imported)
├── io/             # I/O operations
├── fs/             # File system
├── collections/    # Data structures
├── string/         # String utilities
├── math/           # Mathematical functions
├── sync/           # Synchronization primitives
├── channel/        # Channel types
├── time/           # Time and duration
├── os/             # OS interaction
└── net/            # Networking
```

### 6.2 Prelude (Auto-Imported)

The following are automatically available:

```li
// Always available without import
bool, int, float, string, char, void
List<T>, Map<K,V>, Set<T>
Option<T>, Result<T, E>
print, println
assert, panic
```

### 6.3 Standard Library Modules

#### std.core

```li
import std.core.{
    Bool, Int, Float, String, Char,
    List, Map, Set, Tuple,
    Option, Result,
    Clone, Copy, Eq, Hash, Ord,
    ToString, FromString,
    Default, Debug,
}
```

#### std.io

```li
import std.io.{
    print, println, eprint, eprintln,
    read_line,
    stdin, stdout, stderr,
    Reader, Writer, BufReader, BufWriter,
}
```

#### std.fs

```li
import std.fs.{
    File, OpenOptions,
    read_file, write_file, append_file,
    read_dir, create_dir, remove_dir,
    exists, is_file, is_dir,
    copy, rename, remove,
    Path, PathBuf,
}
```

#### std.collections

```li
import std.collections.{
    List, Map, Set,
    Queue, Deque, Stack,
    BinaryHeap, BTreeMap, BTreeSet,
    LinkedList,
}
```

#### std.string

```li
import std.string.{
    trim, trim_start, trim_end,
    split, join,
    contains, starts_with, ends_with,
    replace, replace_all,
    to_lowercase, to_uppercase,
    pad_start, pad_end,
    repeat,
}
```

#### std.math

```li
import std.math.{
    PI, E, TAU,
    abs, min, max, clamp,
    floor, ceil, round, trunc,
    sqrt, cbrt, pow,
    sin, cos, tan, asin, acos, atan, atan2,
    sinh, cosh, tanh,
    exp, ln, log, log10, log2,
    random, random_range,
}
```

#### std.sync

```li
import std.sync.{
    Mutex, RwLock,
    Semaphore, WaitGroup,
    Once, Condvar,
    atomic.*,
}
```

#### std.time

```li
import std.time.{
    Duration,
    Instant, SystemTime,
    sleep, timeout, ticker,
    now, elapsed,
}
```

#### std.os

```li
import std.os.{
    env, set_env, remove_env,
    args, current_dir, set_current_dir,
    exit,
    Process, Command,
}
```

---

## 7. Build System Integration

### 7.1 Build Commands

```bash
# Compile to bytecode
lirac build

# Compile specific file
lirac build src/main.li -o build/main.lic

# Release build (optimized)
lirac build --release

# Check without building
lirac check

# Run directly
lirac run

# Run with arguments
lirac run -- arg1 arg2
```

### 7.2 Build Configuration

```toml
# li.toml

[build]
# Output directory
out_dir = "build"

# Optimization level: "none", "size", "speed", "aggressive"
optimization = "speed"

# Include debug info
debug_info = true

# Target platform
target = "macos-x86_64"

# Custom target specification
# target_spec = "custom-target.json"

[build.release]
optimization = "aggressive"
debug_info = false
```

### 7.3 Conditional Compilation

```li
// Platform-specific code
#[cfg(target = "macos")]
fn platform_init() {
    macos_init()
}

#[cfg(target = "test")]
fn platform_init() {
    test_init()
}

// Feature flags
#[cfg(feature = "gui")]
import native.gui

#[cfg(feature = "network")]
mod network_module { }

// Combine conditions
#[cfg(all(target = "macos", feature = "gui"))]
fn init_gui() { }

#[cfg(any(debug, feature = "verbose"))]
fn debug_log(msg: string) {
    println("[DEBUG] ${msg}")
}

#[cfg(not(feature = "legacy"))]
fn new_api() { }
```

### 7.4 Build Scripts

```li
// build.li - Runs before compilation

fn main() {
    // Generate code
    let version = read_file("VERSION").trim()
    write_file("src/version.li", """
        pub const VERSION = "${version}"
    """)

    // Set environment
    env.set("LI_BUILD_TIME", now().to_string())
}
```

### 7.5 Module Resolution Order

When resolving `import foo.bar`:

1. Check `src/foo/bar.li`
2. Check `src/foo/bar/mod.li`
3. Check dependencies in `li.toml`
4. Check standard library (`std.*`)

---

## Appendix A: Import Grammar

```
ImportDecl     ::= 'import' ImportPath ImportItems? ImportAlias?
ImportPath     ::= ModulePath | RelativePath
ModulePath     ::= Identifier ('.' Identifier)*
RelativePath   ::= ('.')+ Identifier ('.' Identifier)*
ImportItems    ::= '.' '{' ImportItem (',' ImportItem)* '}'
               | '.' '*'
               | '.' Identifier
ImportItem     ::= Identifier ('as' Identifier)?
ImportAlias    ::= 'as' Identifier
```

---

## Appendix B: Export Grammar

```
ExportDecl     ::= 'export' ExportItems
               | 'export' Declaration
               | 'pub' 'use' ImportPath ImportItems?
ExportItems    ::= '{' ExportItem (',' ExportItem)* '}'
ExportItem     ::= Identifier ('as' Identifier)?
```

---

## Appendix C: Package Name Conventions

- Package names: `lowercase-with-hyphens`
- Module names: `snake_case`
- Public types: `PascalCase`
- Public functions: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

---

_This document is part of the Lira Language Specification._

# IntelliJ Lira Plugin

Syntax highlighting for Lira programming language in JetBrains IDEs.

## Installation

### From JetBrains Marketplace (Coming Soon)

1. Open your JetBrains IDE (IntelliJ IDEA, WebStorm, etc.)
2. Go to Settings/Preferences > Plugins
3. Search for "Lira"
4. Click Install

### Manual Installation

1. Download the latest `.zip` from releases
2. Go to Settings/Preferences > Plugins > Gear icon > Install Plugin from Disk
3. Select the downloaded `.zip` file

### Build from Source

```bash
cd editors/intellij-lira
./gradlew buildPlugin
# Plugin will be in build/distributions/
```

## Features

- Syntax highlighting for `.li` and `.lira` files
- Comment toggling (Ctrl+/)
- Bracket matching

## LSP Support (Experimental)

For full IDE features, you can use a generic LSP plugin with `lira-lsp`:

### Option 1: LSP4IJ Plugin

1. Install the [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin from JetBrains Marketplace
2. Install `lira-lsp`:
   ```bash
   cargo install --path crates/lira-lsp
   ```
3. Configure LSP4IJ:
   - Go to Settings > Languages & Frameworks > Language Server Protocol
   - Add a new server configuration:
     - Name: `Lira`
     - Command: `lira-lsp`
     - File patterns: `*.li`, `*.lira`

### Option 2: LSP Support Plugin

1. Install the [LSP Support](https://plugins.jetbrains.com/plugin/10209-lsp-support) plugin
2. Configure in Settings > Languages & Frameworks > Language Server Protocol:
   - Server executable: `lira-lsp` (or full path)
   - Languages: `lira`

### LSP Features

When configured, you get:
- Real-time error diagnostics
- Code completion
- Hover information
- Go to definition
- Find references

## Highlighted Elements

- **Keywords**: `fn`, `let`, `var`, `const`, `struct`, `class`, `enum`, `trait`, `impl`, etc.
- **Control Flow**: `if`, `else`, `match`, `while`, `for`, `loop`, `break`, `continue`, `return`
- **Concurrency**: `spawn`, `select`, `async`
- **Types**: `int`, `float`, `bool`, `string`, `List`, `Map`, `Option`, `Result`, etc.
- **Literals**: Numbers, strings, characters, booleans
- **Comments**: Line (`//`) and block (`/* */`) comments
- **String Interpolation**: `${expression}`

## Development

### Prerequisites

- JDK 17+
- Gradle 8+

### Build

```bash
./gradlew build
```

### Run in Development IDE

```bash
./gradlew runIde
```

### Package

```bash
./gradlew buildPlugin
```

## License

MIT

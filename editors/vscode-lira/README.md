# Lira for VS Code

Syntax highlighting and language server support for the Lira programming language.

## Features

- Syntax highlighting for `.li` and `.lira` files
- LSP integration for:
  - Diagnostics (error reporting)
  - Code completion
  - Hover information
  - Go to definition
  - Find all references
  - Document outline

## Installation

### From VS Code Marketplace (Coming Soon)

1. Open VS Code
2. Go to Extensions (Ctrl+Shift+X)
3. Search for "Lira"
4. Click Install

### Install from VSIX

```bash
# Build the extension
cd editors/vscode-lira
npm install
npm run package

# Install in VS Code
code --install-extension lira-lang-0.1.0.vsix
```

### Development Mode

```bash
cd editors/vscode-lira
npm install
npm run compile

# Open VS Code in extension development mode
code --extensionDevelopmentPath=$(pwd)
```

## Language Server

For full IDE features, install the Lira language server:

```bash
# From the lira repository
cargo install --path crates/lira-lsp

# Or use the justfile
just install
```

The extension will automatically connect to `lira-lsp` if it's in your PATH.

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `lira.languageServer.enable` | `true` | Enable/disable the language server |
| `lira.languageServer.path` | `"lira-lsp"` | Path to the lira-lsp binary |

## Highlighted Elements

- **Keywords**: `fn`, `let`, `var`, `const`, `struct`, `class`, `enum`, `trait`, `impl`, etc.
- **Control Flow**: `if`, `else`, `match`, `while`, `for`, `loop`, `break`, `continue`, `return`
- **Concurrency**: `spawn`, `select`, `async`
- **Types**: `int`, `float`, `bool`, `string`, `List`, `Map`, `Option`, `Result`, etc.
- **Literals**: Numbers, strings, characters, booleans
- **Comments**: Line (`//`) and block (`/* */`) comments
- **String Interpolation**: `${expression}`

## License

MIT

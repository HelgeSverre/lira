# Lira for Zed

Syntax highlighting and language support for the Lira programming language in [Zed](https://zed.dev).

## Features

- Syntax highlighting via tree-sitter grammar
- Code folding
- Auto-indentation
- Bracket matching and auto-closing
- String interpolation support

## Installation

### From Zed Extensions (Recommended)

1. Open Zed
2. Press `Cmd+Shift+P` (macOS) or `Ctrl+Shift+P` (Linux)
3. Type "zed: extensions"
4. Search for "Lira"
5. Click Install

### Manual Installation (Development)

```bash
# Clone the lira repository
git clone https://github.com/lira-lang/lira.git
cd lira

# Install as dev extension
just zed-install
```

## Language Server

The extension automatically configures the Lira language server for full IDE features. Just install `lira-lsp` and it will be detected automatically.

### Install lira-lsp

```bash
# From the lira repository
cargo install --path crates/lira-lsp

# Or build and install with just
just install

# Verify installation
lira-lsp --version
```

The extension will automatically use `lira-lsp` if it's in your PATH.

### Custom Path (Optional)

If lira-lsp is not in your PATH, add to your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "lsp": {
    "lira-lsp": {
      "binary": {
        "path": "/path/to/lira-lsp"
      }
    }
  }
}
```

## LSP Features

When the language server is configured, you get:

- Real-time error diagnostics
- Code completion (keywords, builtins, user symbols)
- Hover information (types, documentation)
- Go to definition
- Find all references
- Document outline/symbols
- Signature help (parameter hints)
- Inlay hints (inline type annotations)
- Rename symbol
- Code actions (quick fixes)
- Call hierarchy
- Folding ranges
- Document links (clickable imports)

## Highlighted Elements

- **Keywords**: `fn`, `let`, `var`, `const`, `struct`, `class`, `enum`, `trait`, `impl`, etc.
- **Control Flow**: `if`, `else`, `match`, `while`, `for`, `loop`, `break`, `continue`, `return`
- **Concurrency**: `spawn`, `select`, `async`
- **Types**: `int`, `float`, `bool`, `string`, `List`, `Map`, `Option`, `Result`, etc.
- **Literals**: Numbers, strings, characters, booleans
- **Comments**: Line (`//`) and block (`/* */`) comments
- **String Interpolation**: `${expression}`

## Troubleshooting

### LSP not working

1. Ensure `lira-lsp` is installed and in your PATH:
   ```bash
   which lira-lsp
   lira-lsp --version
   ```

2. Check Zed logs for errors:
   - Press `Cmd+Shift+P` / `Ctrl+Shift+P`
   - Type "zed: open log"

3. Verify your settings.json is valid JSON

### Syntax highlighting not working

1. Ensure the file has `.li` or `.lira` extension
2. Try restarting Zed
3. Check if the extension is installed: Extensions > Installed

## Development

### Building the Grammar

```bash
cd editors/tree-sitter-lira
npm install
npx tree-sitter generate
npx tree-sitter test
```

### Testing Changes

```bash
# Install as dev extension
just zed-install

# Restart Zed to load changes
```

## License

MIT

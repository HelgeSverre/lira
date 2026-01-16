# lira-lsp

Language Server Protocol implementation for Lira.

## Features

| Feature | Description |
|---------|-------------|
| **Diagnostics** | Real-time syntax and type error reporting |
| **Completion** | Keywords, builtins, types, snippets, and user-defined symbols |
| **Hover** | Type information and documentation on hover |
| **Go to Definition** | Navigate to symbol definitions |
| **Go to Type Definition** | Jump to the type definition of a variable |
| **Find References** | Find all references to a symbol |
| **Document Symbols** | Outline view with functions, structs, enums, etc. |
| **Workspace Symbols** | Search symbols across all open files |
| **Semantic Tokens** | Enhanced syntax highlighting |
| **Signature Help** | Parameter hints while typing function calls |
| **Folding Ranges** | Code folding for functions, blocks, and imports |
| **Document Links** | Clickable import paths to navigate to files |
| **Document Highlight** | Highlight all occurrences of symbol under cursor |
| **Selection Range** | Smart expand/shrink selection |
| **Inlay Hints** | Inline type annotations for inferred types |
| **Rename Symbol** | Rename variables and functions across the document |
| **Call Hierarchy** | Show callers and callees of functions |
| **Code Actions** | Quick fixes and refactoring (let↔var, add docs, generate impl, organize imports) |

## Usage

```bash
# Start the LSP server (stdio)
lira-lsp

# Or run directly with cargo
cargo run -p lira-lsp
```

## Editor Configuration

### VS Code

Install the [vscode-lira](../../editors/vscode-lira) extension.

### Neovim

```lua
-- lua/lspconfig.lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

configs.lira = {
  default_config = {
    cmd = { 'lira-lsp' },
    filetypes = { 'lira' },
    root_dir = lspconfig.util.root_pattern('*.li', '.git'),
  },
}

lspconfig.lira.setup{}
```

### Helix

```toml
# ~/.config/helix/languages.toml
[[language]]
name = "lira"
language-servers = ["lira-lsp"]

[language-server.lira-lsp]
command = "lira-lsp"
```

### Zed

Install the [zed-lira](../../editors/zed-lira) extension.

## Architecture

```
lira-lsp/
├── src/
│   ├── lib.rs              # Main LSP server
│   ├── call_hierarchy.rs   # Callers/callees
│   ├── code_actions.rs     # Quick fixes & refactoring
│   ├── completion.rs       # Completion provider
│   ├── definition.rs       # Go to definition
│   ├── diagnostics.rs      # Error reporting
│   ├── document_highlight.rs # Highlight occurrences
│   ├── document_links.rs   # Clickable imports
│   ├── folding.rs          # Code folding
│   ├── hover.rs            # Hover information
│   ├── inlay_hints.rs      # Inline type hints
│   ├── references.rs       # Find references
│   ├── rename.rs           # Rename symbol
│   ├── selection_range.rs  # Smart selection
│   ├── semantic_tokens.rs  # Enhanced highlighting
│   ├── signature_help.rs   # Parameter hints
│   ├── symbols.rs          # Document symbols
│   ├── type_definition.rs  # Go to type definition
│   ├── utils.rs            # Shared utilities
│   └── workspace_symbols.rs # Workspace-wide search
└── tests/
    └── lsp_tests.rs        # Integration tests
```

## Testing

```bash
# Run all tests
cargo test -p lira-lsp

# Run unit tests only
cargo test -p lira-lsp --lib

# Run integration tests only
cargo test -p lira-lsp --test lsp_tests
```

## Dependencies

- [tower-lsp](https://github.com/ebkalderon/tower-lsp) - LSP server framework
- [ropey](https://github.com/cessen/ropey) - Rope data structure for text
- [dashmap](https://github.com/xacrimon/dashmap) - Concurrent HashMap
- [regex](https://github.com/rust-lang/regex) - Pattern matching

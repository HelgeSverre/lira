# Helix Configuration for Lira

Helix editor support for the Lira programming language.

## Installation

### 1. Add Language Configuration

Copy `languages.toml` to your Helix config directory:

```bash
# Create config directory if needed
mkdir -p ~/.config/helix

# Merge or copy the language configuration
cat languages.toml >> ~/.config/helix/languages.toml
```

Or manually merge the contents into your existing `languages.toml`.

### 2. Install Tree-sitter Grammar

Build and install the Lira tree-sitter grammar:

```bash
# From the lira repository root
cd editors/tree-sitter-lira
npm install
npx tree-sitter generate

# Build the grammar for Helix
hx --grammar fetch
hx --grammar build
```

### 3. Install Query Files

Copy the query files to Helix's runtime:

```bash
# Create query directory
mkdir -p ~/.config/helix/runtime/queries/lira

# Copy query files
cp runtime/queries/lira/*.scm ~/.config/helix/runtime/queries/lira/
```

### 4. Install Language Server (Optional)

For full IDE features, install the Lira language server:

```bash
# From the lira repository
cargo install --path crates/lira-lsp
```

## Features

- Syntax highlighting via tree-sitter
- Code folding
- Auto-indentation
- LSP support (diagnostics, completion, hover, go-to-definition)

## Configuration

Example `languages.toml`:

```toml
[[language]]
name = "lira"
scope = "source.lira"
injection-regex = "lira"
file-types = ["li", "lira"]
comment-token = "//"
block-comment-tokens = { start = "/*", end = "*/" }
indent = { tab-width = 4, unit = "    " }
language-servers = ["lira-lsp"]

[language-server.lira-lsp]
command = "lira-lsp"

[[grammar]]
name = "lira"
source = { git = "https://github.com/lira-lang/lira", subpath = "editors/tree-sitter-lira" }
```

## License

MIT

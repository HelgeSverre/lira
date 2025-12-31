# tree-sitter-lira

Tree-sitter grammar for the Lira programming language.

## Status

Implemented. Covers the full Lira language syntax.

## Installation

```bash
npm install
npx tree-sitter generate
```

## Usage

```bash
# Parse a Lira file
npx tree-sitter parse path/to/file.li

# Run tests
npx tree-sitter test
```

## Structure

```
tree-sitter-lira/
├── grammar.js           # Grammar definition
├── src/
│   ├── parser.c         # Generated parser
│   └── ...
├── queries/
│   ├── highlights.scm   # Syntax highlighting
│   ├── folds.scm        # Code folding
│   └── locals.scm       # Scope tracking
├── package.json
└── tree-sitter.json
```

## Features Supported

- All keywords (fn, let, var, struct, class, enum, trait, impl, etc.)
- All operators (arithmetic, comparison, logical, bitwise, assignment)
- Literals (integers, floats, strings with interpolation, chars, booleans)
- Type annotations and generics
- Function declarations with default parameters
- Struct and class declarations with methods
- Enum declarations with variants
- Trait and impl blocks
- Control flow (if, while, for, loop, match)
- Pattern matching with guards
- Error handling (try/catch, ? operator)
- Concurrency (spawn, channels, select)
- Lambda expressions
- Import and module system

## Editor Integration

### Neovim

Add to your tree-sitter configuration:

```lua
require'nvim-treesitter.configs'.setup {
  ensure_installed = { "lira" },
  highlight = { enable = true },
}
```

### VS Code

Use the Lira VS Code extension (coming soon).

## License

MIT

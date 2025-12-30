# tree-sitter-lira

Tree-sitter grammar for the Lira programming language.

## Status

Not yet implemented. See docs/ROADMAP.md Phase 8.

## Structure

```
tree-sitter-lira/
├── grammar.js           # Grammar definition
├── src/
│   └── parser.c         # Generated parser
├── queries/
│   ├── highlights.scm   # Syntax highlighting
│   ├── folds.scm        # Code folding
│   └── locals.scm       # Scope tracking
├── package.json
└── Cargo.toml
```

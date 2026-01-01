# Lira Build System

# Default recipe - list available commands
default:
    @just --list

# ─────────────────────────────────────────────────────────────────────────────
# Development
# ─────────────────────────────────────────────────────────────────────────────

# Build compiler and VM
[group('dev')]
build:
    cargo build --package lirac --package liravm

# Build all including LSP, doc generator, and spec validator
[group('dev')]
build-all:
    cargo build --package lirac --package liravm --package lira-lsp --package lira-doc --package lira-spec

# Build in release mode
[group('dev')]
release:
    cargo build --package lirac --package liravm --release

# Build all in release mode
[group('dev')]
release-all:
    cargo build --package lirac --package liravm --package lira-lsp --package lira-doc --package lira-spec --release

# Run all tests
[group('dev')]
test:
    cargo test --package lirac --package liravm --package lira-core --package lira-spec
    cargo test --package lirac --test integration

# Run all tests with output
[group('dev')]
test-verbose:
    cargo test --package lirac --package liravm --package lira-core -- --nocapture
    cargo test --package lirac --test integration -- --nocapture

# Type check without building
[group('dev')]
check:
    cargo check --package lirac --package liravm --package lira-core --package lira-spec

# Compile and run a Lira file
[group('dev')]
run file:
    cargo build --package lirac --package liravm --release
    ./target/release/lirac compile {{file}} -o /tmp/out.lic
    ./target/release/liravm run /tmp/out.lic

# Run clippy lints
[group('dev')]
clippy:
    cargo clippy --package lirac --package liravm --package lira-core --package lira-spec -- -D warnings

# Format code
[group('dev')]
fmt:
    cargo fmt --all

# Clean build artifacts
[group('dev')]
clean:
    cargo clean

# Run the LSP server (for testing)
[group('dev')]
lsp:
    cargo run --package lira-lsp

# Generate documentation for stdlib
[group('dev')]
doc:
    cargo run --package lira-doc -- generate stdlib/ -o docs/stdlib/

# Generate documentation for a specific file
[group('dev')]
doc-file file:
    cargo run --package lira-doc -- generate {{file}}

# Generate combined mdBook (stdlib + examples)
[group('dev')]
doc-book:
    cargo run --package lira-doc -- book -o docs/book/

# Build mdBook documentation (requires mdbook)
[group('dev')]
doc-build: doc-book
    cd docs/book && mdbook build

# Serve mdBook documentation locally (requires mdbook)
[group('dev')]
doc-serve: doc-book
    cd docs/book && mdbook serve

# Generate stdlib-only mdBook
[group('dev')]
doc-stdlib:
    cargo run --package lira-doc -- generate stdlib/ --mdbook --title "Lira Standard Library" -o docs/stdlib-book/

# ─────────────────────────────────────────────────────────────────────────────
# Specification
# ─────────────────────────────────────────────────────────────────────────────

# Validate implementation against formal specification
[group('spec')]
spec-validate:
    cargo run --package lira-spec -- validate docs/FORMAL_SPECIFICATION.md

# Compare EBNF spec with tree-sitter grammar
[group('spec')]
spec-compare:
    cargo run --package lira-spec -- compare docs/FORMAL_SPECIFICATION.md editors/tree-sitter-lira/grammar.js

# Run specification conformance tests
[group('spec')]
spec-test:
    cargo test --package lira-spec

# Run specification conformance tests with output
[group('spec')]
spec-test-verbose:
    cargo test --package lira-spec -- --nocapture

# Check spec crate compiles
[group('spec')]
spec-check:
    cargo check --package lira-spec

# ─────────────────────────────────────────────────────────────────────────────
# Installation
# ─────────────────────────────────────────────────────────────────────────────

# Build and install binaries to ~/.local/bin
[group('install')]
install: release-all
    mkdir -p ~/.local/bin
    cp target/release/lirac ~/.local/bin/
    cp target/release/liravm ~/.local/bin/
    cp target/release/lira-lsp ~/.local/bin/
    cp target/release/lira-doc ~/.local/bin/
    cp target/release/lira-spec ~/.local/bin/

# Install Vim/Neovim syntax highlighting
[group('install')]
vim-install:
    @echo "Installing vim-lira..."
    mkdir -p ~/.vim/ftdetect ~/.vim/ftplugin ~/.vim/syntax
    cp editors/vim-lira/ftdetect/lira.vim ~/.vim/ftdetect/
    cp editors/vim-lira/ftplugin/lira.vim ~/.vim/ftplugin/
    cp editors/vim-lira/syntax/lira.vim ~/.vim/syntax/
    @echo "Installed to ~/.vim/"
    @echo "For Neovim, symlink or copy to ~/.config/nvim/"

# Install Neovim syntax highlighting
[group('install')]
nvim-install:
    @echo "Installing vim-lira for Neovim..."
    mkdir -p ~/.config/nvim/ftdetect ~/.config/nvim/ftplugin ~/.config/nvim/syntax
    cp editors/vim-lira/ftdetect/lira.vim ~/.config/nvim/ftdetect/
    cp editors/vim-lira/ftplugin/lira.vim ~/.config/nvim/ftplugin/
    cp editors/vim-lira/syntax/lira.vim ~/.config/nvim/syntax/
    @echo "Installed to ~/.config/nvim/"

# Install Zed extension (dev mode)
[group('install')]
zed-install:
    @echo "Installing zed-lira as dev extension..."
    mkdir -p ~/.config/zed/extensions/installed/lira
    cp -r editors/zed-lira/* ~/.config/zed/extensions/installed/lira/
    @echo "Installed to ~/.config/zed/extensions/installed/lira/"
    @echo "Restart Zed to load the extension"

# Install Helix configuration
[group('install')]
helix-install:
    @echo "Installing helix-lira..."
    mkdir -p ~/.config/helix/runtime/queries/lira
    cp editors/helix-lira/runtime/queries/lira/*.scm ~/.config/helix/runtime/queries/lira/
    @echo "Query files installed to ~/.config/helix/runtime/queries/lira/"
    @echo ""
    @echo "Add this to ~/.config/helix/languages.toml:"
    @echo ""
    @cat editors/helix-lira/languages.toml
    @echo ""

# Install VS Code extension
[group('install')]
vscode-install: vscode-build
    @echo "Installing VS Code extension..."
    code --install-extension editors/vscode-lira/lira-lang-0.1.0.vsix
    @echo "Extension installed! Restart VS Code to activate."

# Install all editor extensions
[group('install')]
editors-install: vim-install nvim-install zed-install helix-install
    @echo ""
    @echo "All editor extensions installed!"
    @echo "Note: VS Code and IntelliJ require separate build steps:"
    @echo "  just vscode-install  - Build and install VS Code extension"
    @echo "  just intellij-build  - Build IntelliJ plugin"

# ─────────────────────────────────────────────────────────────────────────────
# Editor Extensions
# ─────────────────────────────────────────────────────────────────────────────

# Build tree-sitter grammar
[group('editors')]
ts-build:
    cd editors/tree-sitter-lira && npm install && npx tree-sitter generate

# Test tree-sitter grammar
[group('editors')]
ts-test:
    cd editors/tree-sitter-lira && npx tree-sitter test

# Parse a file with tree-sitter (for debugging)
[group('editors')]
ts-parse file:
    cd editors/tree-sitter-lira && npx tree-sitter parse {{file}}

# Highlight a file with tree-sitter (for debugging)
[group('editors')]
ts-highlight file:
    cd editors/tree-sitter-lira && npx tree-sitter highlight {{file}}

# Open a test file in Vim to verify highlighting
[group('editors')]
vim-test:
    vim examples/hello.li

# Open a test file in Neovim to verify highlighting
[group('editors')]
nvim-test:
    nvim examples/hello.li

# Open Zed with a test file
[group('editors')]
zed-test:
    zed examples/hello.li

# Open Helix with a test file
[group('editors')]
helix-test:
    hx examples/hello.li

# Build IntelliJ plugin
[group('editors')]
intellij-build:
    @echo "Building IntelliJ plugin..."
    cd editors/intellij-lira && ./gradlew buildPlugin
    @echo "Plugin built: editors/intellij-lira/build/distributions/"

# Open IntelliJ with the plugin (dev mode)
[group('editors')]
intellij-test:
    cd editors/intellij-lira && ./gradlew runIde

# Build VS Code extension
[group('editors')]
vscode-build:
    @echo "Building VS Code extension..."
    cd editors/vscode-lira && npm install && npm run compile && npm run package
    @echo "Extension built: editors/vscode-lira/*.vsix"

# Open VS Code with a test file
[group('editors')]
vscode-test:
    code examples/hello.li

# Open VS Code in extension development mode
[group('editors')]
vscode-dev:
    cd editors/vscode-lira && code --extensionDevelopmentPath=$(pwd) ../../examples/hello.li

# Test all editor installations by opening files
[group('editors')]
editors-test-vim: vim-test

[group('editors')]
editors-test-nvim: nvim-test

[group('editors')]
editors-test-zed: zed-test

[group('editors')]
editors-test-helix: helix-test

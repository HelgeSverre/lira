# Lira Build System

# Default recipe
default: build

# Build compiler and VM
build:
    cargo build --package lirac --package liravm

# Build all including LSP
build-all:
    cargo build --package lirac --package liravm --package lira-lsp

# Build in release mode
release:
    cargo build --package lirac --package liravm --release

# Build all in release mode
release-all:
    cargo build --package lirac --package liravm --package lira-lsp --release

# Run all tests
test:
    cargo test --package lirac --package liravm --package lira-core
    cargo test --package lirac --test integration

# Run all tests with output
test-verbose:
    cargo test --package lirac --package liravm --package lira-core -- --nocapture
    cargo test --package lirac --test integration -- --nocapture

# Type check without building
check:
    cargo check --package lirac --package liravm --package lira-core

# Compile and run a Lira file
run file:
    cargo build --package lirac --package liravm --release
    ./target/release/lirac compile {{file}} -o /tmp/out.lic
    ./target/release/liravm run /tmp/out.lic

# Run clippy lints
clippy:
    cargo clippy --package lirac --package liravm --package lira-core -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Clean build artifacts
clean:
    cargo clean

# Run the LSP server (for testing)
lsp:
    cargo run --package lira-lsp

# Build and install binaries to ~/.local/bin
install: release-all
    mkdir -p ~/.local/bin
    cp target/release/lirac ~/.local/bin/
    cp target/release/liravm ~/.local/bin/
    cp target/release/lira-lsp ~/.local/bin/

# ─────────────────────────────────────────────────────────────────────────────
# Tree-sitter Grammar
# ─────────────────────────────────────────────────────────────────────────────

# Build tree-sitter grammar
ts-build:
    cd editors/tree-sitter-lira && npm install && npx tree-sitter generate

# Test tree-sitter grammar
ts-test:
    cd editors/tree-sitter-lira && npx tree-sitter test

# Parse a file with tree-sitter (for debugging)
ts-parse file:
    cd editors/tree-sitter-lira && npx tree-sitter parse {{file}}

# Highlight a file with tree-sitter (for debugging)
ts-highlight file:
    cd editors/tree-sitter-lira && npx tree-sitter highlight {{file}}

# ─────────────────────────────────────────────────────────────────────────────
# Editor Extensions
# ─────────────────────────────────────────────────────────────────────────────

# Install Vim/Neovim syntax highlighting
vim-install:
    @echo "Installing vim-lira..."
    mkdir -p ~/.vim/ftdetect ~/.vim/ftplugin ~/.vim/syntax
    cp editors/vim-lira/ftdetect/lira.vim ~/.vim/ftdetect/
    cp editors/vim-lira/ftplugin/lira.vim ~/.vim/ftplugin/
    cp editors/vim-lira/syntax/lira.vim ~/.vim/syntax/
    @echo "Installed to ~/.vim/"
    @echo "For Neovim, symlink or copy to ~/.config/nvim/"

# Install Neovim syntax highlighting
nvim-install:
    @echo "Installing vim-lira for Neovim..."
    mkdir -p ~/.config/nvim/ftdetect ~/.config/nvim/ftplugin ~/.config/nvim/syntax
    cp editors/vim-lira/ftdetect/lira.vim ~/.config/nvim/ftdetect/
    cp editors/vim-lira/ftplugin/lira.vim ~/.config/nvim/ftplugin/
    cp editors/vim-lira/syntax/lira.vim ~/.config/nvim/syntax/
    @echo "Installed to ~/.config/nvim/"

# Open a test file in Vim to verify highlighting
vim-test:
    vim examples/hello.li

# Open a test file in Neovim to verify highlighting
nvim-test:
    nvim examples/hello.li

# Install Zed extension (dev mode)
zed-install:
    @echo "Installing zed-lira as dev extension..."
    mkdir -p ~/.config/zed/extensions/installed/lira
    cp -r editors/zed-lira/* ~/.config/zed/extensions/installed/lira/
    @echo "Installed to ~/.config/zed/extensions/installed/lira/"
    @echo "Restart Zed to load the extension"

# Open Zed with a test file
zed-test:
    zed examples/hello.li

# Install Helix configuration
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

# Open Helix with a test file
helix-test:
    hx examples/hello.li

# Build IntelliJ plugin
intellij-build:
    @echo "Building IntelliJ plugin..."
    cd editors/intellij-lira && ./gradlew buildPlugin
    @echo "Plugin built: editors/intellij-lira/build/distributions/"

# Open IntelliJ with the plugin (dev mode)
intellij-test:
    cd editors/intellij-lira && ./gradlew runIde

# Build VS Code extension
vscode-build:
    @echo "Building VS Code extension..."
    cd editors/vscode-lira && npm install && npm run compile && npm run package
    @echo "Extension built: editors/vscode-lira/*.vsix"

# Install VS Code extension
vscode-install: vscode-build
    @echo "Installing VS Code extension..."
    code --install-extension editors/vscode-lira/lira-lang-0.1.0.vsix
    @echo "Extension installed! Restart VS Code to activate."

# Open VS Code with a test file
vscode-test:
    code examples/hello.li

# Open VS Code in extension development mode
vscode-dev:
    cd editors/vscode-lira && code --extensionDevelopmentPath=$(pwd) ../../examples/hello.li

# Install all editor extensions
editors-install: vim-install nvim-install zed-install helix-install
    @echo ""
    @echo "All editor extensions installed!"
    @echo "Note: VS Code and IntelliJ require separate build steps:"
    @echo "  just vscode-install  - Build and install VS Code extension"
    @echo "  just intellij-build  - Build IntelliJ plugin"

# Test all editor installations by opening files
editors-test-vim: vim-test
editors-test-nvim: nvim-test
editors-test-zed: zed-test
editors-test-helix: helix-test

# Lira Build System

# Default recipe - list available commands
default:
    @just --list

# ─────────────────────────────────────────────────────────────────────────────
# Build - Core compilation targets
# ─────────────────────────────────────────────────────────────────────────────

# Build compiler, VM, and unified CLI
[group('build')]
build:
    cargo build --package lira --package lirac --package liravm

# Build all packages (CLI, compiler, VM, LSP, doc generator, spec, playground)
[group('build')]
build-all:
    cargo build --package lira --package lirac --package liravm --package lira-lsp --package lira-doc --package lira-spec --package lira-playground

# Build in release mode
[group('build')]
release:
    cargo build --package lira --package lirac --package liravm --release

# Build all in release mode
[group('build')]
release-all:
    cargo build --package lira --package lirac --package liravm --package lira-lsp --package lira-doc --package lira-spec --package lira-playground --release

# Clean build artifacts
[group('build')]
clean:
    cargo clean

# Run the LSP server
[group('build')]
lsp:
    cargo run --package lira-lsp

# ─────────────────────────────────────────────────────────────────────────────
# Dev - Development workflow commands
# ─────────────────────────────────────────────────────────────────────────────

# Compile and run a Lira file
[group('dev')]
run file:
    cargo build --package lira --release
    ./target/release/lira run {{ file }}

# Run all tests
[group('dev')]
test:
    cargo test --package lirac --package liravm --package lira-core --package lira-spec --package lira-playground
    cargo test --package lirac --test integration

# Run all tests with output
[group('dev')]
test-verbose:
    cargo test --package lirac --package liravm --package lira-core --package lira-spec --package lira-playground -- --nocapture
    cargo test --package lirac --test integration -- --nocapture

# Type check without building
[group('dev')]
check:
    cargo check --workspace

# Run clippy lints
[group('dev')]
clippy:
    cargo clippy --workspace -- -D warnings

# Format code
[group('dev')]
fmt:
    cargo fmt --all

# Check formatting without modifying
[group('dev')]
fmt-check:
    cargo fmt --all -- --check

# Run all checks (fmt, clippy, test)
[group('dev')]
ci: fmt-check clippy test

# ─────────────────────────────────────────────────────────────────────────────
# Docs - Documentation generation
# ─────────────────────────────────────────────────────────────────────────────

# Generate documentation for stdlib
[group('docs')]
doc:
    cargo run --package lira-doc -- generate stdlib/ -o docs/stdlib/

# Generate documentation for a specific file
[group('docs')]
doc-file file:
    cargo run --package lira-doc -- generate {{ file }}

# Generate combined mdBook (stdlib + examples)
[group('docs')]
doc-book:
    cargo run --package lira-doc -- book -o docs/book/

# Build mdBook documentation (requires mdbook)
[group('docs')]
doc-build: doc-book
    cd docs/book && mdbook build

# Serve mdBook documentation locally (requires mdbook)
[group('docs')]
doc-serve: doc-book
    #!/usr/bin/env bash
    set -e
    cd docs/book

    # Find an available port starting from 3000
    PORT=3000
    while lsof -Pi :$PORT -sTCP:LISTEN -t >/dev/null 2>&1; do
        PORT=$((PORT + 1))
        if [ $PORT -gt 3100 ]; then
            echo "Error: Could not find an available port between 3000-3100"
            exit 1
        fi
    done

    echo "Serving documentation at http://localhost:$PORT"
    mdbook serve -p $PORT

# Generate stdlib-only mdBook
[group('docs')]
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
# Playground - Web playground commands
# ─────────────────────────────────────────────────────────────────────────────

# Build playground backend
[group('playground')]
playground-build:
    cargo build --package lira-playground --release

# Run playground backend server
[group('playground')]
playground-server port="3001":
    PORT={{ port }} cargo run --package lira-playground --release

# Run playground unit tests
[group('playground')]
playground-test:
    cargo test --package lira-playground

# Run playground E2E tests (requires frontend deps installed)
[group('playground')]
playground-e2e:
    cd lira-playground/frontend && npx playwright test

# Run playground E2E tests with UI
[group('playground')]
playground-e2e-ui:
    cd lira-playground/frontend && npx playwright test --ui

# Run playground E2E tests in headed mode (visible browser)
[group('playground')]
playground-e2e-headed:
    cd lira-playground/frontend && npx playwright test --headed

# Run playground E2E tests in debug mode
[group('playground')]
playground-e2e-debug:
    cd lira-playground/frontend && npx playwright test --debug

# Show last playground E2E test report
[group('playground')]
playground-e2e-report:
    cd lira-playground/frontend && npx playwright show-report

# Install playground frontend dependencies
[group('playground')]
playground-frontend-install:
    cd lira-playground/frontend && pnpm install

# Build playground frontend
[group('playground')]
playground-frontend-build:
    cd lira-playground/frontend && pnpm build

# Run playground frontend dev server
[group('playground')]
playground-frontend-dev:
    cd lira-playground/frontend && pnpm dev

# Run complete playground (backend + frontend)
[group('playground')]
playground port="3001":
    #!/usr/bin/env bash
    set -e

    echo "Building Lira Playground..."

    # Ensure frontend deps are installed and build
    cd lira-playground/frontend && pnpm install --silent && pnpm build && cd ../..

    # Build backend
    cargo build --package lira-playground --release

    echo ""
    echo "Lira Playground running at http://localhost:{{ port }}"
    echo "Press Ctrl+C to stop"
    echo ""

    # Run backend (serves frontend from dist/)
    PORT={{ port }} ./target/release/lira-playground

# ─────────────────────────────────────────────────────────────────────────────
# Install - Binary and extension installation
# ─────────────────────────────────────────────────────────────────────────────

# Build and install binaries to ~/.local/bin
[group('install')]
install: release-all
    mkdir -p ~/.local/bin
    cp target/release/lira ~/.local/bin/
    cp target/release/lirac ~/.local/bin/
    cp target/release/liravm ~/.local/bin/
    cp target/release/lira-lsp ~/.local/bin/
    cp target/release/lira-doc ~/.local/bin/
    cp target/release/lira-spec ~/.local/bin/
    cp target/release/lira-playground ~/.local/bin/

# Install only the LSP server to cargo bin
[group('install')]
lsp-install:
    cargo install --path crates/lira-lsp --force

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
# Editor - Editor extension testing
# ─────────────────────────────────────────────────────────────────────────────

# Open a test file in Vim to verify highlighting
[group('editor')]
vim-test:
    vim examples/hello.li

# Open a test file in Neovim to verify highlighting
[group('editor')]
nvim-test:
    nvim examples/hello.li

# Open Zed with a test file
[group('editor')]
zed-test:
    zed examples/hello.li

# Open Zed in development mode (rebuilds LSP first)
[group('editor')]
zed-dev: lsp-install
    zed examples/hello.li

# Open Helix with a test file
[group('editor')]
helix-test:
    hx examples/hello.li

# Open Helix in development mode (rebuilds LSP first)
[group('editor')]
helix-dev: lsp-install
    hx examples/hello.li

# Build IntelliJ plugin
[group('editor')]
intellij-build:
    @echo "Building IntelliJ plugin..."
    cd editors/intellij-lira && ./gradlew buildPlugin
    @echo "Plugin built: editors/intellij-lira/build/distributions/"

# Open IntelliJ with the plugin (dev mode)
[group('editor')]
intellij-test:
    cd editors/intellij-lira && ./gradlew runIde

# Build VS Code extension
[group('editor')]
vscode-build:
    @echo "Building VS Code extension..."
    cd editors/vscode-lira && npm install && npm run compile && npm run package
    @echo "Extension built: editors/vscode-lira/*.vsix"

# Open VS Code with a test file
[group('editor')]
vscode-test:
    code examples/hello.li

# Open VS Code in extension development mode (rebuilds LSP first)
[group('editor')]
vscode-dev: lsp-install
    cd editors/vscode-lira && code --extensionDevelopmentPath=$(pwd) ../../examples/

# Open VS Code in extension development mode (skip LSP rebuild)
[group('editor')]
vscode-dev-quick:
    cd editors/vscode-lira && code --extensionDevelopmentPath=$(pwd) ../../examples/

# ─────────────────────────────────────────────────────────────────────────────
# Tree-sitter - Grammar development
# ─────────────────────────────────────────────────────────────────────────────

# Build tree-sitter grammar
[group('treesitter')]
ts-build:
    cd editors/tree-sitter-lira && npm install && npx tree-sitter generate

# Test tree-sitter grammar
[group('treesitter')]
ts-test:
    cd editors/tree-sitter-lira && npx tree-sitter test

# Parse a file with tree-sitter (for debugging)
[group('treesitter')]
ts-parse file:
    cd editors/tree-sitter-lira && npx tree-sitter parse {{ file }}

# Highlight a file with tree-sitter (for debugging)
[group('treesitter')]
ts-highlight file:
    cd editors/tree-sitter-lira && npx tree-sitter highlight {{ file }}

# ─────────────────────────────────────────────────────────────────────────────
# Website - Static site development
# ─────────────────────────────────────────────────────────────────────────────

# Serve website locally
[group('website')]
website-serve port="3000":
    cd website && bunx serve -l {{ port }}

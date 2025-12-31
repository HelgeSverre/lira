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

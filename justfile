# Lira Build System

# Default recipe
default: build

# Build compiler and VM
build:
    cargo build --package lirac --package liravm

# Build in release mode
release:
    cargo build --package lirac --package liravm --release

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

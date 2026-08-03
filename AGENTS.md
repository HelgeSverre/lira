# AGENTS.md — Lira

This file is a single source of truth for AI coding agents working on the
Lira repository. Lira is a systems programming language with Go-like fiber
concurrency, pattern matching, and strong typing. It compiles to bytecode and
runs on a custom virtual machine.

## Tangible Progress, Anti-Ceremony, and Honest Credit

The purpose of this project is a working language toolchain whose compiler
type-checks, lowers, and produces correct bytecode and whose VM executes it
deterministically. Deliver changes as usable vertical slices through lexing,
parsing, semantic analysis, code generation, the runtime, and the CLI. Process
exists to serve that outcome; it is not the product.

- **No process porn.** Plans, matrices, status documents, compatibility
  inventories, and progress reports count only when they gate a named behavior,
  specification clause, platform decision, or release. Do not substitute
  administration for product behavior.
- **Feature-first ratio.** The overwhelming majority of open tasks must deliver
  behavior a user or consuming tool can exercise: language constructs that
  parse, type-check, compile, and run correctly; compiler diagnostics; VM
  instructions; runtime functions; or CLI subcommands. Documentation and
  repository tasks must name the behavior, invariant, or release gate they
  unblock.
- **Honesty is absolute.** Never fake a test, weaken a type-safety rule or
  validation to make a case pass, regenerate expected bytecode or diagnostic
  output around a regression, hand-craft bytecode that the compiler does not
  produce, or claim stronger evidence than was run. Report exactly which layer
  was verified: parser, checker, codegen, VM execution, CLI integration,
  platform, or end-to-end example.
- **Refusal is not delivery.** Structured compiler errors for malformed source
  are required safety behavior, but they do not constitute implementation of
  the rejected feature. Each language feature and stdlib function needs a
  positive case that constructs real source, compiles it through the normal
  pipeline, and executes to its specified result.
- **Vertical slices ship together.** A language feature is not complete when
  only its AST node, parser rule, type-checking logic, codegen lowering, VM
  instruction, test, or documentation exists. Implement and verify every
  applicable layer in the same change.

These rules bind humans and agents working in this repository and shape the
acceptance criteria and review of every implementation task.

## Capability Claims

Use precise language in code, documentation, issues, and handoffs:

- **Parsed** means the lexer tokenizes and the parser produces the correct AST
  node. An accepted parse with the wrong AST shape is a bug.
- **Checked** means the type checker accepts the construct and infers or
  assigns the specified type. `Any` or an overly broad type is not a correct
  type check.
- **Compiled** means the codegen phase produces correct bytecode for the
  construct. Hand-crafted bytecode or a codegen path that only handles the
  trivial case is not compilation support.
- **Executed** means the VM runs the compiled bytecode and produces the
  specified output, value, or side effect. A clean exit code with no output
  verification is insufficient.
- **Round-tripped** means source compiles to bytecode, the bytecode can be
  loaded and disassembled, and re-execution produces identical semantics.
- **Diagnosable** means the compiler emits a specific, correct error message
  with the correct source span. A generic or misleading diagnostic is not
  support.
- **Complete** means positive and negative behavior, compiler diagnostics for
  misuse, error handling in the runtime, bounded resource usage, and the
  relevant test layers are all handled.

Do not hide limitations behind "supports," "safe," "fallback," or "mostly."
State what is accepted, rejected, lowered, optimized, or deferred. In
particular:

- A parser that accepts syntax but builds the wrong AST is a correctness bug,
  not a feature.
- A type checker that accepts `Any` for an unknown type is an implementation
  gap, not type safety.
- A codegen path that emits bytecode for only the trivial case and panics on
  edge cases is not compilation support.
- A VM instruction that is implemented but not reachable from compiled source
  code is scaffolding, not a feature.
- A feature that works only when the source is presented in a specific order,
  spacing, or naming convention has a robustness gap.

## Named Reward-Hacking Patterns (Forbidden)

1. **Gate self-weakening** — changing type-checking rules, parser acceptance,
   VM semantics, expected test output, or diagnostic expectations so a failure
   passes without fixing the underlying behavior. `docs/FORMAL_SPECIFICATION.md`
   is normative. If the contract is wrong, change it deliberately and document
   the reason; otherwise fix the implementation and add a regression test.

2. **Proof-class inflation** — presenting hand-built ASTs, parser-only
   fixtures, manually assembled bytecode, string-presence assertions, or
   isolated unit tests as proof the full pipeline works. Tests at each layer
   cover only that layer. End-to-end claims require real `.li` source through
   compile and execute. Performance claims require release-build benchmarks.

3. **Golden regeneration reflex** — regenerating expected bytecode, diagnostic
   snapshots, or `@expect` directives to match broken output instead of fixing
   the compiler or VM. Every golden change must be explained as an intentional
   semantic change.

4. **Compile-stream pumping** — adding AST variants, module stubs,
   `todo!()`/`unimplemented!()`, or codegen scaffolding and counting `cargo
   check` as delivery. Compilation is the floor, not the feature. Reachable
   milestone code must implement its positive path and ship with meaningful
   tests.

5. **Tautological tests** — duplicating the implementation's own logic in a
   test, asserting only that output exists or parses, snapshotting unstable
   output without semantic assertions, or omitting negative and edge cases.
   Each feature test must trace to a specification rule and include at least
   one case a plausible but incorrect implementation would fail.

6. **Easy-task cherry-picking** — adding stdlib functions, parser grammar
   rules, editor support, or long-tail edge-case handling while core
   correctness, diagnostic quality, error recovery, or VM performance remains
   incomplete.

7. **Premature completion** — declaring a language feature, stdlib module, VM
   instruction, or milestone complete because its type compiles, its happy-path
   fixture passes, or its error type exists. Completion requires the applicable
   positive and negative behavior, compiler diagnostics, and test evidence at
   every relevant layer.

8. **Scope-splitting** — claiming separate completion credit for the AST node,
   parser rule, checker logic, codegen lowering, tests, and documentation of
   one behavior. They are one vertical slice.

9. **Spec-editing as progress** — weakening or endlessly refining
   `docs/FORMAL_SPECIFICATION.md` instead of implementing it. Specification
   edits may clarify or deliberately change the contract, but do not satisfy an
   implementation milestone.

10. **Conformance metastasis** — adding speculative compliance matrices,
    benchmarks, reports, or abstractions without a named product invariant,
    observed defect class, compatibility boundary, or release target.

11. **Dependency smuggling** — using a crate, feature flag, helper process, or
    generated file to bypass compiler correctness, VM safety boundaries, or
    sandbox invariants. New dependencies must preserve bounded, deterministic
    operation.

12. **Demo-path hardcoding** — special-casing example file names, fixture
    paths, import ordering, variable names, or the developer's current platform
    so showcased cases pass. Detection, compilation, and execution must derive
    behavior from the source, not from the filename or path.

13. **Refusal farming** — accumulating descriptive compiler errors for
    unsupported constructs while avoiding the positive implementation paths.
    Error messages refine an existing feature; they do not deliver it.

14. **Bytecode fudging** — hand-assembling or patching `.lic` output so VM
    tests pass when the compiler cannot produce that bytecode from real source.
    Bytecode in tests must be reproducible through `lirac compile`.

## Project-Specific Completion Gates

Apply only the gates relevant to the change, but do not omit a relevant layer:

- A new language construct is parsed, checked, compiled, and executed. Each
  layer has at least one positive test and one negative (misuse/boundary) test.
- Type checker changes include inference tests for nested, generic, and
  edge-case usage. A single top-level declaration test is insufficient.
- Codegen changes produce bytecode that survives a round trip: compile →
  disassemble → load → execute, producing the expected output.
- VM instruction changes include register/stack interaction tests, failure-mode
  tests (invalid operands, overflow), and a compiled-source integration test.
- New runtime functions handle valid input, invalid input, and resource
  exhaustion where applicable.
- Compiler diagnostics include the correct error code, expected message
  pattern, and correct source span. Test both the exact error and that
  neighboring valid constructs are not spuriously rejected.
- Import/module resolution changes test relative imports, `std.*` imports,
  circular imports, missing imports, and nested import chains.
- Fiber/concurrency changes test single-fiber correctness, multi-fiber
  interleaving, channel communication, `select` semantics, and fiber
  cancellation.
- Example files in `examples/` or `tests/samples/` added for a new feature
  include `@expect:` or `@expect-contains:` directives that verify the
  observable output.

The normal Rust floor is:

```text
just fmt
just clippy
just test
```

If an unrelated failure prevents the full gate, run the largest relevant
focused suites, report the exact failing test and error, and do not describe
the workspace as green.

Before completing a substantial implementation, review the diff for
correctness, security, regressions, performance, maintainability, and adequate
tests. Fix findings before handoff when they are in scope.

## Documentation Lookup

Use Context7 whenever a task asks about a library, framework, SDK, API, CLI, or
cloud service. Start with `resolve-library-id`, choose the best exact and
version-relevant match, then call `query-docs` with the full focused question.
Use separate queries for separate concepts. Do not use Context7 for ordinary
refactoring, business-logic debugging, code review, or general programming.

## Pull Request Branch Strategy

- Prefer independent pull requests based on the intended upstream base branch.
  Each branch contains only that PR's commits.
- A branch targeting `main` is still stacked if its history contains another
  unmerged PR.
- Before publishing multiple PRs, determine whether each change can be rebased
  or cherry-picked onto the common base and pass independently.
- Use a stacked PR only for a real code or semantic dependency, and document
  the dependency and merge/rebase order.
- If independent versus stacked is ambiguous, ask before creating or pushing.

---

## Project overview

## Project overview

Lira is implemented as a Rust Cargo workspace. The language source files use the
`.li` extension and compiled bytecode uses `.lic`. The repository contains:

- A compiler (`lirac`) that transforms `.li` source into `.lic` bytecode.
- A virtual machine (`liravm`) that executes `.lic` files.
- A unified CLI (`lira`) wrapping both compiler and VM.
- A Language Server Protocol implementation (`lira-lsp`).
- A documentation generator (`lira-doc`).
- A formal-spec conformance validator (`lira-spec`).
- A web playground (`lira-playground`) with a Rust/Axum backend and a
  React/TypeScript/Vite frontend.
- Editor support for VS Code, Zed, Neovim/Vim, Helix, and IntelliJ IDEA.
- A standard library under `stdlib/` and example programs under `examples/`.

The project is MIT licensed and targets Rust 1.70+.

## Technology stack

- **Core language tooling**: Rust (edition 2021), Cargo workspace.
- **Build runner**: [`just`](https://github.com/casey/just) — see `justfile`.
- **Frontend technologies** (playground + website):
  - Playground: React 19, TypeScript 5.9, Vite 8, Monaco editor, Zustand,
    Playwright for E2E tests, pnpm.
  - Website: Astro 7, TypeScript, Shiki syntax highlighting, npm.
- **Editor tooling**:
  - VS Code extension: TypeScript/Node.
  - Tree-sitter grammar: JavaScript grammar file.
  - IntelliJ plugin: Gradle-based IntelliJ Platform SDK plugin.
- **Runtime dependencies of note**:
  - `gc` crate (with `derive`, `unstable-stats`, `unstable-config` features) for
    garbage collection in the VM.
  - `ureq`, `regex`, `serde_json`, `uuid`, `chrono`, `sha1`, `sha2`, `md-5`,
    `base64`, `hex`, `dirs` for stdlib/runtime functionality.
  - `tower-lsp`, `tokio`, `ropey`, `dashmap` for the LSP server.
  - `axum`, `tokio`, `tower-http` for the playground backend.

## Repository layout

```
lira/
├── Cargo.toml              # Workspace manifest
├── justfile                # Primary build/task runner recipes
├── mutants.toml            # cargo-mutants configuration
├── crates/
│   ├── lira/               # Unified `lira` CLI binary
│   ├── lirac/              # Compiler library + `lirac` binary
│   ├── liravm/             # VM library + `liravm` binary
│   ├── lira-core/          # Shared opcodes and bytecode types
│   ├── lira-lsp/           # Language server
│   ├── lira-doc/           # Documentation generator
│   └── lira-spec/          # Spec validation and conformance tests
├── lira-playground/
│   ├── backend/            # Rust/Axum server (workspace member)
│   └── frontend/           # React/TypeScript/Vite UI
├── editors/
│   ├── tree-sitter-lira/   # Tree-sitter grammar
│   ├── vscode-lira/        # VS Code extension
│   ├── vim-lira/           # Vim/Neovim syntax + LSP config
│   ├── zed-lira/           # Zed extension
│   ├── helix-lira/         # Helix queries/config
│   └── intellij-lira/      # IntelliJ Platform plugin
├── stdlib/                 # Standard library `.li` modules
├── examples/               # Example programs (also integration tests)
├── tests/samples/          # Additional sample/test programs
├── docs/                   # Language specifications and mdBook source
│   ├── book/               # mdBook output (generated, partly gitignored)
│   ├── FORMAL_SPECIFICATION.md
│   ├── TESTING.md
│   └── *.md
└── website/                # Astro-based public website
```

## Build commands

Use `just` for day-to-day tasks. Run `just` with no arguments to list recipes.

### Core builds

```bash
just build        # Build lira, lirac, liravm (debug)
just release      # Build lira, lirac, liravm (release)
just build-all    # Build all crates including LSP, doc, spec, playground
just clean        # cargo clean
```

### Development workflow

```bash
just run <file.li>       # Compile and run a Lira source file
just test                # Run unit tests + integration tests
just test-verbose        # Run tests with --nocapture
just check               # cargo check --workspace
just clippy              # Run clippy with -D warnings
just fmt                 # Format Rust code
just fmt-check           # Check formatting without modifying
just ci                  # fmt-check + clippy + test
```

### Manual equivalents (without just)

```bash
cargo build --package lira --package lirac --package liravm
cargo nextest run --package lirac --package liravm --package lira-core \
                  --package lira-spec --package lira-playground

# Run a single example-based integration test
cargo nextest run --package lirac --test integration -- test_hello

# Compile and run a file manually
cargo run --package lirac -- compile examples/hello.li -o /tmp/hello.lic
cargo run --package liravm -- run /tmp/hello.lic
```

### Documentation

```bash
just doc                 # Generate Markdown docs for stdlib/
just doc-book            # Generate combined mdBook source
just doc-build           # Build mdBook (requires mdbook installed)
just doc-serve           # Serve docs locally on an available port
```

### Specification

```bash
just spec-test           # Run spec conformance tests
just spec-validate       # Validate implementation against formal spec
just spec-compare        # Compare EBNF spec with tree-sitter grammar
```

### Playground

```bash
just playground-build              # Build backend release binary
just playground-server [port]      # Run backend (default port 3001)
just playground-frontend-install   # pnpm install in frontend/
just playground-frontend-build     # pnpm build
just playground-frontend-dev       # pnpm dev
just playground-e2e                # Run Playwright E2E tests
just playground [port]             # Full stack (frontend build + backend)
```

## Code organization

### `lira-core` — shared types

- `opcode.rs` — all VM bytecode instructions (`Opcode`).
- `bytecode.rs` — bytecode header, constant pool, line-info and debug symbols.
- `lib.rs` — re-exports plus `BYTECODE_MAGIC` / `BYTECODE_VERSION`.

This crate has no external dependencies and is depended on by `lirac`,
`liravm`, `lira`, `lira-lsp`, `lira-doc`, `lira-spec`, and
`lira-playground`.

### `lirac` — compiler

Public modules under `crates/lirac/src/`:

- `lexer.rs` — tokenization.
- `parser.rs` — AST construction.
- `ast.rs` — AST node definitions.
- `checker.rs` — type checking and inference.
- `sema.rs` — semantic tables (types, symbols, type members).
- `codegen.rs` — bytecode generation.
- `module_loader.rs` — import resolution and multi-file compilation.
- `errors.rs` — diagnostic/error types.
- `ids.rs` — identifier/type-id utilities.
- `lib.rs` / `main.rs` — library and CLI entry points.

Key library APIs (from `lirac::`):

- `compile(source)` — compile source string to bytecode.
- `compile_with_imports(source_file, source)` — compile with import resolution.
- `check(source)` / `check_with_imports(...)` — type check only.
- `analyze(source)` / `analyze_with_imports(...)` — error-tolerant analysis that
  returns AST + semantic tables + diagnostics, used by the LSP server.

The `serde` feature (enabled by default) adds JSON AST serialization.

### `liravm` — virtual machine

Public modules under `crates/liravm/src/`:

- `vm.rs` — bytecode interpreter and main execution loop.
- `fiber.rs` — green-thread scheduler.
- `runtime.rs` — built-in functions and syscalls.
- `value.rs` — runtime value representation.
- `bytecode.rs` — bytecode loading.
- `memory.rs` — ARC-based heap management.
- `io_pool.rs` — asynchronous I/O pool.
- `debug.rs`, `debug_session.rs`, `vm_snapshot.rs` — debugger API and state
  snapshots used by the playground.

Key library APIs (from `liravm::`):

- `run(bytecode)` — execute and return exit code.
- `run_with_capture(bytecode)` — execute and return `(exit_code, output_lines)`.
- `run_with_capture_structured(bytecode)` — like `run_with_capture` but returns
  a structured `RuntimeError` with line/column and call-stack names.
- `create_vm(bytecode)` — create a `VM` instance for manual control.
- `DebugSession` — full debug session API (breakpoints, stepping, state
  inspection).

### `lira` — unified CLI

`crates/lira/src/main.rs` provides the `lira` binary with subcommands:
`run`, `compile`, `check`, `ast`, `disasm`, `help`, `version`.

### `lira-lsp` — language server

Implements LSP using `tower-lsp` + `tokio`. Modules cover diagnostics,
completion, hover, definition, references, rename, document symbols, semantic
highlighting, signature help, folding, document links, inlay hints, call
hierarchy, code actions, workspace symbols, and document highlights.

Run with `cargo run --package lira-lsp` or `just lsp`. It communicates over
stdio.

### `lira-doc` — documentation generator

Extracts doc comments and declarations from `.li` files and generates Markdown.
Run with `cargo run --package lira-doc -- generate <path>`.

### `lira-spec` — specification validator

Validates the implementation against `docs/FORMAL_SPECIFICATION.md`, compares
EBNF with the tree-sitter grammar, and runs type-system conformance tests.

### `lira-playground/backend`

Rust/Axum server exposing HTTP endpoints (`/api/compile`, `/api/run`,
`/api/check`) and a WebSocket endpoint (`/ws`) for debug sessions. Depends on
`lirac` and `liravm`.

## Testing strategy

### Example-driven integration tests

Files in `examples/` and `tests/samples/` serve as both documentation and test
cases. `crates/lirac/tests/integration.rs` parses directive comments from each
`.li` file:

- `// @expect: <output>` — expect this exact output line.
- `// @expect-contains: <text>` — output must contain the text.
- `// @expect-error` — compilation is expected to fail.
- `// @skip` — skip the test.

Each listed example has its own `#[test]` for clear failure reporting, plus
aggregate tests that run all examples.

### Unit tests

Each crate contains `#[cfg(test)]` unit tests. Run them with `cargo nextest run` or
`just test`. The project uses nextest (not plain `cargo test`) for faster parallel
execution and automatic failure output. Do not add doctests — Lira has no Rust-facing
doc-test mechanism.

### Property-based / fuzz tests

Located in `crates/lirac/tests/`:

- `differential.rs` — generates random well-typed programs and compares Lira
  output against a Rust oracle.
- `robustness.rs` — feeds random token soup through the compiler and asserts it
  never panics.

Run individually:

```bash
cargo nextest run -p lirac --test differential
PROPTEST_CASES=50000 cargo nextest run -p lirac --test differential
cargo nextest run -p lirac --test robustness
```

### Mutation testing

Configured in `mutants.toml`. `cargo-mutants` edits source and re-runs the test
suite; surviving mutants indicate untested logic.

```bash
cargo install cargo-mutants
cargo mutants --file crates/lirac/src/codegen.rs   # fast, scoped
cargo mutants                                      # slow, full run
```

Excluded from mutation: playground, LSP, doc generator, and spec crates.

### Specification conformance

`lira-spec` tests parse `docs/FORMAL_SPECIFICATION.md`, validate grammar rules,
and compare EBNF with the tree-sitter grammar in
`editors/tree-sitter-lira/grammar.js`.

### Playground E2E tests

Frontend Playwright tests live in `lira-playground/frontend/e2e/`.

```bash
cd lira-playground/frontend
pnpm install
npx playwright test
```

## Code style and conventions

- **Formatting**: Rust code is formatted with `rustfmt` (`cargo fmt --all`).
- **Linting**: Clippy is run with `-D warnings` (`just clippy`).
- **Editions**: all Rust crates use edition 2021.
- **Error handling**: the compiler and VM surface errors as `Result<T, String>`
  for legacy entry points; newer APIs return structured diagnostics (see
  `lirac::Diagnostic`, `liravm::RuntimeError`).
- **Comments**: use `//!` module docs and `///` item docs in Rust. English is
  the project language.
- **Tests**: prefer explicit regression tests for bugs; add example files with
  `@expect:` directives for language-feature coverage.
- **Imports**: Lira source uses `import std.module.{symbol}` style. The module
  loader resolves `std.*` imports relative to the `stdlib/` directory and
  relative file imports relative to the importing file.

## Runtime architecture

- Lira source is lexed → parsed → type-checked → lowered to bytecode.
- Bytecode files have a 24-byte header, constant pool, and code section. See
  `docs/10-bytecode-format.md` and `docs/11-instruction-set.md`.
- The VM is a stack machine with local variables, closures, channels, fibers,
  and garbage collection.
- Concurrency is fiber-based (green threads) with channels and `select`.
- The runtime provides built-in functions and syscalls for I/O, networking,
  hashing, JSON, HTTP, regex, UUID, dates, random numbers, etc.
- The playground backend runs the compiler and VM in a sandboxed thread per
  request/debug session.

## Editor and tooling support

Editor integrations live under `editors/`:

- VS Code: `editors/vscode-lira/`
- Zed: `editors/zed-lira/`
- Vim/Neovim: `editors/vim-lira/`
- Helix: `editors/helix-lira/`
- IntelliJ: `editors/intellij-lira/`
- Tree-sitter grammar: `editors/tree-sitter-lira/`

Install helpers are available in the `justfile` (e.g. `just nvim-install`,
`just vscode-install`, `just zed-install`, `just helix-install`,
`just intellij-build`).

## Deployment and CI

- **Website**: hosted on Vercel. The GitHub Actions workflow `.github/workflows/site.yml`
  builds the Lira CLI in release mode, runs `npm run check:examples` to verify
  embedded `.li` snippets compile and run, then builds the Astro site.
- **Playground**: can be run locally or deployed as a container/service. The
  backend serves the frontend `dist/` directory when built via `just playground`.
- **Binaries**: `just install` copies release binaries to `~/.local/bin`.

## Security considerations

- The playground compiles and executes user-supplied Lira code. The backend
  should run with tight resource limits, timeouts, and network restrictions in
  any public deployment.
- Runtime syscalls and stdlib functions perform I/O and networking; do not run
  untrusted `.li` code outside a sandbox.
- `.env`/Vercel local files are gitignored; only `.vercel/project.json` is kept
  tracked.

## Useful references

- `README.md` — high-level project intro and quick start.
- `CLAUDE.md` — shorter agent cheat sheet.
- `justfile` — full list of runnable recipes.
- `docs/TESTING.md` — detailed testing philosophy (differential fuzzing,
  robustness fuzzing, mutation testing).
- `docs/FORMAL_SPECIFICATION.md` — language specification.
- `docs/ROADMAP.md` — current status and planned work.
- `docs/10-bytecode-format.md` — `.lic` file format.
- `docs/11-instruction-set.md` — VM instruction reference.

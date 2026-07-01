# Testing the Lira compiler & VM

Beyond the example-driven integration tests (`// @expect:` directives in
`examples/*.li`) and per-crate unit tests, the language core has two automated
bug-finding layers. Both live in `crates/lirac/tests/` and run as ordinary
`cargo test` — so `just test` / CI already exercise them.

## Why these exist

Two real bugs shipped undetected because the example suite only checks a
*prefix* of output and no example exercised the feature at all:

- **`main()` double-invocation** — `fn main() { ... }` followed by an explicit
  `main()` ran main twice, doubling all output. 28 examples did this; the
  `@expect:` harness never checked output *length*, so it passed.
- **Field/element assignment no-op** — `obj.field = v` and `arr[i] = v`
  compiled the RHS and silently dropped the store. No example ever assigned to
  an indexed or field lvalue, so nothing caught it.

Both are *silent-wrong-output on valid programs* — invisible to crash-fuzzing.
The layers below are designed to catch that class going forward.

## 1. Differential property fuzzer — `tests/differential.rs`

Generates random **well-typed** programs over a small all-`int` subset
(variables, a fixed array, a two-field struct, `+ - *`, plain + compound
assignment, `println`) and asserts Lira's output equals a trivial Rust
**oracle** that evaluates the same program.

- Each program is rendered three ways — top-level, wrapped in `main()` + an
  explicit call, and wrapped relying on auto-invoke — all of which must equal
  the single oracle output. This covers the `main()` double-invoke class.
- Divergence sources are engineered out (no division/modulo, only in-range
  literal indices, small literals + wrapping arithmetic, fully-parenthesized
  rendering) so a mismatch is a **real bug**, not a subset gap.
- On failure, proptest **shrinks** to a minimal counterexample and prints the
  offending Lira source.

```sh
cargo test -p lirac --test differential          # 1500 cases (~3s)
PROPTEST_CASES=50000 cargo test -p lirac --test differential   # deeper hunt
```

## 2. Robustness fuzzer — `tests/robustness.rs`

Feeds random token soup from Lira's vocabulary through lex → parse → check →
codegen and asserts the compiler never **panics** (only `Ok`/`Err`). Compile
only — running arbitrary programs could loop forever; bounded execution is the
differential fuzzer's job.

```sh
cargo test -p lirac --test robustness            # 4000 cases
```

## 3. Mutation testing — cargo-mutants

Measures *test quality*: cargo-mutants edits the compiler/VM source (e.g. `+`→
`-`, or replaces a function body with a no-op) and re-runs the tests. A
**surviving** mutant means no test noticed — that logic is untested, which is
exactly where both hand-found bugs lived.

```sh
cargo install cargo-mutants                      # once
cargo mutants --file crates/lirac/src/codegen.rs # scope to one file (fast)
cargo mutants                                    # full run (slow; nightly/CI)
```

Config in `mutants.toml` (excludes the non-semantic crates, sets timeouts). A
full run is ~14s/mutant × thousands, so prefer `--file` scoping or a nightly
job. Inspect survivors in `mutants.out/outcomes.json`.

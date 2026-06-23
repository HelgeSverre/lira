# Lira — Known Issues & Shallow Implementations

Findings from a deep code review comparing Lira to Sema. Items to address before Lira can be considered robust.

> **Living document.** Some original findings have since been fixed, corrected, or reframed — these are annotated inline with a date (e.g. _CORRECTED (2026-06-23)_, _FIXED_, _DONE_). Notably: the type system is a static-check-then-tagged-dynamic-runtime by design (not a facade), `select` *does* parse, `char_literals.li` was a malformed example (not a lexer bug), and checker/codegen now emit structured errors. Treat un-annotated items as still open; verify against current code before acting.

## 🔴 Critical — Features That Don't Work As Advertised

### Type System: Static Check + Tagged Dynamic Runtime (by design, not a bug)
- **Reframed (2026-06-23):** This is the intended architecture, not a correctness defect. Lira is **statically type-checked, then erased to a tagged-value dynamic runtime.** The VM is a tagged dynamically-typed interpreter: every `Value` carries its own type tag and operations dispatch on those tags at runtime.
- The checker validates types at compile time; codegen now consumes `SemanticTables` (`sema.expr_types`) for resolved expression types where it needs them, but the VM does not require static types to be correct — type errors are caught before bytecode runs.
- `TypedProgram` is still `pub type TypedProgram = Program;` — there is no separate fully-typed AST/IR, because the tagged runtime does not need one for correctness.
- **Not a bug:** typed opcodes / unboxing (e.g. dedicated `IAdd` vs generic `Add`, untagged primitive storage) are a future **performance** option, not a correctness fix. They would let the VM skip tag checks for statically-known-monomorphic code.
- Remaining (optional, perf): introduce typed opcodes and/or a typed IR that carries resolved types into codegen to elide runtime tag dispatch.

### Generics Don't Work
- `checker.rs:65`: _"Currently using type erasure — TypeParam becomes Any at runtime"_
- `codegen.rs:4071`: `type_args: _, // TODO: Handle explicit type args for monomorphization`
- Generic functions compile to untyped code — no monomorphization, no specialization
- `docs/ROADMAP.md` T7.6 (monomorphization) is now corrected to `[ ]` with an honest note: generics work via **runtime type erasure** on the tagged dynamic VM, not monomorphization. Codegen does not consume `sema.generic_instantiations`.
- This is acceptable for correctness (the tagged runtime dispatches on value tags). Monomorphization is a future **performance** task, not a correctness blocker.
- Fix (optional, perf): implement monomorphization / specialization by consuming `sema.generic_instantiations` in codegen.

### Fiber/Channel System Is Dead Code
- A substantial implementation exists (714-line scheduler in `fiber.rs`, 9 VM opcodes, full compiler pipeline support) but **cannot run**
- `vm.rs:164`: `fiber_mode: false` (default) — the setter `set_fiber_mode(true)` exists but **nobody calls it**
- `Yield` is a no-op when `fiber_mode` is false; `ChanSend`/`ChanRecv` always return errors
- Spawn creates fibers in the ready queue but no scheduler loop ever drives them
- `Select` opcode is an explicit no-op (`vm.rs:1381`)
- `async`/`await` are lexer tokens only (`lexer.rs:363-364`) — not parsed, not type-checked, not compiled; no async state machine generation
- `std.sync.*` (Mutex, RwLock, WaitGroup) documented in `docs/04-concurrency.md` but no stdlib modules exist
- Test samples in `tests/samples/` (worker-pool, ping-pong, parallel-sum) are rich but only test syntax parsing
- **Playground integration missing**: `handlers.rs` (`run()`, `step()`, `DebugSession::load()`) never enable fiber mode; WebSocket `VmThreadHandle` has no API to enable it
- **No scheduler loop in VM execution**: `run()` (lines 576-638) and `step_instruction()` execute single fiber sequentially; when fiber ops block, they call `schedule()` but no outer loop keeps running while fibers are runnable
- ~~**`select { ... }` syntax doesn't parse**~~ **CORRECTED (2026-06-23): FALSE.** `select { ... }` *does* parse — see `parser.rs::select_expression` (~lines 1804–1906), producing `ExpressionKind::Select(arms)`. Covered by passing unit tests (`test_select_with_send_variable`, `test_select_with_recv_variable`, `test_select_default_case`, `test_select_mixed_arms`). The real gap is **runtime**: the `Select` opcode is a no-op in the VM (see below), so parsing succeeds but execution does not drive channels.
- **Protocol defines fiber/channel events but never emitted**: `protocol.rs:102-119` defines `FiberSpawned`, `FiberStateChanged`, `ChannelCreated`, `ChannelMessage` — but VM never runs with `fiber_mode=true`
- **Debug session has no fiber/channel inspection**: `get_snapshot()` returns only locals/stack/call_stack — no fiber/channel state
- Fix: Enable `fiber_mode`, wire the scheduler loop into VM execution, implement `Select`

### Memory Management Module Is Dead Code
- `memory.rs` defines ARC + cycle detection GC (`ObjectRef`, `GcStats`, `Object`, `ObjectKind`) but the VM **never imports or uses it**
- Objects are created inline as `Rc<RefCell<HashMap>>` in `vm.rs` `NewObject` handler
- String interning (`intern_string`, `make_string`) defined on VM but `#[allow(dead_code)]` — unused
- Fix: Either integrate the memory module or delete it

## 🟡 Significant — Shallow Implementations

### LSP Is Partly Semantic (diagnostics DONE; hover/completion/references still regex)
- **DONE:** Diagnostics are semantic — driven by the real compiler (`lirac::check_with_imports`) and now surface the **structured** `CheckerError`/`CodegenError` types (`crates/lirac/src/errors.rs`) as structured LSP diagnostics (codes + spans), not ad-hoc strings.
- **Still regex-based (remaining):**
  - Completions: keyword/builtin list + regex scan for user symbols — no type-aware completion
  - Hover: regex pattern matching on source text, not resolved type info
  - References/Rename: text-based search, not scope-aware
  - Code actions: string manipulation (`let` ↔ `var` toggle, etc.)
- Fix (remaining): wire the checker's `TypeEnv`/`SemanticTables` into LSP hover, completion, and references for real scope-aware intelligence

### Tests: mostly green (was reported broken — partly corrected)
- ~~`cargo test` fails on `examples/char_literals.li`~~ **FIXED (2026-06-23).** The failure was a **malformed example**, not a lexer bug — the lexer handles `\n`/`\t`/`\'`/`\\` char escapes fine. The corrected `examples/char_literals.li` compiles, runs, and matches its `@expect` directives. The full example suite is green (85/85 examples; `cargo test --workspace` all pass).
- **Test count corrected:** ~~1,077~~ → **~675** `#[test]` functions across `crates/` (count is approximate and drifts; verify with `grep -rc '#\[test\]' crates/`). The old 1,077 figure was stale.
- No cross-verification between compiler phases (still true — nice-to-have)
- **Concurrency samples not tested**: `tests/samples/` has 5 rich concurrency tests (ping-pong, worker-pool, parallel-sum, producer-consumer, fibers-basic) — none run in integration tests. (Note: these parse, including `select`; they don't *execute* concurrently because `fiber_mode` is never enabled and `Select` is a VM no-op.)
- Fix: add substantially more tests, especially cross-phase and concurrency runtime tests

### Error Handling: compile-time DONE; VM runtime errors still basic
- **DONE (2026-06-23):** The **checker and codegen** now emit structured error types — `CheckerError` / `CodegenError` in `crates/lirac/src/errors.rs` — with error codes and source spans (e.g. the structured `UndefinedVariable` error). These flow through to structured LSP diagnostics.
- **Still remaining (VM runtime errors):**
  - VM errors are still just `String` — e.g. `format!("Cannot add {:?} and {:?}", a, b)` (debug format in user-facing messages)
  - No source spans carried through to runtime errors
  - VM has `get_current_location()` via `DebugInfo` but it's only used for breakpoints, never for errors
  - Playground always passes `location: None` for runtime errors
- Fix (remaining): give the VM a structured runtime-error type that carries spans/locations and user-friendly formatting

## 🟠 Minor — Missing Pieces

### TODOs in Code
- `checker.rs:3241`: Handle named argument reordering
- `checker.rs:4015`: Handle explicit type args for generic methods  
- `parser.rs:805`: Parse supertraits (`trait Ord: Eq { }`)
- `parser.rs:1584`: Parse turbofish syntax (`::<T>`)
- `vm.rs:539`: Get function name from debug info for call frames
- `lira-doc/extractor.rs:380`: Format default parameter expressions

### VM Code Duplication
- `run()` and `step_instruction()` + `execute_one()` duplicate the main loop logic (breakpoint checking, opcode dispatch)
- `execute_opcode()` is ~800 lines in a single function (`vm.rs:641-1429`)

### Syscall Boilerplate
- ~112 syscalls implemented as repetitive pop-match-call-push patterns
- No macro or abstraction to reduce the boilerplate

### Stdlib Is Thin Wrappers
- Most stdlib modules (fs, env, os, net) are thin wrappers around syscalls
- `collections.li` (814 lines) is the only substantial one
- No tests for stdlib modules themselves

### Playground Backend
- Backend exists (1,806 lines) but no WASM compilation — runs VM server-side
- Frontend exists but unclear if it's functional
- **No fiber mode toggle in API/UI**: `RunRequest`, `StepRequest`, `Debug` messages have no `fiber_mode` field; no way to enable from frontend
- **WebSocket VM thread never drives fibers**: `VmThreadHandle` creates `DebugSession` which creates VM with `fiber_mode=false`; no command to enable fiber mode
- **Debug protocol has fiber/channel types but no data**: `VmStateJson` includes `fibers` and `channels` arrays but `DebugSession` never populates them

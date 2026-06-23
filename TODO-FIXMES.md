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

### Generics — bounded operations DONE; monomorphization deferred (perf)
- **DONE (2026-06-23):** the checker is now **bound-aware** — operations on type parameters are allowed when the bound supports them: `fn add<T: Numeric>(a: T, b: T) -> T { return a + b }` type-checks (arithmetic/bitwise under `Numeric`, ordering under `Comparable`/`Ord`); unbounded `fn add<T>` still errors. (`type_param_has_bound` is wired in; was dead code.)
- Generics run via **runtime type erasure** on the tagged dynamic VM (TypeParam → tagged Any), which is correct. The dead monomorphization infra (`mangled_name`, `sema.generic_instantiations`) is relabeled UNUSED and `docs/ROADMAP.md` T7.6 corrected to `[ ]`.
- Remaining (optional, perf): full monomorphization / specialization by consuming `sema.generic_instantiations` in codegen — a performance task, not a correctness blocker. (Turbofish `::<T>` explicit type args still unparsed — see TODOs.)

### Fiber/Channel System — DONE (concurrency now executes)
- **DONE (2026-06-23):** Concurrency is **live**. The VM drives the fiber scheduler; fibers run cooperatively and share the object heap (`Rc<RefCell<...>>`), so a struct field mutated between yield/block points is safe across fibers.
- **DONE — working primitives:**
  - `spawn func(args)` — creates a fiber with args bound as locals; the scheduler runs it.
  - `chan(n)` (buffered) / `chan()` (unbuffered) — channel construction.
  - `send(ch, v)` — real blocking send; `recv(ch)` — real blocking recv; `close(ch)`.
  - `select { v = <-ch => ...,  val -> ch => ...,  _ => ... }` — blocking + polling, real send/recv arms, default arm. The `Select` opcode is implemented (no longer a VM no-op).
  - `fiber_yield()` / `fiber_id()` builtins.
  - **Deadlock detection** when all fibers are blocked.
- **DONE — `std.sync` core** (`stdlib/sync.li`, `import std.sync`): `IntMutex` / `StringMutex` (capacity-1 channel; `lock`/`unlock`/`with`), `WaitGroup` (`done`/`wait(n)`), `Semaphore` (`acquire`/`release`). VM-honest, zero compiler changes. Proven by `examples/sync_mutex_waitgroup.li`, `examples/sync_semaphore.li`, `examples/sync_with_closure.li`, wired into the integration harness. Docs reconciled in `docs/04-concurrency.md` §5.
- **DONE — concurrency now tested at runtime:** the `test_sync_*` integration tests execute real fibers/channels (not just parse). The `tests/samples/` programs (worker-pool, ping-pong, parallel-sum) parse, and the live model is covered by the executing example tests.
- ~~`select { ... }` syntax doesn't parse~~ — was already corrected; **now also executes** at runtime.
- **Remaining gaps (honest):**
  - **RAII-guard Mutex** (auto-unlock on scope exit), **RwLock**, **Condvar** — *not implementable*: Lira has no Drop/RAII. `std.sync` uses explicit `lock`/`unlock` + the `with()` closure idiom instead (documented deviation).
  - **`async`/`await`** — still lexer tokens only (`lexer.rs:363-364`); not parsed, type-checked, or compiled, and **not the recommended model**. Fibers + channels + `select` are the concurrency model. Documented as not-implemented in `docs/04-concurrency.md` §6.
  - **Atomics** (`AtomicInt`, memory orderings) — *meaningless* on a cooperative single-threaded shared-heap VM; intentionally omitted. Use `IntMutex` for mutual exclusion across blocking points.
  - **`try_lock()` / generic `Mutex<T>`** — deferred (need an `Option`/sentinel return and a checker `type_params` patch respectively).
  - **Playground/LSP fiber inspection** — VM execution drives fibers, but the playground/debug-protocol fiber & channel inspection (`get_snapshot`, `VmStateJson.fibers`/`channels`) is still not populated; no fiber-mode toggle in the playground API/UI. (See Playground Backend below.)

### Memory Management Module Is Dead Code — DELETED
- **DONE (2026-06-23):** `crates/liravm/src/memory.rs` (orphaned mark-sweep GC + cycle collector, zero consumers, structurally incompatible with the live `Value::Object(Rc<RefCell<HashMap>>)` model) and the unused string-interning (`string_pool`/`intern_string`/`make_string`) were **deleted**. `docs/13-memory-model.md` notes the specified cycle collector is not implemented — the runtime uses host `Rc` ARC. Real cycle reclamation would be a separate redesign.

## 🟡 Significant — Shallow Implementations

### LSP Is Mostly Semantic now (diagnostics + hover + completion DONE; references/rename remain)
- **DONE:** Diagnostics surface structured `CheckerError`/`CodegenError` (`crates/lirac/src/errors.rs`) with spans.
- **DONE (2026-06-23):** **Hover** and **member completion** are type-aware via `SemanticTables`. New error-tolerant `lirac::analyze()` returns AST + `sema` + diagnostics even on errors (editor buffers are usually mid-edit); a cursor→innermost-NodeId walker (`lira-lsp/src/sema_index.rs`) resolves the node under the cursor; hover shows the resolved `Type` (`expr_types`/`field_resolution`/`call_resolution`), and `.`-completion lists a receiver's real fields + methods (`type_members`). Parser hardened with a recursion-depth guard so malformed buffers can't crash the LSP.
- **Still regex/text-based (remaining):**
  - References/Rename: text-based search, not scope-aware
  - Code actions: string manipulation (`let` ↔ `var` toggle, etc.)
- Fix (remaining): wire `SemanticTables`/`symbol_refs` into scope-aware references + rename.

### Tests: mostly green (was reported broken — partly corrected)
- ~~`cargo test` fails on `examples/char_literals.li`~~ **FIXED (2026-06-23).** The failure was a **malformed example**, not a lexer bug — the lexer handles `\n`/`\t`/`\'`/`\\` char escapes fine. The corrected `examples/char_literals.li` compiles, runs, and matches its `@expect` directives. The full example suite is green (85/85 examples; `cargo test --workspace` all pass).
- **Test count corrected:** ~~1,077~~ → **~675** `#[test]` functions across `crates/` (count is approximate and drifts; verify with `grep -rc '#\[test\]' crates/`). The old 1,077 figure was stale.
- No cross-verification between compiler phases (still true — nice-to-have)
- **Concurrency now executes in tests (was: not tested):** the `test_sync_*` integration tests run real fibers/channels/`select` end-to-end (`examples/sync_*.li`). The `tests/samples/` programs (ping-pong, worker-pool, parallel-sum, producer-consumer, fibers-basic) are still not individually wired into the harness, but `fiber_mode` is now enabled and `Select` executes, so the runtime they exercise is live and covered by the example tests.
- Fix: add substantially more tests, especially cross-phase and concurrency runtime tests

### Error Handling — DONE (compile-time and runtime)
- **DONE:** the **checker and codegen** emit structured `CheckerError` / `CodegenError` (`crates/lirac/src/errors.rs`) with spans, flowing through to structured LSP diagnostics.
- **DONE (2026-06-23):** **VM runtime errors** now carry source locations and use readable messages. `Value::type_name()` + `Display for Value` replaced `{:?}` (users see `Cannot add string and int`, not `Cannot add String("x") and Int(3)`). `run()` attaches `get_current_location()` so runtime errors are `line:col: message` (e.g. `4:5: Division by zero`). A structured `RuntimeError { message, line, column }` + `run_with_capture_structured()` lets the playground populate a real location instead of `None`.

## 🟠 Minor — Missing Pieces

### TODOs in Code
- `checker.rs:3241`: Handle named argument reordering
- `checker.rs:4015`: Handle explicit type args for generic methods  
- `parser.rs:805`: Parse supertraits (`trait Ord: Eq { }`)
- `parser.rs:1584`: Parse turbofish syntax (`::<T>`)
- `vm.rs:539`: Get function name from debug info for call frames
- `lira-doc/extractor.rs:380`: Format default parameter expressions

### VM Code Duplication — mostly DONE
- **DONE (2026-06-23):** the duplicated fetch/decode block was extracted into a shared `decode_next()`, and the ~789-line `execute_opcode()` was split into category helpers (`execute_arithmetic`/`comparison`/`memory`/`control_flow`/`type`/`system`/`fiber_channel`).
- Remaining: `run()` vs the stepping path (`step_instruction`/`execute_one`) still have separate outer loops — a full collapse was deliberately skipped because `run()` (breakpoints as `Err(String)`, no pause-flag/exec-state mutation) is not byte-identical to the stepping machinery.

### Syscall Boilerplate — DONE
- **DONE (2026-06-23):** the ~85–95 uniform syscall arms were collapsed with `macro_rules!` generators (`sys_noarg!`/`sys_typed!`/`sys_math1!`/`sys_math2!`), shrinking `handle_syscall` by ~726 lines, behavior-preserving (numbers/coercions/errors unchanged). The ~15–20 irregular arms (exit, print, env_get Option, json/http, file I/O fallbacks) stay explicit and commented.

### Stdlib — test coverage added; still thin wrappers
- **DONE (2026-06-23):** deterministic test coverage added for the pure modules (core, io, deep collections, plus pre-existing strings/math/json/hash/base64/url/uuid/path). Fixed a real compiler bug found while testing: chained `self.x().y()` method calls inside `impl` blocks crashed (`Cannot get field from array`) — codegen now tracks impl-method return types to dispatch chained calls.
- Remaining: most I/O modules (fs, env, os, net, http) are still thin syscall wrappers; nondeterministic modules (random/uuid/time/fs) have only structural coverage.

### Playground Backend
- Backend exists (1,806 lines) but no WASM compilation — runs VM server-side
- Frontend exists but unclear if it's functional
- **No fiber mode toggle in API/UI**: `RunRequest`, `StepRequest`, `Debug` messages have no `fiber_mode` field; no way to enable from frontend
- **WebSocket VM thread never drives fibers**: `VmThreadHandle` creates `DebugSession` which creates VM with `fiber_mode=false`; no command to enable fiber mode
- **Debug protocol has fiber/channel types but no data**: `VmStateJson` includes `fibers` and `channels` arrays but `DebugSession` never populates them

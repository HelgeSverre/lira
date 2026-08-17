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
  - **Playground/LSP fiber inspection — DONE (2026-06-26):** `VmStateJson.fibers`/`channels` are populated via `VM::scheduler_snapshot`, a fiber-mode toggle exists in the playground API + UI, and concurrent step debugging works end to end (breakpoints inside spawned fibers, live per-fiber state). Proven by the playground e2e + unit/integration tests. (See Playground Backend below.)

### Memory Management Module Is Dead Code — DELETED
- **DONE (2026-06-23):** `crates/liravm/src/memory.rs` (orphaned mark-sweep GC + cycle collector, zero consumers, structurally incompatible with the live `Value::Object(Rc<RefCell<HashMap>>)` model) and the unused string-interning (`string_pool`/`intern_string`/`make_string`) were **deleted**. `docs/13-memory-model.md` notes the specified cycle collector is not implemented — the runtime uses host `Rc` ARC. Real cycle reclamation would be a separate redesign.

## 🟡 Significant — Shallow Implementations

### LSP Is Mostly Semantic now (diagnostics + hover + completion + references/rename DONE)
- **DONE:** Diagnostics surface structured `CheckerError`/`CodegenError` (`crates/lirac/src/errors.rs`) with spans.
- **DONE (2026-06-23):** **Hover** and **member completion** are type-aware via `SemanticTables`. New error-tolerant `lirac::analyze()` returns AST + `sema` + diagnostics even on errors (editor buffers are usually mid-edit); a cursor→innermost-NodeId walker (`lira-lsp/src/sema_index.rs`) resolves the node under the cursor; hover shows the resolved `Type` (`expr_types`/`field_resolution`/`call_resolution`), and `.`-completion lists a receiver's real fields + methods (`type_members`). Parser hardened with a recursion-depth guard so malformed buffers can't crash the LSP.
- **DONE (2026-06-24):** **References/Rename** are scope-aware via `sema_refs::resolve_symbol_at` + `collect_symbol_ranges` (uses the checker's `symbol_refs`/`SymbolId` binding table), with member-aware fallbacks (`resolve_member_at`/`collect_member_ranges`). Shadowed bindings resolve to the correct declaration.
- **DONE (2026-07-02):** **Go-to-definition** is scope-aware — `definition.rs` resolves the `SymbolId`/member under the cursor via `sema_refs` and jumps to the binding's declaration, falling back to the type/top-level regex search only for symbols the semantic tables don't track. Shadowed locals jump to the correct `let`.
- **Still regex/text-based (remaining):**
  - Code actions: string manipulation (`let` ↔ `var` toggle, etc.)
  - Document highlight, workspace symbols, document links, inlay hints: still largely regex-driven (polish, not correctness).

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

### TODOs in Code — DONE (2026-07-02)
- **All six resolved and verified against current source** (`grep -n 'TODO\|FIXME'` over these files is now clean):
  - ~~`checker.rs`: named argument reordering~~ — handled.
  - ~~`checker.rs`: explicit type args for generic methods~~ — turbofish type args are parsed and erased at runtime (type-erased generics, by design).
  - ~~`parser.rs`: parse supertraits (`trait Ord: Eq { }`)~~ — parsed (`parser.rs` supertrait list, `trait Ord: Eq + Clone { }`).
  - ~~`parser.rs`: parse turbofish syntax (`::<T>`)~~ — parsed (`parse_turbofish_args`, both path and method-call forms).
  - ~~`vm.rs`: function name from debug info for call frames~~ — call frames resolve names via `function_name_at(frame.func_offset)`.
  - ~~`lira-doc/extractor.rs`: format default parameter expressions~~ — handled.

### Native Codegen: bare array-element assignment as a trailing expression
- **Open (2026-08-17):** a function whose *last statement* is a bare `arr[i] = x` is expression-oriented in Lira, so the checker infers its return value (confirmed: `let y = f(a)` type-checks when `f` ends with `a[0] = 1`). The bytecode VM runs this fine, but native codegen rejects it with the misleading `cannot box 'void' as 'any'` when the call is used in statement position inside a trailing `if`/block expression — the arm is lowered as void while the expression expects a value. Repro: `fn f(a: [int]) { a[0] = 1 }` used as a statement in an if-arm, `lira build` fails; `lira run` succeeds. `examples/game_of_life.li` works around it by giving the write helper an explicit return type; the codegen should either lower the discarded value or reject with a precise message.

### VM Code Duplication — mostly DONE
- **DONE (2026-06-23):** the duplicated fetch/decode block was extracted into a shared `decode_next()`, and the ~789-line `execute_opcode()` was split into category helpers (`execute_arithmetic`/`comparison`/`memory`/`control_flow`/`type`/`system`/`fiber_channel`).
- Remaining: `run()` vs the stepping path (`step_instruction`/`execute_one`) still have separate outer loops — a full collapse was deliberately skipped because `run()` (breakpoints as `Err(String)`, no pause-flag/exec-state mutation) is not byte-identical to the stepping machinery.

### Syscall Boilerplate — DONE
- **DONE (2026-06-23):** the ~85–95 uniform syscall arms were collapsed with `macro_rules!` generators (`sys_noarg!`/`sys_typed!`/`sys_math1!`/`sys_math2!`), shrinking `handle_syscall` by ~726 lines, behavior-preserving (numbers/coercions/errors unchanged). The ~15–20 irregular arms (exit, print, env_get Option, json/http, file I/O fallbacks) stay explicit and commented.

### Stdlib — test coverage added; still thin wrappers
- **DONE (2026-06-23):** deterministic test coverage added for the pure modules (core, io, deep collections, plus pre-existing strings/math/json/hash/base64/url/uuid/path). Fixed a real compiler bug found while testing: chained `self.x().y()` method calls inside `impl` blocks crashed (`Cannot get field from array`) — codegen now tracks impl-method return types to dispatch chained calls.
- **Runtime builtins — hermetic coverage added (2026-06-25):** an llvm-cov audit found 18 effectful `Runtime` builtins with **zero** test execution (http_get/post/request, tcp_read/write/read_line/close, env_all/keys/exe, file_seek, os_chdir, random_bytes, …). `crates/liravm/tests/runtime_builtins.rs` now exercises them hermetically (HTTP/TCP against a localhost server on an ephemeral port, temp-file seek, unique-key env round-trip). This lifted `runtime.rs` from 70%→**87.5%** region / 72%→**89%** line coverage; only `print`/`println` (covered functionally via `@expect` on a different VM path) and `read_line` (stdin, would block) remain unexecuted.
- Remaining: the **stdlib `.li` wrappers** for fs/env/os/net/http aren't run at the `.li` level (the underlying Rust builtins are now covered); nondeterministic modules (uuid/time) have only structural coverage.

### Playground Backend
- Backend exists (1,806 lines) but no WASM compilation — runs VM server-side
- Frontend exists but unclear if it's functional
- **Fiber mode toggle — backend DONE (2026-06-25):** `Run`/`Debug` client messages carry a `#[serde(default)] fiber_mode` field; `DebugSession::set_fiber_mode` applies it. Plain `Run` goes straight to completion (`run_to_completion`); fiber-mode `Debug` is **steppable** (see concurrent step debugging below). Frontend: a header **Fibers** toggle (`uiStore.fiberMode`) routes Run/Debug through the scheduler-aware path and the Fibers/Channels tabs render live state.
- **WebSocket VM thread now drives fibers — DONE (2026-06-25):** `handle_run` enables fiber mode before load and drives the scheduler (run-to-completion for `Run`, stepping for `Debug`).
- **Debug protocol fiber/channel data — DONE (2026-06-25):** new `VM::scheduler_snapshot()` → `SchedulerSnapshot` (value-carrying via `RichValue`); `ServerMessage::VmStateJson` carries populated `fibers[]`/`channels[]` and is emitted after every fiber-mode drive (initial run, each step, each continue, breakpoint pauses). The running fiber's snapshot reflects its live context (with resolved frame names). Proven by `vm_thread::tests::fiber_mode_debug_returns_populated_fibers_and_channels` and `vm::tests::test_scheduler_snapshot_after_spawn_channel`.
- **Concurrent step debugging — DONE (2026-06-25):** `VM::step_instruction` is fiber-aware — it bootstraps `main` as a fiber and reschedules across fibers (shared `ensure_fiber_runtime_started` + `pump_scheduler`), so `step`/`continue`/breakpoints drive the scheduler. Breakpoints fire inside spawned fibers (with `main` shown blocked); deadlock is surfaced while stepping; mixing `run()` and stepping does not double-spawn `main`. Proven by `vm::tests::test_step_instruction_drives_fibers`, `vm_thread::tests::concurrent_step_debugging_breakpoint_in_spawned_fiber`, and adversarial probes (`probe_d1_deadlock_while_stepping`, `probe_d2_breakpoint_dedup_across_fibers`, `probe_c_pure_step_instruction_two_workers`, `probe_mix_step_then_run_no_double_spawn`).
- **Frontend fiber visualization — DONE (2026-06-25):** header **Fibers** toggle (`uiStore.fiberMode`); `vmStateJson` messages flow into the rich VM store (`vmStore.applyVmStateJson`), and the Fibers/Channels tabs (`FiberInspector`) render live per-fiber state + channel buffers/waiters. Run with breakpoints in fiber mode for concurrent step debugging in the browser. Typechecks + builds (`tsc -b` + `vite build`); e2e in `e2e/playground-fibers.spec.ts` (drives the real backend over WS — needs port 3001 free + Playwright browsers installed).

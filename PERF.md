# Performance Profiling — Lira VM

Date: 2026-08-03

## Methodology

- **Profiler:** macOS Time Profiler via `cargo flamegraph` and `samply`
- **Build:** `release` profile with `debug = true` for symbols
- **VM workload:** `25 × fib(30)` — ~67.5M recursive function calls, ~14s runtime, 9,663 samples
- **Compiler workload:** 2000 compilations of `test_math.li` (334 lines) — 0.68s total (0.34ms/compile), too fast for meaningful profiling

## VM Top Hotspots

| Rank | Samples | % | Function | Location |
|------|---------|---|----------|----------|
| 1 | 927 | **9.6%** | `gc::gc::finalizer_safe` (Value Drop via derive Finalize) | `value.rs:39` |
| 2 | 805 | **8.3%** | `_nanov2_free` | macOS allocator — temp Vec dealloc |
| 3 | 798 | **8.3%** | Iterator `try_process` / call-arg collection | `vm.rs:1673-1678` |
| 4 | 694 | **7.2%** | `Opcode::from_byte` | `opcode.rs:164-228` |
| 5 | 687 | **7.1%** | `VM::read_u16` (two callsites) | `vm.rs:2271-2275` |
| 6 | 654 | **6.8%** | `VM::execute_opcode` | `vm.rs:1029-1103` |
| 7 | 483 | **5.0%** | `Vec<Value>::push_mut` (two callsites) | stack operations |
| 8 | 458 | **4.7%** | `VM::decode_next` | `vm.rs:672-696` |
| 9 | 370 | **3.8%** | `Value::clone` (two callsites) | LoadConst, LoadLocal, Dup, GetField |
| 10 | 289 | **3.0%** | `drop_glue::<Value>` (two callsites) | Return, Pop path |

## Root Causes

### 1. Per-Call Vec allocation (~17% combined)
`vm.rs:1673-1678` — Every `Call` instruction allocates two temporary Vecs for arguments:

```rust
let args: Vec<Value> = (0..arg_count)
    .map(|_| self.pop()).collect::<Result<Vec<_>>>()?  // alloc 1
    .into_iter().rev().collect();                       // alloc 2
```

For our workload: 67.5M × 2 Vec allocations. Same pattern in `MakeClosure`.

**Fix:** Pop directly into a pre-allocated buffer or reuse an arena Vec. Skip the double-reverse.

### 2. Value Drop overhead via GC finalizer (9.6%)
`Value` derives `Finalize` from the `gc` crate. Every Value drop calls through `finalizer_safe`, even for scalars where it's a no-op. 67.5M drops in this workload.

**Fix:** Split Value into Copy scalars and heap variants, or implement Drop manually with early-exit path.

### 3. Two-level opcode dispatch (6.8%)
`vm.rs:1029-1103` — First match sorts into 6 categories, then each handler does another match on the same opcode. The spec documents a planned `cfg(feature = "threaded")` jump table but it's not wired up. `#[repr(u8)]` on `Opcode` is ready.

**Fix:** Flatten to single match or implement jump table.

### 4. Per-byte bounds checks in read_u16 (7.1%)
`vm.rs:2271-2275` — `read_u16` calls `read_u8` twice, each with its own bounds check `self.ip >= self.program.code.len()`. Many opcodes combine `read_u16` + additional `read_u8` calls.

**Fix:** Single bounds check then slice access: `u16::from_le_bytes([code[ip], code[ip+1]])`.

### 5. No typed opcodes (indirect cost)
All arithmetic dispatches through runtime `Value` tag matching even though the type checker knows the types. `compare_values` at 0.9% even on our int-only workload.

**Fix:** Add `IAdd`/`FAdd` etc. checker-emitted typed opcodes. Noted in `TODO-FIXMES.md:13`.

### 6. GC collect interval (2-5%)
`vm.rs:157` — `AUTO_COLLECT_INTERVAL = 10_000` triggers `gc::force_collect()` every 10k cyclic allocations.

**Fix:** Raise to 100k or make time-based.

## Optimization Plan

| # | Fix | Est. Speedup | Effort | Done |
|---|-----|-------------|--------|------|
| 1 | Fused `read_u16` — single bounds check | ~3-5% | 5 lines | [x] |
| 2 | Eliminate per-Call Vec allocations | Contributes to combined | ~10 lines per arm | [x] |
| 3 | Flat opcode dispatch — `#[inline(always)]` | Contributes to combined | 7 annotations | [x] |
| 4 | Manual Drop / split Value | ~5-10% | Medium | deferred |
| 5 | GC interval tuning | ~2-5% on alloc-heavy | 1 constant | [x] |
| 6 | Typed opcodes (IAdd/FAdd etc) | ~5-15% on numeric code | Large | deferred |

**Combined result (fixes 1-3, 5): ~8.5% speedup** measured on 25× fib(30).
Baseline: 14.00s avg. Optimized: 12.90s avg. 10 runs each, all 58+112 tests pass.

## Value Size

`Value` enum is **16 bytes** (verified via `size_of`):
- `Null`, `Bool`, `Int(i64)`, `Float(f64)`, `Function(usize)`, `Fiber(u64)`, `Channel(u64)` — 8-byte payload
- `String(IString)` — `Rc<String>`, 8-byte pointer
- `Array`, `Object` — `Gc<GcCell<...>>`, 8-byte pointer
- `Closure` — `Gc<ClosureData>`, 8-byte pointer

## Cross-Language Benchmark: `fib(30) × 25`

67M recursive calls, user time, macOS arm64. Run via: `scripts/bench-fib [N] [ITERS]`

| Language | Time | vs Lira | Runtime |
|----------|------|---------|---------|
| C (clang -O2) | 0.00s | — | native |
| Rust (-O3) | 0.06s | 226× | native |
| Go | 0.07s | 194× | native |
| Dart AOT | 0.12s | 113× | native |
| Node.js | 0.19s | 72× | V8 JIT |
| PHP | 1.64s | 8.3× | opcode interpreter |
| Ruby | 2.01s | 6.8× | CRuby interpreter |
| Sema (.semac) | 3.12s | 4.4× | bytecode VM |
| **Lira (.lic)** | **13.59s** | **1×** | bytecode VM |

This is a worst-case benchmark for Lira — pure recursion with tiny function bodies
exercises call/return overhead exclusively. On real programs (e.g. `test_math.li`,
334 lines of math operations), Lira runs in <1ms since time is spent in native
runtime functions rather than VM dispatch.

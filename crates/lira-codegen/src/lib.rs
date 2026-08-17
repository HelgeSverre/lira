//! Native code generation for Lira, built on Cranelift.
//!
//! `lirac::codegen` lowers the checked AST to bytecode for `liravm`; this crate
//! lowers the same AST to machine code. Because the checker has already proved
//! every type, the emitted code is fully unboxed — an `int` is an `i64`
//! register, a struct field access is a load at a constant offset — and the only
//! runtime support needed is `liblira_rt` (allocation, strings, arrays, and the
//! fiber scheduler).
//!
//! Two drivers sit on top of the same lowering:
//!
//! - [`jit_run`] compiles in a worker process and runs the program with hard
//!   resource limits.
//! - [`aot::build`] writes a standalone native executable.
//!
//! See `docs/60-native-backend.md` for the design and the current limits.

pub mod abi;
pub mod aot;
pub mod error;
mod isolate;
pub mod jit;
pub mod layout;
pub mod lower;
pub mod runtime;

use std::path::Path;

pub use error::{CodegenError, CodegenResult};

/// Maximum UTF-8 source size accepted by the isolated JIT entry points.
pub const ISOLATED_JIT_MAX_SOURCE_BYTES: usize = isolate::MAX_SOURCE_BYTES;

/// Compile Lira source (with imports resolved) to a native executable.
pub fn build_native(source_file: &str, source: &str, output: &Path) -> Result<(), String> {
    let analysis = analyze(source_file, source)?;
    aot::build(&analysis.program, &analysis.sema, output).map_err(String::from)
}

/// Compile Lira source (with imports resolved) and run it in a worker process.
///
/// This is the safe public entry point. It fails closed unless
/// `LIRA_JIT_WORKER` names an executable worker; callers should normally set
/// that variable to the CLI executable or use [`jit_run_isolated`] directly.
pub fn jit_run(source_file: &str, source: &str) -> Result<i32, String> {
    let worker = std::env::var_os("LIRA_JIT_WORKER")
        .ok_or_else(|| "JIT isolation unavailable: LIRA_JIT_WORKER is not set".to_string())?;
    if worker.is_empty() {
        return Err("JIT isolation unavailable: LIRA_JIT_WORKER is empty".to_string());
    }
    jit_run_isolated(Path::new(&worker), source_file, source)
}

/// Compile Lira source and run it in an isolated worker process.
///
/// The worker is placed in its own process group and is bounded by CPU,
/// memory, wall-clock, output, native-runtime, and fiber limits. Its result
/// is returned only through a private result file, never inferred from worker
/// stdout. The worker executable itself must be trusted; the boundary contains
/// generated Lira code, not a hostile executable that deliberately escapes its
/// process group. This is the API embedders should use for untrusted source.
pub fn jit_run_isolated(worker: &Path, source_file: &str, source: &str) -> Result<i32, String> {
    isolate::run(worker, source_file, source)
}

/// Compile Lira source (with imports resolved) and run it in this process.
///
/// This API is intentionally explicit and has no process-level deadline. It
/// is suitable only for trusted source in a host that supplies its own
/// containment. Use [`jit_run`] or [`jit_run_isolated`] for untrusted source.
pub fn jit_run_in_process(source_file: &str, source: &str) -> Result<i32, String> {
    let analysis = analyze(source_file, source)?;
    jit::run(&analysis.program, &analysis.sema).map_err(String::from)
}

/// Parse, resolve imports and type check, refusing to generate code for a
/// program the checker rejected.
fn analyze(source_file: &str, source: &str) -> Result<lirac::Analysis, String> {
    let analysis = lirac::analyze_with_imports(source_file, source)
        .map_err(|d| format!("{}:{}: {}", d.line, d.column, d.message))?;
    if let Some(first) = analysis.diagnostics.first() {
        return Err(format!(
            "{}:{}: {}",
            first.line, first.column, first.message
        ));
    }
    Ok(analysis)
}

#[cfg(test)]
#[allow(clashing_extern_declarations)]
mod native_gc_tests {
    use std::ffi::c_void;

    #[repr(C)]
    struct LiraArray {
        _private: [u8; 0],
    }

    extern "C" {
        #[link_name = "lira_rt_boot"]
        fn gc_boot(entry: extern "C" fn(*mut c_void), env: *mut c_void) -> i32;
        #[link_name = "lira_rt_array_new"]
        fn gc_array_new(capacity: i64) -> *mut LiraArray;
        #[link_name = "lira_rt_array_push"]
        fn gc_array_push(array: *mut LiraArray, value: i64);
        #[link_name = "lira_rt_collect"]
        fn gc_collect();
        #[link_name = "lira_rt_gc_live_bytes"]
        fn gc_live_bytes() -> i64;
        #[link_name = "lira_rt_gc_live_objects"]
        fn gc_live_objects() -> i64;
    }

    static mut CYCLE_SURVIVED: bool = false;
    static mut CYCLE_RECLAIMED: bool = false;
    static mut PEAK_BYTES: i64 = 0;

    extern "C" fn collector_fiber(_env: *mut c_void) {
        let mut a = unsafe { gc_array_new(1) };
        let mut b = unsafe { gc_array_new(1) };
        unsafe {
            gc_array_push(a, b as usize as i64);
            gc_array_push(b, a as usize as i64);
            gc_collect();
            CYCLE_SURVIVED = gc_live_objects() >= 2;
            std::ptr::write_volatile(&mut a, std::ptr::null_mut());
            std::ptr::write_volatile(&mut b, std::ptr::null_mut());
            gc_collect();
            CYCLE_RECLAIMED = gc_live_objects() == 0;
        }

        let mut value = std::ptr::null_mut();
        for _ in 0..100_000 {
            value = unsafe { gc_array_new(16) };
            unsafe {
                PEAK_BYTES = PEAK_BYTES.max(gc_live_bytes());
            }
        }
        unsafe {
            std::ptr::write_volatile(&mut value, std::ptr::null_mut());
            gc_collect();
        }
    }

    #[test]
    fn collector_preserves_cycles_and_reclaims_bounded_garbage() {
        {
            let _runtime_guard = super::jit::lock_runtime();
            unsafe {
                CYCLE_SURVIVED = false;
                CYCLE_RECLAIMED = false;
                PEAK_BYTES = 0;
                assert_eq!(gc_boot(collector_fiber, std::ptr::null_mut()), 0);
                assert!(CYCLE_SURVIVED, "reachable mutual cycle was reclaimed");
                assert!(CYCLE_RECLAIMED, "unreachable cycle was not reclaimed");
                assert!(
                    PEAK_BYTES < 16 * 1024 * 1024,
                    "collector did not stay bounded"
                );
                assert_eq!(gc_live_objects(), 0);
                assert_eq!(gc_live_bytes(), 0);
            }
        }

        // JIT globals live in the module's temporary data section. Running
        // two modules in one process proves the first root-slot set was
        // unregistered before its mapping was released.
        assert_eq!(
            super::jit_run_in_process("gc-first.li", "var retained = [1]\ncollect()"),
            Ok(0)
        );
        assert_eq!(
            super::jit_run_in_process("gc-second.li", "var retained = [2]\ncollect()"),
            Ok(0)
        );
    }
}

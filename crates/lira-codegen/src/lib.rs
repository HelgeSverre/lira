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
//! - [`jit::run`] compiles in memory and runs the program in this process.
//! - [`aot::build`] writes a standalone native executable.
//!
//! See `docs/60-native-backend.md` for the design and the current limits.

pub mod abi;
pub mod aot;
pub mod error;
pub mod jit;
pub mod layout;
pub mod lower;
pub mod runtime;

use std::path::Path;

pub use error::{CodegenError, CodegenResult};

/// Compile Lira source (with imports resolved) to a native executable.
pub fn build_native(source_file: &str, source: &str, output: &Path) -> Result<(), String> {
    let analysis = analyze(source_file, source)?;
    aot::build(&analysis.program, &analysis.sema, output).map_err(String::from)
}

/// Compile Lira source (with imports resolved) and run it in-process.
pub fn jit_run(source_file: &str, source: &str) -> Result<i32, String> {
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

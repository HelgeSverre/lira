//! Lira Virtual Machine Library
//!
//! Provides the core VM for executing Lira bytecode.
//!
//! Components:
//! - VM: The bytecode interpreter
//! - Fiber: Green thread scheduler
//! - Runtime: Built-in functions and syscall bindings
//! - Value: Runtime value types

pub mod bytecode;
pub mod debug;
pub mod debug_session;
pub mod fiber;
pub mod runtime;
pub mod value;
pub mod vm;

// Re-export commonly used types
pub use debug::{
    CallFrameInfo, DebugSnapshot, ExecutionState, LocalInfo, PauseFlag, RichValue, StepContext,
    StepMode, StepOutcome, ValueInfo,
};
pub use debug_session::{DebugEvent, DebugSession, SessionState};
pub use fiber::{FiberEvent, FiberState};
pub use value::{ChannelId, FiberId, Value};
pub use vm::{
    ChannelStateSnapshot, FiberFrameSnapshot, FiberStateSnapshot, RuntimeError, SchedulerSnapshot,
    StepResult, VM,
};

use std::fs;

/// Run a bytecode file and return the exit code
pub fn run_file(path: &str) -> Result<i32, String> {
    // Read bytecode file
    let bytecode = fs::read(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

    // Execute bytecode
    run(&bytecode)
}

/// Run bytecode and return the exit code
pub fn run(bytecode: &[u8]) -> Result<i32, String> {
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM
    let mut vm = vm::VM::new(program);
    vm.set_fiber_mode(true);

    // Execute
    vm.run()
}

/// Run bytecode and capture output (for testing)
pub fn run_with_capture(bytecode: &[u8]) -> Result<(i32, Vec<String>), String> {
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM with output capture
    let mut vm = vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);

    // Execute
    let exit_code = vm.run()?;

    Ok((exit_code, vm.get_output().to_vec()))
}

/// Run bytecode and capture output, returning a structured [`RuntimeError`] on
/// failure.
///
/// Behaves like [`run_with_capture`], but on a runtime error returns the bare
/// message together with the source location (when recoverable) and the output
/// produced before the failure. Intended for callers (e.g. the playground) that
/// need to surface a real source location rather than parsing the prefixed
/// string form.
pub fn run_with_capture_structured(
    bytecode: &[u8],
) -> Result<(i32, Vec<String>), (Vec<String>, RuntimeError)> {
    let program = bytecode::load(bytecode).map_err(|message| {
        (
            vec![],
            RuntimeError {
                message,
                line: None,
                column: None,
                stack: Vec::new(),
            },
        )
    })?;

    let mut vm = vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_capture_output(true);

    match vm.run_inner() {
        Ok(exit_code) => Ok((exit_code, vm.get_output().to_vec())),
        Err(message) => {
            let (line, column) = match vm.get_current_location() {
                Some((line, column)) => (Some(line), Some(column)),
                None => (None, None),
            };
            let stack = vm.build_call_stack_names();
            Err((
                vm.get_output().to_vec(),
                RuntimeError {
                    message,
                    line,
                    column,
                    stack,
                },
            ))
        }
    }
}

/// Run bytecode with debug tracing enabled
pub fn run_with_debug(bytecode: &[u8]) -> Result<i32, String> {
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM with debug enabled
    let mut vm = vm::VM::new(program);
    vm.set_fiber_mode(true);
    vm.set_debug(true);

    // Execute
    vm.run()
}

/// Create a VM for manual stepping/control
pub fn create_vm(bytecode: &[u8]) -> Result<VM, String> {
    let program = bytecode::load(bytecode)?;
    Ok(vm::VM::new(program))
}

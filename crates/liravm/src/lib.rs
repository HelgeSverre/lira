//! Lira Virtual Machine Library
//!
//! Provides the core VM for executing Lira bytecode.
//!
//! Components:
//! - VM: The bytecode interpreter
//! - Memory: ARC-based heap management
//! - Fiber: Green thread scheduler
//! - Runtime: Built-in functions and syscall bindings
//! - Value: Runtime value types

pub mod bytecode;
pub mod debug;
pub mod debug_session;
pub mod fiber;
pub mod memory;
pub mod runtime;
pub mod value;
pub mod vm;

// Re-export commonly used types
pub use debug::{
    CallFrameInfo, DebugSnapshot, ExecutionState, LocalInfo, PauseFlag, StepContext, StepMode,
    StepOutcome, ValueInfo,
};
pub use debug_session::{DebugEvent, DebugSession, SessionState};
pub use value::{ChannelId, FiberId, Value};
pub use vm::{ChannelSnapshot, FiberSnapshot, StepResult, VmSnapshot, VM};

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

    // Execute
    vm.run()
}

/// Run bytecode and capture output (for testing)
pub fn run_with_capture(bytecode: &[u8]) -> Result<(i32, Vec<String>), String> {
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM with output capture
    let mut vm = vm::VM::new(program);
    vm.set_capture_output(true);

    // Execute
    let exit_code = vm.run()?;

    Ok((exit_code, vm.get_output().to_vec()))
}

/// Run bytecode with debug tracing enabled
pub fn run_with_debug(bytecode: &[u8]) -> Result<i32, String> {
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM with debug enabled
    let mut vm = vm::VM::new(program);
    vm.set_debug(true);

    // Execute
    vm.run()
}

/// Run bytecode with output streaming callback
/// Returns (exit_code, final_snapshot)
pub fn run_with_streaming<F>(
    bytecode: &[u8],
    on_output: F,
) -> Result<(i32, VmSnapshot), String>
where
    F: FnMut(&str) + Send + 'static,
{
    // Load bytecode
    let program = bytecode::load(bytecode)?;

    // Create VM with output callback
    let mut vm = vm::VM::new(program);
    vm.set_capture_output(true);
    vm.set_output_callback(on_output);

    // Execute
    let exit_code = vm.run()?;

    // Get final snapshot
    let snapshot = vm.get_snapshot();

    Ok((exit_code, snapshot))
}

/// Create a VM for manual stepping/control
pub fn create_vm(bytecode: &[u8]) -> Result<VM, String> {
    let program = bytecode::load(bytecode)?;
    Ok(vm::VM::new(program))
}

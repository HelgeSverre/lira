//! Debug Session
//!
//! Thread-safe wrapper around VM for interactive debugging.
//! Provides high-level debugging operations for use by IDE integrations,
//! WebSocket sessions, and future DAP adapters.

use crate::bytecode::load;
use crate::debug::{DebugSnapshot, PauseFlag, StepOutcome};
use crate::fiber::FiberEvent;
use crate::vm::VM;
use std::sync::{Mutex, RwLock};

/// Session state for tracking what phase we're in
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SessionState {
    /// No program loaded
    #[default]
    Empty,
    /// Program loaded, ready to run
    Ready,
    /// Currently executing
    Running,
    /// Paused (at breakpoint or suspended)
    Paused,
    /// Execution finished
    Finished { exit_code: i32 },
    /// Error occurred
    Error { message: String },
}

/// Debug event emitted during execution
#[derive(Debug, Clone)]
pub enum DebugEvent {
    /// Execution state changed
    StateChanged(DebugSnapshot),
    /// Program produced output
    Output(String),
    /// Hit a breakpoint
    BreakpointHit { line: u32, column: u32, ip: usize },
    /// Step operation completed
    StepCompleted { line: u32, column: u32, ip: usize },
    /// Execution paused by user request
    Paused { line: u32, column: u32, ip: usize },
    /// Program finished normally
    Finished { exit_code: i32 },
    /// Runtime error occurred
    Error {
        message: String,
        location: Option<(u32, u32)>,
    },
}

/// Thread-safe debug session wrapping VM
pub struct DebugSession {
    /// The VM instance (None before program is loaded)
    vm: Mutex<Option<VM>>,
    /// Original source code
    source: RwLock<Option<String>>,
    /// Compiled bytecode
    bytecode: RwLock<Option<Vec<u8>>>,
    /// Current session state
    state: RwLock<SessionState>,
    /// Active breakpoints (line numbers)
    breakpoints: RwLock<Vec<u32>>,
    /// Pause flag (shared with VM for thread-safe pause requests)
    pause_flag: PauseFlag,
    /// Accumulated output
    output: RwLock<Vec<String>>,
}

impl DebugSession {
    /// Create a new empty debug session
    pub fn new() -> Self {
        Self {
            vm: Mutex::new(None),
            source: RwLock::new(None),
            bytecode: RwLock::new(None),
            state: RwLock::new(SessionState::Empty),
            breakpoints: RwLock::new(Vec::new()),
            pause_flag: PauseFlag::new(),
            output: RwLock::new(Vec::new()),
        }
    }

    /// Load source code and compile to bytecode
    /// Returns error message if compilation fails
    pub fn load(&self, source: &str, bytecode: Vec<u8>) -> Result<(), String> {
        // Store source
        *self.source.write().unwrap() = Some(source.to_string());
        *self.bytecode.write().unwrap() = Some(bytecode.clone());

        // Load bytecode into program
        let program = load(&bytecode)?;

        // Create VM
        let mut vm = VM::new(program);
        vm.prepare();
        vm.set_capture_output(true);

        // Apply breakpoints
        let breakpoints = self.breakpoints.read().unwrap().clone();
        vm.set_breakpoints(breakpoints);

        // Store VM
        *self.vm.lock().unwrap() = Some(vm);
        *self.state.write().unwrap() = SessionState::Ready;

        Ok(())
    }

    /// Start or restart execution from the beginning
    pub fn start(&self) -> Result<DebugSnapshot, String> {
        let bytecode = self.bytecode.read().unwrap();
        let bytecode = bytecode.as_ref().ok_or("No program loaded")?.clone();

        // Reload the program to reset state
        let program = load(&bytecode)?;
        let mut vm = VM::new(program);
        vm.prepare();
        vm.set_capture_output(true);

        // Apply breakpoints
        let breakpoints = self.breakpoints.read().unwrap().clone();
        vm.set_breakpoints(breakpoints);

        // Clear output
        *self.output.write().unwrap() = Vec::new();

        // Get initial snapshot
        let snapshot = vm.get_debug_snapshot();

        // Store VM
        *self.vm.lock().unwrap() = Some(vm);
        *self.state.write().unwrap() = SessionState::Ready;

        Ok(snapshot)
    }

    /// Continue execution until breakpoint or completion
    pub fn continue_execution(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.continue_execution();
        let event = self.outcome_to_event(outcome, vm);

        // Update session state based on event
        self.update_state_from_event(&event);

        // Capture any new output
        self.capture_output(vm);

        Ok(event)
    }

    /// Execute a single bytecode instruction
    pub fn step_instruction(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.step_instruction();
        let event = self.outcome_to_event(outcome, vm);

        self.update_state_from_event(&event);
        self.capture_output(vm);

        Ok(event)
    }

    /// Step to next source line
    pub fn step_line(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.step_line();
        let event = self.outcome_to_event(outcome, vm);

        self.update_state_from_event(&event);
        self.capture_output(vm);

        Ok(event)
    }

    /// Step into function calls
    pub fn step_into(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.step_into();
        let event = self.outcome_to_event(outcome, vm);

        self.update_state_from_event(&event);
        self.capture_output(vm);

        Ok(event)
    }

    /// Step over function calls
    pub fn step_over(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.step_over();
        let event = self.outcome_to_event(outcome, vm);

        self.update_state_from_event(&event);
        self.capture_output(vm);

        Ok(event)
    }

    /// Step out of current function
    pub fn step_out(&self) -> Result<DebugEvent, String> {
        let mut vm_guard = self.vm.lock().unwrap();
        let vm = vm_guard.as_mut().ok_or("No program loaded")?;

        *self.state.write().unwrap() = SessionState::Running;

        let outcome = vm.step_out();
        let event = self.outcome_to_event(outcome, vm);

        self.update_state_from_event(&event);
        self.capture_output(vm);

        Ok(event)
    }

    /// Request a pause (can be called from another thread)
    pub fn pause(&self) {
        self.pause_flag.request();
    }

    /// Stop execution and reset
    pub fn stop(&self) {
        *self.vm.lock().unwrap() = None;
        *self.state.write().unwrap() = SessionState::Empty;
    }

    /// Set breakpoints (replaces all existing breakpoints)
    pub fn set_breakpoints(&self, lines: Vec<u32>) {
        *self.breakpoints.write().unwrap() = lines.clone();

        // Apply to VM if loaded
        if let Some(ref mut vm) = *self.vm.lock().unwrap() {
            vm.set_breakpoints(lines);
        }
    }

    /// Get current breakpoints
    pub fn get_breakpoints(&self) -> Vec<u32> {
        self.breakpoints.read().unwrap().clone()
    }

    /// Get current session state
    pub fn get_state(&self) -> SessionState {
        self.state.read().unwrap().clone()
    }

    /// Get debug snapshot (current VM state)
    pub fn get_snapshot(&self) -> Option<DebugSnapshot> {
        let vm_guard = self.vm.lock().unwrap();
        vm_guard.as_ref().map(|vm| vm.get_debug_snapshot())
    }

    /// Get accumulated output
    pub fn get_output(&self) -> Vec<String> {
        self.output.read().unwrap().clone()
    }

    /// Get the pause flag for external use
    pub fn get_pause_flag(&self) -> PauseFlag {
        self.pause_flag.clone()
    }

    /// Drain all pending fiber/channel events from the VM
    pub fn drain_fiber_events(&self) -> Vec<FiberEvent> {
        let vm_guard = self.vm.lock().unwrap();
        if let Some(ref vm) = *vm_guard {
            vm.drain_fiber_events()
        } else {
            Vec::new()
        }
    }

    // Private helper methods

    fn outcome_to_event(&self, outcome: StepOutcome, vm: &VM) -> DebugEvent {
        match outcome {
            StepOutcome::Continue => DebugEvent::StateChanged(vm.get_debug_snapshot()),
            StepOutcome::Breakpoint { line, column, ip } => {
                DebugEvent::BreakpointHit { line, column, ip }
            }
            StepOutcome::Paused { line, column, ip } => DebugEvent::Paused { line, column, ip },
            StepOutcome::StepCompleted { line, column, ip } => {
                DebugEvent::StepCompleted { line, column, ip }
            }
            StepOutcome::Finished { exit_code } => DebugEvent::Finished { exit_code },
            StepOutcome::Error { message } => {
                let location = vm.get_current_location();
                DebugEvent::Error { message, location }
            }
        }
    }

    fn update_state_from_event(&self, event: &DebugEvent) {
        let new_state = match event {
            DebugEvent::StateChanged(_) => SessionState::Running,
            DebugEvent::Output(_) => return, // Don't change state for output
            DebugEvent::BreakpointHit { .. } => SessionState::Paused,
            DebugEvent::StepCompleted { .. } => SessionState::Paused,
            DebugEvent::Paused { .. } => SessionState::Paused,
            DebugEvent::Finished { exit_code } => SessionState::Finished {
                exit_code: *exit_code,
            },
            DebugEvent::Error { message, .. } => SessionState::Error {
                message: message.clone(),
            },
        };
        *self.state.write().unwrap() = new_state;
    }

    fn capture_output(&self, vm: &VM) {
        let vm_output = vm.get_output();
        let mut output = self.output.write().unwrap();
        if vm_output.len() > output.len() {
            for line in vm_output.iter().skip(output.len()) {
                output.push(line.clone());
            }
        }
    }
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = DebugSession::new();
        assert_eq!(session.get_state(), SessionState::Empty);
        assert!(session.get_snapshot().is_none());
    }

    #[test]
    fn test_breakpoint_management() {
        let session = DebugSession::new();

        // Set breakpoints
        session.set_breakpoints(vec![5, 10, 15]);
        assert_eq!(session.get_breakpoints(), vec![5, 10, 15]);

        // Update breakpoints
        session.set_breakpoints(vec![7]);
        assert_eq!(session.get_breakpoints(), vec![7]);
    }

    #[test]
    fn test_stop_resets_state() {
        let session = DebugSession::new();
        session.stop();
        assert_eq!(session.get_state(), SessionState::Empty);
    }
}

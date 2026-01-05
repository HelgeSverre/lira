//! Debug Support Types
//!
//! Types for debugging execution: stepping, state inspection, and pause/resume.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Execution state of the VM during debugging
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExecutionState {
    /// VM is ready to execute (not started or after reset)
    #[default]
    Ready,
    /// Currently running
    Running,
    /// Paused at a breakpoint
    Paused {
        line: u32,
        column: u32,
        ip: usize,
    },
    /// Suspended by user pause request
    Suspended {
        line: u32,
        column: u32,
        ip: usize,
    },
    /// Program finished normally
    Finished {
        exit_code: i32,
    },
    /// Runtime error occurred
    Error {
        message: String,
        location: Option<(u32, u32)>,
    },
}


/// Step mode for single-stepping execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Execute one bytecode instruction
    Instruction,
    /// Execute until reaching a different source line
    Line,
    /// Step into function calls (same as Line but enters functions)
    Into,
    /// Step over function calls (execute until same or lower call depth)
    Over,
    /// Step out of current function (execute until return)
    Out,
}

/// Result of a stepping operation
#[derive(Debug, Clone)]
pub enum StepOutcome {
    /// More instructions available, execution can continue
    Continue,
    /// Hit a breakpoint
    Breakpoint {
        line: u32,
        column: u32,
        ip: usize,
    },
    /// Pause requested by user
    Paused {
        line: u32,
        column: u32,
        ip: usize,
    },
    /// Step operation completed (for line/into/over/out)
    StepCompleted {
        line: u32,
        column: u32,
        ip: usize,
    },
    /// Program finished normally
    Finished {
        exit_code: i32,
    },
    /// Runtime error occurred
    Error {
        message: String,
    },
}

impl StepOutcome {
    /// Check if execution should stop (not Continue)
    pub fn should_stop(&self) -> bool {
        !matches!(self, StepOutcome::Continue)
    }

    /// Check if this is an error
    pub fn is_error(&self) -> bool {
        matches!(self, StepOutcome::Error { .. })
    }

    /// Check if execution finished
    pub fn is_finished(&self) -> bool {
        matches!(self, StepOutcome::Finished { .. })
    }
}

/// Stepping context for tracking step operations
#[derive(Debug, Clone, Default)]
pub struct StepContext {
    /// Current step mode (None if not stepping)
    pub mode: Option<StepMode>,
    /// Starting source line for line-based stepping
    pub start_line: Option<u32>,
    /// Target call stack depth for step over/out
    pub target_depth: Option<usize>,
}

impl StepContext {
    /// Create a new empty step context
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new step operation
    pub fn start(&mut self, mode: StepMode, current_line: Option<u32>, current_depth: usize) {
        self.mode = Some(mode);
        self.start_line = current_line;
        self.target_depth = match mode {
            StepMode::Over => Some(current_depth),
            StepMode::Out => {
                if current_depth > 0 {
                    Some(current_depth - 1)
                } else {
                    None // At top level, run to completion
                }
            }
            _ => None,
        };
    }

    /// Clear the step context
    pub fn clear(&mut self) {
        self.mode = None;
        self.start_line = None;
        self.target_depth = None;
    }

    /// Check if a step is complete based on current state
    pub fn is_complete(&self, current_line: Option<u32>, current_depth: usize) -> bool {
        match self.mode {
            Some(StepMode::Instruction) => true, // Always complete after one instruction
            Some(StepMode::Line) | Some(StepMode::Into) => {
                // Complete when line changes
                match (self.start_line, current_line) {
                    (Some(start), Some(current)) => current != start,
                    _ => false,
                }
            }
            Some(StepMode::Over) => {
                // Complete when line changes AND depth <= start depth
                match (self.start_line, self.target_depth, current_line) {
                    (Some(start), Some(target_depth), Some(current)) => {
                        current != start && current_depth <= target_depth
                    }
                    _ => false,
                }
            }
            Some(StepMode::Out) => {
                // Complete when depth < start depth (returned from function)
                match self.target_depth {
                    Some(target_depth) => current_depth <= target_depth,
                    None => false, // At top level, run to completion
                }
            }
            None => false,
        }
    }
}

/// Thread-safe pause request flag
#[derive(Clone)]
pub struct PauseFlag {
    flag: Arc<AtomicBool>,
}

impl PauseFlag {
    /// Create a new pause flag
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request a pause (can be called from any thread)
    pub fn request(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Check if pause was requested and clear the flag
    pub fn check_and_clear(&self) -> bool {
        self.flag.swap(false, Ordering::SeqCst)
    }

    /// Check if pause is requested without clearing
    pub fn is_requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Clear the pause request
    pub fn clear(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl Default for PauseFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// Detailed debug snapshot with type information
#[derive(Debug, Clone)]
pub struct DebugSnapshot {
    /// Current execution state
    pub state: ExecutionState,
    /// Current instruction pointer
    pub ip: usize,
    /// Current source location
    pub location: Option<(u32, u32)>,
    /// Stack values with type info
    pub stack: Vec<ValueInfo>,
    /// Local variables with names
    pub locals: Vec<LocalInfo>,
    /// Call stack frames
    pub call_stack: Vec<CallFrameInfo>,
    /// Program output so far
    pub output: Vec<String>,
}

/// Value information for debugging
#[derive(Debug, Clone)]
pub struct ValueInfo {
    /// Display representation
    pub display: String,
    /// Type name
    pub type_name: String,
}

impl ValueInfo {
    pub fn new(display: String, type_name: String) -> Self {
        Self { display, type_name }
    }
}

/// Local variable information
#[derive(Debug, Clone)]
pub struct LocalInfo {
    /// Slot index
    pub slot: usize,
    /// Variable name (from debug info if available)
    pub name: Option<String>,
    /// Value info
    pub value: ValueInfo,
}

/// Call frame information for debugging
#[derive(Debug, Clone)]
pub struct CallFrameInfo {
    /// Function name (if available from debug info)
    pub function_name: Option<String>,
    /// Return address (instruction pointer)
    pub return_addr: usize,
    /// Source location of the call
    pub source_location: Option<(u32, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_context_line() {
        let mut ctx = StepContext::new();
        ctx.start(StepMode::Line, Some(10), 0);

        // Same line - not complete
        assert!(!ctx.is_complete(Some(10), 0));

        // Different line - complete
        assert!(ctx.is_complete(Some(11), 0));
    }

    #[test]
    fn test_step_context_over() {
        let mut ctx = StepContext::new();
        ctx.start(StepMode::Over, Some(10), 2);

        // Same line - not complete
        assert!(!ctx.is_complete(Some(10), 2));

        // Different line but deeper - not complete
        assert!(!ctx.is_complete(Some(11), 3));

        // Different line and same depth - complete
        assert!(ctx.is_complete(Some(11), 2));

        // Different line and shallower - complete
        assert!(ctx.is_complete(Some(11), 1));
    }

    #[test]
    fn test_step_context_out() {
        let mut ctx = StepContext::new();
        ctx.start(StepMode::Out, Some(10), 2);

        // Same depth - not complete
        assert!(!ctx.is_complete(Some(10), 2));

        // Shallower depth - complete
        assert!(ctx.is_complete(Some(15), 1));
    }

    #[test]
    fn test_pause_flag() {
        let flag = PauseFlag::new();

        assert!(!flag.is_requested());

        flag.request();
        assert!(flag.is_requested());

        // Check and clear
        assert!(flag.check_and_clear());
        assert!(!flag.is_requested());
    }
}

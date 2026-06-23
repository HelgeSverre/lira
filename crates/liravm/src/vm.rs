//! Lira VM Core
//!
//! Stack-based bytecode interpreter with fiber support.
//! See docs/lira/12-vm-runtime.md for the full specification.

use crate::bytecode::Program;
use crate::debug::{
    CallFrameInfo, DebugSnapshot, ExecutionState, LocalInfo, PauseFlag, StepContext, StepMode,
    StepOutcome, ValueInfo,
};
use crate::fiber::{FiberEvent, Scheduler};
use crate::runtime::Runtime;
use crate::value::{ChannelId, ClosureData, FiberId, Value};
use lira_core::opcode::Opcode;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};

/// Type alias for output callback function
pub type OutputCallback = Box<dyn FnMut(&str) + Send>;

/// Result of executing a single instruction
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Execution continues (more instructions to run)
    Continue,
    /// Program halted normally with exit code
    Halted(i32),
    /// Runtime error occurred
    Error(String),
    /// Execution paused at a breakpoint
    BreakpointHit {
        /// Source line number (1-based)
        line: u32,
        /// Source column number (1-based)
        column: u32,
        /// Instruction pointer where we stopped
        ip: usize,
    },
    /// Execution stopped by user
    Stopped,
}

/// Snapshot of VM state for debugging/visualization
#[derive(Debug, Clone)]
pub struct VmSnapshot {
    /// Current instruction pointer
    pub ip: usize,
    /// Current stack values (formatted as strings)
    pub stack: Vec<String>,
    /// Local variable values (formatted as strings)
    pub locals: Vec<String>,
    /// Call stack depth
    pub call_stack_depth: usize,
    /// Program output so far
    pub output: Vec<String>,
    /// All fibers with their states
    pub fibers: Vec<FiberSnapshot>,
    /// All channels with their states
    pub channels: Vec<ChannelSnapshot>,
    /// Current fiber ID (if in fiber mode)
    pub current_fiber_id: Option<FiberId>,
}

/// Snapshot of a single fiber
#[derive(Debug, Clone)]
pub struct FiberSnapshot {
    pub id: FiberId,
    pub state: String,
    pub ip: usize,
    pub stack_size: usize,
    pub locals_count: usize,
}

/// Snapshot of a channel
#[derive(Debug, Clone)]
pub struct ChannelSnapshot {
    pub id: ChannelId,
    pub capacity: usize,
    pub buffer_size: usize,
    pub closed: bool,
    pub waiting_receivers: usize,
    pub waiting_senders: usize,
}

/// Call frame for function calls
#[derive(Debug, Clone)]
struct CallFrame {
    /// Return address (instruction pointer to return to)
    return_addr: usize,
    /// Base pointer for local variables
    locals_base: usize,
    /// Number of local variables in this frame
    #[allow(dead_code)]
    local_count: usize,
    /// Base pointer for operand stack (to isolate caller's values)
    stack_base: usize,
    /// Captured values for this closure call (if any)
    captures: Option<Rc<ClosureData>>,
}

/// The Lira Virtual Machine
pub struct VM {
    /// The loaded program
    program: Program,
    /// Operand stack (used when running single-fiber mode)
    stack: Vec<Value>,
    /// Call stack
    call_stack: Vec<CallFrame>,
    /// Instruction pointer
    ip: usize,
    /// Local variables (flat array, indexed by frame base + slot)
    locals: Vec<Value>,
    /// Debug mode
    debug: bool,
    /// Fiber scheduler for concurrent execution
    scheduler: Scheduler,
    /// Whether we're running in fiber mode
    fiber_mode: bool,
    /// Runtime context for host primitives (file I/O, etc.)
    runtime: Runtime,
    /// Captured output (for testing)
    output: Vec<String>,
    /// Whether to capture output instead of printing
    capture_output: bool,
    /// Callback for streaming output
    output_callback: Option<OutputCallback>,
    /// Callback to check if execution should stop (for cooperative stopping)
    stop_check: Option<Box<dyn Fn() -> bool + Send>>,
    /// Breakpoint line numbers (1-based)
    breakpoints: HashSet<u32>,
    /// Track the last line we checked to avoid hitting same breakpoint repeatedly
    last_breakpoint_line: Option<u32>,
    /// Current execution state for debugging
    execution_state: ExecutionState,
    /// Stepping context for step operations
    step_context: StepContext,
    /// Pause request flag (thread-safe)
    pause_flag: PauseFlag,
    /// Receiver for fiber/channel events
    fiber_event_rx: Option<Receiver<FiberEvent>>,
    /// Fiber id of the main program (when running in fiber mode)
    main_fiber_id: FiberId,
    /// Exit code captured when the main fiber finishes
    main_exit_code: i32,
    /// Saved native call stacks per fiber. The VM-native `CallFrame` carries
    /// more state (stack base, captures) than the scheduler's `FiberCallFrame`,
    /// so each fiber's call stack is parked here across context switches.
    fiber_call_stacks: HashMap<FiberId, Vec<CallFrame>>,
}

impl VM {
    /// Create a new VM with the given program
    pub fn new(program: Program) -> Self {
        // Set up fiber event channel
        let (tx, rx) = channel();
        let mut scheduler = Scheduler::new();
        scheduler.set_event_sender(tx);

        Self {
            program,
            stack: Vec::with_capacity(1024),
            call_stack: Vec::with_capacity(64),
            ip: 0,
            locals: Vec::with_capacity(256),
            debug: false,
            scheduler,
            fiber_mode: false,
            runtime: Runtime::new(),
            output: Vec::new(),
            capture_output: false,
            output_callback: None,
            stop_check: None,
            breakpoints: HashSet::new(),
            last_breakpoint_line: None,
            execution_state: ExecutionState::Ready,
            step_context: StepContext::new(),
            pause_flag: PauseFlag::new(),
            fiber_event_rx: Some(rx),
            main_fiber_id: 0,
            main_exit_code: 0,
            fiber_call_stacks: HashMap::new(),
        }
    }

    /// Enable output capture mode (for testing)
    pub fn set_capture_output(&mut self, capture: bool) {
        self.capture_output = capture;
    }

    /// Enable debug mode
    pub fn set_debug(&mut self, debug: bool) {
        self.debug = debug;
    }

    /// Get captured output
    pub fn get_output(&self) -> &[String] {
        &self.output
    }

    /// Get captured output as a single string
    pub fn get_output_string(&self) -> String {
        self.output.join("\n")
    }

    /// Enable fiber mode for concurrent execution
    pub fn set_fiber_mode(&mut self, enabled: bool) {
        self.fiber_mode = enabled;
    }

    /// Set an output callback for streaming output during execution
    /// The callback is called with each output line as it's produced
    pub fn set_output_callback<F>(&mut self, callback: F)
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.output_callback = Some(Box::new(callback));
    }

    /// Clear the output callback
    pub fn clear_output_callback(&mut self) {
        self.output_callback = None;
    }

    /// Set a callback to check if execution should stop
    /// The callback is called at the start of each instruction.
    /// If it returns true, execution stops with an error.
    pub fn set_stop_check<F>(&mut self, callback: F)
    where
        F: Fn() -> bool + Send + 'static,
    {
        self.stop_check = Some(Box::new(callback));
    }

    /// Clear the stop check callback
    pub fn clear_stop_check(&mut self) {
        self.stop_check = None;
    }

    /// Set breakpoint line numbers (1-based)
    pub fn set_breakpoints(&mut self, lines: Vec<u32>) {
        self.breakpoints = lines.into_iter().collect();
    }

    /// Add a single breakpoint at the given line (1-based)
    pub fn add_breakpoint(&mut self, line: u32) {
        self.breakpoints.insert(line);
    }

    /// Remove a breakpoint at the given line
    pub fn remove_breakpoint(&mut self, line: u32) {
        self.breakpoints.remove(&line);
    }

    /// Clear all breakpoints
    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Drain all pending fiber/channel events
    pub fn drain_fiber_events(&self) -> Vec<FiberEvent> {
        let mut events = Vec::new();
        if let Some(ref rx) = self.fiber_event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        events
    }

    /// Get the current source location (line, column) from the instruction pointer
    /// Uses the debug info embedded in the bytecode
    pub fn get_current_location(&self) -> Option<(u32, u32)> {
        self.program.debug_info.lookup(self.ip as u32)
    }

    /// Get a snapshot of the current VM state
    pub fn get_snapshot(&self) -> VmSnapshot {
        let fibers: Vec<FiberSnapshot> = self
            .scheduler
            .fibers
            .values()
            .map(|f| FiberSnapshot {
                id: f.id,
                state: format!("{:?}", f.state),
                ip: f.ip,
                stack_size: f.stack.len(),
                locals_count: f.locals.len(),
            })
            .collect();

        let channels: Vec<ChannelSnapshot> = self
            .scheduler
            .channels
            .values()
            .map(|c| ChannelSnapshot {
                id: c.id,
                capacity: c.capacity,
                buffer_size: c.buffer.len(),
                closed: c.closed,
                waiting_receivers: c.receivers.len(),
                waiting_senders: c.senders.len(),
            })
            .collect();

        VmSnapshot {
            ip: self.ip,
            stack: self.stack.iter().map(|v| format!("{:?}", v)).collect(),
            locals: self.locals.iter().map(|v| format!("{:?}", v)).collect(),
            call_stack_depth: self.call_stack.len(),
            output: self.output.clone(),
            fibers,
            channels,
            current_fiber_id: self.scheduler.current,
        }
    }

    // ==================== Stepping Methods ====================

    /// Initialize the VM at entry point without executing
    pub fn prepare(&mut self) {
        self.ip = self.program.entry_point;
        self.execution_state = ExecutionState::Ready;
        self.step_context.clear();
        self.last_breakpoint_line = None;
    }

    /// Get the current execution state
    pub fn get_execution_state(&self) -> &ExecutionState {
        &self.execution_state
    }

    /// Get the pause flag for external pause requests
    pub fn get_pause_flag(&self) -> PauseFlag {
        self.pause_flag.clone()
    }

    /// Request a pause (can be called from another thread)
    pub fn request_pause(&self) {
        self.pause_flag.request();
    }

    /// Execute a single bytecode instruction
    pub fn step_instruction(&mut self) -> StepOutcome {
        // Check pause request first
        if self.pause_flag.check_and_clear() {
            let (line, column) = self.get_current_location().unwrap_or((0, 0));
            self.execution_state = ExecutionState::Suspended {
                line,
                column,
                ip: self.ip,
            };
            return StepOutcome::Paused {
                line,
                column,
                ip: self.ip,
            };
        }

        // Check cooperative stop
        if let Some(ref check) = self.stop_check {
            if check() {
                self.execution_state = ExecutionState::Error {
                    message: "Execution stopped by user".to_string(),
                    location: self.get_current_location(),
                };
                return StepOutcome::Error {
                    message: "Execution stopped by user".to_string(),
                };
            }
        }

        // Check breakpoints (only if we have any set)
        if !self.breakpoints.is_empty() {
            if let Some((line, column)) = self.get_current_location() {
                if self.breakpoints.contains(&line) && self.last_breakpoint_line != Some(line) {
                    self.last_breakpoint_line = Some(line);
                    self.execution_state = ExecutionState::Paused {
                        line,
                        column,
                        ip: self.ip,
                    };
                    return StepOutcome::Breakpoint {
                        line,
                        column,
                        ip: self.ip,
                    };
                } else if !self.breakpoints.contains(&line) {
                    self.last_breakpoint_line = None;
                }
            }
        }

        // Check if at end of program
        if self.ip >= self.program.code.len() {
            self.execution_state = ExecutionState::Finished { exit_code: 0 };
            return StepOutcome::Finished { exit_code: 0 };
        }

        // Execute one instruction
        self.execution_state = ExecutionState::Running;
        match self.execute_one() {
            Ok(Some(exit_code)) => {
                self.execution_state = ExecutionState::Finished { exit_code };
                StepOutcome::Finished { exit_code }
            }
            Ok(None) => StepOutcome::Continue,
            Err(e) => {
                self.execution_state = ExecutionState::Error {
                    message: e.clone(),
                    location: self.get_current_location(),
                };
                StepOutcome::Error { message: e }
            }
        }
    }

    /// Step to the next source line
    pub fn step_line(&mut self) -> StepOutcome {
        let start_line = self.get_current_location().map(|(l, _)| l);
        self.step_context
            .start(StepMode::Line, start_line, self.call_stack.len());
        self.run_until_step_complete()
    }

    /// Step into function calls (same as step_line, naturally enters functions)
    pub fn step_into(&mut self) -> StepOutcome {
        self.step_line()
    }

    /// Step over function calls (execute until same or lower call depth and line changed)
    pub fn step_over(&mut self) -> StepOutcome {
        let start_line = self.get_current_location().map(|(l, _)| l);
        self.step_context
            .start(StepMode::Over, start_line, self.call_stack.len());
        self.run_until_step_complete()
    }

    /// Step out of current function (execute until call depth decreases)
    pub fn step_out(&mut self) -> StepOutcome {
        if self.call_stack.is_empty() {
            // At top level, run to completion
            return self.continue_execution();
        }
        let start_line = self.get_current_location().map(|(l, _)| l);
        self.step_context
            .start(StepMode::Out, start_line, self.call_stack.len());
        self.run_until_step_complete()
    }

    /// Continue execution until breakpoint or completion
    pub fn continue_execution(&mut self) -> StepOutcome {
        self.step_context.clear();
        // Note: Don't clear last_breakpoint_line here - we need to remember
        // which breakpoint we just stopped at to avoid hitting it again immediately

        loop {
            match self.step_instruction() {
                StepOutcome::Continue => continue,
                outcome => return outcome,
            }
        }
    }

    /// Run until step operation completes
    fn run_until_step_complete(&mut self) -> StepOutcome {
        loop {
            let outcome = self.step_instruction();
            match &outcome {
                StepOutcome::Continue => {
                    // Check if step is complete
                    let current_line = self.get_current_location().map(|(l, _)| l);
                    let current_depth = self.call_stack.len();
                    if self.step_context.is_complete(current_line, current_depth) {
                        self.step_context.clear();
                        if let Some((line, column)) = self.get_current_location() {
                            return StepOutcome::StepCompleted {
                                line,
                                column,
                                ip: self.ip,
                            };
                        }
                    }
                }
                _ => {
                    // Breakpoint, Pause, Finished, Error - return immediately
                    self.step_context.clear();
                    return outcome;
                }
            }
        }
    }

    /// Fetch and decode the opcode at the current instruction pointer,
    /// advancing `ip` past the opcode byte.
    ///
    /// Emits the per-instruction debug trace when `self.debug` is set. This is
    /// the shared decode step used by both `run()` and `execute_one()`; the
    /// caller is responsible for any bounds check on `ip` before calling.
    fn decode_next(&mut self) -> Result<Opcode, String> {
        let opcode_byte = self.program.code[self.ip];
        self.ip += 1;

        let opcode = Opcode::from_byte(opcode_byte).ok_or_else(|| {
            format!(
                "Invalid opcode: 0x{:02X} at offset {}",
                opcode_byte,
                self.ip - 1
            )
        })?;

        if self.debug {
            let stack_repr: Vec<String> = self.stack.iter().map(|v| format!("{:?}", v)).collect();
            eprintln!(
                "[VM] ip={:04} {:?} stack=[{}] locals={}",
                self.ip - 1,
                opcode,
                stack_repr.join(", "),
                self.locals.len()
            );
        }

        Ok(opcode)
    }

    /// Execute a single instruction, returning Some(exit_code) if halted
    fn execute_one(&mut self) -> Result<Option<i32>, String> {
        let opcode = self.decode_next()?;
        // Execute the opcode and return whether we should halt
        self.execute_opcode(opcode)
    }

    /// Get a detailed debug snapshot with type information
    pub fn get_debug_snapshot(&self) -> DebugSnapshot {
        use crate::debug::RichValue;

        let stack: Vec<ValueInfo> = self
            .stack
            .iter()
            .map(|v| {
                ValueInfo::with_rich_value(
                    format!("{:?}", v),
                    self.value_type_name(v),
                    RichValue::from_value(v),
                )
            })
            .collect();

        // Get local names from debug info at current IP
        let local_names = self.program.debug_info.get_local_names_at(self.ip as u32);
        let locals: Vec<LocalInfo> = self
            .locals
            .iter()
            .enumerate()
            .map(|(i, v)| LocalInfo {
                slot: i,
                name: local_names.get(&(i as u16)).map(|s| s.to_string()),
                value: ValueInfo::with_rich_value(
                    format!("{:?}", v),
                    self.value_type_name(v),
                    RichValue::from_value(v),
                ),
            })
            .collect();

        let call_stack: Vec<CallFrameInfo> = self
            .call_stack
            .iter()
            .map(|frame| {
                CallFrameInfo {
                    function_name: None, // TODO: Get from debug info if available
                    return_addr: frame.return_addr,
                    source_location: self.program.debug_info.lookup(frame.return_addr as u32),
                }
            })
            .collect();

        DebugSnapshot {
            state: self.execution_state.clone(),
            ip: self.ip,
            location: self.get_current_location(),
            stack,
            locals,
            call_stack,
            output: self.output.clone(),
        }
    }

    /// Get the type name for a value
    fn value_type_name(&self, value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Int(_) => "int".to_string(),
            Value::Float(_) => "float".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Array(_) => "array".to_string(),
            Value::Object(_) => "object".to_string(),
            Value::Function(_) => "function".to_string(),
            Value::Closure(_) => "closure".to_string(),
            Value::Fiber(_) => "fiber".to_string(),
            Value::Channel(_) => "channel".to_string(),
        }
    }

    // ==================== End Stepping Methods ====================

    /// Run the program and return exit code
    pub fn run(&mut self) -> Result<i32, String> {
        // Start at entry point
        self.ip = self.program.entry_point;

        // In fiber mode, register the main program as a real fiber so its
        // context is saved/restored across context switches. Without this,
        // save_fiber_state/load_fiber_state silently drop main's state because
        // there is no "current" fiber.
        if self.fiber_mode {
            let main_id = self.scheduler.spawn(self.program.entry_point);
            self.main_fiber_id = main_id;
            // Pop main straight into the Running state and make it current.
            self.scheduler.schedule();
            self.ip = self.program.entry_point;
        }

        loop {
            // When the current fiber has parked or finished, re-enter the
            // scheduler to pick the next runnable fiber. Sequential programs
            // never reach this branch: main stays Running until it Halts.
            if self.fiber_mode && self.scheduler.current.is_none() {
                match self.scheduler.schedule() {
                    Some(_) => self.load_fiber_state(),
                    None => {
                        if self.scheduler.is_deadlocked() {
                            return Err("deadlock: all fibers are blocked".to_string());
                        }
                        if !self.scheduler.has_runnable() {
                            return Ok(self.main_exit_code);
                        }
                    }
                }
            }

            // Check if we should stop (cooperative stopping)
            if let Some(ref check) = self.stop_check {
                if check() {
                    return Err("Execution stopped by user".to_string());
                }
            }

            // Check breakpoints (only if we have any set)
            if !self.breakpoints.is_empty() {
                if let Some((line, column)) = self.get_current_location() {
                    // Only trigger if we're on a breakpoint line AND it's different from last hit
                    // (to avoid stopping on the same breakpoint repeatedly)
                    if self.breakpoints.contains(&line) && self.last_breakpoint_line != Some(line) {
                        self.last_breakpoint_line = Some(line);
                        return Err(format!(
                            "Breakpoint hit at line {}, column {} (ip={})",
                            line, column, self.ip
                        ));
                    } else if !self.breakpoints.contains(&line) {
                        // Clear last breakpoint line when we move to a different line
                        self.last_breakpoint_line = None;
                    }
                }
            }

            if self.ip >= self.program.code.len() {
                return Ok(0);
            }

            let opcode = self.decode_next()?;

            // Execute the opcode
            if let Some(exit_code) = self.execute_opcode(opcode)? {
                return Ok(exit_code);
            }
        }
    }

    /// Execute a single opcode, returning Some(exit_code) for Halt, None otherwise
    fn execute_opcode(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            Opcode::Nop => Ok(None),

            // Stack / local / object / array / closure data operations
            Opcode::LoadConst
            | Opcode::Pop
            | Opcode::Dup
            | Opcode::LoadLocal
            | Opcode::StoreLocal
            | Opcode::GetField
            | Opcode::SetField
            | Opcode::NewObject
            | Opcode::NewArray
            | Opcode::ArrayGet
            | Opcode::ArraySet
            | Opcode::ArrayLen
            | Opcode::ArrayPush
            | Opcode::ArrayPop
            | Opcode::MakeClosure
            | Opcode::LoadCapture => self.execute_memory(opcode),

            // Arithmetic operations
            Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Mod
            | Opcode::Neg
            | Opcode::Pow => self.execute_arithmetic(opcode),

            // Comparison / logical / bitwise operations
            Opcode::Eq
            | Opcode::Ne
            | Opcode::Lt
            | Opcode::Le
            | Opcode::Gt
            | Opcode::Ge
            | Opcode::And
            | Opcode::Or
            | Opcode::Not
            | Opcode::BitAnd
            | Opcode::BitOr
            | Opcode::BitXor
            | Opcode::BitNot
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::UShr => self.execute_comparison(opcode),

            // Control flow and function call/return
            Opcode::Jump
            | Opcode::JumpIfTrue
            | Opcode::JumpIfFalse
            | Opcode::Call
            | Opcode::Return
            | Opcode::Halt => self.execute_control_flow(opcode),

            // Type operations
            Opcode::TypeIs | Opcode::Cast => self.execute_type(opcode),

            // System operations
            Opcode::Print | Opcode::Syscall => self.execute_system(opcode),

            // Fiber and channel operations
            Opcode::Spawn
            | Opcode::Yield
            | Opcode::FiberId
            | Opcode::ChanNew
            | Opcode::ChanSend
            | Opcode::ChanRecv
            | Opcode::ChanClose
            | Opcode::ChanTryRecv
            | Opcode::Select => self.execute_fiber_channel(opcode),
        }
    }

    /// Stack, local variable, object, array and closure-data operations.
    fn execute_memory(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Stack operations
            Opcode::LoadConst => {
                let index = self.read_u16()? as usize;
                let constant = self
                    .program
                    .constants
                    .get(index)
                    .cloned()
                    .ok_or_else(|| format!("Invalid constant index: {}", index))?;
                self.stack.push(constant);
            }

            Opcode::Pop => {
                // Use safe pop to respect stack base
                let _ = self.pop();
            }

            Opcode::Dup => {
                let value = self.stack.last().cloned().ok_or("Stack underflow")?;
                self.stack.push(value);
            }

            // Local variable operations
            Opcode::LoadLocal => {
                let slot = self.read_u16()? as usize;
                let index = self.locals_base() + slot;

                // Extend locals if necessary
                while self.locals.len() <= index {
                    self.locals.push(Value::Null);
                }

                let value = self.locals.get(index).cloned().unwrap_or(Value::Null);
                self.stack.push(value);
            }

            Opcode::StoreLocal => {
                let slot = self.read_u16()? as usize;
                let index = self.locals_base() + slot;
                let value = self.pop()?;

                // Extend locals if necessary
                while self.locals.len() <= index {
                    self.locals.push(Value::Null);
                }

                self.locals[index] = value;
            }

            // Object operations
            Opcode::GetField => {
                let field_idx = self.read_u16()? as usize;
                let field_name = match self.program.constants.get(field_idx) {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err("Invalid field name constant".to_string()),
                };
                let object = self.pop()?;

                match object {
                    Value::Object(obj) => {
                        let value = obj
                            .borrow()
                            .get(&*field_name)
                            .cloned()
                            .unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err(format!("Cannot get field from {:?}", object)),
                }
            }

            Opcode::SetField => {
                let field_idx = self.read_u16()? as usize;
                let field_name = match self.program.constants.get(field_idx) {
                    Some(Value::String(s)) => (**s).clone(),
                    _ => return Err("Invalid field name constant".to_string()),
                };
                let value = self.pop()?;
                let object = self.pop()?;

                match object {
                    Value::Object(obj) => {
                        obj.borrow_mut().insert(field_name, value);
                    }
                    _ => return Err(format!("Cannot set field on {:?}", object)),
                }
            }

            Opcode::NewObject => {
                let obj = Rc::new(RefCell::new(HashMap::new()));
                self.stack.push(Value::Object(obj));
            }

            // Array operations
            Opcode::NewArray => {
                let size = self.pop()?;
                let size = match size {
                    Value::Int(n) => n as usize,
                    _ => return Err("Array size must be an integer".to_string()),
                };
                let arr = Rc::new(RefCell::new(vec![Value::Null; size]));
                self.stack.push(Value::Array(arr));
            }

            Opcode::ArrayGet => {
                let index = self.pop()?;
                let array = self.pop()?;

                match (array, index) {
                    (Value::Array(arr), Value::Int(idx)) => {
                        let idx = idx as usize;
                        let value = arr.borrow().get(idx).cloned().unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    (Value::String(s), Value::Int(idx)) => {
                        let idx = idx as usize;
                        let value = s
                            .chars()
                            .nth(idx)
                            .map(|c| Value::String(Rc::new(c.to_string())))
                            .unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    // Support object indexing with string keys (for JSON objects)
                    (Value::Object(obj), Value::String(key)) => {
                        let value = obj.borrow().get(&*key).cloned().unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err("Invalid array/index types".to_string()),
                }
            }

            Opcode::ArraySet => {
                let value = self.pop()?;
                let index = self.pop()?;
                let array = self.pop()?;

                match (array, index) {
                    (Value::Array(arr), Value::Int(idx)) => {
                        let idx = idx as usize;
                        let mut arr = arr.borrow_mut();
                        if idx < arr.len() {
                            arr[idx] = value;
                        }
                    }
                    _ => return Err("Invalid array/index types".to_string()),
                }
            }

            Opcode::ArrayLen => {
                let array = self.pop()?;
                let len = match array {
                    Value::Array(arr) => arr.borrow().len() as i64,
                    Value::String(s) => s.len() as i64,
                    _ => return Err("Cannot get length of non-array/string".to_string()),
                };
                self.stack.push(Value::Int(len));
            }

            Opcode::ArrayPush => {
                let value = self.pop()?;
                let array = self.pop()?;
                match array {
                    Value::Array(arr) => {
                        arr.borrow_mut().push(value);
                    }
                    _ => return Err("Cannot push to non-array".to_string()),
                }
            }

            Opcode::ArrayPop => {
                let array = self.pop()?;
                match array {
                    Value::Array(arr) => {
                        let value = arr.borrow_mut().pop().unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err("Cannot pop from non-array".to_string()),
                }
            }

            // Closure operations
            Opcode::MakeClosure => {
                let code_offset = self.read_u16()? as usize;
                let capture_count = self.read_u8()? as usize;

                // Pop captured values from stack (in reverse order)
                let mut captures = Vec::with_capacity(capture_count);
                for _ in 0..capture_count {
                    captures.push(self.pop()?);
                }
                captures.reverse();

                let closure = ClosureData {
                    code_offset,
                    captures,
                };
                self.stack.push(Value::Closure(Rc::new(closure)));
            }

            Opcode::LoadCapture => {
                let capture_idx = self.read_u8()? as usize;

                // Get captures from current call frame
                if let Some(frame) = self.call_stack.last() {
                    if let Some(ref closure) = frame.captures {
                        if capture_idx < closure.captures.len() {
                            self.stack.push(closure.captures[capture_idx].clone());
                        } else {
                            return Err(format!("Capture index {} out of bounds", capture_idx));
                        }
                    } else {
                        return Err("LoadCapture outside of closure".to_string());
                    }
                } else {
                    return Err("LoadCapture with no call frame".to_string());
                }
            }

            _ => unreachable!("execute_memory called with non-memory opcode"),
        }
        Ok(None)
    }

    /// Arithmetic operations.
    fn execute_arithmetic(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Arithmetic operations
            Opcode::Add => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a + b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    (Value::String(a), Value::String(b)) => {
                        Value::String(Rc::new(format!("{}{}", a, b)))
                    }
                    (Value::String(a), b) => Value::String(Rc::new(format!("{}{}", a, b))),
                    (a, Value::String(b)) => Value::String(Rc::new(format!("{}{}", a, b))),
                    _ => return Err(format!("Cannot add {:?} and {:?}", a, b)),
                };
                self.stack.push(result);
            }

            Opcode::Sub => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 - b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a - *b as f64),
                    _ => return Err(format!("Cannot subtract {:?} from {:?}", b, a)),
                };
                self.stack.push(result);
            }

            Opcode::Mul => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a * b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 * b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a * *b as f64),
                    _ => return Err(format!("Cannot multiply {:?} and {:?}", a, b)),
                };
                self.stack.push(result);
            }

            Opcode::Div => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => {
                        if *b == 0 {
                            return Err("Division by zero".to_string());
                        }
                        Value::Int(a / b)
                    }
                    (Value::Float(a), Value::Float(b)) => Value::Float(a / b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 / b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a / *b as f64),
                    _ => return Err(format!("Cannot divide {:?} by {:?}", a, b)),
                };
                self.stack.push(result);
            }

            Opcode::Mod => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => {
                        if *b == 0 {
                            return Err("Modulo by zero".to_string());
                        }
                        Value::Int(a % b)
                    }
                    (Value::Float(a), Value::Float(b)) => Value::Float(a % b),
                    _ => return Err(format!("Cannot modulo {:?} by {:?}", a, b)),
                };
                self.stack.push(result);
            }

            Opcode::Neg => {
                let a = self.pop()?;
                let result = match a {
                    Value::Int(n) => Value::Int(-n),
                    Value::Float(f) => Value::Float(-f),
                    _ => return Err(format!("Cannot negate {:?}", a)),
                };
                self.stack.push(result);
            }

            Opcode::Pow => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(base), Value::Int(exp)) => {
                        if *exp < 0 {
                            return Err("Negative exponent not supported for integers".to_string());
                        }
                        Value::Int(base.pow(*exp as u32))
                    }
                    (Value::Float(base), Value::Float(exp)) => Value::Float(base.powf(*exp)),
                    (Value::Int(base), Value::Float(exp)) => {
                        Value::Float((*base as f64).powf(*exp))
                    }
                    (Value::Float(base), Value::Int(exp)) => Value::Float(base.powi(*exp as i32)),
                    _ => return Err(format!("Cannot compute power of {:?} ^ {:?}", a, b)),
                };
                self.stack.push(result);
            }

            _ => unreachable!("execute_arithmetic called with non-arithmetic opcode"),
        }
        Ok(None)
    }

    /// Comparison, logical and bitwise operations.
    fn execute_comparison(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Comparison operations
            Opcode::Eq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }

            Opcode::Ne => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = !self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }

            Opcode::Lt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = self.compare_values(&a, &b)? < 0;
                self.stack.push(Value::Bool(result));
            }

            Opcode::Le => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = self.compare_values(&a, &b)? <= 0;
                self.stack.push(Value::Bool(result));
            }

            Opcode::Gt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = self.compare_values(&a, &b)? > 0;
                self.stack.push(Value::Bool(result));
            }

            Opcode::Ge => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = self.compare_values(&a, &b)? >= 0;
                self.stack.push(Value::Bool(result));
            }

            // Logical operations
            Opcode::And => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = a.is_truthy() && b.is_truthy();
                self.stack.push(Value::Bool(result));
            }

            Opcode::Or => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = a.is_truthy() || b.is_truthy();
                self.stack.push(Value::Bool(result));
            }

            Opcode::Not => {
                let a = self.pop()?;
                let result = !a.is_truthy();
                self.stack.push(Value::Bool(result));
            }

            // Bitwise operations
            Opcode::BitAnd => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x & y)),
                    _ => return Err("Bitwise AND requires integer operands".to_string()),
                }
            }

            Opcode::BitOr => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x | y)),
                    _ => return Err("Bitwise OR requires integer operands".to_string()),
                }
            }

            Opcode::BitXor => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x ^ y)),
                    _ => return Err("Bitwise XOR requires integer operands".to_string()),
                }
            }

            Opcode::BitNot => {
                let a = self.pop()?;
                match a {
                    Value::Int(x) => self.stack.push(Value::Int(!x)),
                    _ => return Err("Bitwise NOT requires integer operand".to_string()),
                }
            }

            Opcode::Shl => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Int(x << (y as u32))),
                    _ => return Err("Shift left requires integer operands".to_string()),
                }
            }

            Opcode::Shr => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        // Arithmetic right shift (preserves sign)
                        self.stack.push(Value::Int(x >> (y as u32)))
                    }
                    _ => return Err("Shift right requires integer operands".to_string()),
                }
            }

            Opcode::UShr => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        // Logical right shift (zero-fill)
                        let result = ((x as u64) >> (y as u32)) as i64;
                        self.stack.push(Value::Int(result))
                    }
                    _ => {
                        return Err("Unsigned shift right requires integer operands".to_string());
                    }
                }
            }

            _ => unreachable!("execute_comparison called with non-comparison opcode"),
        }
        Ok(None)
    }

    /// Control flow, function calls and program halt.
    fn execute_control_flow(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Control flow
            Opcode::Jump => {
                let offset = self.read_i16()?;
                self.ip = ((self.ip as i32) + (offset as i32)) as usize;
            }

            Opcode::JumpIfTrue => {
                let offset = self.read_i16()?;
                let condition = self.pop()?;
                if condition.is_truthy() {
                    self.ip = ((self.ip as i32) + (offset as i32)) as usize;
                }
            }

            Opcode::JumpIfFalse => {
                let offset = self.read_i16()?;
                let condition = self.pop()?;
                if !condition.is_truthy() {
                    self.ip = ((self.ip as i32) + (offset as i32)) as usize;
                }
            }

            // Function operations
            Opcode::Call => {
                let arg_count = self.read_u8()? as usize;
                let callee = self.pop()?;

                match callee {
                    Value::Function(code_offset) => {
                        // Pop arguments and store as locals
                        let args: Vec<Value> = (0..arg_count)
                            .map(|_| self.pop())
                            .collect::<Result<Vec<_>, _>>()?
                            .into_iter()
                            .rev()
                            .collect();

                        // Save stack base AFTER popping args - this isolates the caller's stack
                        let frame = CallFrame {
                            return_addr: self.ip,
                            locals_base: self.locals.len(),
                            local_count: arg_count,
                            stack_base: self.stack.len(),
                            captures: None,
                        };
                        self.call_stack.push(frame);

                        for arg in args {
                            self.locals.push(arg);
                        }

                        // Jump to function
                        self.ip = code_offset;
                    }
                    Value::Closure(closure_data) => {
                        // Pop arguments and store as locals
                        let args: Vec<Value> = (0..arg_count)
                            .map(|_| self.pop())
                            .collect::<Result<Vec<_>, _>>()?
                            .into_iter()
                            .rev()
                            .collect();

                        // Save stack base AFTER popping args - this isolates the caller's stack
                        let frame = CallFrame {
                            return_addr: self.ip,
                            locals_base: self.locals.len(),
                            local_count: arg_count,
                            stack_base: self.stack.len(),
                            captures: Some(closure_data.clone()),
                        };
                        self.call_stack.push(frame);

                        for arg in args {
                            self.locals.push(arg);
                        }

                        // Jump to closure code
                        self.ip = closure_data.code_offset;
                    }
                    _ => return Err(format!("Cannot call {:?}", callee)),
                }
            }

            Opcode::Return => {
                let return_value = self.pop().unwrap_or(Value::Null);

                if let Some(frame) = self.call_stack.pop() {
                    // Clean up locals
                    self.locals.truncate(frame.locals_base);
                    // Truncate stack to frame's stack base (discard any leftovers)
                    self.stack.truncate(frame.stack_base);
                    // Restore instruction pointer
                    self.ip = frame.return_addr;
                    // Push return value
                    self.stack.push(return_value);
                } else {
                    // Top-level return: the fiber body has run to completion.
                    if self.fiber_mode {
                        let finishing = self.scheduler.current;
                        self.scheduler.finish_current(return_value);
                        if finishing == Some(self.main_fiber_id) {
                            self.main_exit_code = 0;
                        }
                        // Fall back to the outer scheduler loop to pick the
                        // next runnable fiber (current is now None).
                        return Ok(None);
                    }
                    return Ok(Some(0));
                }
            }

            Opcode::Halt => {
                return Ok(Some(0));
            }

            _ => unreachable!("execute_control_flow called with non-control-flow opcode"),
        }
        Ok(None)
    }

    /// Type test and cast operations.
    fn execute_type(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Type operations
            Opcode::TypeIs => {
                let type_id = self.read_u8()?;
                let value = self.pop()?;
                let matches = match type_id {
                    0 => matches!(value, Value::Null),
                    1 => matches!(value, Value::Bool(_)),
                    2 => matches!(value, Value::Int(_)),
                    3 => matches!(value, Value::Float(_)),
                    4 => matches!(value, Value::String(_)),
                    5 => matches!(value, Value::Array(_)),
                    6 => matches!(value, Value::Object(_)),
                    7 => matches!(value, Value::Function(_) | Value::Closure(_)),
                    _ => false,
                };
                self.stack.push(Value::Bool(matches));
            }

            Opcode::Cast => {
                let type_id = self.read_u8()?;
                let value = self.pop()?;
                let result = match type_id {
                    // Cast to int
                    2 => match value {
                        Value::Int(n) => Value::Int(n),
                        Value::Float(f) => Value::Int(f as i64),
                        Value::Bool(b) => Value::Int(if b { 1 } else { 0 }),
                        Value::String(s) => Value::Int(s.parse().unwrap_or(0)),
                        _ => Value::Int(0),
                    },
                    // Cast to float
                    3 => match value {
                        Value::Int(n) => Value::Float(n as f64),
                        Value::Float(f) => Value::Float(f),
                        Value::Bool(b) => Value::Float(if b { 1.0 } else { 0.0 }),
                        Value::String(s) => Value::Float(s.parse().unwrap_or(0.0)),
                        _ => Value::Float(0.0),
                    },
                    // Cast to string
                    4 => Value::String(Rc::new(value.to_string())),
                    // Cast to bool
                    1 => Value::Bool(value.is_truthy()),
                    _ => value,
                };
                self.stack.push(result);
            }

            _ => unreachable!("execute_type called with non-type opcode"),
        }
        Ok(None)
    }

    /// System operations (print and syscall dispatch).
    fn execute_system(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // System operations
            Opcode::Print => {
                let value = self.pop()?;
                let output_str = value.to_string();

                // Call output callback if set
                if let Some(ref mut callback) = self.output_callback {
                    callback(&output_str);
                }

                if self.capture_output {
                    self.output.push(output_str);
                } else {
                    println!("{}", output_str);
                }
            }

            Opcode::Syscall => {
                let syscall_num = self.read_u8()?;
                self.handle_syscall(syscall_num)?;
            }

            _ => unreachable!("execute_system called with non-system opcode"),
        }
        Ok(None)
    }

    /// Fiber and channel operations.
    ///
    /// Kept together as a single unit so the scheduler/channel machinery can be
    /// evolved cohesively by later work.
    fn execute_fiber_channel(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Fiber operations
            Opcode::Spawn => {
                let code_offset = self.read_u16()? as usize;
                let arg_count = self.read_u8()? as usize;

                // Collect arguments from stack
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.pop()?);
                }
                args.reverse();

                // Create new fiber
                let fiber_id = self.scheduler.spawn_with_args(code_offset, args);
                self.stack.push(Value::Fiber(fiber_id));
            }

            Opcode::Yield => {
                // In non-fiber mode, yield is a no-op
                if self.fiber_mode {
                    // Save the running fiber's state, then park it. Leaving
                    // current == None hands control back to the outer
                    // scheduler loop in run(), which selects the next fiber.
                    self.save_fiber_state();
                    self.scheduler.yield_current();
                }
            }

            Opcode::FiberId => {
                let id = self.scheduler.current.unwrap_or(0);
                self.stack.push(Value::Int(id as i64));
            }

            // Channel operations
            Opcode::ChanNew => {
                let capacity = self.pop()?;
                let capacity = match capacity {
                    Value::Int(n) => n as usize,
                    _ => 0,
                };
                let channel_id = self.scheduler.create_channel(capacity);
                self.stack.push(Value::Channel(channel_id));
            }

            Opcode::ChanSend => {
                let value = self.pop()?;
                let channel = self.pop()?;

                match channel {
                    Value::Channel(channel_id) => {
                        if self.fiber_mode {
                            // Snapshot the resume state (ip is already past this
                            // instruction) into the current fiber before the
                            // send, in case it blocks and clears `current`.
                            let blocker = self.scheduler.current;
                            self.save_fiber_state();
                            match self.scheduler.channel_send(channel_id, value) {
                                Ok(true) => {
                                    // Sent immediately; this fiber keeps running,
                                    // so discard the snapshot by reloading it.
                                    if blocker.is_some() {
                                        self.load_fiber_state();
                                    }
                                }
                                Ok(false) => {
                                    // Blocked: channel_send cleared `current`.
                                    // The outer scheduler loop resumes the next
                                    // runnable fiber.
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            return Err("Channel send requires fiber mode".to_string());
                        }
                    }
                    _ => return Err("Cannot send on non-channel".to_string()),
                }
            }

            Opcode::ChanRecv => {
                let channel = self.pop()?;

                match channel {
                    Value::Channel(channel_id) => {
                        if self.fiber_mode {
                            // Snapshot resume state before the receive in case it
                            // blocks and clears `current`.
                            let blocker = self.scheduler.current;
                            self.save_fiber_state();
                            match self.scheduler.channel_receive(channel_id) {
                                Ok(Some((value, ok))) => {
                                    // Got a value immediately; this fiber keeps
                                    // running. Restore our snapshot and push the
                                    // received pair onto it.
                                    if blocker.is_some() {
                                        self.load_fiber_state();
                                    }
                                    self.stack.push(value);
                                    self.stack.push(Value::Bool(ok));
                                }
                                Ok(None) => {
                                    // Blocked: channel_receive cleared `current`.
                                    // The outer scheduler loop resumes the next
                                    // runnable fiber. When this fiber is later
                                    // woken, the sender/closer has already pushed
                                    // (value, ok) onto its saved stack.
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            return Err("Channel receive requires fiber mode".to_string());
                        }
                    }
                    _ => return Err("Cannot receive from non-channel".to_string()),
                }
            }

            Opcode::ChanClose => {
                let channel = self.pop()?;
                match channel {
                    Value::Channel(channel_id) => {
                        self.scheduler.close_channel(channel_id)?;
                    }
                    _ => return Err("Cannot close non-channel".to_string()),
                }
            }

            Opcode::ChanTryRecv => {
                let channel = self.pop()?;
                match channel {
                    Value::Channel(channel_id) => {
                        // Try non-blocking receive
                        // Returns the value if available, or null otherwise
                        if let Some(ch) = self.scheduler.channels.get_mut(&channel_id) {
                            if let Some(value) = ch.buffer.pop_front() {
                                self.stack.push(value);
                            } else {
                                self.stack.push(Value::Null);
                            }
                        } else {
                            return Err("Invalid channel".to_string());
                        }
                    }
                    _ => return Err("Cannot receive from non-channel".to_string()),
                }
            }

            Opcode::Select => {
                return self.execute_select();
            }

            _ => unreachable!("execute_fiber_channel called with non-fiber/channel opcode"),
        }
        Ok(None)
    }

    /// Execute a `Select` opcode.
    ///
    /// Encoding (operands follow the opcode byte; `self.ip` is positioned just
    /// past the opcode when this is called):
    /// ```text
    /// Select(0xA5) u8:arm_count  [arm_count × u8:tag]  [arm_count × i16:body_rel]
    /// ```
    /// Tags: 0 = recv, 1 = send, 2 = default. Each `body_rel` is relative to the
    /// byte just after that i16 (same convention as `patch_jump`). The per-arm
    /// operands are pushed onto the stack by codegen *before* the opcode, in arm
    /// order: recv pushes the channel; send pushes channel then value; default
    /// pushes nothing.
    ///
    /// Semantics (Go-like): poll all recv/send arms; the first ready one wins. A
    /// recv arm pushes the received value for the body to bind. If none is ready
    /// and a `default` arm exists, run it (non-blocking poll). Otherwise park the
    /// fiber until any arm becomes ready, then re-run this opcode and commit.
    fn execute_select(&mut self) -> Result<Option<i32>, String> {
        // The opcode byte was already consumed; remember its offset so a parked
        // select resumes by re-executing this very opcode.
        let select_ip = self.ip - 1;

        let arm_count = self.read_u8()? as usize;

        let mut tags = Vec::with_capacity(arm_count);
        for _ in 0..arm_count {
            tags.push(self.read_u8()?);
        }

        let mut body_targets = Vec::with_capacity(arm_count);
        for _ in 0..arm_count {
            let rel = self.read_i16()?;
            // Target is relative to the byte after this i16 field.
            let target = (self.ip as isize + rel as isize) as usize;
            body_targets.push(target);
        }

        // Pop per-arm operands in reverse arm order. Each arm records the channel
        // and (for send) the value, plus whether it is the default.
        enum Arm {
            Recv { channel: ChannelId },
            Send { channel: ChannelId, value: Value },
            Default,
        }

        let mut arms: Vec<Arm> = Vec::with_capacity(arm_count);
        // Build in reverse, then reverse to restore arm order.
        for tag in tags.iter().rev() {
            let arm = match tag {
                0 => {
                    let channel = match self.pop()? {
                        Value::Channel(id) => id,
                        _ => return Err("select recv arm requires a channel".to_string()),
                    };
                    Arm::Recv { channel }
                }
                1 => {
                    let value = self.pop()?;
                    let channel = match self.pop()? {
                        Value::Channel(id) => id,
                        _ => return Err("select send arm requires a channel".to_string()),
                    };
                    Arm::Send { channel, value }
                }
                2 => Arm::Default,
                _ => return Err("invalid select arm tag".to_string()),
            };
            arms.push(arm);
        }
        arms.reverse();

        // If a waker already resolved this select while it was parked, commit to
        // the recorded arm.
        let resolution = self
            .scheduler
            .current_fiber_mut()
            .and_then(|f| f.select_resolution.take());
        if let Some(res) = resolution {
            for (i, arm) in arms.iter().enumerate() {
                let matches = match arm {
                    Arm::Recv { channel } => res.recv.is_some() && *channel == res.channel_id,
                    Arm::Send { channel, .. } => res.recv.is_none() && *channel == res.channel_id,
                    Arm::Default => false,
                };
                if matches {
                    self.deregister_current_select_waiter();
                    if let Some((value, _ok)) = res.recv {
                        self.stack.push(value);
                    }
                    self.ip = body_targets[i];
                    return Ok(None);
                }
            }
            // Resolution did not match any arm (spurious); fall through to a
            // fresh poll below.
        }

        // Poll recv arms first.
        let recv_ids: Vec<ChannelId> = arms
            .iter()
            .filter_map(|a| match a {
                Arm::Recv { channel } => Some(*channel),
                _ => None,
            })
            .collect();
        if !recv_ids.is_empty() {
            if let Some((local_idx, value, _ok)) = self.scheduler.try_select(&recv_ids) {
                // Map the recv-local index back to the arm index.
                let arm_idx = arms
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| matches!(a, Arm::Recv { .. }))
                    .nth(local_idx)
                    .map(|(i, _)| i)
                    .expect("recv index out of range");
                self.deregister_current_select_waiter();
                self.stack.push(value);
                self.ip = body_targets[arm_idx];
                return Ok(None);
            }
        }

        // Poll send arms.
        for (i, arm) in arms.iter().enumerate() {
            if let Arm::Send { channel, value } = arm {
                if self.scheduler.try_select_send(*channel, value.clone()) {
                    self.deregister_current_select_waiter();
                    self.ip = body_targets[i];
                    return Ok(None);
                }
            }
        }

        // No arm ready. If a default exists, run it (non-blocking poll).
        if let Some((i, _)) = arms
            .iter()
            .enumerate()
            .find(|(_, a)| matches!(a, Arm::Default))
        {
            self.deregister_current_select_waiter();
            self.ip = body_targets[i];
            return Ok(None);
        }

        // No default: park until an arm becomes ready, then re-run this opcode.
        if !self.fiber_mode {
            return Err("select requires fiber mode".to_string());
        }

        let send_specs: Vec<(ChannelId, Value)> = arms
            .iter()
            .filter_map(|a| match a {
                Arm::Send { channel, value } => Some((*channel, value.clone())),
                _ => None,
            })
            .collect();

        // Re-push the per-arm operands so that when this fiber wakes and
        // re-executes the Select opcode (ip rewound below), they are on the
        // stack exactly as codegen left them before the opcode.
        for arm in arms.iter() {
            match arm {
                Arm::Recv { channel } => self.stack.push(Value::Channel(*channel)),
                Arm::Send { channel, value } => {
                    self.stack.push(Value::Channel(*channel));
                    self.stack.push(value.clone());
                }
                Arm::Default => {}
            }
        }

        // Resume by re-executing this Select opcode.
        self.ip = select_ip;
        self.save_fiber_state();
        self.scheduler.park_select(&recv_ids, &send_specs);
        Ok(None)
    }

    /// De-register the current fiber from every channel it registered on while
    /// parked in a `select`. Called when a woken select-waiter commits to one
    /// arm: the registrations on the *losing* arms must be purged so an
    /// abandoned send value cannot be delivered to a later receiver and an
    /// abandoned recv registration cannot wake (and corrupt) the already
    /// committed/finished fiber. A no-op for a select that never parked.
    fn deregister_current_select_waiter(&mut self) {
        if let Some(current_id) = self.scheduler.current {
            self.scheduler.deregister_select_waiter(current_id);
        }
    }

    /// Save current execution state to the current fiber
    fn save_fiber_state(&mut self) {
        if let Some(current_id) = self.scheduler.current {
            if let Some(fiber) = self.scheduler.fibers.get_mut(&current_id) {
                fiber.ip = self.ip;
                fiber.stack = std::mem::take(&mut self.stack);
                fiber.locals = std::mem::take(&mut self.locals);
            }
            // The native call stack lives outside the scheduler's Fiber, so
            // stash it here so the next fiber starts with its own frames.
            self.fiber_call_stacks
                .insert(current_id, std::mem::take(&mut self.call_stack));
        }
    }

    /// Load execution state from the current fiber
    fn load_fiber_state(&mut self) {
        if let Some(current_id) = self.scheduler.current {
            if let Some(fiber) = self.scheduler.fibers.get_mut(&current_id) {
                self.ip = fiber.ip;
                self.stack = std::mem::take(&mut fiber.stack);
                self.locals = std::mem::take(&mut fiber.locals);
            }
            self.call_stack = self
                .fiber_call_stacks
                .remove(&current_id)
                .unwrap_or_default();
        }
    }

    // Helper methods

    fn pop(&mut self) -> Result<Value, String> {
        let stack_base = self.stack_base();
        if self.stack.len() <= stack_base {
            return Err("Stack underflow".to_string());
        }
        self.stack
            .pop()
            .ok_or_else(|| "Stack underflow".to_string())
    }

    /// Get the stack base for the current frame (to protect caller's values)
    fn stack_base(&self) -> usize {
        self.call_stack.last().map(|f| f.stack_base).unwrap_or(0)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        if self.ip >= self.program.code.len() {
            return Err("Unexpected end of bytecode".to_string());
        }
        let value = self.program.code[self.ip];
        self.ip += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let lo = self.read_u8()? as u16;
        let hi = self.read_u8()? as u16;
        Ok(lo | (hi << 8))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        Ok(self.read_u16()? as i16)
    }

    fn locals_base(&self) -> usize {
        self.call_stack.last().map(|f| f.locals_base).unwrap_or(0)
    }

    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::String(a), Value::String(b)) => a == b,
            _ => false,
        }
    }

    fn compare_values(&self, a: &Value, b: &Value) -> Result<i32, String> {
        match (a, b) {
            (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b) as i32),
            (Value::Float(a), Value::Float(b)) => {
                Ok(a.partial_cmp(b).map(|o| o as i32).unwrap_or(0))
            }
            (Value::Int(a), Value::Float(b)) => {
                let a = *a as f64;
                Ok(a.partial_cmp(b).map(|o| o as i32).unwrap_or(0))
            }
            (Value::Float(a), Value::Int(b)) => {
                let b = *b as f64;
                Ok(a.partial_cmp(&b).map(|o| o as i32).unwrap_or(0))
            }
            (Value::String(a), Value::String(b)) => Ok(a.cmp(b) as i32),
            _ => Err(format!("Cannot compare {:?} and {:?}", a, b)),
        }
    }

    fn handle_syscall(&mut self, num: u8) -> Result<(), String> {
        match num {
            // sys_exit
            0 => {
                let code = self.pop()?;
                match code {
                    Value::Int(n) => std::process::exit(n as i32),
                    _ => std::process::exit(1),
                }
            }
            // sys_print
            1 => {
                let value = self.pop()?;
                self.runtime.print(&value);
                Ok(())
            }
            // sys_println
            2 => {
                let value = self.pop()?;
                self.runtime.println(&value);
                Ok(())
            }
            // sys_read_line
            3 => {
                let line = self.runtime.read_line()?;
                self.stack.push(Value::String(Rc::new(line)));
                Ok(())
            }
            // sys_time_ms
            4 => {
                let ms = self.runtime.current_time_millis();
                self.stack.push(Value::Int(ms));
                Ok(())
            }
            // sys_sleep_ms
            5 => {
                let millis = self.pop()?;
                match millis {
                    Value::Int(ms) => {
                        self.runtime.sleep(ms);
                        Ok(())
                    }
                    _ => Err("sleep requires integer milliseconds".to_string()),
                }
            }
            // time_secs() -> int
            6 => {
                let secs = self.runtime.current_time_secs();
                self.stack.push(Value::Int(secs));
                Ok(())
            }
            // time_micros() -> int
            7 => {
                let micros = self.runtime.current_time_micros();
                self.stack.push(Value::Int(micros));
                Ok(())
            }
            // time_nanos() -> int
            8 => {
                let nanos = self.runtime.current_time_nanos();
                self.stack.push(Value::Int(nanos));
                Ok(())
            }

            // ================================================================
            // File I/O syscalls (10-19)
            // ================================================================

            // file_open(path: string, mode: int) -> int
            10 => {
                let mode = self.pop()?;
                let path = self.pop()?;
                match (path, mode) {
                    (Value::String(path), Value::Int(mode)) => {
                        match self.runtime.file_open(&path, mode) {
                            Ok(fd) => {
                                self.stack.push(Value::Int(fd));
                                Ok(())
                            }
                            Err(e) => {
                                // Return -1 on error and push error message
                                self.stack.push(Value::Int(-1));
                                eprintln!("file_open error: {}", e);
                                Ok(())
                            }
                        }
                    }
                    _ => Err("file_open requires (string, int) arguments".to_string()),
                }
            }
            // file_read(fd: int, max_bytes: int) -> string
            11 => {
                let max_bytes = self.pop()?;
                let fd = self.pop()?;
                match (fd, max_bytes) {
                    (Value::Int(fd), Value::Int(max_bytes)) => {
                        match self.runtime.file_read(fd, max_bytes) {
                            Ok(data) => {
                                self.stack.push(Value::String(Rc::new(data)));
                                Ok(())
                            }
                            Err(e) => {
                                self.stack.push(Value::String(Rc::new(String::new())));
                                eprintln!("file_read error: {}", e);
                                Ok(())
                            }
                        }
                    }
                    _ => Err("file_read requires (int, int) arguments".to_string()),
                }
            }
            // file_write(fd: int, data: string) -> int
            12 => {
                let data = self.pop()?;
                let fd = self.pop()?;
                match (fd, data) {
                    (Value::Int(fd), Value::String(data)) => {
                        match self.runtime.file_write(fd, &data) {
                            Ok(bytes) => {
                                self.stack.push(Value::Int(bytes));
                                Ok(())
                            }
                            Err(e) => {
                                self.stack.push(Value::Int(-1));
                                eprintln!("file_write error: {}", e);
                                Ok(())
                            }
                        }
                    }
                    _ => Err("file_write requires (int, string) arguments".to_string()),
                }
            }
            // file_close(fd: int) -> bool
            13 => {
                let fd = self.pop()?;
                match fd {
                    Value::Int(fd) => {
                        let success = self.runtime.file_close(fd).is_ok();
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("file_close requires int argument".to_string()),
                }
            }
            // file_exists(path: string) -> bool
            14 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let exists = self.runtime.file_exists(&path);
                        self.stack.push(Value::Bool(exists));
                        Ok(())
                    }
                    _ => Err("file_exists requires string argument".to_string()),
                }
            }
            // file_size(path: string) -> int
            15 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let size = self.runtime.file_size(&path).unwrap_or(-1);
                        self.stack.push(Value::Int(size));
                        Ok(())
                    }
                    _ => Err("file_size requires string argument".to_string()),
                }
            }

            // ================================================================
            // Environment syscalls (20-29)
            // ================================================================

            // env_get(name: string) -> string (or null)
            20 => {
                let name = self.pop()?;
                match name {
                    Value::String(name) => {
                        let value = self.runtime.env_get(&name);
                        self.stack.push(match value {
                            Some(v) => Value::String(Rc::new(v)),
                            None => Value::Null,
                        });
                        Ok(())
                    }
                    _ => Err("env_get requires string argument".to_string()),
                }
            }
            // env_args() -> array of strings
            21 => {
                let args = self.runtime.env_args();
                let arr: Vec<Value> = args
                    .into_iter()
                    .map(|s| Value::String(Rc::new(s)))
                    .collect();
                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                Ok(())
            }

            // ================================================================
            // Extended environment syscalls (200-207)
            // ================================================================

            // env_set(name: string, value: string) -> bool
            200 => {
                let value = self.pop()?;
                let name = self.pop()?;
                match (name, value) {
                    (Value::String(name), Value::String(value)) => {
                        let result = self.runtime.env_set(&name, &value);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("env_set requires (string, string) arguments".to_string()),
                }
            }
            // env_remove(name: string) -> bool
            201 => {
                let name = self.pop()?;
                match name {
                    Value::String(name) => {
                        let result = self.runtime.env_remove(&name);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("env_remove requires string argument".to_string()),
                }
            }
            // env_all() -> [string] (key=value pairs)
            202 => {
                let vars = self.runtime.env_all();
                let arr: Vec<Value> = vars
                    .into_iter()
                    .map(|s| Value::String(Rc::new(s)))
                    .collect();
                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                Ok(())
            }
            // env_keys() -> [string]
            203 => {
                let keys = self.runtime.env_keys();
                let arr: Vec<Value> = keys
                    .into_iter()
                    .map(|s| Value::String(Rc::new(s)))
                    .collect();
                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                Ok(())
            }
            // env_has(name: string) -> bool
            204 => {
                let name = self.pop()?;
                match name {
                    Value::String(name) => {
                        let result = self.runtime.env_has(&name);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("env_has requires string argument".to_string()),
                }
            }
            // env_exe() -> string
            205 => {
                let path = self.runtime.env_exe();
                self.stack.push(Value::String(Rc::new(path)));
                Ok(())
            }
            // env_temp_dir() -> string
            206 => {
                let path = self.runtime.env_temp_dir();
                self.stack.push(Value::String(Rc::new(path)));
                Ok(())
            }
            // env_home_dir() -> string
            207 => {
                let path = self.runtime.env_home_dir();
                self.stack.push(Value::String(Rc::new(path)));
                Ok(())
            }

            // ================================================================
            // String operation syscalls (30-39)
            // ================================================================

            // str_char_code(str: string, index: int) -> int
            30 => {
                let index = self.pop()?;
                let s = self.pop()?;
                match (s, index) {
                    (Value::String(s), Value::Int(idx)) => {
                        let code = self.runtime.str_char_code(&s, idx);
                        self.stack.push(Value::Int(code));
                        Ok(())
                    }
                    _ => Err("str_char_code requires (string, int) arguments".to_string()),
                }
            }
            // str_from_char_code(code: int) -> string
            31 => {
                let code = self.pop()?;
                match code {
                    Value::Int(code) => {
                        let s = self.runtime.str_from_char_code(code);
                        self.stack.push(Value::String(Rc::new(s)));
                        Ok(())
                    }
                    _ => Err("str_from_char_code requires int argument".to_string()),
                }
            }
            // str_to_upper(str: string) -> string
            32 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.str_to_upper(&s);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_to_upper requires string argument".to_string()),
                }
            }
            // str_to_lower(str: string) -> string
            33 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.str_to_lower(&s);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_to_lower requires string argument".to_string()),
                }
            }
            // str_substring(str: string, start: int, end: int) -> string
            34 => {
                let end = self.pop()?;
                let start = self.pop()?;
                let s = self.pop()?;
                match (s, start, end) {
                    (Value::String(s), Value::Int(start), Value::Int(end)) => {
                        let result = self.runtime.str_substring(&s, start, end);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_substring requires (string, int, int) arguments".to_string()),
                }
            }
            // str_index_of(str: string, substr: string) -> int
            35 => {
                let substr = self.pop()?;
                let s = self.pop()?;
                match (s, substr) {
                    (Value::String(s), Value::String(substr)) => {
                        let idx = self.runtime.str_index_of(&s, &substr);
                        self.stack.push(Value::Int(idx));
                        Ok(())
                    }
                    _ => Err("str_index_of requires (string, string) arguments".to_string()),
                }
            }
            // str_split(str: string, delimiter: string) -> [string]
            36 => {
                let delimiter = self.pop()?;
                let s = self.pop()?;
                match (s, delimiter) {
                    (Value::String(s), Value::String(delimiter)) => {
                        let parts = self.runtime.str_split(&s, &delimiter);
                        let arr: Vec<Value> = parts
                            .into_iter()
                            .map(|s| Value::String(Rc::new(s)))
                            .collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("str_split requires (string, string) arguments".to_string()),
                }
            }
            // str_trim(str: string) -> string
            37 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.str_trim(&s);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_trim requires string argument".to_string()),
                }
            }
            // str_trim_start(str: string) -> string
            38 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.str_trim_start(&s);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_trim_start requires string argument".to_string()),
                }
            }
            // str_trim_end(str: string) -> string
            39 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.str_trim_end(&s);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("str_trim_end requires string argument".to_string()),
                }
            }

            // ================================================================
            // Random number generation syscalls (40-41)
            // ================================================================

            // random_float() -> float (0.0 to 1.0)
            40 => {
                let value = self.runtime.random_float();
                self.stack.push(Value::Float(value));
                Ok(())
            }
            // random_int(min: int, max: int) -> int
            41 => {
                let max = self.pop()?;
                let min = self.pop()?;
                match (min, max) {
                    (Value::Int(min), Value::Int(max)) => {
                        let value = self.runtime.random_int(min, max);
                        self.stack.push(Value::Int(value));
                        Ok(())
                    }
                    _ => Err("random_int requires (int, int) arguments".to_string()),
                }
            }

            // ================================================================
            // Base64 encoding/decoding syscalls (60-63)
            // ================================================================

            // base64_encode(input: string) -> string
            60 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let encoded = self.runtime.base64_encode(&s);
                        self.stack.push(Value::String(Rc::new(encoded)));
                        Ok(())
                    }
                    _ => Err("base64_encode requires string argument".to_string()),
                }
            }
            // base64_decode(input: string) -> string
            61 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        match self.runtime.base64_decode(&s) {
                            Ok(decoded) => {
                                self.stack.push(Value::String(Rc::new(decoded)));
                                Ok(())
                            }
                            Err(e) => {
                                // Push empty string on decode error
                                self.stack.push(Value::String(Rc::new(String::new())));
                                eprintln!("base64_decode error: {}", e);
                                Ok(())
                            }
                        }
                    }
                    _ => Err("base64_decode requires string argument".to_string()),
                }
            }
            // base64_encode_url(input: string) -> string
            62 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let encoded = self.runtime.base64_encode_url(&s);
                        self.stack.push(Value::String(Rc::new(encoded)));
                        Ok(())
                    }
                    _ => Err("base64_encode_url requires string argument".to_string()),
                }
            }
            // base64_decode_url(input: string) -> string
            63 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        match self.runtime.base64_decode_url(&s) {
                            Ok(decoded) => {
                                self.stack.push(Value::String(Rc::new(decoded)));
                                Ok(())
                            }
                            Err(e) => {
                                // Push empty string on decode error
                                self.stack.push(Value::String(Rc::new(String::new())));
                                eprintln!("base64_decode_url error: {}", e);
                                Ok(())
                            }
                        }
                    }
                    _ => Err("base64_decode_url requires string argument".to_string()),
                }
            }

            // ================================================================
            // Cryptographic hash syscalls (70-73)
            // ================================================================

            // md5(input: string) -> string (hex)
            70 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let hash = self.runtime.hash_md5(&s);
                        self.stack.push(Value::String(Rc::new(hash)));
                        Ok(())
                    }
                    _ => Err("md5 requires string argument".to_string()),
                }
            }
            // sha1(input: string) -> string (hex)
            71 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let hash = self.runtime.hash_sha1(&s);
                        self.stack.push(Value::String(Rc::new(hash)));
                        Ok(())
                    }
                    _ => Err("sha1 requires string argument".to_string()),
                }
            }
            // sha256(input: string) -> string (hex)
            72 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let hash = self.runtime.hash_sha256(&s);
                        self.stack.push(Value::String(Rc::new(hash)));
                        Ok(())
                    }
                    _ => Err("sha256 requires string argument".to_string()),
                }
            }
            // sha512(input: string) -> string (hex)
            73 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let hash = self.runtime.hash_sha512(&s);
                        self.stack.push(Value::String(Rc::new(hash)));
                        Ok(())
                    }
                    _ => Err("sha512 requires string argument".to_string()),
                }
            }

            // ================================================================
            // JSON syscalls (50-52)
            // ================================================================

            // json_parse(json_str: string) -> value
            50 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => match self.runtime.json_parse(&s) {
                        Ok(value) => {
                            self.stack.push(value);
                            Ok(())
                        }
                        Err(e) => {
                            // Push null on parse error
                            self.stack.push(Value::Null);
                            eprintln!("json_parse error: {}", e);
                            Ok(())
                        }
                    },
                    _ => Err("json_parse requires string argument".to_string()),
                }
            }
            // json_stringify(value) -> string
            51 => {
                let value = self.pop()?;
                let json_str = self.runtime.json_stringify(&value);
                self.stack.push(Value::String(Rc::new(json_str)));
                Ok(())
            }
            // json_stringify_pretty(value) -> string
            52 => {
                let value = self.pop()?;
                let json_str = self.runtime.json_stringify_pretty(&value);
                self.stack.push(Value::String(Rc::new(json_str)));
                Ok(())
            }

            // ================================================================
            // TCP Networking syscalls (80-84)
            // ================================================================

            // tcp_connect(host: string, port: int) -> int (socket id or -1)
            80 => {
                let port = self.pop()?;
                let host = self.pop()?;
                match (host, port) {
                    (Value::String(host), Value::Int(port)) => {
                        let socket_id = self.runtime.tcp_connect(&host, port);
                        self.stack.push(Value::Int(socket_id));
                        Ok(())
                    }
                    _ => Err("tcp_connect requires (string, int) arguments".to_string()),
                }
            }
            // tcp_write(socket_id: int, data: string) -> int (bytes written or -1)
            81 => {
                let data = self.pop()?;
                let socket_id = self.pop()?;
                match (socket_id, data) {
                    (Value::Int(socket_id), Value::String(data)) => {
                        let bytes = self.runtime.tcp_write(socket_id, &data);
                        self.stack.push(Value::Int(bytes));
                        Ok(())
                    }
                    _ => Err("tcp_write requires (int, string) arguments".to_string()),
                }
            }
            // tcp_read(socket_id: int, max_bytes: int) -> string
            82 => {
                let max_bytes = self.pop()?;
                let socket_id = self.pop()?;
                match (socket_id, max_bytes) {
                    (Value::Int(socket_id), Value::Int(max_bytes)) => {
                        let data = self.runtime.tcp_read(socket_id, max_bytes);
                        self.stack.push(Value::String(Rc::new(data)));
                        Ok(())
                    }
                    _ => Err("tcp_read requires (int, int) arguments".to_string()),
                }
            }
            // tcp_close(socket_id: int) -> bool
            83 => {
                let socket_id = self.pop()?;
                match socket_id {
                    Value::Int(socket_id) => {
                        let success = self.runtime.tcp_close(socket_id);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("tcp_close requires int argument".to_string()),
                }
            }
            // dns_lookup(hostname: string) -> string (IP address or empty)
            84 => {
                let hostname = self.pop()?;
                match hostname {
                    Value::String(hostname) => {
                        let ip = self.runtime.dns_lookup(&hostname);
                        self.stack.push(Value::String(Rc::new(ip)));
                        Ok(())
                    }
                    _ => Err("dns_lookup requires string argument".to_string()),
                }
            }

            // ================================================================
            // OS syscalls (90-101)
            // ================================================================

            // getcwd() -> string
            90 => {
                let cwd = self.runtime.os_getcwd();
                self.stack.push(Value::String(Rc::new(cwd)));
                Ok(())
            }
            // chdir(path: string) -> bool
            91 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_chdir(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("chdir requires string argument".to_string()),
                }
            }
            // mkdir(path: string) -> bool
            92 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_mkdir(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("mkdir requires string argument".to_string()),
                }
            }
            // mkdir_all(path: string) -> bool
            93 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_mkdir_all(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("mkdir_all requires string argument".to_string()),
                }
            }
            // rmdir(path: string) -> bool
            94 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_rmdir(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("rmdir requires string argument".to_string()),
                }
            }
            // remove(path: string) -> bool
            95 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_remove(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("remove requires string argument".to_string()),
                }
            }
            // remove_all(path: string) -> bool
            96 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let success = self.runtime.os_remove_all(&path);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("remove_all requires string argument".to_string()),
                }
            }
            // listdir(path: string) -> [string]
            97 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let entries = self.runtime.os_listdir(&path);
                        let arr: Vec<Value> = entries
                            .into_iter()
                            .map(|s| Value::String(Rc::new(s)))
                            .collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("listdir requires string argument".to_string()),
                }
            }
            // is_dir(path: string) -> bool
            98 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let is_dir = self.runtime.os_is_dir(&path);
                        self.stack.push(Value::Bool(is_dir));
                        Ok(())
                    }
                    _ => Err("is_dir requires string argument".to_string()),
                }
            }
            // is_file(path: string) -> bool
            99 => {
                let path = self.pop()?;
                match path {
                    Value::String(path) => {
                        let is_file = self.runtime.os_is_file(&path);
                        self.stack.push(Value::Bool(is_file));
                        Ok(())
                    }
                    _ => Err("is_file requires string argument".to_string()),
                }
            }
            // rename(from: string, to: string) -> bool
            100 => {
                let to = self.pop()?;
                let from = self.pop()?;
                match (from, to) {
                    (Value::String(from), Value::String(to)) => {
                        let success = self.runtime.os_rename(&from, &to);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("rename requires (string, string) arguments".to_string()),
                }
            }
            // copy(from: string, to: string) -> bool
            101 => {
                let to = self.pop()?;
                let from = self.pop()?;
                match (from, to) {
                    (Value::String(from), Value::String(to)) => {
                        let success = self.runtime.os_copy(&from, &to);
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("copy requires (string, string) arguments".to_string()),
                }
            }

            // ================================================================
            // URL encoding/decoding syscalls (110-111)
            // ================================================================

            // url_encode(input: string) -> string
            110 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let encoded = self.runtime.url_encode(&s);
                        self.stack.push(Value::String(Rc::new(encoded)));
                        Ok(())
                    }
                    _ => Err("url_encode requires string argument".to_string()),
                }
            }
            // url_decode(input: string) -> string
            111 => {
                let input = self.pop()?;
                match input {
                    Value::String(s) => {
                        let decoded = self.runtime.url_decode(&s);
                        self.stack.push(Value::String(Rc::new(decoded)));
                        Ok(())
                    }
                    _ => Err("url_decode requires string argument".to_string()),
                }
            }

            // ================================================================
            // HTTP Client syscalls (120-122)
            // ================================================================

            // http_get(url: string) -> [int, string] (status, body)
            120 => {
                let url = self.pop()?;
                match url {
                    Value::String(url) => {
                        match self.runtime.http_get(&url) {
                            Ok((status, _headers, body)) => {
                                // Return array [status, body]
                                let arr = vec![Value::Int(status), Value::String(Rc::new(body))];
                                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                                Ok(())
                            }
                            Err(e) => {
                                // Return error as [-1, error_message]
                                let arr = vec![Value::Int(-1), Value::String(Rc::new(e))];
                                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                                Ok(())
                            }
                        }
                    }
                    _ => Err("http_get requires string argument".to_string()),
                }
            }
            // http_post(url: string, body: string, content_type: string) -> [int, string]
            121 => {
                let content_type = self.pop()?;
                let body = self.pop()?;
                let url = self.pop()?;
                match (url, body, content_type) {
                    (Value::String(url), Value::String(body), Value::String(content_type)) => {
                        match self.runtime.http_post(&url, &body, &content_type) {
                            Ok((status, _headers, response_body)) => {
                                let arr =
                                    vec![Value::Int(status), Value::String(Rc::new(response_body))];
                                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                                Ok(())
                            }
                            Err(e) => {
                                let arr = vec![Value::Int(-1), Value::String(Rc::new(e))];
                                self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                                Ok(())
                            }
                        }
                    }
                    _ => Err("http_post requires (string, string, string) arguments".to_string()),
                }
            }
            // http_request(method: string, url: string, headers: string, body: string) -> [int, string]
            122 => {
                let body = self.pop()?;
                let headers = self.pop()?;
                let url = self.pop()?;
                let method = self.pop()?;
                match (method, url, headers, body) {
                    (
                        Value::String(method),
                        Value::String(url),
                        Value::String(headers),
                        Value::String(body),
                    ) => match self.runtime.http_request(&method, &url, &headers, &body) {
                        Ok((status, response_body)) => {
                            let arr =
                                vec![Value::Int(status), Value::String(Rc::new(response_body))];
                            self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                            Ok(())
                        }
                        Err(e) => {
                            let arr = vec![Value::Int(-1), Value::String(Rc::new(e))];
                            self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                            Ok(())
                        }
                    },
                    _ => Err(
                        "http_request requires (string, string, string, string) arguments"
                            .to_string(),
                    ),
                }
            }

            // ================================================================
            // Math syscalls (140-163)
            // ================================================================

            // sqrt(x: float) -> float
            140 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("sqrt requires numeric argument".to_string()),
                };
                let result = self.runtime.math_sqrt(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // pow(base: float, exp: float) -> float
            141 => {
                let exp = self.pop()?;
                let base = self.pop()?;
                let base = match base {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("pow requires numeric arguments".to_string()),
                };
                let exp = match exp {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("pow requires numeric arguments".to_string()),
                };
                let result = self.runtime.math_pow(base, exp);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // exp(x: float) -> float
            142 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("exp requires numeric argument".to_string()),
                };
                let result = self.runtime.math_exp(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // ln(x: float) -> float
            143 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("ln requires numeric argument".to_string()),
                };
                let result = self.runtime.math_ln(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // log10(x: float) -> float
            144 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("log10 requires numeric argument".to_string()),
                };
                let result = self.runtime.math_log10(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // log2(x: float) -> float
            145 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("log2 requires numeric argument".to_string()),
                };
                let result = self.runtime.math_log2(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // sin(x: float) -> float
            146 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("sin requires numeric argument".to_string()),
                };
                let result = self.runtime.math_sin(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // cos(x: float) -> float
            147 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("cos requires numeric argument".to_string()),
                };
                let result = self.runtime.math_cos(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // tan(x: float) -> float
            148 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("tan requires numeric argument".to_string()),
                };
                let result = self.runtime.math_tan(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // asin(x: float) -> float
            149 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("asin requires numeric argument".to_string()),
                };
                let result = self.runtime.math_asin(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // acos(x: float) -> float
            150 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("acos requires numeric argument".to_string()),
                };
                let result = self.runtime.math_acos(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // atan(x: float) -> float
            151 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("atan requires numeric argument".to_string()),
                };
                let result = self.runtime.math_atan(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // atan2(y: float, x: float) -> float
            152 => {
                let x = self.pop()?;
                let y = self.pop()?;
                let y = match y {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("atan2 requires numeric arguments".to_string()),
                };
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("atan2 requires numeric arguments".to_string()),
                };
                let result = self.runtime.math_atan2(y, x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // sinh(x: float) -> float
            153 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("sinh requires numeric argument".to_string()),
                };
                let result = self.runtime.math_sinh(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // cosh(x: float) -> float
            154 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("cosh requires numeric argument".to_string()),
                };
                let result = self.runtime.math_cosh(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // tanh(x: float) -> float
            155 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("tanh requires numeric argument".to_string()),
                };
                let result = self.runtime.math_tanh(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // floor(x: float) -> float
            156 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("floor requires numeric argument".to_string()),
                };
                let result = self.runtime.math_floor(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // ceil(x: float) -> float
            157 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("ceil requires numeric argument".to_string()),
                };
                let result = self.runtime.math_ceil(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // round(x: float) -> float
            158 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("round requires numeric argument".to_string()),
                };
                let result = self.runtime.math_round(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // trunc(x: float) -> float
            159 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("trunc requires numeric argument".to_string()),
                };
                let result = self.runtime.math_trunc(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }
            // is_nan(x: float) -> bool
            160 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("is_nan requires numeric argument".to_string()),
                };
                let result = self.runtime.math_is_nan(x);
                self.stack.push(Value::Bool(result));
                Ok(())
            }
            // is_infinite(x: float) -> bool
            161 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("is_infinite requires numeric argument".to_string()),
                };
                let result = self.runtime.math_is_infinite(x);
                self.stack.push(Value::Bool(result));
                Ok(())
            }
            // is_finite(x: float) -> bool
            162 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("is_finite requires numeric argument".to_string()),
                };
                let result = self.runtime.math_is_finite(x);
                self.stack.push(Value::Bool(result));
                Ok(())
            }
            // abs(x: float) -> float
            163 => {
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err("abs requires numeric argument".to_string()),
                };
                let result = self.runtime.math_abs_float(x);
                self.stack.push(Value::Float(result));
                Ok(())
            }

            // ================================================================
            // Extended Time syscalls (130-135)
            // ================================================================

            // time_format_iso(timestamp_ms: int) -> string
            130 => {
                let timestamp_ms = self.pop()?;
                match timestamp_ms {
                    Value::Int(ms) => {
                        let result = self.runtime.time_format_iso(ms);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("time_format_iso requires int argument".to_string()),
                }
            }
            // time_format(timestamp_ms: int, format: string) -> string
            131 => {
                let format = self.pop()?;
                let timestamp_ms = self.pop()?;
                match (timestamp_ms, format) {
                    (Value::Int(ms), Value::String(fmt)) => {
                        let result = self.runtime.time_format(ms, &fmt);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("time_format requires (int, string) arguments".to_string()),
                }
            }
            // time_parse_iso(string) -> int
            132 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.time_parse_iso(&s);
                        self.stack.push(Value::Int(result));
                        Ok(())
                    }
                    _ => Err("time_parse_iso requires string argument".to_string()),
                }
            }
            // time_timezone_offset() -> int
            133 => {
                let offset = self.runtime.time_timezone_offset();
                self.stack.push(Value::Int(offset));
                Ok(())
            }
            // time_components(timestamp_ms: int) -> [int]
            134 => {
                let timestamp_ms = self.pop()?;
                match timestamp_ms {
                    Value::Int(ms) => {
                        let components = self.runtime.time_components(ms);
                        let arr: Vec<Value> = components.into_iter().map(Value::Int).collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("time_components requires int argument".to_string()),
                }
            }
            // time_from_components(year, month, day, hour, min, sec) -> int
            135 => {
                let sec = self.pop()?;
                let min = self.pop()?;
                let hour = self.pop()?;
                let day = self.pop()?;
                let month = self.pop()?;
                let year = self.pop()?;
                match (year, month, day, hour, min, sec) {
                    (
                        Value::Int(y),
                        Value::Int(mo),
                        Value::Int(d),
                        Value::Int(h),
                        Value::Int(mi),
                        Value::Int(s),
                    ) => {
                        let result = self.runtime.time_from_components(y, mo, d, h, mi, s);
                        self.stack.push(Value::Int(result));
                        Ok(())
                    }
                    _ => Err("time_from_components requires 6 int arguments".to_string()),
                }
            }

            // ================================================================
            // Regex syscalls (170-177)
            // ================================================================

            // regex_match(pattern: string, text: string) -> bool
            170 => {
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text) {
                    (Value::String(pattern), Value::String(text)) => {
                        let result = self.runtime.regex_match(&pattern, &text);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("regex_match requires (string, string) arguments".to_string()),
                }
            }
            // regex_find(pattern: string, text: string) -> string
            171 => {
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text) {
                    (Value::String(pattern), Value::String(text)) => {
                        let result = self.runtime.regex_find(&pattern, &text);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err("regex_find requires (string, string) arguments".to_string()),
                }
            }
            // regex_find_all(pattern: string, text: string) -> [string]
            172 => {
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text) {
                    (Value::String(pattern), Value::String(text)) => {
                        let matches = self.runtime.regex_find_all(&pattern, &text);
                        let arr: Vec<Value> = matches
                            .into_iter()
                            .map(|s| Value::String(Rc::new(s)))
                            .collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("regex_find_all requires (string, string) arguments".to_string()),
                }
            }
            // regex_replace(pattern: string, text: string, replacement: string) -> string
            173 => {
                let replacement = self.pop()?;
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text, replacement) {
                    (Value::String(pattern), Value::String(text), Value::String(replacement)) => {
                        let result = self.runtime.regex_replace(&pattern, &text, &replacement);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => {
                        Err("regex_replace requires (string, string, string) arguments".to_string())
                    }
                }
            }
            // regex_replace_all(pattern: string, text: string, replacement: string) -> string
            174 => {
                let replacement = self.pop()?;
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text, replacement) {
                    (Value::String(pattern), Value::String(text), Value::String(replacement)) => {
                        let result = self
                            .runtime
                            .regex_replace_all(&pattern, &text, &replacement);
                        self.stack.push(Value::String(Rc::new(result)));
                        Ok(())
                    }
                    _ => Err(
                        "regex_replace_all requires (string, string, string) arguments".to_string(),
                    ),
                }
            }
            // regex_split(pattern: string, text: string) -> [string]
            175 => {
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text) {
                    (Value::String(pattern), Value::String(text)) => {
                        let parts = self.runtime.regex_split(&pattern, &text);
                        let arr: Vec<Value> = parts
                            .into_iter()
                            .map(|s| Value::String(Rc::new(s)))
                            .collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("regex_split requires (string, string) arguments".to_string()),
                }
            }
            // regex_captures(pattern: string, text: string) -> [string]
            176 => {
                let text = self.pop()?;
                let pattern = self.pop()?;
                match (pattern, text) {
                    (Value::String(pattern), Value::String(text)) => {
                        let captures = self.runtime.regex_captures(&pattern, &text);
                        let arr: Vec<Value> = captures
                            .into_iter()
                            .map(|s| Value::String(Rc::new(s)))
                            .collect();
                        self.stack.push(Value::Array(Rc::new(RefCell::new(arr))));
                        Ok(())
                    }
                    _ => Err("regex_captures requires (string, string) arguments".to_string()),
                }
            }
            // regex_is_valid(pattern: string) -> bool
            177 => {
                let pattern = self.pop()?;
                match pattern {
                    Value::String(pattern) => {
                        let result = self.runtime.regex_is_valid(&pattern);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("regex_is_valid requires string argument".to_string()),
                }
            }

            // ================================================================
            // UUID syscalls (190-193)
            // ================================================================

            // uuid_v4() -> string
            190 => {
                let uuid = self.runtime.uuid_v4();
                self.stack.push(Value::String(Rc::new(uuid)));
                Ok(())
            }
            // uuid_v7() -> string
            191 => {
                let uuid = self.runtime.uuid_v7();
                self.stack.push(Value::String(Rc::new(uuid)));
                Ok(())
            }
            // uuid_is_valid(s: string) -> bool
            192 => {
                let s = self.pop()?;
                match s {
                    Value::String(s) => {
                        let result = self.runtime.uuid_is_valid(&s);
                        self.stack.push(Value::Bool(result));
                        Ok(())
                    }
                    _ => Err("uuid_is_valid requires string argument".to_string()),
                }
            }
            // uuid_nil() -> string
            193 => {
                let uuid = self.runtime.uuid_nil();
                self.stack.push(Value::String(Rc::new(uuid)));
                Ok(())
            }

            _ => Err(format!("Unknown syscall: {}", num)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_program(constants: Vec<Value>, code: Vec<u8>) -> Program {
        use lira_core::bytecode::DebugInfo;
        Program {
            constants,
            code,
            entry_point: 0,
            functions: Vec::new(),
            debug_info: DebugInfo::new(),
            source_file: None,
        }
    }

    #[test]
    fn test_add_integers() {
        let program = make_program(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 10
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 20
                Opcode::Add as u8,   // Add
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_comparison() {
        let program = make_program(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 10
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 20
                Opcode::Lt as u8,    // 10 < 20
                Opcode::Print as u8, // Print true
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_local_variables() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 42
                Opcode::StoreLocal as u8,
                0,
                0, // Store in local 0
                Opcode::LoadLocal as u8,
                0,
                0,                   // Load from local 0
                Opcode::Print as u8, // Print 42
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_subtraction() {
        let program = make_program(
            vec![Value::Int(50), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 50
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 20
                Opcode::Sub as u8,   // 50 - 20 = 30
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiplication() {
        let program = make_program(
            vec![Value::Int(7), Value::Int(6)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 7
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 6
                Opcode::Mul as u8,   // 7 * 6 = 42
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_division() {
        let program = make_program(
            vec![Value::Int(42), Value::Int(6)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 42
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 6
                Opcode::Div as u8,   // 42 / 6 = 7
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_modulo() {
        let program = make_program(
            vec![Value::Int(10), Value::Int(3)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 10
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 3
                Opcode::Mod as u8,   // 10 % 3 = 1
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_negation() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,                   // Load 42
                Opcode::Neg as u8,   // -42
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_bitwise_and() {
        let program = make_program(
            vec![Value::Int(0b1100), Value::Int(0b1010)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 12
                Opcode::LoadConst as u8,
                1,
                0,                    // Load 10
                Opcode::BitAnd as u8, // 12 & 10 = 8
                Opcode::Print as u8,  // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_bitwise_or() {
        let program = make_program(
            vec![Value::Int(0b1100), Value::Int(0b1010)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 12
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 10
                Opcode::BitOr as u8, // 12 | 10 = 14
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_bitwise_xor() {
        let program = make_program(
            vec![Value::Int(0b1100), Value::Int(0b1010)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 12
                Opcode::LoadConst as u8,
                1,
                0,                    // Load 10
                Opcode::BitXor as u8, // 12 ^ 10 = 6
                Opcode::Print as u8,  // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_shift_left() {
        let program = make_program(
            vec![Value::Int(1), Value::Int(4)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 1
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 4
                Opcode::Shl as u8,   // 1 << 4 = 16
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_shift_right() {
        let program = make_program(
            vec![Value::Int(16), Value::Int(2)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 16
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 2
                Opcode::Shr as u8,   // 16 >> 2 = 4
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_logical_not() {
        let program = make_program(
            vec![Value::Bool(true)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,                   // Load true
                Opcode::Not as u8,   // !true = false
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_dup_stack() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,                   // Load 42
                Opcode::Dup as u8,   // Duplicate
                Opcode::Print as u8, // Print 42
                Opcode::Print as u8, // Print 42 again
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_new_array() {
        let program = make_program(
            vec![Value::Int(3), Value::Int(1), Value::Int(2), Value::Int(3)],
            vec![
                Opcode::LoadConst as u8,
                1,
                0, // Load 1
                Opcode::LoadConst as u8,
                2,
                0, // Load 2
                Opcode::LoadConst as u8,
                3,
                0, // Load 3
                Opcode::LoadConst as u8,
                0,
                0,                      // Load size (3)
                Opcode::NewArray as u8, // Create array [1, 2, 3]
                Opcode::Print as u8,    // Print array
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_jump_if_false() {
        // Skip printing 42 if false
        let program = make_program(
            vec![Value::Bool(false), Value::Int(42), Value::Int(100)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load false
                Opcode::JumpIfFalse as u8,
                11,
                0, // Jump to offset 11 if false
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 42 (skipped)
                Opcode::Print as u8, // Print 42 (skipped)
                Opcode::LoadConst as u8,
                2,
                0,                   // Load 100
                Opcode::Print as u8, // Print 100
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_float_operations() {
        let program = make_program(
            vec![Value::Float(3.14), Value::Float(2.0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 3.14
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 2.0
                Opcode::Mul as u8,   // 3.14 * 2.0
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_string_concatenation() {
        let program = make_program(
            vec![
                Value::String(Rc::new("Hello, ".to_string())),
                Value::String(Rc::new("World!".to_string())),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load "Hello, "
                Opcode::LoadConst as u8,
                1,
                0,                   // Load "World!"
                Opcode::Add as u8,   // Concatenate
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_equality_operators() {
        // Test ==
        let program = make_program(
            vec![Value::Int(10), Value::Int(10)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 10
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 10
                Opcode::Eq as u8,    // 10 == 10 = true
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_not_equal() {
        let program = make_program(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 10
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 20
                Opcode::Ne as u8,    // 10 != 20 = true
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_greater_than() {
        let program = make_program(
            vec![Value::Int(20), Value::Int(10)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 20
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 10
                Opcode::Gt as u8,    // 20 > 10 = true
                Opcode::Print as u8, // Print result
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
    }

    // ==========================================================================
    // Power Operator Tests
    // ==========================================================================

    #[test]
    fn test_power_int() {
        // 2 ** 3 = 8
        let program = make_program(
            vec![Value::Int(2), Value::Int(3)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 2
                Opcode::LoadConst as u8,
                1,
                0,                 // Load 3
                Opcode::Pow as u8, // 2 ** 3 = 8
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        // Stack should have 8
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, 8),
            other => panic!("Expected Int(8), got {:?}", other),
        }
    }

    #[test]
    fn test_power_float() {
        // 2.0 ** 3.0 = 8.0
        let program = make_program(
            vec![Value::Float(2.0), Value::Float(3.0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 2.0
                Opcode::LoadConst as u8,
                1,
                0,                 // Load 3.0
                Opcode::Pow as u8, // 2.0 ** 3.0 = 8.0
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Float(f) => assert!((f - 8.0).abs() < 0.0001),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn test_power_zero_exponent() {
        // 5 ** 0 = 1
        let program = make_program(
            vec![Value::Int(5), Value::Int(0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::Pow as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, 1),
            other => panic!("Expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn test_power_large_exponent() {
        // 10 ** 2 = 100
        let program = make_program(
            vec![Value::Int(10), Value::Int(2)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::Pow as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, 100),
            other => panic!("Expected Int(100), got {:?}", other),
        }
    }

    // ==========================================================================
    // Stepping Tests
    // ==========================================================================

    fn make_program_with_debug(
        constants: Vec<Value>,
        code: Vec<u8>,
        line_mappings: Vec<(u32, u32, u32, u32)>, // (start_offset, end_offset, line, column)
    ) -> Program {
        use lira_core::bytecode::DebugInfo;
        let mut debug_info = DebugInfo::new();
        for (start, end, line, col) in line_mappings {
            debug_info.add_line(start, end, line, col);
        }
        Program {
            constants,
            code,
            entry_point: 0,
            functions: Vec::new(),
            debug_info,
            source_file: None,
        }
    }

    #[test]
    fn test_step_instruction_basic() {
        // Simple program: LoadConst, LoadConst, Add, Halt
        let program = make_program(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // offset 0-2: Load 10
                Opcode::LoadConst as u8,
                1,
                0,                  // offset 3-5: Load 20
                Opcode::Add as u8,  // offset 6: Add
                Opcode::Halt as u8, // offset 7: Halt
            ],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        // Step 1: LoadConst 10
        let outcome1 = vm.step_instruction();
        assert!(matches!(outcome1, StepOutcome::Continue));
        assert_eq!(vm.stack.len(), 1);
        assert!(matches!(&vm.stack[0], Value::Int(10)));
        assert_eq!(vm.ip, 3);

        // Step 2: LoadConst 20
        let outcome2 = vm.step_instruction();
        assert!(matches!(outcome2, StepOutcome::Continue));
        assert_eq!(vm.stack.len(), 2);
        assert_eq!(vm.ip, 6);

        // Step 3: Add
        let outcome3 = vm.step_instruction();
        assert!(matches!(outcome3, StepOutcome::Continue));
        assert_eq!(vm.stack.len(), 1);
        assert!(matches!(&vm.stack[0], Value::Int(30)));
        assert_eq!(vm.ip, 7);

        // Step 4: Halt
        let outcome4 = vm.step_instruction();
        assert!(matches!(outcome4, StepOutcome::Finished { exit_code: 0 }));
    }

    #[test]
    fn test_step_instruction_error() {
        // Program that causes stack underflow
        let program = make_program(
            vec![],
            vec![
                Opcode::Add as u8, // No values on stack - should error
            ],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        let outcome = vm.step_instruction();
        assert!(matches!(outcome, StepOutcome::Error { .. }));
    }

    #[test]
    fn test_step_line_basic() {
        // Program with debug info mapping different offsets to different lines
        let program = make_program_with_debug(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // offset 0-2: Line 1
                Opcode::LoadConst as u8,
                1,
                0,                  // offset 3-5: Line 2
                Opcode::Add as u8,  // offset 6: Line 3
                Opcode::Halt as u8, // offset 7: Line 4
            ],
            vec![
                (0, 3, 1, 1), // LoadConst on line 1 (offsets 0-2)
                (3, 6, 2, 1), // LoadConst on line 2 (offsets 3-5)
                (6, 7, 3, 1), // Add on line 3 (offset 6)
                (7, 8, 4, 1), // Halt on line 4 (offset 7)
            ],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        // Step line 1 -> should stop at line 2
        let outcome1 = vm.step_line();
        match &outcome1 {
            StepOutcome::StepCompleted { line, .. } => {
                assert_eq!(*line, 2, "Should be on line 2 after stepping from line 1");
            }
            other => panic!("Expected StepCompleted, got {:?}", other),
        }
        assert_eq!(vm.ip, 3);

        // Step line 2 -> should stop at line 3
        let outcome2 = vm.step_line();
        match &outcome2 {
            StepOutcome::StepCompleted { line, .. } => {
                assert_eq!(*line, 3, "Should be on line 3 after stepping from line 2");
            }
            other => panic!("Expected StepCompleted, got {:?}", other),
        }
    }

    #[test]
    fn test_continue_execution() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        let outcome = vm.continue_execution();
        assert!(matches!(outcome, StepOutcome::Finished { exit_code: 0 }));
    }

    #[test]
    fn test_breakpoint_hit() {
        // Program with debug info
        let program = make_program_with_debug(
            vec![Value::Int(10), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Line 1
                Opcode::LoadConst as u8,
                1,
                0,                  // Line 2
                Opcode::Add as u8,  // Line 3
                Opcode::Halt as u8, // Line 4
            ],
            vec![(0, 3, 1, 1), (3, 6, 2, 1), (6, 7, 3, 1), (7, 8, 4, 1)],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        // Set breakpoint on line 3
        vm.set_breakpoints(vec![3]);

        // Continue execution - should stop at breakpoint
        let outcome = vm.continue_execution();
        match &outcome {
            StepOutcome::Breakpoint { line, .. } => {
                assert_eq!(*line, 3, "Should hit breakpoint on line 3");
            }
            other => panic!("Expected Breakpoint, got {:?}", other),
        }

        // Continue again - should finish
        let outcome2 = vm.continue_execution();
        assert!(matches!(outcome2, StepOutcome::Finished { exit_code: 0 }));
    }

    #[test]
    fn test_pause_flag() {
        let program = make_program(
            vec![Value::Int(1)],
            vec![
                // Simple loop that runs forever without pause
                Opcode::LoadConst as u8,
                0,
                0,                 // Load 1
                Opcode::Pop as u8, // Pop it
                Opcode::Jump as u8,
                0,
                0, // Jump back to start
            ],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        // Get the pause flag and request pause
        let flag = vm.get_pause_flag();
        flag.request();

        // Step instruction should return Paused
        let outcome = vm.step_instruction();
        assert!(matches!(outcome, StepOutcome::Paused { .. }));
    }

    #[test]
    fn test_execution_state_transitions() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![Opcode::LoadConst as u8, 0, 0, Opcode::Halt as u8],
        );
        let mut vm = VM::new(program);

        // Initial state is Ready
        assert!(matches!(vm.execution_state, ExecutionState::Ready));

        // After prepare, still Ready
        vm.prepare();
        assert!(matches!(vm.execution_state, ExecutionState::Ready));

        // After step, state changes based on outcome
        vm.step_instruction();
        // Note: execution_state is updated by step_instruction based on outcome
    }

    #[test]
    fn test_debug_snapshot() {
        let program = make_program_with_debug(
            vec![Value::Int(42), Value::Int(100)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Line 1
                Opcode::LoadConst as u8,
                1,
                0,                  // Line 2
                Opcode::Halt as u8, // Line 3
            ],
            vec![(0, 3, 1, 1), (3, 6, 2, 1), (6, 7, 3, 1)],
        );
        let mut vm = VM::new(program);
        vm.prepare();

        // Execute two instructions
        vm.step_instruction();
        vm.step_instruction();

        // Get snapshot
        let snapshot = vm.get_debug_snapshot();
        assert_eq!(snapshot.stack.len(), 2);
        assert_eq!(snapshot.ip, 6);
        assert!(snapshot.location.is_some());
    }

    /// Effort A gate: a spawned worker must actually run and hand a value back
    /// to main over a channel. Exercises main-registered-as-fiber, spawn (with
    /// the channel passed as a fiber-local arg), a real blocking ChanRecv on
    /// main, the scheduler resuming the worker which sends, and fiber
    /// completion via Return without killing the VM.
    #[test]
    fn test_spawn_channel_handoff() {
        // Worker starts at offset W; main starts at entry_point 0.
        const W: u16 = 15;
        let program = make_program(
            vec![Value::Int(1), Value::Int(42)],
            vec![
                // --- main @0 ---
                Opcode::LoadConst as u8,
                0,
                0,                     // capacity 1
                Opcode::ChanNew as u8, // -> ch                 @3
                Opcode::Dup as u8,     // ch, ch                @4
                Opcode::Spawn as u8,
                (W & 0xff) as u8,
                (W >> 8) as u8,
                1,                      // spawn worker(ch): 1 arg  @5..8
                Opcode::Pop as u8,      // drop Fiber handle    @9
                Opcode::Yield as u8,    // park main            @10
                Opcode::ChanRecv as u8, // -> 42, true          @11
                Opcode::Pop as u8,      // drop ok flag         @12
                Opcode::Print as u8,    // print 42             @13
                Opcode::Halt as u8,     //                      @14
                // --- worker @15 (locals[0] == ch from spawn arg) ---
                Opcode::LoadLocal as u8,
                0,
                0, // push ch               @15..17
                Opcode::LoadConst as u8,
                1,
                0,                      // push 42               @18..20
                Opcode::ChanSend as u8, // send 42 into ch       @21
                Opcode::LoadConst as u8,
                0,
                0,                    // dummy return value    @22..24
                Opcode::Return as u8, //                       @25
            ],
        );
        let mut vm = VM::new(program);
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        let code = vm.run().expect("spawn+channel program should run");
        assert_eq!(code, 0);
        assert_eq!(vm.get_output(), &["42".to_string()]);
    }
}

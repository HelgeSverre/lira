//! Lira VM Core
//!
//! Stack-based bytecode interpreter with fiber support.
//! See docs/lira/12-vm-runtime.md for the full specification.

use crate::bytecode::Program;
use crate::debug::{
    CallFrameInfo, DebugSnapshot, ExecutionState, LocalInfo, PauseFlag, RichValueBuilder,
    StepContext, StepMode, StepOutcome, ValueInfo,
};
use crate::fiber::{FiberEvent, Scheduler};
use crate::runtime::Runtime;
use crate::value::{
    ChannelId, ClosureData, FiberId, InterfaceData, InterfaceIntrinsic, InterfaceMethod, TupleData,
    Value,
};
use crate::vm_snapshot::{
    ChannelStateSnapshot, FiberFrameSnapshot, FiberStateSnapshot, SchedulerSnapshot,
};
use gc::{Gc, GcCell};
use lira_core::opcode::Opcode;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Write as _};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};

/// Maximum backing storage allocated by one array or tuple construction.
///
/// Bytecode is an external input format, so a malformed `NewArray` or
/// `NewTuple` operand must fail before it can ask the allocator for most of the
/// address space. Mutable array growth observes the same per-collection cap.
const MAX_COLLECTION_ALLOCATION_BYTES: usize = 16 * 1024 * 1024;
const MAX_COLLECTION_ELEMENTS: usize =
    MAX_COLLECTION_ALLOCATION_BYTES / std::mem::size_of::<Value>();

/// Captured output is retained in both raw-stream and logical-line form for
/// compatibility with the public test/debug APIs. Bound both representations
/// so hostile programs cannot exhaust the host by printing indefinitely.
const MAX_CAPTURE_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CAPTURE_OUTPUT_LINES: usize = 100_000;

/// Type alias for output callback function
pub type OutputCallback = Box<dyn FnMut(&str) + Send>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRenderFailure {
    Limit,
    Allocation,
}

struct BoundedOutputString {
    text: String,
    failure: Option<OutputRenderFailure>,
}

impl BoundedOutputString {
    fn new() -> Self {
        Self {
            text: String::new(),
            failure: None,
        }
    }
}

impl fmt::Write for BoundedOutputString {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let Some(new_len) = self.text.len().checked_add(text.len()) else {
            self.failure = Some(OutputRenderFailure::Limit);
            return Err(fmt::Error);
        };
        if new_len > MAX_CAPTURE_OUTPUT_BYTES {
            self.failure = Some(OutputRenderFailure::Limit);
            return Err(fmt::Error);
        }
        if self.text.try_reserve(text.len()).is_err() {
            self.failure = Some(OutputRenderFailure::Allocation);
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

fn render_output_value(value: &Value) -> Result<String, String> {
    let mut rendered = BoundedOutputString::new();
    if write!(&mut rendered, "{value}").is_err() {
        return Err(match rendered.failure {
            Some(OutputRenderFailure::Limit) => format!(
                "one printed value exceeded the {} byte output limit",
                MAX_CAPTURE_OUTPUT_BYTES
            ),
            Some(OutputRenderFailure::Allocation) => {
                "printed value could not be rendered within available memory".to_string()
            }
            None => "printed value could not be rendered".to_string(),
        });
    }
    Ok(rendered.text)
}

fn value_matches_type_id(value: &Value, type_id: u8) -> bool {
    match type_id {
        0 => matches!(value, Value::Null),
        1 => matches!(value, Value::Bool(_)),
        2 => matches!(value, Value::Int(_)),
        3 => matches!(value, Value::Float(_)),
        4 => matches!(value, Value::String(_)),
        5 => matches!(value, Value::Array(_)),
        6 => matches!(value, Value::Object(_) | Value::Struct(_)),
        7 => matches!(value, Value::Function(_) | Value::Closure(_)),
        8 => matches!(value, Value::Tuple(_)),
        9 => matches!(value, Value::Channel(_)),
        10 => matches!(value, Value::Interface(_)),
        _ => false,
    }
}

/// Validate the runtime's outer representation for aggregate/function casts.
///
/// The bytecode cast operand contains only the coarse type ID, so this check
/// intentionally does not claim to validate array element, function
/// signature, tuple element, channel payload, or nominal metadata.
fn validate_runtime_cast(value: &Value, type_id: u8) -> Result<(), String> {
    let expected = match type_id {
        5 => "array",
        7 => "function",
        8 => "tuple",
        9 => "channel",
        _ => return Ok(()),
    };

    if value_matches_type_id(value, type_id) {
        Ok(())
    } else {
        Err(format!("Cannot cast {} to {expected}", value.type_name()))
    }
}

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

/// A runtime error with an optional source location.
///
/// The `message` is the bare error text (NOT location-prefixed); `line` and
/// `column` carry the source position recovered from the VM's debug info at
/// the point of failure, when available.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// The bare error message (not prefixed with a location).
    pub message: String,
    /// Source line (1-based) where the error occurred, if known.
    pub line: Option<u32>,
    /// Source column (1-based) where the error occurred, if known.
    pub column: Option<u32>,
    /// Function-named call stack of the active fiber, innermost first
    /// (the failing function first, the entry point last). Empty when no
    /// function-symbol debug info is available to resolve against.
    pub stack: Vec<String>,
}

#[derive(Debug, Clone)]
struct ErrorContext {
    location: Option<(u32, u32)>,
    stack: Vec<String>,
}

/// Call frame for function calls
#[derive(Debug, Clone)]
struct CallFrame {
    /// Bytecode offset of the called function's entry point (for debug naming)
    func_offset: usize,
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
    captures: Option<Gc<ClosureData>>,
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
    /// Exact captured byte stream, including newline boundaries.
    output_raw: String,
    /// Whether the last logical line in `output` is unterminated.
    output_line_open: bool,
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
    /// Whether the fiber runtime has been bootstrapped (main registered as a
    /// fiber and first-scheduled). Shared by `run_inner` and the stepping path
    /// so the bootstrap happens exactly once per execution.
    fiber_runtime_started: bool,
    /// Thread pool for offloading blocking syscalls (HTTP) so they run in
    /// parallel instead of stalling the single VM thread. Created lazily on the
    /// first offloaded call; programs without blocking I/O spawn no threads.
    io_pool: Option<crate::io_pool::IoPool>,
    /// Exit code captured when the main fiber finishes
    main_exit_code: i32,
    /// State for the deterministic SplitMix64 select arbiter.
    select_rng: u64,
    /// True when sys_exit has been called, signaling a clean VM-level halt
    exit_requested: bool,
    /// Saved native call stacks per fiber. The VM-native `CallFrame` carries
    /// more state (stack base, captures) than the scheduler's `FiberCallFrame`,
    /// so each fiber's call stack is parked here across context switches.
    fiber_call_stacks: HashMap<FiberId, Vec<CallFrame>>,
    /// Count of cyclic-capable heap allocations (objects, arrays, closures)
    /// since startup. Drives the periodic auto-collection at the interpreter
    /// loop boundary (see [`VM::AUTO_COLLECT_INTERVAL`]).
    allocations: u64,
    /// Source context for the first runtime error, including errors reported
    /// by a parked child fiber after the root fiber has moved on.
    error_context: Option<ErrorContext>,
}

impl VM {
    const DEFAULT_SELECT_SEED: u64 = 0x6a09_e667_f3bc_c909;

    /// Number of cyclic-capable heap allocations between automatic collections.
    ///
    /// The tracing cycle collector is also driven implicitly by rust-gc's own
    /// allocation-threshold heuristic (it may collect inside `Gc::new`), but we
    /// additionally force a collection from the interpreter loop every this many
    /// object/array/closure allocations. Driving it from the loop boundary
    /// guarantees collection happens at a point where no `GcCell` is borrowed
    /// (every `execute_*` handler releases its borrows before returning), making
    /// the trigger deterministic and independent of allocation-site timing.
    const AUTO_COLLECT_INTERVAL: u64 = 100_000;

    /// Record a cyclic-capable heap allocation (object, array, or closure).
    #[inline]
    fn note_allocation(&mut self) {
        self.allocations = self.allocations.wrapping_add(1);
    }

    /// Create a new VM with the given program
    pub fn new(program: Program) -> Self {
        // Set up fiber event channel
        let (tx, rx) = channel();
        let mut scheduler = Scheduler::new();
        scheduler.set_event_sender(tx);

        let select_rng = std::env::var("LIRA_SELECT_SEED")
            .ok()
            .and_then(|seed| seed.parse::<u64>().ok())
            .filter(|seed| *seed != 0)
            .unwrap_or(Self::DEFAULT_SELECT_SEED);

        Self {
            program,
            stack: Vec::with_capacity(4096),
            call_stack: Vec::with_capacity(4096),
            ip: 0,
            locals: Vec::with_capacity(4096),
            debug: false,
            scheduler,
            fiber_mode: false,
            fiber_runtime_started: false,
            io_pool: None,
            runtime: Runtime::new(),
            output: Vec::new(),
            output_raw: String::new(),
            output_line_open: false,
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
            select_rng,
            exit_requested: false,
            fiber_call_stacks: HashMap::new(),
            allocations: 0,
            error_context: None,
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
        self.output_raw.clone()
    }

    fn capture_output_chunk(&mut self, text: &str, newline: bool) -> Result<(), String> {
        let additional = text
            .len()
            .checked_add(usize::from(newline))
            .ok_or_else(|| "captured output length overflowed".to_string())?;
        let new_len = self
            .output_raw
            .len()
            .checked_add(additional)
            .ok_or_else(|| "captured output length overflowed".to_string())?;
        if new_len > MAX_CAPTURE_OUTPUT_BYTES {
            return Err(format!(
                "captured output exceeded the {} byte limit",
                MAX_CAPTURE_OUTPUT_BYTES
            ));
        }

        let mut projected_open = self.output_line_open;
        let mut new_lines = 0_usize;
        for piece in text.split_inclusive('\n') {
            let terminates = piece.ends_with('\n');
            let segment = piece.strip_suffix('\n').unwrap_or(piece);
            if (!segment.is_empty() || terminates) && !projected_open {
                new_lines = new_lines
                    .checked_add(1)
                    .ok_or_else(|| "captured output line count overflowed".to_string())?;
                projected_open = true;
            }
            if terminates {
                projected_open = false;
            }
        }
        if newline && !projected_open {
            new_lines = new_lines
                .checked_add(1)
                .ok_or_else(|| "captured output line count overflowed".to_string())?;
        }
        if self
            .output
            .len()
            .checked_add(new_lines)
            .is_none_or(|count| count > MAX_CAPTURE_OUTPUT_LINES)
        {
            return Err(format!(
                "captured output exceeded the {} line limit",
                MAX_CAPTURE_OUTPUT_LINES
            ));
        }

        self.output_raw.try_reserve(additional).map_err(|_| {
            "captured output could not be retained within available memory".to_string()
        })?;
        self.output.try_reserve(new_lines).map_err(|_| {
            "captured output lines could not be retained within available memory".to_string()
        })?;

        let mut append = |segment: &str, terminates: bool| -> Result<(), String> {
            if !segment.is_empty() || terminates {
                if !self.output_line_open {
                    self.output.push(String::new());
                    self.output_line_open = true;
                }
                let line = self
                    .output
                    .last_mut()
                    .expect("an open captured output line must exist");
                line.try_reserve(segment.len()).map_err(|_| {
                    "captured output line could not be retained within available memory".to_string()
                })?;
                line.push_str(segment);
            }
            if terminates {
                self.output_line_open = false;
            }
            Ok(())
        };

        for piece in text.split_inclusive('\n') {
            let terminates = piece.ends_with('\n');
            let segment = piece.strip_suffix('\n').unwrap_or(piece);
            append(segment, terminates)?;
        }
        if newline {
            append("", true)?;
        }

        self.output_raw.push_str(text);
        if newline {
            self.output_raw.push('\n');
        }
        Ok(())
    }

    /// Enable fiber mode for concurrent execution
    pub fn set_fiber_mode(&mut self, enabled: bool) {
        self.fiber_mode = enabled;
    }

    /// Set an environment value visible only to this VM instance.
    pub fn set_env_override(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.runtime.set_env_override(name, value);
    }

    /// Set the deterministic seed used to arbitrate ready `select` arms.
    ///
    /// The VM uses SplitMix64, which is small, reproducible, and has no
    /// zero-state trap. A zero seed is mapped to the documented default so a
    /// caller cannot accidentally request a degenerate stream.
    pub fn set_select_seed(&mut self, seed: u64) {
        self.select_rng = if seed == 0 {
            Self::DEFAULT_SELECT_SEED
        } else {
            seed
        };
    }

    /// Advance the select PRNG for one arbitration round.
    fn next_select_seed(&mut self) -> u64 {
        self.select_rng = self.select_rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        Self::mix_select(self.select_rng)
    }

    /// SplitMix64's output function. The arm's channel id, direction, and
    /// ordinal are folded into the round seed before this function is called.
    /// The ordinal breaks ties for duplicate offers without making the later
    /// source arm win deterministically.
    fn mix_select(mut mixed: u64) -> u64 {
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^= mixed >> 31;
        mixed
    }

    /// Score one communication arm for a select arbitration round.
    ///
    /// The ordinal is part of the score deliberately: two arms can name the
    /// same channel and direction. Ambiguous parked offers are deferred until
    /// their owner chooses a value, while endpoint resolution for a completed
    /// operation scores matching duplicate bodies again without changing the
    /// exactly-once channel operation.
    fn select_arm_score(round_seed: u64, index: usize, channel: ChannelId, is_send: bool) -> u64 {
        let kind = u64::from(is_send);
        Self::mix_select(
            round_seed
                ^ channel.wrapping_mul(0xd6e8_feb8_6659_fd93)
                ^ kind.wrapping_mul(0xa5a3_5634_1f12_5d3b)
                ^ (index as u64).wrapping_mul(0x8cb9_2d3b_5e4f_1a77),
        )
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
        self.error_context
            .as_ref()
            .and_then(|context| context.location)
            .or_else(|| self.program.debug_info.lookup(self.ip as u32))
    }

    /// Build a function-named call stack for the active fiber, innermost first.
    ///
    /// Each entry is resolved through the same debug-info function-symbol table
    /// used by [`VM::get_debug_snapshot`]. The currently-executing function is
    /// listed first (the deepest `CallFrame`), followed by its callers down to
    /// `main` (which is itself a `CallFrame`, since the synthetic top-level
    /// driver at the entry point calls it). Frames whose name cannot be
    /// recovered are dropped, so the synthetic driver frame (no symbol) does not
    /// leak into the rendered stack. Returns an empty vec when there are no
    /// resolvable frames (no/empty debug info), letting callers render just the
    /// message as before.
    pub(crate) fn build_call_stack_names(&self) -> Vec<String> {
        if let Some(context) = &self.error_context {
            if !context.stack.is_empty() {
                return context.stack.clone();
            }
        }
        self.call_stack_names_for(&self.call_stack)
    }

    fn call_stack_names_for(&self, frames: &[CallFrame]) -> Vec<String> {
        frames
            .iter()
            .rev()
            .filter_map(|frame| {
                self.program
                    .debug_info
                    .function_name_at(frame.func_offset as u32)
                    .map(|s| s.to_string())
            })
            .collect()
    }

    fn function_name_for_ip(&self, ip: usize) -> Option<String> {
        self.program
            .debug_info
            .function_symbols
            .iter()
            .filter(|symbol| symbol.code_offset as usize <= ip)
            .max_by_key(|symbol| symbol.code_offset)
            .map(|symbol| symbol.name.clone())
    }

    fn capture_error_context(&mut self, location_ip: usize, frames: &[CallFrame]) {
        self.error_context = Some(ErrorContext {
            location: self.program.debug_info.lookup(location_ip as u32),
            stack: self.call_stack_names_for(frames),
        });
    }

    fn capture_error_context_at(&mut self, ip: usize) {
        let frames = self.call_stack.clone();
        self.capture_error_context(ip, &frames);
        if self
            .error_context
            .as_ref()
            .is_some_and(|context| context.stack.is_empty())
        {
            if let Some(name) = self.function_name_for_ip(ip) {
                if let Some(context) = self.error_context.as_mut() {
                    context.stack.push(name);
                }
            }
        }
    }

    fn capture_fiber_failure_context(&mut self, fiber_id: FiberId) {
        let Some(fiber) = self.scheduler.get_fiber(fiber_id) else {
            self.error_context = Some(ErrorContext {
                location: None,
                stack: Vec::new(),
            });
            return;
        };

        let frames = self
            .fiber_call_stacks
            .get(&fiber_id)
            .cloned()
            .unwrap_or_default();
        // The scheduler records the operation offset before changing a parked
        // fiber to Failed. This distinguishes a direct ChanSend (whose saved
        // IP is already past the opcode) from a parked Select (whose saved IP
        // is rewound to the Select opcode).
        let location_ip = fiber.failure_ip.unwrap_or(fiber.ip);
        let location = self
            .program
            .debug_info
            .lookup(location_ip as u32)
            .or_else(|| self.program.debug_info.lookup(fiber.ip as u32));
        let mut stack = self.call_stack_names_for(&frames);
        if stack.is_empty() {
            if let Some(name) = self.function_name_for_ip(fiber.ip) {
                stack.push(name);
            }
        }
        self.error_context = Some(ErrorContext { location, stack });
    }

    /// Get a detailed, value-carrying snapshot of the fiber scheduler.
    ///
    /// Captures every fiber and channel with their live contents (as
    /// [`RichValue`]s), for fiber-mode debugging/visualization. Fibers and
    /// channels are sorted by id so the output is deterministic regardless of
    /// the scheduler's internal `HashMap` ordering.
    pub fn scheduler_snapshot(&self) -> SchedulerSnapshot {
        // The currently-running fiber's live context lives in `self.{ip,stack,
        // locals,call_stack}`, not in its parked `Fiber` struct (which holds the
        // last-saved state). Surface the live state for it so a mid-step
        // snapshot is accurate, and resolve its frame function names from debug
        // info. Parked fibers are read straight from their `Fiber`.
        let current = self.scheduler.current;
        let mut rich_values = RichValueBuilder::new();
        let mut fibers: Vec<FiberStateSnapshot> = self
            .scheduler
            .fibers
            .values()
            .map(|f| {
                let is_current = Some(f.id) == current;
                // The running fiber's live operand stack/locals/ip live in
                // `self.*`; parked fibers are read from their `Fiber` struct.
                let (ip, stack_src, locals_src) = if is_current {
                    (self.ip, &self.stack, &self.locals)
                } else {
                    (f.ip, &f.stack, &f.locals)
                };
                // Only the running fiber has native call frames (with a func
                // offset we can resolve to a name); parked fibers expose just
                // the return/base addresses.
                let call_stack = if is_current {
                    self.call_stack
                        .iter()
                        .map(|frame| FiberFrameSnapshot {
                            return_addr: frame.return_addr,
                            locals_base: frame.locals_base,
                            function_name: self
                                .program
                                .debug_info
                                .function_name_at(frame.func_offset as u32)
                                .map(|s| s.to_string()),
                        })
                        .collect()
                } else {
                    f.call_stack
                        .iter()
                        .map(|frame| FiberFrameSnapshot {
                            return_addr: frame.return_addr,
                            locals_base: frame.locals_base,
                            function_name: None,
                        })
                        .collect()
                };
                FiberStateSnapshot {
                    id: f.id,
                    state: f.state.clone(),
                    ip,
                    stack: stack_src
                        .iter()
                        .map(|value| rich_values.convert(value))
                        .collect(),
                    locals: locals_src
                        .iter()
                        .map(|value| rich_values.convert(value))
                        .collect(),
                    call_stack,
                    result: f.result.as_ref().map(|value| rich_values.convert(value)),
                }
            })
            .collect();
        fibers.sort_by_key(|f| f.id);

        let mut channels: Vec<ChannelStateSnapshot> = self
            .scheduler
            .channels
            .values()
            .map(|c| ChannelStateSnapshot {
                id: c.id,
                buffer: c
                    .buffer
                    .iter()
                    .map(|value| rich_values.convert(value))
                    .collect(),
                capacity: c.capacity,
                receivers: c.receivers.iter().copied().collect(),
                senders: c
                    .senders
                    .iter()
                    .map(|(id, value)| (*id, rich_values.convert(value)))
                    .collect(),
                closed: c.closed,
            })
            .collect();
        channels.sort_by_key(|c| c.id);

        SchedulerSnapshot {
            fibers,
            channels,
            current_fiber_id: self.scheduler.current,
            ready_queue: self.scheduler.ready_queue_ids(),
        }
    }

    // ==================== Stepping Methods ====================

    /// Initialize the VM at entry point without executing
    pub fn prepare(&mut self) {
        self.ip = self.program.entry_point;
        self.execution_state = ExecutionState::Ready;
        self.step_context.clear();
        self.last_breakpoint_line = None;
        self.fiber_runtime_started = false;
    }

    /// Bootstrap the fiber runtime exactly once: register the main program as a
    /// real fiber and schedule it as the running fiber, so its context is
    /// saved/restored across context switches. Idempotent and shared by both
    /// `run_inner` and the stepping path (`step_instruction`). A no-op outside
    /// fiber mode.
    fn ensure_fiber_runtime_started(&mut self) {
        if self.fiber_mode && !self.fiber_runtime_started {
            let main_id = self.scheduler.spawn(self.program.entry_point);
            self.main_fiber_id = main_id;
            // Pop main straight into the Running state and make it current.
            self.scheduler.schedule();
            self.ip = self.program.entry_point;
            self.fiber_runtime_started = true;
        }
    }

    /// If the current fiber has parked/finished (`scheduler.current` is `None`),
    /// pick the next runnable fiber and load its execution context. Returns
    /// `Some(terminal)` when the program is done (no runnable fibers, exit with
    /// the main fiber's code) or deadlocked; `None` when execution may proceed.
    /// A no-op (returns `None`) outside fiber mode or while a fiber is running.
    ///
    /// This mirrors the reschedule branch at the top of `run_inner`'s loop, so
    /// the stepping path drives the scheduler identically.
    fn pump_scheduler(&mut self) -> Option<StepOutcome> {
        if self.fiber_mode && self.scheduler.current.is_none() {
            if let Some((fiber_id, message)) = self.scheduler.first_failure() {
                self.capture_fiber_failure_context(fiber_id);
                return Some(StepOutcome::Error { message });
            }
            // Wake fibers whose offloaded I/O completed, then pick one.
            self.harvest_io();
            if self.scheduler.schedule().is_some() {
                self.load_fiber_state();
                return None;
            }
            // Nothing ready. If blocking I/O is in flight, wait for it (fibers
            // parked on I/O are not deadlocked), then schedule the woken fiber.
            if self.io_pending() > 0 {
                match self.io_pool.as_mut().and_then(|p| p.wait_one()) {
                    Some(comp) => self.deliver_io(comp),
                    None => {
                        return Some(StepOutcome::Error {
                            message: "I/O pool terminated with work in flight".to_string(),
                        })
                    }
                }
                if self.scheduler.schedule().is_some() {
                    self.load_fiber_state();
                }
                return None;
            }
            if self.scheduler.is_deadlocked() {
                return Some(StepOutcome::Error {
                    message: "deadlock: all fibers are blocked".to_string(),
                });
            }
            if !self.scheduler.has_runnable() {
                return Some(StepOutcome::Finished {
                    exit_code: self.main_exit_code,
                });
            }
        }
        None
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
        self.error_context = None;
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

        // Fiber mode: bootstrap the runtime on the first step, then (if the
        // current fiber has parked/finished) switch to the next runnable fiber
        // before checking breakpoints/executing. This is what makes stepping
        // drive the scheduler across fibers exactly as `run_inner` does.
        self.ensure_fiber_runtime_started();
        if let Some(terminal) = self.pump_scheduler() {
            self.execution_state = match &terminal {
                StepOutcome::Finished { exit_code } => ExecutionState::Finished {
                    exit_code: *exit_code,
                },
                StepOutcome::Error { message } => ExecutionState::Error {
                    message: message.clone(),
                    location: self.get_current_location(),
                },
                _ => self.execution_state.clone(),
            };
            return terminal;
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
        let opcode_ip = self.ip;
        match self.execute_one() {
            Ok(Some(exit_code)) => {
                self.execution_state = ExecutionState::Finished { exit_code };
                StepOutcome::Finished { exit_code }
            }
            Ok(None) => StepOutcome::Continue,
            Err(e) => {
                self.capture_error_context_at(opcode_ip);
                self.scheduler.fail_current(e.clone());
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
            let stack_repr: Vec<String> = self.stack.iter().map(Value::debug_summary).collect();
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
        let mut rich_values = RichValueBuilder::new();

        let stack: Vec<ValueInfo> = self
            .stack
            .iter()
            .map(|v| {
                let rich_value = rich_values.convert(v);
                ValueInfo::with_rich_value(
                    rich_value.display_summary(),
                    v.type_name().to_string(),
                    rich_value,
                )
            })
            .collect();

        // Get local names from debug info at current IP
        let local_names = self.program.debug_info.get_local_names_at(self.ip as u32);
        let locals: Vec<LocalInfo> = self
            .locals
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let rich_value = rich_values.convert(v);
                LocalInfo {
                    slot: i,
                    name: local_names.get(&(i as u16)).map(|s| s.to_string()),
                    value: ValueInfo::with_rich_value(
                        rich_value.display_summary(),
                        v.type_name().to_string(),
                        rich_value,
                    ),
                }
            })
            .collect();

        let call_stack: Vec<CallFrameInfo> = self
            .call_stack
            .iter()
            .map(|frame| CallFrameInfo {
                function_name: self
                    .program
                    .debug_info
                    .function_name_at(frame.func_offset as u32)
                    .map(|s| s.to_string()),
                return_addr: frame.return_addr,
                source_location: self.program.debug_info.lookup(frame.return_addr as u32),
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

    // ==================== End Stepping Methods ====================

    /// Run the program and return exit code.
    ///
    /// On a runtime error, the returned message is prefixed with the source
    /// location (`"line:column: message"`) when one can be recovered from the
    /// VM's debug info. Breakpoint-hit sentinels are passed through unchanged
    /// so downstream parsers (e.g. the playground) keep working.
    pub fn run(&mut self) -> Result<i32, String> {
        self.error_context = None;
        match self.run_inner() {
            Ok(code) => Ok(code),
            Err(msg) => {
                // Don't re-prefix breakpoint sentinels (parsed by callers).
                if msg.starts_with("Breakpoint hit at line ") {
                    return Err(msg);
                }
                // First line stays byte-compatible with the historical form
                // (`line:col: message`); the function-named call stack, when
                // available, is appended on subsequent lines.
                let context = self.error_context.clone();
                let location = context
                    .as_ref()
                    .and_then(|context| context.location)
                    .or_else(|| self.program.debug_info.lookup(self.ip as u32));
                let stack = context
                    .as_ref()
                    .map(|context| context.stack.as_slice())
                    .unwrap_or(&[]);
                let mut rendered = match location {
                    Some((line, col)) => format!("{}:{}: {}", line, col, msg),
                    None => msg,
                };
                let stack = if stack.is_empty() {
                    self.build_call_stack_names()
                } else {
                    stack.to_vec()
                };
                for name in stack {
                    rendered.push_str("\n  at ");
                    rendered.push_str(&name);
                }
                Err(rendered)
            }
        }
    }

    /// Offload a blocking syscall to the I/O pool and park the current fiber.
    /// The caller must have already popped the syscall's arguments; `job`
    /// carries the current fiber's id so the result routes back to it. `ip` is
    /// past the syscall, so the fiber resumes just after it with the result on
    /// its stack (pushed by [`Self::deliver_io`] when the pool completes).
    fn offload_io(&mut self, job: crate::io_pool::IoJob) {
        self.save_fiber_state();
        let submit_result = self
            .io_pool
            .get_or_insert_with(crate::io_pool::IoPool::new)
            .submit(job);
        match submit_result {
            // Submitted: park the fiber; a completion will wake it.
            Ok(()) => self.scheduler.block_current_on_io(),
            // Pool unavailable (all workers gone): run the job inline on the VM
            // thread so the fiber is never stranded. The job closure is a plain
            // `FnOnce` — callable here — and re-inserts any checked-out handle.
            Err(job) => {
                self.load_fiber_state();
                let value = self.io_outcome_to_value((job.run)());
                self.stack.push(value);
            }
        }
    }

    /// Jobs submitted to the I/O pool but not yet harvested.
    fn io_pending(&self) -> usize {
        self.io_pool.as_ref().map_or(0, |p| p.pending())
    }

    /// The fiber to park if a blocking syscall should be offloaded — `Some`
    /// only in fiber mode with a running fiber. `None` runs the syscall inline
    /// (sequential programs, where there is no scheduler to hand control to).
    fn io_offload_target(&self) -> Option<FiberId> {
        if self.fiber_mode {
            self.scheduler.current
        } else {
            None
        }
    }

    /// Resolve a path to absolute on the VM thread before offloading an fs op,
    /// so a pool thread is unaffected by a concurrent `chdir` (cwd is
    /// process-global). Unlike `canonicalize`, this does not require the path to
    /// exist (needed for `mkdir`/`copy` destinations). Absolute paths pass through.
    fn absolutize(&self, path: &str) -> String {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            return path.to_string();
        }
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
            Err(_) => path.to_string(),
        }
    }

    /// Wake every fiber whose offloaded I/O has already completed.
    fn harvest_io(&mut self) {
        let completed = match self.io_pool.as_mut() {
            Some(pool) => pool.drain_completed(),
            None => return,
        };
        for comp in completed {
            self.deliver_io(comp);
        }
    }

    /// Convert an offloaded syscall's completion into the `Value` it should
    /// yield, then wake the fiber. Runs on the VM thread, so it is the only
    /// place `Rc`/`Gc` values are built and handle registries are touched.
    fn deliver_io(&mut self, comp: crate::io_pool::IoCompletion) {
        let value = self.io_outcome_to_value(comp.outcome);
        self.scheduler.wake_io(comp.fiber, value);
    }

    fn io_outcome_to_value(&mut self, outcome: Result<crate::io_pool::IoValue, String>) -> Value {
        match outcome {
            Ok(v) => self.io_value_to_value(v),
            // Reserved for unexpected failures (a panicked job). Syscalls encode
            // their own error contracts inside `Ok(IoValue)`, so this is rare.
            Err(e) => Value::String(Rc::new(e)),
        }
    }

    /// Turn plain `IoValue` data into a runtime `Value`. Handle-carrying
    /// variants re-insert their checked-out `File`/`TcpStream` first (so a
    /// following syscall on the same handle checks out cleanly), then convert
    /// their inner result.
    fn io_value_to_value(&mut self, v: crate::io_pool::IoValue) -> Value {
        use crate::io_pool::IoValue;
        match v {
            IoValue::Unit => Value::Null,
            IoValue::Int(n) => Value::Int(n),
            IoValue::Str(s) => Value::String(Rc::new(s)),
            IoValue::Bool(b) => Value::Bool(b),
            IoValue::Strs(items) => {
                let arr: Vec<Value> = items
                    .into_iter()
                    .map(|s| Value::String(Rc::new(s)))
                    .collect();
                Value::Array(Gc::new(GcCell::new(arr)))
            }
            IoValue::HttpResponse { status, body } => {
                let tuple = vec![Value::Int(status), Value::String(Rc::new(body))];
                Value::Tuple(Gc::new(GcCell::new(TupleData::sealed(tuple))))
            }
            IoValue::FileOpened(file) => {
                // Allocate the fd only now (open succeeded), so a failed open —
                // which yields Int(-1) instead — consumes no handle id.
                let fd = self.runtime.alloc_fd();
                self.runtime.insert_file(fd, file);
                Value::Int(fd)
            }
            IoValue::FileOp { fd, file, result } => {
                self.runtime.insert_file(fd, file);
                self.io_value_to_value(*result)
            }
            IoValue::TcpConnected(stream) => {
                let id = self.runtime.alloc_socket_id();
                self.runtime.insert_socket(id, stream);
                Value::Int(id)
            }
            IoValue::TcpOp { id, stream, result } => {
                self.runtime.insert_socket(id, stream);
                self.io_value_to_value(*result)
            }
        }
    }

    /// Run the program loop without attaching a source location to errors.
    ///
    /// This holds the actual execution loop; [`VM::run`] wraps it to prefix the
    /// location, and structured callers use it directly to recover the bare
    /// message alongside [`VM::get_current_location`].
    pub(crate) fn run_inner(&mut self) -> Result<i32, String> {
        self.error_context = None;
        // Start at entry point
        self.ip = self.program.entry_point;

        // In fiber mode, register the main program as a real fiber so its
        // context is saved/restored across context switches. Without this,
        // save_fiber_state/load_fiber_state silently drop main's state because
        // there is no "current" fiber. Shared with the stepping path; runs once.
        self.ensure_fiber_runtime_started();

        loop {
            if let Some((fiber_id, message)) = self.scheduler.first_failure() {
                self.capture_fiber_failure_context(fiber_id);
                return Err(message);
            }

            // When the current fiber has parked or finished, re-enter the
            // scheduler to pick the next runnable fiber. Sequential programs
            // never reach this branch: main stays Running until it Halts.
            if self.fiber_mode && self.scheduler.current.is_none() {
                // Wake any fibers whose offloaded I/O finished while others ran.
                self.harvest_io();
                match self.scheduler.schedule() {
                    Some(_) => self.load_fiber_state(),
                    None => {
                        // Nothing runnable right now.
                        if self.io_pending() > 0 {
                            // Blocking I/O is in flight; wait for a completion to
                            // wake a fiber rather than declaring deadlock. Loop
                            // back so the scheduler runs the now-ready fiber.
                            match self.io_pool.as_mut().and_then(|p| p.wait_one()) {
                                Some(comp) => {
                                    self.deliver_io(comp);
                                    continue;
                                }
                                // Pool gone with work in flight: cannot make
                                // progress, so surface it instead of spinning.
                                None => {
                                    return Err(
                                        "I/O pool terminated with work in flight".to_string()
                                    )
                                }
                            }
                        } else if self.scheduler.is_deadlocked() {
                            return Err("deadlock: all fibers are blocked".to_string());
                        } else if !self.scheduler.has_runnable() {
                            return Ok(self.main_exit_code);
                        }
                        // Runnable exists but wasn't selected this pass; retry
                        // rather than falling through with no current fiber.
                        continue;
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

            // Auto-collection safe point. We are at an instruction boundary: the
            // operand stack, locals, call frames, and fiber state all hold their
            // `Value`s (and thus `Gc` handles) directly in owned Rust containers,
            // so they are GC roots; no `GcCell` borrow guard is alive here. This
            // is the ONLY place the VM forces a collection, upholding the
            // invariant "never collect while a GcCell is borrowed, never collect
            // mid-handler". Driving it here (rather than relying solely on the
            // implicit collection inside `Gc::new`) keeps it deterministic.
            if self.allocations >= Self::AUTO_COLLECT_INTERVAL {
                self.allocations = 0;
                gc::force_collect();
            }

            // Decode and execute in one step — avoid the separate decode_next call
            // which would re-check bounds and convert the byte twice.
            let opcode_ip = self.ip;
            let opcode_byte = self.program.code[opcode_ip];
            self.ip += 1;
            let opcode = Opcode::from_byte(opcode_byte).ok_or_else(|| {
                format!(
                    "Invalid opcode: 0x{:02X} at offset {}",
                    opcode_byte,
                    self.ip - 1
                )
            })?;

            match self.execute_opcode(opcode) {
                Ok(Some(exit_code)) => return Ok(exit_code),
                Ok(None) => {}
                Err(message) => {
                    self.capture_error_context_at(opcode_ip);
                    self.scheduler.fail_current(message.clone());
                    return Err(message);
                }
            }
        }
    }

    /// Execute a single opcode, returning Some(exit_code) for Halt, None otherwise
    fn execute_opcode(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            Opcode::Nop => Ok(None),

            // ── Typed integer arithmetic: no Value tag matching ──
            Opcode::IAdd => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Int(a.wrapping_add(b)));
                Ok(None)
            }
            Opcode::ISub => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Int(a.wrapping_sub(b)));
                Ok(None)
            }
            Opcode::IMul => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Int(a.wrapping_mul(b)));
                Ok(None)
            }
            Opcode::IDiv => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                if b == 0 {
                    return Err("Division by zero".to_string());
                }
                self.stack.push(Value::Int(a / b));
                Ok(None)
            }
            Opcode::IMod => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                if b == 0 {
                    return Err("Modulo by zero".to_string());
                }
                self.stack.push(Value::Int(a % b));
                Ok(None)
            }
            Opcode::INeg => {
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Int(a.wrapping_neg()));
                Ok(None)
            }

            // ── Typed float arithmetic ──
            Opcode::FAdd => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(a + b));
                Ok(None)
            }
            Opcode::FSub => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(a - b));
                Ok(None)
            }
            Opcode::FMul => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(a * b));
                Ok(None)
            }
            Opcode::FDiv => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(a / b));
                Ok(None)
            }
            Opcode::FMod => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(a % b));
                Ok(None)
            }
            Opcode::FNeg => {
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Float(-a));
                Ok(None)
            }

            // ── Typed integer comparison ──
            Opcode::IEq => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a == b));
                Ok(None)
            }
            Opcode::INe => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a != b));
                Ok(None)
            }
            Opcode::ILt => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a < b));
                Ok(None)
            }
            Opcode::ILe => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a <= b));
                Ok(None)
            }
            Opcode::IGt => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a > b));
                Ok(None)
            }
            Opcode::IGe => {
                let b = pop_int_fast(&mut self.stack)?;
                let a = pop_int_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a >= b));
                Ok(None)
            }

            // ── Typed float comparison ──
            Opcode::FEq => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a == b));
                Ok(None)
            }
            Opcode::FNe => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a != b));
                Ok(None)
            }
            Opcode::FLt => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a < b));
                Ok(None)
            }
            Opcode::FLe => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a <= b));
                Ok(None)
            }
            Opcode::FGt => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a > b));
                Ok(None)
            }
            Opcode::FGe => {
                let b = pop_float_fast(&mut self.stack)?;
                let a = pop_float_fast(&mut self.stack)?;
                self.stack.push(Value::Bool(a >= b));
                Ok(None)
            }

            // Stack / local / object / array / closure data operations
            Opcode::LoadConst
            | Opcode::Pop
            | Opcode::Dup
            | Opcode::CopyValue
            | Opcode::LoadLocal
            | Opcode::StoreLocal
            | Opcode::GetField
            | Opcode::SetField
            | Opcode::NewObject
            | Opcode::NewStruct
            | Opcode::InterfaceBox
            | Opcode::NewArray
            | Opcode::ArrayGet
            | Opcode::ArraySet
            | Opcode::ArrayLen
            | Opcode::ArrayPush
            | Opcode::ArrayPop
            | Opcode::NewTuple
            | Opcode::TupleSet
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
            | Opcode::TailCall
            | Opcode::InterfaceCall
            | Opcode::Return
            | Opcode::Halt => self.execute_control_flow(opcode),

            // Type operations
            Opcode::TypeIs | Opcode::Cast | Opcode::InterfaceIs => self.execute_type(opcode),

            // System operations
            Opcode::Print
            | Opcode::Println
            | Opcode::Assert
            | Opcode::Collect
            | Opcode::Syscall => self.execute_system(opcode),

            // Fiber and channel operations
            Opcode::Spawn
            | Opcode::Yield
            | Opcode::FiberId
            | Opcode::ChanNew
            | Opcode::ChanSend
            | Opcode::ChanRecv
            | Opcode::ChanClose
            | Opcode::Select => self.execute_fiber_channel(opcode),
        }
    }

    /// Stack, local variable, object, array and closure-data operations.
    #[inline(always)]
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
                self.pop()?;
            }

            Opcode::Dup => {
                let value = self.stack.last().cloned().ok_or("Stack underflow")?;
                self.stack.push(value);
            }

            Opcode::CopyValue => {
                let value = self.pop()?;
                let (copied, allocations) = value.semantic_copy_with_allocations();
                self.stack.push(copied);
                self.allocations = self.allocations.wrapping_add(allocations);
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

                // Match by reference: borrowing the handle and cloning what we
                // need keeps the `_` arm's `object.type_name()` usable too.
                match &object {
                    Value::Object(obj) | Value::Struct(obj) => {
                        let value = obj
                            .borrow()
                            .get(&*field_name)
                            .cloned()
                            .unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err(format!("Cannot get field from {}", object.type_name())),
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

                match &object {
                    Value::Object(obj) | Value::Struct(obj) => {
                        obj.borrow_mut().insert(field_name, value);
                    }
                    _ => return Err(format!("Cannot set field on {}", object.type_name())),
                }
            }

            Opcode::NewObject => {
                let obj = Gc::new(GcCell::new(HashMap::new()));
                self.note_allocation();
                self.stack.push(Value::Object(obj));
            }

            Opcode::NewStruct => {
                let obj = Gc::new(GcCell::new(HashMap::new()));
                self.note_allocation();
                self.stack.push(Value::Struct(obj));
            }

            Opcode::InterfaceBox => {
                let flags = self.read_u8()?;
                let method_count = self.read_u8()? as usize;
                let receiver = self.pop()?;
                let existing = match &receiver {
                    Value::Interface(interface) => Some(interface.clone()),
                    _ => None,
                };
                let mut methods = HashMap::with_capacity(method_count);
                for _ in 0..method_count {
                    let name_idx = self.read_u16()? as usize;
                    let name = match self.program.constants.get(name_idx) {
                        Some(Value::String(name)) => (**name).clone(),
                        _ => return Err("Invalid interface method name constant".to_string()),
                    };
                    let kind = self.read_u8()?;
                    let witness = match kind {
                        0 => match existing
                            .as_ref()
                            .and_then(|interface| interface.methods.get(&name))
                            .cloned()
                        {
                            Some(method) => method,
                            None => {
                                let value = match &receiver {
                                    Value::Object(object) | Value::Struct(object) => {
                                        object.borrow().get(&name).cloned().ok_or_else(|| {
                                            format!(
                                                "receiver does not provide interface method '{name}'"
                                            )
                                        })?
                                    }
                                    _ => {
                                        return Err(format!(
                                            "receiver does not provide interface method '{name}'"
                                        ));
                                    }
                                };
                                if !matches!(value, Value::Function(_) | Value::Closure(_)) {
                                    return Err(format!(
                                        "interface witness '{name}' is not callable"
                                    ));
                                }
                                InterfaceMethod::Value(value)
                            }
                        },
                        1 => InterfaceMethod::Value(Value::Function(self.read_u16()? as usize)),
                        2 => InterfaceMethod::Intrinsic(match self.read_u8()? {
                            0 => InterfaceIntrinsic::StringLen,
                            1 => InterfaceIntrinsic::ArrayLen,
                            2 => InterfaceIntrinsic::ArrayPush,
                            3 => InterfaceIntrinsic::ArrayPop,
                            _ => return Err("Invalid interface intrinsic method".to_string()),
                        }),
                        3 => match &receiver {
                            Value::Interface(interface) => {
                                interface.methods.get(&name).cloned().ok_or_else(|| {
                                    format!("receiver does not provide interface method '{name}'")
                                })?
                            }
                            Value::String(_) if name == "len" => {
                                InterfaceMethod::Intrinsic(InterfaceIntrinsic::StringLen)
                            }
                            Value::Array(_) if name == "len" => {
                                InterfaceMethod::Intrinsic(InterfaceIntrinsic::ArrayLen)
                            }
                            Value::Array(_) if name == "push" => {
                                InterfaceMethod::Intrinsic(InterfaceIntrinsic::ArrayPush)
                            }
                            Value::Array(_) if name == "pop" => {
                                InterfaceMethod::Intrinsic(InterfaceIntrinsic::ArrayPop)
                            }
                            Value::Object(object) | Value::Struct(object) => {
                                let value =
                                    object.borrow().get(&name).cloned().ok_or_else(|| {
                                        format!(
                                            "receiver does not provide interface method '{name}'"
                                        )
                                    })?;
                                if !matches!(value, Value::Function(_) | Value::Closure(_)) {
                                    return Err(format!(
                                        "interface witness '{name}' is not callable"
                                    ));
                                }
                                InterfaceMethod::Value(value)
                            }
                            _ => {
                                return Err(format!(
                                    "erased receiver cannot provide interface method '{name}'"
                                ));
                            }
                        },
                        _ => return Err("Invalid interface witness kind".to_string()),
                    };
                    methods.insert(name, witness);
                }

                if let Some(interface) = existing {
                    if methods
                        .keys()
                        .any(|name| !interface.methods.contains_key(name))
                    {
                        return Err("interface value is missing a required method".to_string());
                    }
                    self.stack.push(Value::Interface(interface));
                } else if matches!(&receiver, Value::Null) {
                    // Optional interface values use the same representation as
                    // their payload at runtime; null remains the empty option.
                    self.stack.push(Value::Null);
                } else {
                    let receiver = if flags & 1 != 0 {
                        let (receiver, allocations) = receiver.semantic_copy_with_allocations();
                        self.allocations = self.allocations.wrapping_add(allocations);
                        receiver
                    } else {
                        receiver
                    };
                    self.note_allocation();
                    self.stack.push(Value::Interface(Gc::new(InterfaceData {
                        receiver,
                        methods,
                    })));
                }
            }

            // Array operations
            Opcode::NewArray => {
                let elements = allocate_null_collection("Array", self.pop()?)?;
                let arr = Gc::new(GcCell::new(elements));
                self.note_allocation();
                self.stack.push(Value::Array(arr));
            }

            Opcode::NewTuple => {
                let elements = allocate_null_collection("Tuple", self.pop()?)?;
                let tuple = Gc::new(GcCell::new(TupleData::initializing(elements)));
                self.note_allocation();
                self.stack.push(Value::Tuple(tuple));
            }

            Opcode::ArrayGet => {
                let index = self.pop()?;
                let array = self.pop()?;

                match (&array, &index) {
                    (Value::Array(arr), Value::Int(idx)) => {
                        if *idx < 0 {
                            return Err("Index out of bounds: negative index".to_string());
                        }
                        let idx = *idx as usize;
                        let value = arr
                            .borrow()
                            .get(idx)
                            .cloned()
                            .ok_or_else(|| "Index out of bounds".to_string())?;
                        self.stack.push(value);
                    }
                    (Value::Tuple(tuple), Value::Int(idx)) => {
                        if *idx < 0 {
                            return Err("Index out of bounds: negative index".to_string());
                        }
                        let idx = *idx as usize;
                        let value = tuple
                            .borrow()
                            .elements
                            .get(idx)
                            .cloned()
                            .ok_or_else(|| "Index out of bounds".to_string())?;
                        self.stack.push(value);
                    }
                    (Value::String(s), Value::Int(idx)) => {
                        if *idx < 0 {
                            return Err("Index out of bounds: negative index".to_string());
                        }
                        let idx = *idx as usize;
                        let value = s
                            .chars()
                            .nth(idx)
                            .map(|c| Value::String(Rc::new(c.to_string())))
                            .ok_or_else(|| "Index out of bounds".to_string())?;
                        self.stack.push(value);
                    }
                    // Support object indexing with string keys (for JSON objects)
                    (Value::Object(obj) | Value::Struct(obj), Value::String(key)) => {
                        let value = obj.borrow().get(&**key).cloned().unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err("Invalid array/index types".to_string()),
                }
            }

            Opcode::ArraySet => {
                let value = self.pop()?;
                let index = self.pop()?;
                let array = self.pop()?;

                match (&array, &index) {
                    (Value::Array(arr), Value::Int(idx)) => {
                        if *idx < 0 {
                            return Err("Array index out of bounds: negative index".to_string());
                        }
                        let idx = *idx as usize;
                        let mut arr = arr.borrow_mut();
                        if idx >= arr.len() {
                            return Err(format!(
                                "Array index out of bounds: {} (len={})",
                                idx,
                                arr.len()
                            ));
                        }
                        arr[idx] = value;
                    }
                    _ => return Err("Invalid array/index types".to_string()),
                }
            }

            Opcode::TupleSet => {
                let value = self.pop()?;
                let index = self.pop()?;
                let tuple = self.pop()?;

                match (&tuple, &index) {
                    (Value::Tuple(tuple), Value::Int(idx)) => {
                        if *idx < 0 {
                            return Err("Tuple index out of bounds: negative index".to_string());
                        }
                        let idx = *idx as usize;
                        let mut tuple = tuple.borrow_mut();
                        if idx >= tuple.elements.len() {
                            return Err(format!(
                                "Tuple index out of bounds: {} (len={})",
                                idx,
                                tuple.elements.len()
                            ));
                        }
                        let Some(expected) = tuple.next_initializer else {
                            return Err(
                                "TupleSet is only valid while constructing a tuple".to_string()
                            );
                        };
                        if idx != expected {
                            return Err(format!(
                                "Tuple elements must be initialized in order: expected index {expected}, got {idx}"
                            ));
                        }
                        tuple.elements[idx] = value;
                        tuple.next_initializer =
                            (expected + 1 < tuple.elements.len()).then_some(expected + 1);
                    }
                    _ => return Err("Invalid tuple/index types".to_string()),
                }
            }

            Opcode::ArrayLen => {
                let array = self.pop()?;
                let len = match &array {
                    Value::Array(arr) => arr.borrow().len() as i64,
                    Value::Tuple(tuple) => tuple.borrow().elements.len() as i64,
                    Value::Object(obj) => obj.borrow().len() as i64,
                    Value::String(s) => s.len() as i64,
                    _ => return Err("Cannot get length of non-sized value".to_string()),
                };
                self.stack.push(Value::Int(len));
            }

            Opcode::ArrayPush => {
                let value = self.pop()?;
                let array = self.pop()?;
                match &array {
                    Value::Array(arr) => {
                        let mut arr = arr.borrow_mut();
                        if arr.len() >= MAX_COLLECTION_ELEMENTS {
                            return Err(format!(
                                "Array size exceeds VM collection limit of {MAX_COLLECTION_ELEMENTS} elements"
                            ));
                        }
                        arr.try_reserve_exact(1).map_err(|_| {
                            format!(
                                "Array allocation failed while growing to {} elements",
                                arr.len() + 1
                            )
                        })?;
                        arr.push(value);
                    }
                    _ => return Err("Cannot push to non-array".to_string()),
                }
            }

            Opcode::ArrayPop => {
                let array = self.pop()?;
                match &array {
                    Value::Array(arr) => {
                        let value = arr.borrow_mut().pop().unwrap_or(Value::Null);
                        self.stack.push(value);
                    }
                    _ => return Err("Cannot pop from non-array".to_string()),
                }
            }

            // Closure operations
            Opcode::MakeClosure => {
                // TODO: The closure code offset is encoded as u16, limiting
                // function targets to offsets < 65536. If the code section
                // exceeds 64 KiB, closures cannot reference functions in the
                // later part. This is a bytecode format limitation; fixing it
                // would require widening the instruction encoding (e.g. using
                // a u32 operand or a constant-pool indirection).
                let code_offset = self.read_u16()? as usize;
                let capture_count = self.read_u8()? as usize;

                // The captured values are the top `capture_count` operand-stack
                // slots in declaration order; drain them in one move.
                let start = self.stack_top_start(capture_count)?;
                let captures: Vec<Value> = self.stack.drain(start..).collect();

                let closure = ClosureData {
                    code_offset,
                    captures,
                };
                self.note_allocation();
                self.stack.push(Value::Closure(Gc::new(closure)));
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
    #[inline(always)]
    fn execute_arithmetic(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Arithmetic operations
            Opcode::Add => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_add(*b)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    (Value::String(a), Value::String(b)) => {
                        Value::String(Rc::new(format!("{}{}", a, b)))
                    }
                    (Value::String(a), b) => Value::String(Rc::new(format!("{}{}", a, b))),
                    (a, Value::String(b)) => Value::String(Rc::new(format!("{}{}", a, b))),
                    _ => {
                        return Err(format!(
                            "Cannot add {} and {}",
                            a.type_name(),
                            b.type_name()
                        ))
                    }
                };
                self.stack.push(result);
            }

            Opcode::Sub => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_sub(*b)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 - b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a - *b as f64),
                    _ => {
                        return Err(format!(
                            "Cannot subtract {} from {}",
                            b.type_name(),
                            a.type_name()
                        ))
                    }
                };
                self.stack.push(result);
            }

            Opcode::Mul => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match (&a, &b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a.wrapping_mul(*b)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 * b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a * *b as f64),
                    _ => {
                        return Err(format!(
                            "Cannot multiply {} and {}",
                            a.type_name(),
                            b.type_name()
                        ))
                    }
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
                    _ => {
                        return Err(format!(
                            "Cannot divide {} by {}",
                            a.type_name(),
                            b.type_name()
                        ))
                    }
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
                    (Value::Int(a), Value::Float(b)) => {
                        if *b == 0.0 {
                            return Err("Modulo by zero".to_string());
                        }
                        Value::Float(*a as f64 % b)
                    }
                    (Value::Float(a), Value::Int(b)) => {
                        if *b == 0 {
                            return Err("Modulo by zero".to_string());
                        }
                        Value::Float(a % *b as f64)
                    }
                    _ => {
                        return Err(format!(
                            "Cannot modulo {} by {}",
                            a.type_name(),
                            b.type_name()
                        ))
                    }
                };
                self.stack.push(result);
            }

            Opcode::Neg => {
                let a = self.pop()?;
                let result = match a {
                    Value::Int(n) => Value::Int(n.wrapping_neg()),
                    Value::Float(f) => Value::Float(-f),
                    _ => return Err(format!("Cannot negate {}", a.type_name())),
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
                        Value::Int(base.wrapping_pow(*exp as u32))
                    }
                    (Value::Float(base), Value::Float(exp)) => Value::Float(base.powf(*exp)),
                    (Value::Int(base), Value::Float(exp)) => {
                        Value::Float((*base as f64).powf(*exp))
                    }
                    (Value::Float(base), Value::Int(exp)) => Value::Float(base.powi(*exp as i32)),
                    _ => {
                        return Err(format!(
                            "Cannot compute power of {} ^ {}",
                            a.type_name(),
                            b.type_name()
                        ))
                    }
                };
                self.stack.push(result);
            }

            _ => unreachable!("execute_arithmetic called with non-arithmetic opcode"),
        }
        Ok(None)
    }

    /// Comparison, logical and bitwise operations.
    #[inline(always)]
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
                let result = match self.compare_values(&a, &b) {
                    Ok(ord) => ord < 0,
                    Err(_) => false, // NaN is unordered
                };
                self.stack.push(Value::Bool(result));
            }

            Opcode::Le => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match self.compare_values(&a, &b) {
                    Ok(ord) => ord <= 0,
                    Err(_) => false, // NaN is unordered
                };
                self.stack.push(Value::Bool(result));
            }

            Opcode::Gt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match self.compare_values(&a, &b) {
                    Ok(ord) => ord > 0,
                    Err(_) => false, // NaN is unordered
                };
                self.stack.push(Value::Bool(result));
            }

            Opcode::Ge => {
                let b = self.pop()?;
                let a = self.pop()?;
                let result = match self.compare_values(&a, &b) {
                    Ok(ord) => ord >= 0,
                    Err(_) => false, // NaN is unordered
                };
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
                    (Value::Int(x), Value::Int(y)) => {
                        self.stack.push(Value::Int(x << ((y as u32).min(63))))
                    }
                    _ => return Err("Shift left requires integer operands".to_string()),
                }
            }

            Opcode::Shr => {
                let b = self.pop()?;
                let a = self.pop()?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        // Arithmetic right shift (preserves sign)
                        self.stack.push(Value::Int(x >> ((y as u32).min(63))))
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
                        let result = ((x as u64) >> ((y as u32).min(63))) as i64;
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
    #[inline(always)]
    fn execute_control_flow(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        /// Compute the target of a relative jump and validate it lies within
        /// the code section. A jump past the end would otherwise fall through
        /// the interpreter loop and silently report a clean exit.
        fn jump_target(ip: usize, offset: i16, code_len: usize) -> Result<usize, String> {
            let target = (ip as isize).wrapping_add(offset as isize);
            if target < 0 {
                return Err("jump target out of bounds".to_string());
            }
            let target = target as usize;
            if target >= code_len {
                return Err(format!(
                    "jump target {target} is out of bounds for a {code_len} byte code section"
                ));
            }
            Ok(target)
        }

        match opcode {
            // Control flow
            Opcode::Jump => {
                let offset = self.read_i16()?;
                self.ip = jump_target(self.ip, offset, self.program.code.len())?;
            }

            Opcode::JumpIfTrue => {
                let offset = self.read_i16()?;
                let condition = self.pop()?;
                if condition.is_truthy() {
                    self.ip = jump_target(self.ip, offset, self.program.code.len())?;
                }
            }

            Opcode::JumpIfFalse => {
                let offset = self.read_i16()?;
                let condition = self.pop()?;
                if !condition.is_truthy() {
                    self.ip = jump_target(self.ip, offset, self.program.code.len())?;
                }
            }

            // Function operations
            Opcode::InterfaceCall => {
                let method_idx = self.read_u16()? as usize;
                let arg_count = self.read_u8()? as usize;
                let method_name = match self.program.constants.get(method_idx) {
                    Some(Value::String(name)) => (**name).clone(),
                    _ => return Err("Invalid interface method name constant".to_string()),
                };
                let start = self.stack_top_start(arg_count + 1)?;
                let interface = self.stack.get(start).cloned().ok_or("Stack underflow")?;
                let Value::Interface(interface) = interface else {
                    return Err(format!(
                        "Cannot call interface method on {}",
                        interface.type_name()
                    ));
                };
                let method = interface
                    .methods
                    .get(&method_name)
                    .cloned()
                    .ok_or_else(|| format!("interface has no method '{method_name}'"))?;
                match &method {
                    InterfaceMethod::Value(callee) => {
                        self.stack[start] = interface.receiver.clone();
                        match callee {
                            Value::Function(code_offset) => {
                                let frame = CallFrame {
                                    func_offset: *code_offset,
                                    return_addr: self.ip,
                                    locals_base: self.locals.len(),
                                    local_count: arg_count + 1,
                                    stack_base: start,
                                    captures: None,
                                };
                                self.call_stack.push(frame);
                                self.locals.extend(self.stack.drain(start..));
                                self.ip = *code_offset;
                            }
                            Value::Closure(closure_data) => {
                                let frame = CallFrame {
                                    func_offset: closure_data.code_offset,
                                    return_addr: self.ip,
                                    locals_base: self.locals.len(),
                                    local_count: arg_count + 1,
                                    stack_base: start,
                                    captures: Some(closure_data.clone()),
                                };
                                self.call_stack.push(frame);
                                self.locals.extend(self.stack.drain(start..));
                                self.ip = closure_data.code_offset;
                            }
                            other => return Err(format!("Cannot call {}", other.type_name())),
                        }
                    }
                    InterfaceMethod::Intrinsic(intrinsic) => {
                        let arguments: Vec<Value> = self.stack.drain(start..).collect();
                        let receiver = interface.receiver.clone();
                        let explicit = &arguments[1..];
                        let result = match intrinsic {
                            InterfaceIntrinsic::StringLen if explicit.is_empty() => {
                                match receiver {
                                    Value::String(value) => {
                                        // `len` on a string is a byte count
                                        // (matching `len()`, the docs, and the
                                        // native backend), not a scalar count.
                                        Value::Int(value.len() as i64)
                                    }
                                    _ => {
                                        return Err("interface string len receiver is not a string"
                                            .to_string())
                                    }
                                }
                            }
                            InterfaceIntrinsic::ArrayLen if explicit.is_empty() => match receiver {
                                Value::Array(array) => Value::Int(array.borrow().len() as i64),
                                _ => {
                                    return Err(
                                        "interface array len receiver is not an array".to_string()
                                    )
                                }
                            },
                            InterfaceIntrinsic::ArrayPush if explicit.len() == 1 => {
                                match receiver {
                                    Value::Array(array) => {
                                        if array.borrow().len() >= MAX_COLLECTION_ELEMENTS {
                                            return Err("Array size exceeds VM collection limit"
                                                .to_string());
                                        }
                                        array.borrow_mut().push(explicit[0].clone());
                                        Value::Null
                                    }
                                    _ => {
                                        return Err("interface array push receiver is not an array"
                                            .to_string())
                                    }
                                }
                            }
                            InterfaceIntrinsic::ArrayPop if explicit.is_empty() => match receiver {
                                Value::Array(array) => {
                                    array.borrow_mut().pop().unwrap_or(Value::Null)
                                }
                                _ => {
                                    return Err(
                                        "interface array pop receiver is not an array".to_string()
                                    )
                                }
                            },
                            _ => return Err("invalid interface intrinsic arguments".to_string()),
                        };
                        self.stack.push(result);
                    }
                }
            }
            Opcode::Call => {
                let arg_count = self.read_u8()? as usize;
                let callee = self.pop()?;

                // Match by reference; cloning the `Gc` handle inside the arm is
                // a cheap root-count bump.
                match &callee {
                    Value::Function(code_offset) => {
                        let code_offset = *code_offset;
                        // The arguments are the top `arg_count` operand-stack
                        // slots, already in declaration order; move them into
                        // locals with one drain instead of pop + reverse into a
                        // temporary Vec (which allocated on every call).
                        let start = self.stack_top_start(arg_count)?;

                        // Save stack base AFTER reserving args - this isolates the caller's stack
                        let frame = CallFrame {
                            func_offset: code_offset,
                            return_addr: self.ip,
                            locals_base: self.locals.len(),
                            local_count: arg_count,
                            stack_base: start,
                            captures: None,
                        };
                        self.call_stack.push(frame);

                        self.locals.extend(self.stack.drain(start..));

                        // Jump to function
                        self.ip = code_offset;
                    }
                    Value::Closure(closure_data) => {
                        let start = self.stack_top_start(arg_count)?;

                        // Save stack base AFTER reserving args - this isolates the caller's stack
                        let frame = CallFrame {
                            func_offset: closure_data.code_offset,
                            return_addr: self.ip,
                            locals_base: self.locals.len(),
                            local_count: arg_count,
                            stack_base: start,
                            captures: Some(closure_data.clone()),
                        };
                        self.call_stack.push(frame);

                        self.locals.extend(self.stack.drain(start..));

                        // Jump to closure code
                        self.ip = closure_data.code_offset;
                    }
                    _ => return Err(format!("Cannot call {}", callee.type_name())),
                }
            }

            Opcode::TailCall => {
                let arg_count = self.read_u8()? as usize;
                let callee = self.pop()?;

                match &callee {
                    Value::Function(code_offset) => {
                        let code_offset = *code_offset;
                        let start = self.stack_top_start(arg_count)?;

                        // Reuse the current frame: overwrite locals and jump.
                        // The callee's Return will return to our caller.
                        let locals_base = self.locals_base();
                        self.locals.truncate(locals_base);
                        self.locals.extend(self.stack.drain(start..));
                        if let Some(frame) = self.call_stack.last_mut() {
                            frame.captures = None;
                            frame.func_offset = code_offset;
                            frame.local_count = arg_count;
                        }
                        self.ip = code_offset;
                    }
                    Value::Closure(closure_data) => {
                        let start = self.stack_top_start(arg_count)?;

                        let locals_base = self.locals_base();
                        self.locals.truncate(locals_base);
                        self.locals.extend(self.stack.drain(start..));
                        if let Some(frame) = self.call_stack.last_mut() {
                            frame.captures = Some(closure_data.clone());
                            frame.func_offset = closure_data.code_offset;
                            frame.local_count = arg_count;
                        }
                        self.ip = closure_data.code_offset;
                    }
                    _ => return Err(format!("Cannot tail-call {}", callee.type_name())),
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
    #[inline(always)]
    fn execute_type(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // Type operations
            Opcode::TypeIs => {
                let type_id = self.read_u8()?;
                let value = self.pop()?;
                let matches = value_matches_type_id(&value, type_id);
                self.stack.push(Value::Bool(matches));
            }

            Opcode::Cast => {
                let type_id = self.read_u8()?;
                let value = self.pop()?;
                validate_runtime_cast(&value, type_id)?;
                let result = match type_id {
                    // Cast to int.
                    2 => match &value {
                        Value::Int(n) => Value::Int(*n),
                        Value::Float(f) => Value::Int(*f as i64),
                        Value::Bool(b) => Value::Int(if *b { 1 } else { 0 }),
                        Value::String(s) => Value::Int(s.parse().unwrap_or(0)),
                        _ => Value::Int(0),
                    },
                    // Cast to float
                    3 => match &value {
                        Value::Int(n) => Value::Float(*n as f64),
                        Value::Float(f) => Value::Float(*f),
                        Value::Bool(b) => Value::Float(if *b { 1.0 } else { 0.0 }),
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

            Opcode::InterfaceIs => {
                let source_type = self.read_u8()?;
                let method_count = self.read_u8()? as usize;
                let value = self.pop()?;
                let mut matches = match &value {
                    Value::Interface(_) => true,
                    _ if source_type != u8::MAX => value_matches_type_id(&value, source_type),
                    _ => true,
                };
                for _ in 0..method_count {
                    let name_idx = self.read_u16()? as usize;
                    let name = match self.program.constants.get(name_idx) {
                        Some(Value::String(name)) => (**name).clone(),
                        _ => return Err("Invalid interface method name constant".to_string()),
                    };
                    let kind = self.read_u8()?;
                    let witness_matches = match kind {
                        0 => match &value {
                            Value::Interface(interface) => interface.methods.contains_key(&name),
                            Value::Object(object) | Value::Struct(object) => {
                                object.borrow().get(&name).is_some_and(|value| {
                                    matches!(value, Value::Function(_) | Value::Closure(_))
                                })
                            }
                            _ => false,
                        },
                        1 => source_type != u8::MAX,
                        2 => match self.read_u8()? {
                            0 => matches!(value, Value::String(_)),
                            1..=3 => matches!(value, Value::Array(_)),
                            _ => false,
                        },
                        3 => match &value {
                            Value::Interface(interface) => interface.methods.contains_key(&name),
                            Value::String(_) => name == "len",
                            Value::Array(_) => matches!(name.as_str(), "len" | "push" | "pop"),
                            Value::Object(object) | Value::Struct(object) => {
                                object.borrow().get(&name).is_some_and(|value| {
                                    matches!(value, Value::Function(_) | Value::Closure(_))
                                })
                            }
                            _ => false,
                        },
                        _ => return Err("Invalid interface witness kind".to_string()),
                    };
                    matches &= witness_matches;
                }
                self.stack.push(Value::Bool(matches));
            }

            _ => unreachable!("execute_type called with non-type opcode"),
        }
        Ok(None)
    }

    /// System operations (print and syscall dispatch).
    #[inline(always)]
    fn execute_system(&mut self, opcode: Opcode) -> Result<Option<i32>, String> {
        match opcode {
            // System operations
            Opcode::Print | Opcode::Println => {
                let value = self.pop()?;
                let output_str = render_output_value(&value)?;
                let newline = opcode == Opcode::Println;

                if self.capture_output {
                    self.capture_output_chunk(&output_str, newline)?;
                }

                // Stream one exact chunk per source-level output call.
                if let Some(ref mut callback) = self.output_callback {
                    if newline {
                        let mut chunk = output_str.clone();
                        chunk.try_reserve(1).map_err(|_| {
                            "output callback allocation exceeded available memory".to_string()
                        })?;
                        chunk.push('\n');
                        callback(&chunk);
                    } else {
                        callback(&output_str);
                    }
                }

                if !self.capture_output && newline {
                    println!("{}", output_str);
                } else if !self.capture_output {
                    print!("{}", output_str);
                }
            }

            Opcode::Assert => match self.pop()? {
                Value::Bool(true) => {}
                Value::Bool(false) => return Err("assertion failed".to_string()),
                value => return Err(format!("assert expects bool, got {}", value.type_name())),
            },

            Opcode::Collect => {
                // The `collect()` builtin. We are at an opcode-dispatch boundary:
                // every `execute_*` handler releases its `GcCell` borrows before
                // returning, so no interior-mutability guard is alive here and it
                // is sound to run a collection (see the "never collect while a
                // GcCell is borrowed" invariant documented at the main loop).
                gc::force_collect();
                self.stack.push(Value::Null);
            }

            Opcode::Syscall => {
                let syscall_num = self.read_u8()?;
                self.handle_syscall(syscall_num)?;
                if self.exit_requested {
                    return Ok(Some(self.main_exit_code));
                }
            }

            _ => unreachable!("execute_system called with non-system opcode"),
        }
        Ok(None)
    }

    /// Fiber and channel operations.
    ///
    /// Kept together as a single unit so the scheduler/channel machinery can be
    /// evolved cohesively by later work.
    #[inline(always)]
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
                    Value::Int(n) => usize::try_from(n)
                        .map_err(|_| "Channel capacity must be non-negative".to_string())?,
                    _ => return Err("Channel capacity must be an integer".to_string()),
                };
                let channel_id = self.scheduler.create_channel(capacity)?;
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
    /// Semantics (Go-like): probe all recv/send arms and choose one ready arm
    /// with the VM's deterministic SplitMix64 arbiter. A recv arm pushes the
    /// received value for the body to bind. If none is ready and a `default`
    /// arm exists, run it (non-blocking poll). Otherwise park the fiber until
    /// any arm becomes ready, then re-run this opcode and re-arbitrate.
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
            let round_seed = self.next_select_seed();
            let is_recv = res.recv.is_some();
            let selected = arms
                .iter()
                .enumerate()
                .filter_map(|(index, arm)| match arm {
                    Arm::Recv { channel } if is_recv && *channel == res.channel_id => Some((
                        index,
                        Self::select_arm_score(round_seed, index, *channel, false),
                    )),
                    Arm::Send { channel, .. } if !is_recv && *channel == res.channel_id => Some((
                        index,
                        Self::select_arm_score(round_seed, index, *channel, true),
                    )),
                    _ => None,
                })
                .max_by_key(|(_, score)| *score)
                .map(|(index, _)| index);
            if let Some(index) = selected {
                self.deregister_current_select_waiter();
                if let Some((value, _ok)) = res.recv {
                    self.stack.push(value);
                }
                self.ip = body_targets[index];
                return Ok(None);
            }
            // Resolution did not match any arm (spurious); fall through to a
            // fresh poll below.
        }

        // Probe every communication arm without mutating channel state. The
        // selected index is then committed exactly once, so source order does
        // not privilege receives or earlier arms over other ready choices.
        let ready_indices: Vec<usize> = arms
            .iter()
            .enumerate()
            .filter_map(|(index, arm)| match arm {
                Arm::Recv { channel } => (self.scheduler.select_receive_ready(*channel)
                    && !self.scheduler.select_receive_deferred(*channel))
                .then_some(index),
                Arm::Send { channel, .. } => (self.scheduler.select_send_ready(*channel)
                    && !self.scheduler.select_send_deferred(*channel))
                .then_some(index),
                Arm::Default => None,
            })
            .collect();

        let deferred_communication = arms.iter().any(|arm| match arm {
            Arm::Recv { channel } => self.scheduler.select_receive_deferred(*channel),
            Arm::Send { channel, .. } => self.scheduler.select_send_deferred(*channel),
            Arm::Default => false,
        });

        if !ready_indices.is_empty() {
            let round_seed = self.next_select_seed();
            let arm_idx = ready_indices
                .iter()
                .copied()
                .max_by_key(|index| match &arms[*index] {
                    Arm::Recv { channel } => {
                        Self::select_arm_score(round_seed, *index, *channel, false)
                    }
                    Arm::Send { channel, .. } => {
                        Self::select_arm_score(round_seed, *index, *channel, true)
                    }
                    Arm::Default => unreachable!("default arms are not ready"),
                })
                .expect("ready arm list is non-empty");
            match &arms[arm_idx] {
                Arm::Recv { channel } => {
                    // A cooperative VM cannot mutate this channel between the
                    // probe and commit, so a ready receive must commit. Keep a
                    // defensive fallback for malformed/stale scheduler state.
                    if let Some((value, _ok)) = self.scheduler.try_select_receive(*channel) {
                        self.deregister_current_select_waiter();
                        self.stack.push(value);
                        self.ip = body_targets[arm_idx];
                        return Ok(None);
                    }
                }
                Arm::Send { channel, value } => {
                    if self.scheduler.try_select_send(*channel, value.clone()) {
                        self.deregister_current_select_waiter();
                        self.ip = body_targets[arm_idx];
                        return Ok(None);
                    }
                }
                Arm::Default => unreachable!("default arms are not communication-ready"),
            }
        }

        let recv_ids: Vec<ChannelId> = arms
            .iter()
            .filter_map(|a| match a {
                Arm::Recv { channel } => Some(*channel),
                _ => None,
            })
            .collect();

        // No arm ready. If a default exists, run it (non-blocking poll).
        if !deferred_communication {
            if let Some((i, _)) = arms
                .iter()
                .enumerate()
                .find(|(_, a)| matches!(a, Arm::Default))
            {
                self.deregister_current_select_waiter();
                self.ip = body_targets[i];
                return Ok(None);
            }
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
        if deferred_communication {
            let send_ids: Vec<ChannelId> = send_specs.iter().map(|(id, _)| *id).collect();
            self.scheduler
                .wake_select_counterparts(&recv_ids, &send_ids);
        }
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

    #[inline(always)]
    fn pop(&mut self) -> Result<Value, String> {
        let stack_base = self.stack_base();
        if self.stack.len() <= stack_base {
            return Err("Stack underflow".to_string());
        }
        self.stack
            .pop()
            .ok_or_else(|| "Stack underflow".to_string())
    }

    /// Start index of the top `count` operand-stack values, or an underflow
    /// error. Used with `Vec::drain` to move argument/capture blocks out of
    /// the stack in one go (they are already in declaration order), avoiding
    /// a per-call temporary Vec plus `reverse()`. The check is against the
    /// current frame's stack base so malformed bytecode can never drain into
    /// the caller's stack slots.
    #[inline(always)]
    fn stack_top_start(&self, count: usize) -> Result<usize, String> {
        let base = self.stack_base();
        if self.stack.len() < base + count {
            return Err("Stack underflow".to_string());
        }
        Ok(self.stack.len() - count)
    }

    /// Get the stack base for the current frame (to protect caller's values)
    #[inline(always)]
    fn stack_base(&self) -> usize {
        self.call_stack.last().map(|f| f.stack_base).unwrap_or(0)
    }
}

/// Fast unchecked pop of an int from the stack. Skips the stack-base guard
/// (the codegen ensures balanced pushes before typed opcodes). Returns the
/// raw i64 value on success or an error if the stack is empty / wrong type.
#[inline(always)]
fn pop_int_fast(stack: &mut Vec<Value>) -> Result<i64, String> {
    match stack.pop() {
        Some(Value::Int(n)) => Ok(n),
        Some(v) => Err(format!("Expected int, got {}", v.type_name())),
        None => Err("Stack underflow".to_string()),
    }
}

#[inline(always)]
fn pop_float_fast(stack: &mut Vec<Value>) -> Result<f64, String> {
    match stack.pop() {
        Some(Value::Float(f)) => Ok(f),
        Some(v) => Err(format!("Expected float, got {}", v.type_name())),
        None => Err("Stack underflow".to_string()),
    }
}

fn allocate_null_collection(kind: &str, size: Value) -> Result<Vec<Value>, String> {
    let Value::Int(size) = size else {
        return Err(format!("{kind} size must be an integer"));
    };
    let size = usize::try_from(size).map_err(|_| format!("{kind} size must be non-negative"))?;
    if size > MAX_COLLECTION_ELEMENTS {
        return Err(format!(
            "{kind} size exceeds VM collection limit of {MAX_COLLECTION_ELEMENTS} elements"
        ));
    }

    let mut elements = Vec::new();
    elements
        .try_reserve_exact(size)
        .map_err(|_| format!("{kind} allocation failed while creating {size} elements"))?;
    elements.resize(size, Value::Null);
    Ok(elements)
}

impl VM {
    #[inline(always)]
    fn read_u8(&mut self) -> Result<u8, String> {
        // `get` gives a single bounds check instead of indexing + separate len check.
        let value = *self
            .program
            .code
            .get(self.ip)
            .ok_or_else(|| "Unexpected end of bytecode".to_string())?;
        self.ip += 1;
        Ok(value)
    }

    #[inline(always)]
    fn read_u16(&mut self) -> Result<u16, String> {
        let ip = self.ip;
        let bytes = self
            .program
            .code
            .get(ip..ip + 2)
            .ok_or_else(|| "Unexpected end of bytecode".to_string())?;
        self.ip = ip + 2;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    #[inline(always)]
    fn read_i16(&mut self) -> Result<i16, String> {
        Ok(self.read_u16()? as i16)
    }

    #[inline(always)]
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
                if a.is_nan() || b.is_nan() {
                    Err("NaN is unordered".to_string())
                } else {
                    Ok(a.partial_cmp(b).unwrap() as i32)
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if b.is_nan() {
                    Err("NaN is unordered".to_string())
                } else {
                    let a = *a as f64;
                    Ok(a.partial_cmp(b).unwrap() as i32)
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if a.is_nan() {
                    Err("NaN is unordered".to_string())
                } else {
                    let b = *b as f64;
                    Ok(a.partial_cmp(&b).unwrap() as i32)
                }
            }
            (Value::String(a), Value::String(b)) => Ok(a.cmp(b) as i32),
            _ => Err(format!(
                "Cannot compare {} and {}",
                a.type_name(),
                b.type_name()
            )),
        }
    }

    fn handle_syscall(&mut self, num: u8) -> Result<(), String> {
        // The vast majority of syscalls follow one uniform shape: pop N typed
        // arguments (in reverse-of-push order), call a single `self.runtime`
        // method, then push the wrapped result. The `syscall!` macro below
        // generates those arms so they stay byte-for-byte equivalent to the
        // hand-written form: all arguments are popped first, then matched as one
        // tuple against the expected types (preserving the same pop order and the
        // same single "requires ... arguments" error on a type mismatch).
        //
        // Irregular arms (sys_exit, print/println, env_get's Option->Null,
        // json passthrough, HTTP response tuples, and the few syscalls with
        // bespoke error/Option handling) are left explicit and individually
        // commented further down.
        //
        // Result specifiers wrap the runtime method's return into a Value:
        //   Str      String      -> Value::String
        //   Int      i64         -> Value::Int
        //   Bool     bool        -> Value::Bool
        //   Float    f64         -> Value::Float
        //   StrArray Vec<String> -> Value::Array of Value::String
        //   IntArray Vec<i64>    -> Value::Array of Value::Int
        macro_rules! sys_wrap {
            (Str, $e:expr) => {
                Value::String(Rc::new($e))
            };
            (Int, $e:expr) => {
                Value::Int($e)
            };
            (Bool, $e:expr) => {
                Value::Bool($e)
            };
            (Float, $e:expr) => {
                Value::Float($e)
            };
            (StrArray, $e:expr) => {
                Value::Array(Gc::new(GcCell::new(
                    $e.into_iter()
                        .map(|s| Value::String(Rc::new(s)))
                        .collect::<Vec<_>>(),
                )))
            };
            (IntArray, $e:expr) => {
                Value::Array(Gc::new(GcCell::new(
                    $e.into_iter().map(Value::Int).collect::<Vec<_>>(),
                )))
            };
        }
        // `sys_typed!` covers the str/int-argument arms: it pops every argument
        // first (in last-pushed-first order), then matches them all as a single
        // tuple so a type mismatch yields one combined error and never leaves a
        // partially-drained stack -- identical to the original hand-written form.
        //
        //   sys_pat!  -> the per-arg match pattern (binds the inner value)
        //   sys_call! -> how the bound value is passed to the runtime method
        //   sys_desc! -> the type word used in the mismatch error message
        macro_rules! sys_pat {
            (str, $b:ident) => {
                Value::String($b)
            };
            (int, $b:ident) => {
                Value::Int($b)
            };
        }
        // Bindings come from a by-reference match (see `sys_typed!`), so `str`
        // binds `&IString` and `int` binds `&i64`. Deref to the shapes the
        // runtime methods expect (`&str` and `i64`).
        macro_rules! sys_call {
            (str, $b:ident) => {
                &**$b
            };
            (int, $b:ident) => {
                *$b
            };
        }
        macro_rules! sys_desc {
            (str) => {
                "string"
            };
            (int) => {
                "int"
            };
        }
        macro_rules! sys_typed {
            // Single argument: error reads "NAME requires <type> argument".
            ($name:literal, $a:ident $b:ident => $ret:ident : $method:ident) => {{
                let $b = self.pop()?;
                // Match by reference; `sys_call!` derefs the resulting reference
                // bindings back to the method's arg types.
                match &$b {
                    sys_pat!($a, $b) => {
                        let result = self.runtime.$method(sys_call!($a, $b));
                        self.stack.push(sys_wrap!($ret, result));
                        Ok(())
                    }
                    _ => Err(concat!($name, " requires ", sys_desc!($a), " argument").to_string()),
                }
            }};
            // Multiple arguments: error reads "NAME requires (t1, t2, ...) arguments".
            ($name:literal, $($a:ident $b:ident),+ => $ret:ident : $method:ident) => {{
                // Reverse-pop so the right-most (last-pushed) arg is popped first.
                sys_typed!(@pop_rev self, $($a $b),+);
                match ( $( &$b ),+ , ) {
                    ( $( sys_pat!($a, $b) ),+ , ) => {
                        let result = self.runtime.$method( $( sys_call!($a, $b) ),+ );
                        self.stack.push(sys_wrap!($ret, result));
                        Ok(())
                    }
                    _ => Err(concat!(
                        $name, " requires (",
                        sys_typed!(@desc_list $($a),+),
                        ") arguments"
                    ).to_string()),
                }
            }};
            // Pop the argument list right-to-left, recursing on the tail first so
            // the last-declared (last-pushed) binding is popped before the rest.
            (@pop_rev $s:ident, $a0:ident $b0:ident) => {
                let $b0 = $s.pop()?;
            };
            (@pop_rev $s:ident, $a0:ident $b0:ident, $($a:ident $b:ident),+) => {
                sys_typed!(@pop_rev $s, $($a $b),+);
                let $b0 = $s.pop()?;
            };
            // Comma-joined type words, evaluated at compile time via concat!.
            (@desc_list $a:ident) => { sys_desc!($a) };
            (@desc_list $a0:ident, $($a:ident),+) => {
                concat!(sys_desc!($a0), ", ", sys_typed!(@desc_list $($a),+))
            };
        }
        // `sys_noarg!` covers the zero-argument arms: call the runtime method and
        // push the wrapped result.
        macro_rules! sys_noarg {
            ($ret:ident : $method:ident) => {{
                let result = self.runtime.$method();
                self.stack.push(sys_wrap!($ret, result));
                Ok(())
            }};
        }
        // `sys_math1!` / `sys_math2!` cover the float-math arms: pop the
        // argument(s), coerce Int|Float -> f64 (erroring otherwise with the
        // original "requires numeric argument(s)" message), call, push the result.
        macro_rules! sys_math1 {
            ($name:literal => $ret:ident : $method:ident) => {{
                let x = self.pop()?;
                let x = match x {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err(concat!($name, " requires numeric argument").to_string()),
                };
                let result = self.runtime.$method(x);
                self.stack.push(sys_wrap!($ret, result));
                Ok(())
            }};
        }
        macro_rules! sys_math2 {
            ($name:literal => $ret:ident : $method:ident) => {{
                let b = self.pop()?;
                let a = self.pop()?;
                let a = match a {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err(concat!($name, " requires numeric arguments").to_string()),
                };
                let b = match b {
                    Value::Float(f) => f,
                    Value::Int(i) => i as f64,
                    _ => return Err(concat!($name, " requires numeric arguments").to_string()),
                };
                let result = self.runtime.$method(a, b);
                self.stack.push(sys_wrap!($ret, result));
                Ok(())
            }};
        }
        match num {
            // sys_exit
            0 => {
                let code = self.pop()?;
                self.main_exit_code = match code {
                    Value::Int(n) => n as i32,
                    _ => 1,
                };
                self.exit_requested = true;
                Ok(())
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
            4 => sys_noarg!(Int: current_time_millis),
            // sys_sleep_ms
            5 => {
                let millis = self.pop()?;
                let Value::Int(ms) = millis else {
                    return Err("sleep requires integer milliseconds".to_string());
                };
                // In fiber mode, offload the sleep so the fiber parks and other
                // fibers run — fixing the "sleep blocks every fiber" gotcha.
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        if ms > 0 {
                            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                        }
                        Ok(crate::io_pool::IoValue::Unit)
                    }));
                    return Ok(());
                }
                self.runtime.sleep(ms);
                // Push Null so the inline path matches the offload path (which
                // yields Unit -> Null); the statement's trailing Pop consumes it.
                self.stack.push(Value::Null);
                Ok(())
            }
            // time_secs() -> int
            6 => sys_noarg!(Int: current_time_secs),
            // time_micros() -> int
            7 => sys_noarg!(Int: current_time_micros),
            // time_nanos() -> int
            8 => sys_noarg!(Int: current_time_nanos),

            // ================================================================
            // File I/O syscalls (10-19)
            // ================================================================

            // file_open(path: string, mode: int) -> int
            10 => {
                let mode = self.pop()?;
                let path = self.pop()?;
                let (Value::String(path), Value::Int(mode)) = (&path, &mode) else {
                    return Err("file_open requires (string, int) arguments".to_string());
                };
                let (path, mode) = (self.absolutize(path), *mode);
                // The (blocking) open runs on the pool; deliver_io allocates the
                // fd and inserts on success, so a failed open consumes no id.
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        match Runtime::open_file_blocking(&path, mode) {
                            Ok(file) => Ok(crate::io_pool::IoValue::FileOpened(file)),
                            Err(_) => Ok(crate::io_pool::IoValue::Int(-1)),
                        }
                    }));
                    return Ok(());
                }
                match self.runtime.file_open(&path, mode) {
                    Ok(fd) => self.stack.push(Value::Int(fd)),
                    Err(e) => {
                        self.stack.push(Value::Int(-1));
                        eprintln!("file_open error: {}", e);
                    }
                }
                Ok(())
            }
            // file_read(fd: int, max_bytes: int) -> string
            11 => {
                let max_bytes = self.pop()?;
                let fd = self.pop()?;
                let (Value::Int(fd), Value::Int(max_bytes)) = (&fd, &max_bytes) else {
                    return Err("file_read requires (int, int) arguments".to_string());
                };
                let (fd, max_bytes) = (*fd, *max_bytes);
                // Check the file out of the registry and read on the pool. If it
                // is missing or already in flight, fail with the "" contract.
                if let Some(fiber) = self.io_offload_target() {
                    if let Some(mut file) = self.runtime.checkout_file(fd) {
                        self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                            // On a read error still return the handle (so it isn't
                            // dropped/closed) with the empty-string error contract.
                            let data = Runtime::read_file_blocking(&mut file, max_bytes)
                                .unwrap_or_default();
                            Ok(crate::io_pool::IoValue::FileOp {
                                fd,
                                file,
                                result: Box::new(crate::io_pool::IoValue::Str(data)),
                            })
                        }));
                        return Ok(());
                    }
                    self.stack.push(Value::String(Rc::new(String::new())));
                    return Ok(());
                }
                match self.runtime.file_read(fd, max_bytes) {
                    Ok(data) => self.stack.push(Value::String(Rc::new(data))),
                    Err(e) => {
                        self.stack.push(Value::String(Rc::new(String::new())));
                        eprintln!("file_read error: {}", e);
                    }
                }
                Ok(())
            }
            // file_write(fd: int, data: string) -> int
            12 => {
                let data = self.pop()?;
                let fd = self.pop()?;
                let (Value::Int(fd), Value::String(data)) = (&fd, &data) else {
                    return Err("file_write requires (int, string) arguments".to_string());
                };
                let (fd, data) = (*fd, (**data).clone());
                if let Some(fiber) = self.io_offload_target() {
                    if let Some(mut file) = self.runtime.checkout_file(fd) {
                        self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                            let bytes =
                                Runtime::write_file_blocking(&mut file, &data).unwrap_or(-1);
                            Ok(crate::io_pool::IoValue::FileOp {
                                fd,
                                file,
                                result: Box::new(crate::io_pool::IoValue::Int(bytes)),
                            })
                        }));
                        return Ok(());
                    }
                    self.stack.push(Value::Int(-1));
                    return Ok(());
                }
                match self.runtime.file_write(fd, &data) {
                    Ok(bytes) => self.stack.push(Value::Int(bytes)),
                    Err(e) => {
                        self.stack.push(Value::Int(-1));
                        eprintln!("file_write error: {}", e);
                    }
                }
                Ok(())
            }
            // file_close(fd: int) -> bool
            13 => {
                let fd = self.pop()?;
                match &fd {
                    Value::Int(fd) => {
                        let success = self.runtime.file_close(*fd).is_ok();
                        self.stack.push(Value::Bool(success));
                        Ok(())
                    }
                    _ => Err("file_close requires int argument".to_string()),
                }
            }
            // file_exists(path: string) -> bool
            14 => sys_typed!("file_exists", str path => Bool: file_exists),
            // file_size(path: string) -> int
            15 => {
                let path = self.pop()?;
                match &path {
                    Value::String(path) => {
                        let size = self.runtime.file_size(path).unwrap_or(-1);
                        self.stack.push(Value::Int(size));
                        Ok(())
                    }
                    _ => Err("file_size requires string argument".to_string()),
                }
            }
            // file_seek(fd: int, offset: int, whence: int) -> int
            16 => {
                let whence = self.pop()?;
                let offset = self.pop()?;
                let fd = self.pop()?;
                let (Value::Int(fd), Value::Int(offset), Value::Int(whence)) =
                    (&fd, &offset, &whence)
                else {
                    return Err("file_seek requires (int, int, int) arguments".to_string());
                };
                let pos = self.runtime.file_seek(*fd, *offset, *whence).unwrap_or(-1);
                self.stack.push(Value::Int(pos));
                Ok(())
            }

            // ================================================================
            // Environment syscalls (20-29)
            // ================================================================

            // env_get(name: string) -> string (or null)
            20 => {
                let name = self.pop()?;
                match &name {
                    Value::String(name) => {
                        let value = self.runtime.env_get(name);
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
            21 => sys_noarg!(StrArray: env_args),

            // ================================================================
            // Extended environment syscalls (200-207)
            // ================================================================

            // env_set(name: string, value: string) -> bool
            200 => sys_typed!("env_set", str name, str value => Bool: env_set),
            // env_remove(name: string) -> bool
            201 => sys_typed!("env_remove", str name => Bool: env_remove),
            // env_all() -> [string] (key=value pairs)
            202 => sys_noarg!(StrArray: env_all),
            // env_keys() -> [string]
            203 => sys_noarg!(StrArray: env_keys),
            // env_has(name: string) -> bool
            204 => sys_typed!("env_has", str name => Bool: env_has),
            // env_exe() -> string
            205 => sys_noarg!(Str: env_exe),
            // env_temp_dir() -> string
            206 => sys_noarg!(Str: env_temp_dir),
            // env_home_dir() -> string
            207 => sys_noarg!(Str: env_home_dir),

            // ================================================================
            // String operation syscalls (30-39)
            // ================================================================

            // str_char_code(str: string, index: int) -> int
            30 => sys_typed!("str_char_code", str s, int index => Int: str_char_code),
            // str_from_char_code(code: int) -> string
            31 => sys_typed!("str_from_char_code", int code => Str: str_from_char_code),
            // str_to_upper(str: string) -> string
            32 => sys_typed!("str_to_upper", str s => Str: str_to_upper),
            // str_to_lower(str: string) -> string
            33 => sys_typed!("str_to_lower", str s => Str: str_to_lower),
            // str_substring(str: string, start: int, end: int) -> string
            34 => sys_typed!("str_substring", str s, int start, int end => Str: str_substring),
            // str_index_of(str: string, substr: string) -> int
            35 => sys_typed!("str_index_of", str s, str substr => Int: str_index_of),
            // str_split(str: string, delimiter: string) -> [string]
            36 => sys_typed!("str_split", str s, str delimiter => StrArray: str_split),
            // str_trim(str: string) -> string
            37 => sys_typed!("str_trim", str s => Str: str_trim),
            // str_trim_start(str: string) -> string
            38 => sys_typed!("str_trim_start", str s => Str: str_trim_start),
            // str_trim_end(str: string) -> string
            39 => sys_typed!("str_trim_end", str s => Str: str_trim_end),

            // ================================================================
            // Random number generation syscalls (40-41)
            // ================================================================

            // random_float() -> float (0.0 to 1.0)
            40 => sys_noarg!(Float: random_float),
            // random_int(min: int, max: int) -> int
            41 => sys_typed!("random_int", int min, int max => Int: random_int),

            // ================================================================
            // Base64 encoding/decoding syscalls (60-63)
            // ================================================================

            // base64_encode(input: string) -> string
            60 => sys_typed!("base64_encode", str s => Str: base64_encode),
            // base64_decode(input: string) -> string
            61 => {
                let input = self.pop()?;
                match &input {
                    Value::String(s) => {
                        match self.runtime.base64_decode(s) {
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
            62 => sys_typed!("base64_encode_url", str s => Str: base64_encode_url),
            // base64_decode_url(input: string) -> string
            63 => {
                let input = self.pop()?;
                match &input {
                    Value::String(s) => {
                        match self.runtime.base64_decode_url(s) {
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
            70 => sys_typed!("md5", str s => Str: hash_md5),
            // sha1(input: string) -> string (hex)
            71 => sys_typed!("sha1", str s => Str: hash_sha1),
            // sha256(input: string) -> string (hex)
            72 => sys_typed!("sha256", str s => Str: hash_sha256),
            // sha512(input: string) -> string (hex)
            73 => sys_typed!("sha512", str s => Str: hash_sha512),

            // ================================================================
            // JSON syscalls (50-52)
            // ================================================================

            // json_parse(json_str: string) -> value
            50 => {
                let input = self.pop()?;
                match &input {
                    Value::String(s) => match self.runtime.json_parse(s) {
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
                let (Value::String(host), Value::Int(port)) = (&host, &port) else {
                    return Err("tcp_connect requires (string, int) arguments".to_string());
                };
                let (host, port) = ((**host).clone(), *port);
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        match Runtime::tcp_connect_blocking(&host, port) {
                            Some(stream) => Ok(crate::io_pool::IoValue::TcpConnected(stream)),
                            None => Ok(crate::io_pool::IoValue::Int(-1)),
                        }
                    }));
                    return Ok(());
                }
                let id = self.runtime.tcp_connect(&host, port);
                self.stack.push(Value::Int(id));
                Ok(())
            }
            // tcp_write(socket_id: int, data: string) -> int (bytes written or -1)
            81 => {
                let data = self.pop()?;
                let socket_id = self.pop()?;
                let (Value::Int(socket_id), Value::String(data)) = (&socket_id, &data) else {
                    return Err("tcp_write requires (int, string) arguments".to_string());
                };
                let (socket_id, data) = (*socket_id, (**data).clone());
                if let Some(fiber) = self.io_offload_target() {
                    if let Some(mut stream) = self.runtime.checkout_socket(socket_id) {
                        self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                            let n = Runtime::tcp_write_blocking(&mut stream, &data);
                            Ok(crate::io_pool::IoValue::TcpOp {
                                id: socket_id,
                                stream,
                                result: Box::new(crate::io_pool::IoValue::Int(n)),
                            })
                        }));
                        return Ok(());
                    }
                    self.stack.push(Value::Int(-1));
                    return Ok(());
                }
                let n = self.runtime.tcp_write(socket_id, &data);
                self.stack.push(Value::Int(n));
                Ok(())
            }
            // tcp_read(socket_id: int, max_bytes: int) -> string
            82 => {
                let max_bytes = self.pop()?;
                let socket_id = self.pop()?;
                let (Value::Int(socket_id), Value::Int(max_bytes)) = (&socket_id, &max_bytes)
                else {
                    return Err("tcp_read requires (int, int) arguments".to_string());
                };
                let (socket_id, max_bytes) = (*socket_id, *max_bytes);
                if let Some(fiber) = self.io_offload_target() {
                    if let Some(mut stream) = self.runtime.checkout_socket(socket_id) {
                        self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                            let s = Runtime::tcp_read_blocking(&mut stream, max_bytes);
                            Ok(crate::io_pool::IoValue::TcpOp {
                                id: socket_id,
                                stream,
                                result: Box::new(crate::io_pool::IoValue::Str(s)),
                            })
                        }));
                        return Ok(());
                    }
                    self.stack.push(Value::String(Rc::new(String::new())));
                    return Ok(());
                }
                let s = self.runtime.tcp_read(socket_id, max_bytes);
                self.stack.push(Value::String(Rc::new(s)));
                Ok(())
            }
            // tcp_close(socket_id: int) -> bool  (inline: fast, drops the socket)
            83 => sys_typed!("tcp_close", int socket_id => Bool: tcp_close),
            // dns_lookup(hostname: string) -> string (IP address or empty)
            84 => {
                let hostname = self.pop()?;
                let Value::String(hostname) = &hostname else {
                    return Err("dns_lookup requires string argument".to_string());
                };
                let hostname = (**hostname).clone();
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        Ok(crate::io_pool::IoValue::Str(Runtime::dns_lookup_blocking(
                            &hostname,
                        )))
                    }));
                    return Ok(());
                }
                let s = self.runtime.dns_lookup(&hostname);
                self.stack.push(Value::String(Rc::new(s)));
                Ok(())
            }

            // ================================================================
            // OS syscalls (90-101)
            // ================================================================

            // getcwd() -> string
            90 => sys_noarg!(Str: os_getcwd),
            // chdir(path: string) -> bool
            91 => sys_typed!("chdir", str path => Bool: os_chdir),
            // mkdir(path: string) -> bool
            92 => sys_typed!("mkdir", str path => Bool: os_mkdir),
            // mkdir_all(path: string) -> bool
            93 => sys_typed!("mkdir_all", str path => Bool: os_mkdir_all),
            // rmdir(path: string) -> bool
            94 => sys_typed!("rmdir", str path => Bool: os_rmdir),
            // remove(path: string) -> bool
            95 => sys_typed!("remove", str path => Bool: os_remove),
            // remove_all(path: string) -> bool  (offloaded: recursive delete)
            96 => {
                let path = self.pop()?;
                let Value::String(path) = &path else {
                    return Err("remove_all requires string argument".to_string());
                };
                let path = self.absolutize(path);
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        Ok(crate::io_pool::IoValue::Bool(
                            std::fs::remove_dir_all(&path).is_ok(),
                        ))
                    }));
                    return Ok(());
                }
                self.stack
                    .push(Value::Bool(self.runtime.os_remove_all(&path)));
                Ok(())
            }
            // listdir(path: string) -> [string]  (offloaded: dir scan)
            97 => {
                let path = self.pop()?;
                let Value::String(path) = &path else {
                    return Err("listdir requires string argument".to_string());
                };
                let path = self.absolutize(path);
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        let entries = std::fs::read_dir(&path)
                            .map(|es| {
                                es.filter_map(|e| e.ok())
                                    .map(|e| e.file_name().to_string_lossy().to_string())
                                    .collect()
                            })
                            .unwrap_or_default();
                        Ok(crate::io_pool::IoValue::Strs(entries))
                    }));
                    return Ok(());
                }
                let arr: Vec<Value> = self
                    .runtime
                    .os_listdir(&path)
                    .into_iter()
                    .map(|s| Value::String(Rc::new(s)))
                    .collect();
                self.stack.push(Value::Array(Gc::new(GcCell::new(arr))));
                Ok(())
            }
            // is_dir(path: string) -> bool
            98 => sys_typed!("is_dir", str path => Bool: os_is_dir),
            // is_file(path: string) -> bool
            99 => sys_typed!("is_file", str path => Bool: os_is_file),
            // rename(from: string, to: string) -> bool
            100 => sys_typed!("rename", str from, str to => Bool: os_rename),
            // copy(from: string, to: string) -> bool
            101 => {
                let to = self.pop()?;
                let from = self.pop()?;
                let (Value::String(from), Value::String(to)) = (&from, &to) else {
                    return Err("copy requires (string, string) arguments".to_string());
                };
                let (from, to) = (self.absolutize(from), self.absolutize(to));
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        Ok(crate::io_pool::IoValue::Bool(
                            std::fs::copy(&from, &to).is_ok(),
                        ))
                    }));
                    return Ok(());
                }
                self.stack
                    .push(Value::Bool(self.runtime.os_copy(&from, &to)));
                Ok(())
            }

            // ================================================================
            // URL encoding/decoding syscalls (110-111)
            // ================================================================

            // url_encode(input: string) -> string
            110 => sys_typed!("url_encode", str s => Str: url_encode),
            // url_decode(input: string) -> string
            111 => sys_typed!("url_decode", str s => Str: url_decode),

            // ================================================================
            // HTTP Client syscalls (120-122)
            // ================================================================

            // http_get(url: string) -> (int, string) (status, body)
            120 => {
                let url = self.pop()?;
                let Value::String(url) = &url else {
                    return Err("http_get requires string argument".to_string());
                };
                let url = (**url).clone();
                // In fiber mode, offload the blocking request to the I/O pool and
                // park this fiber so other fibers run (and other requests overlap).
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        let (status, body) = match Runtime::http_get_blocking(&url) {
                            Ok((s, _headers, b)) => (s, b),
                            Err(e) => (-1, e),
                        };
                        Ok(crate::io_pool::IoValue::HttpResponse { status, body })
                    }));
                    return Ok(());
                }
                // Sequential path: block inline.
                let (status, body) = match self.runtime.http_get(&url) {
                    Ok((status, _headers, body)) => (status, body),
                    Err(e) => (-1, e),
                };
                let tuple = vec![Value::Int(status), Value::String(Rc::new(body))];
                self.stack
                    .push(Value::Tuple(Gc::new(GcCell::new(TupleData::sealed(tuple)))));
                Ok(())
            }
            // http_post(url: string, body: string, content_type: string) -> (int, string)
            121 => {
                let content_type = self.pop()?;
                let body = self.pop()?;
                let url = self.pop()?;
                let (Value::String(url), Value::String(body), Value::String(content_type)) =
                    (&url, &body, &content_type)
                else {
                    return Err("http_post requires (string, string, string) arguments".to_string());
                };
                let (url, body, content_type) =
                    ((**url).clone(), (**body).clone(), (**content_type).clone());
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        let (status, body) =
                            match Runtime::http_post_blocking(&url, &body, &content_type) {
                                Ok((s, _headers, b)) => (s, b),
                                Err(e) => (-1, e),
                            };
                        Ok(crate::io_pool::IoValue::HttpResponse { status, body })
                    }));
                    return Ok(());
                }
                let (status, response_body) =
                    match self.runtime.http_post(&url, &body, &content_type) {
                        Ok((status, _headers, response_body)) => (status, response_body),
                        Err(e) => (-1, e),
                    };
                let tuple = vec![Value::Int(status), Value::String(Rc::new(response_body))];
                self.stack
                    .push(Value::Tuple(Gc::new(GcCell::new(TupleData::sealed(tuple)))));
                Ok(())
            }
            // http_request(method: string, url: string, headers: string, body: string) -> (int, string)
            122 => {
                let body = self.pop()?;
                let headers = self.pop()?;
                let url = self.pop()?;
                let method = self.pop()?;
                let (
                    Value::String(method),
                    Value::String(url),
                    Value::String(headers),
                    Value::String(body),
                ) = (&method, &url, &headers, &body)
                else {
                    return Err(
                        "http_request requires (string, string, string, string) arguments"
                            .to_string(),
                    );
                };
                let (method, url, headers, body) = (
                    (**method).clone(),
                    (**url).clone(),
                    (**headers).clone(),
                    (**body).clone(),
                );
                if let Some(fiber) = self.io_offload_target() {
                    self.offload_io(crate::io_pool::IoJob::new(fiber, move || {
                        let (status, body) =
                            match Runtime::http_request_blocking(&method, &url, &headers, &body) {
                                Ok((s, b)) => (s, b),
                                Err(e) => (-1, e),
                            };
                        Ok(crate::io_pool::IoValue::HttpResponse { status, body })
                    }));
                    return Ok(());
                }
                let (status, response_body) =
                    match self.runtime.http_request(&method, &url, &headers, &body) {
                        Ok((status, response_body)) => (status, response_body),
                        Err(e) => (-1, e),
                    };
                let tuple = vec![Value::Int(status), Value::String(Rc::new(response_body))];
                self.stack
                    .push(Value::Tuple(Gc::new(GcCell::new(TupleData::sealed(tuple)))));
                Ok(())
            }

            // ================================================================
            // Math syscalls (140-163)
            // ================================================================

            // sqrt(x: float) -> float
            140 => sys_math1!("sqrt" => Float: math_sqrt),
            // pow(base: float, exp: float) -> float
            141 => sys_math2!("pow" => Float: math_pow),
            // exp(x: float) -> float
            142 => sys_math1!("exp" => Float: math_exp),
            // ln(x: float) -> float
            143 => sys_math1!("ln" => Float: math_ln),
            // log10(x: float) -> float
            144 => sys_math1!("log10" => Float: math_log10),
            // log2(x: float) -> float
            145 => sys_math1!("log2" => Float: math_log2),
            // sin(x: float) -> float
            146 => sys_math1!("sin" => Float: math_sin),
            // cos(x: float) -> float
            147 => sys_math1!("cos" => Float: math_cos),
            // tan(x: float) -> float
            148 => sys_math1!("tan" => Float: math_tan),
            // asin(x: float) -> float
            149 => sys_math1!("asin" => Float: math_asin),
            // acos(x: float) -> float
            150 => sys_math1!("acos" => Float: math_acos),
            // atan(x: float) -> float
            151 => sys_math1!("atan" => Float: math_atan),
            // atan2(y: float, x: float) -> float
            152 => sys_math2!("atan2" => Float: math_atan2),
            // sinh(x: float) -> float
            153 => sys_math1!("sinh" => Float: math_sinh),
            // cosh(x: float) -> float
            154 => sys_math1!("cosh" => Float: math_cosh),
            // tanh(x: float) -> float
            155 => sys_math1!("tanh" => Float: math_tanh),
            // floor(x: float) -> float
            156 => sys_math1!("floor" => Float: math_floor),
            // ceil(x: float) -> float
            157 => sys_math1!("ceil" => Float: math_ceil),
            // round(x: float) -> float
            158 => sys_math1!("round" => Float: math_round),
            // trunc(x: float) -> float
            159 => sys_math1!("trunc" => Float: math_trunc),
            // is_nan(x: float) -> bool
            160 => sys_math1!("is_nan" => Bool: math_is_nan),
            // is_infinite(x: float) -> bool
            161 => sys_math1!("is_infinite" => Bool: math_is_infinite),
            // is_finite(x: float) -> bool
            162 => sys_math1!("is_finite" => Bool: math_is_finite),
            // abs(x: float) -> float
            163 => sys_math1!("abs" => Float: math_abs_float),

            // ================================================================
            // Extended Time syscalls (130-135)
            // ================================================================

            // time_format_iso(timestamp_ms: int) -> string
            130 => sys_typed!("time_format_iso", int ms => Str: time_format_iso),
            // time_format(timestamp_ms: int, format: string) -> string
            131 => sys_typed!("time_format", int ms, str fmt => Str: time_format),
            // time_parse_iso(string) -> int
            132 => sys_typed!("time_parse_iso", str s => Int: time_parse_iso),
            // time_timezone_offset() -> int
            133 => sys_noarg!(Int: time_timezone_offset),
            // time_components(timestamp_ms: int) -> [int]
            134 => sys_typed!("time_components", int ms => IntArray: time_components),
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
            170 => sys_typed!("regex_match", str pattern, str text => Bool: regex_match),
            // regex_find(pattern: string, text: string) -> string
            171 => sys_typed!("regex_find", str pattern, str text => Str: regex_find),
            // regex_find_all(pattern: string, text: string) -> [string]
            172 => sys_typed!("regex_find_all", str pattern, str text => StrArray: regex_find_all),
            // regex_replace(pattern: string, text: string, replacement: string) -> string
            173 => {
                sys_typed!("regex_replace", str pattern, str text, str replacement => Str: regex_replace)
            }
            // regex_replace_all(pattern: string, text: string, replacement: string) -> string
            174 => {
                sys_typed!("regex_replace_all", str pattern, str text, str replacement => Str: regex_replace_all)
            }
            // regex_split(pattern: string, text: string) -> [string]
            175 => sys_typed!("regex_split", str pattern, str text => StrArray: regex_split),
            // regex_captures(pattern: string, text: string) -> [string]
            176 => sys_typed!("regex_captures", str pattern, str text => StrArray: regex_captures),
            // regex_is_valid(pattern: string) -> bool
            177 => sys_typed!("regex_is_valid", str pattern => Bool: regex_is_valid),

            // ================================================================
            // UUID syscalls (190-193)
            // ================================================================

            // uuid_v4() -> string
            190 => sys_noarg!(Str: uuid_v4),
            // uuid_v7() -> string
            191 => sys_noarg!(Str: uuid_v7),
            // uuid_is_valid(s: string) -> bool
            192 => sys_typed!("uuid_is_valid", str s => Bool: uuid_is_valid),
            // uuid_nil() -> string
            193 => sys_noarg!(Str: uuid_nil),

            _ => Err(format!("Unknown syscall: {}", num)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_one_value_beyond_the_output_limit_fails_without_partial_text() {
        // `[` + 466,034 sixteen-byte strings + separators + `]` is
        // 8,388,612 bytes: four bytes beyond the user-facing limit. Build the
        // runtime value directly so the resource-bound test does not spend
        // hundreds of thousands of debug-VM iterations constructing it.
        let element = Value::String(Rc::new("0123456789abcdef".to_string()));
        let elements = vec![element; 466_034];
        let value = Value::Array(Gc::new(GcCell::new(elements)));
        let error = render_output_value(&value).expect_err("oversized render must fail");
        assert_eq!(
            error,
            "one printed value exceeded the 8388608 byte output limit"
        );
    }
    use crate::fiber::FiberState;

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
    fn env_override_is_scoped_to_vm() {
        const NAME: &str = "LIRA_VM_ENV_OVERRIDE_VM_TEST_UNIQUE";
        let process_value = std::env::var(NAME).ok();
        let mut vm = VM::new(make_program(Vec::new(), vec![Opcode::Halt as u8]));
        vm.set_env_override(NAME, "vm-local-value");

        assert_eq!(vm.runtime.env_get(NAME).as_deref(), Some("vm-local-value"));
        assert_eq!(std::env::var(NAME).ok(), process_value);
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
    fn assert_consumes_true_and_continues() {
        let program = make_program(
            vec![Value::Bool(true), Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::Assert as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        assert_eq!(vm.run(), Ok(0));
        assert!(matches!(vm.stack.as_slice(), [Value::Int(42)]));
    }

    #[test]
    fn assert_false_and_invalid_operands_fail_deterministically() {
        for (value, expected) in [
            (Value::Bool(false), "assertion failed"),
            (Value::Int(1), "assert expects bool, got int"),
        ] {
            let program = make_program(
                vec![value],
                vec![
                    Opcode::LoadConst as u8,
                    0,
                    0,
                    Opcode::Assert as u8,
                    Opcode::Halt as u8,
                ],
            );
            let mut vm = VM::new(program);
            assert_eq!(vm.run().expect_err("assert operand must fail"), expected);
        }
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
    fn collection_construction_rejects_negative_and_excessive_sizes_before_allocation() {
        for (opcode, kind) in [(Opcode::NewArray, "Array"), (Opcode::NewTuple, "Tuple")] {
            for (size, expected) in [
                (-1, format!("{kind} size must be non-negative")),
                (
                    i64::MAX,
                    format!(
                        "{kind} size exceeds VM collection limit of {MAX_COLLECTION_ELEMENTS} elements"
                    ),
                ),
            ] {
                let program = make_program(
                    vec![Value::Int(size)],
                    vec![
                        Opcode::LoadConst as u8,
                        0,
                        0,
                        opcode as u8,
                        Opcode::Halt as u8,
                    ],
                );
                let mut vm = VM::new(program);
                let error = vm
                    .run()
                    .expect_err("invalid collection size must fail without allocating");
                assert_eq!(error, expected);
            }
        }
    }

    #[test]
    fn channel_construction_rejects_negative_and_excessive_capacity_before_allocation() {
        for (capacity, expected) in [
            (-1, "Channel capacity must be non-negative".to_string()),
            (
                i64::MAX,
                format!(
                    "Channel capacity exceeds VM limit of {} elements",
                    crate::fiber::MAX_CHANNEL_CAPACITY
                ),
            ),
        ] {
            let program = make_program(
                vec![Value::Int(capacity)],
                vec![
                    Opcode::LoadConst as u8,
                    0,
                    0,
                    Opcode::ChanNew as u8,
                    Opcode::Halt as u8,
                ],
            );
            let mut vm = VM::new(program);
            let error = vm
                .run()
                .expect_err("invalid channel capacity must fail without allocating");
            assert_eq!(error, expected);
        }
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
                4,
                0, // Jump +4 from byte 6 -> byte 10 (Load 100), skipping Print 42
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
    fn test_jump_past_end_of_code_is_an_error() {
        // A jump whose target lands past the end of the code section must be
        // reported as an error rather than silently falling off the end of the
        // interpreter loop (which used to exit cleanly with code 0, dropping
        // any pending fibers).
        let program = make_program(
            Vec::new(),
            vec![
                Opcode::Jump as u8,
                10,
                0,                  // Jump forward +10 from byte 3 -> 13 (past end)
                Opcode::Halt as u8, // would never be reached
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        let error = result.expect_err("an out-of-bounds jump must fail");
        assert!(
            error.contains("out of bounds"),
            "expected an out-of-bounds jump error, got: {}",
            error
        );
    }

    #[test]
    fn test_float_operations() {
        let program = make_program(
            vec![Value::Float(2.5), Value::Float(2.0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 2.5
                Opcode::LoadConst as u8,
                1,
                0,                   // Load 2.0
                Opcode::Mul as u8,   // 2.5 * 2.0
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
    fn test_collect_opcode_runs_and_pushes_null() {
        // Build a 1-element array, make it self-referential (arr contains arr),
        // drop it, then run Collect. This exercises the `collect()` builtin's
        // opcode path: the VM must run a garbage collection at the opcode
        // boundary and push null, leaving the program in a valid state.
        let program = make_program(
            vec![Value::Int(0)],
            vec![
                // NewArray of size 0 -> stack: [arr].
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewArray as u8,
                // Dup -> stack: [arr, arr].
                Opcode::Dup as u8,
                // ArrayPush pops (array, value): arr.push(arr) forms a self-cycle
                // and leaves the stack empty.
                Opcode::ArrayPush as u8,
                // The array is now unreachable from the stack. Collect it; the
                // opcode pushes null.
                Opcode::Collect as u8,
                // Discard the null and halt.
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "Collect opcode should execute cleanly");
    }

    #[test]
    fn test_auto_collection_fires_at_threshold() {
        // A bytecode loop that allocates one array per iteration and immediately
        // drops it, WITHOUT ever calling collect(). Allocating well past
        // AUTO_COLLECT_INTERVAL must trigger the loop-boundary auto-collection,
        // keeping the live heap bounded. Relative jump offsets are computed for
        // the exact byte layout documented inline.
        let count = (VM::AUTO_COLLECT_INTERVAL + 50) as i64;
        // Offsets: JumpIfFalse @9 reads its operand, ip becomes 12; end is @30,
        // so its relative offset is 30 - 12 = 18. Jump @27 -> loop top @6 from
        // ip 30, so its relative offset is 6 - 30 = -24.
        let jif: i16 = 18;
        let back: i16 = -24;
        let program = make_program(
            vec![Value::Int(count), Value::Int(0), Value::Int(1)],
            vec![
                // 0: counter = count
                Opcode::LoadConst as u8,
                0,
                0,
                // 3: store counter -> local 0
                Opcode::StoreLocal as u8,
                0,
                0,
                // 6: loop top -- load counter
                Opcode::LoadLocal as u8,
                0,
                0,
                // 9: if counter == 0 jump to end (+18)
                Opcode::JumpIfFalse as u8,
                jif as u16 as u8,
                (jif as u16 >> 8) as u8,
                // 12: array size 0
                Opcode::LoadConst as u8,
                1,
                0,
                // 15: allocate array (bumps allocation counter)
                Opcode::NewArray as u8,
                // 16: drop it -> unreachable
                Opcode::Pop as u8,
                // 17: counter
                Opcode::LoadLocal as u8,
                0,
                0,
                // 20: 1
                Opcode::LoadConst as u8,
                2,
                0,
                // 23: counter - 1
                Opcode::Sub as u8,
                // 24: store counter
                Opcode::StoreLocal as u8,
                0,
                0,
                // 27: jump back to loop top (-24)
                Opcode::Jump as u8,
                back as u16 as u8,
                (back as u16 >> 8) as u8,
                // 30: end
                Opcode::Halt as u8,
            ],
        );

        gc::force_collect();
        let baseline = gc::stats().bytes_allocated;

        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "allocation loop should run to completion");

        // If auto-collection never fired, ~10,050 self-dropped arrays would have
        // accumulated. The live heap during/after the run must stay far below
        // that. After completion the counter has been reset and one more
        // collection settles the heap to baseline.
        gc::force_collect();
        let after = gc::stats().bytes_allocated;
        assert_eq!(
            after, baseline,
            "all transient arrays must be reclaimed (auto + final collect)"
        );
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

    /// After a fiber-mode spawn+channel program runs, the detailed scheduler
    /// snapshot must expose both fibers (main + worker) and the channel they
    /// communicated over, with values carried as `RichValue`s. This is the
    /// VM-side half of the playground fiber-inspection gate.
    #[test]
    fn test_scheduler_snapshot_after_spawn_channel() {
        const W: u16 = 15;
        let program = make_program(
            vec![Value::Int(1), Value::Int(42)],
            vec![
                // --- main @0 ---
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::ChanNew as u8,
                Opcode::Dup as u8,
                Opcode::Spawn as u8,
                (W & 0xff) as u8,
                (W >> 8) as u8,
                1,
                Opcode::Pop as u8,
                Opcode::Yield as u8,
                Opcode::ChanRecv as u8,
                Opcode::Pop as u8,
                Opcode::Print as u8,
                Opcode::Halt as u8,
                // --- worker @15 ---
                Opcode::LoadLocal as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::ChanSend as u8,
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::Return as u8,
            ],
        );
        let mut vm = VM::new(program);
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        vm.run().expect("spawn+channel program should run");

        let snap = vm.scheduler_snapshot();
        // main (id 1) + worker (id 2) both retained, ordered by id. (Main ends
        // in `Running` on `Halt` — only the worker's `Return` finishes it.)
        assert_eq!(
            snap.fibers.len(),
            2,
            "both fibers present: {:?}",
            snap.fibers
        );
        assert_eq!(snap.fibers[0].id, 1);
        assert_eq!(snap.fibers[1].id, 2);
        // The worker finished and carried back a result value (proves the
        // snapshot captures live `RichValue`s, not just counts).
        let worker = &snap.fibers[1];
        assert!(
            matches!(worker.state, FiberState::Finished),
            "worker finished: {:?}",
            worker
        );
        assert!(worker.result.is_some(), "worker carried a result value");
        // The channel created by `chan(1)` is retained with capacity 1.
        assert_eq!(
            snap.channels.len(),
            1,
            "channel present: {:?}",
            snap.channels
        );
        assert_eq!(snap.channels[0].capacity, 1);
        assert!(!snap.channels[0].closed);
    }

    /// Concurrent STEP debugging: driving a fiber-mode spawn+channel program
    /// purely via `step_instruction()` (never `run()`) must drive the scheduler
    /// across fibers — switching between main and the worker — and complete with
    /// the same output as `run()`. Proves the stepping path is fiber-aware.
    #[test]
    fn test_step_instruction_drives_fibers() {
        const W: u16 = 15;
        let program = make_program(
            vec![Value::Int(1), Value::Int(42)],
            vec![
                // --- main @0 ---
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::ChanNew as u8,
                Opcode::Dup as u8,
                Opcode::Spawn as u8,
                (W & 0xff) as u8,
                (W >> 8) as u8,
                1,
                Opcode::Pop as u8,
                Opcode::Yield as u8,
                Opcode::ChanRecv as u8,
                Opcode::Pop as u8,
                Opcode::Print as u8,
                Opcode::Halt as u8,
                // --- worker @15 ---
                Opcode::LoadLocal as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::ChanSend as u8,
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::Return as u8,
            ],
        );
        let mut vm = VM::new(program);
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        vm.prepare();

        // Step until finished (never call run()). Record which fibers ran and
        // whether we ever observed a blocked fiber mid-execution.
        let mut seen_fibers = std::collections::HashSet::new();
        let mut saw_parked = false;
        let mut exit = None;
        for _ in 0..1000 {
            if let Some(id) = vm.scheduler.current {
                seen_fibers.insert(id);
            }
            // A fiber that has yielded or blocked while another runs is direct
            // evidence the scheduler parked one fiber and switched to another.
            if vm.scheduler.fibers.values().any(|f| {
                matches!(
                    f.state,
                    FiberState::Yielded
                        | FiberState::BlockedReceive(_)
                        | FiberState::BlockedSend(_)
                        | FiberState::BlockedSelect
                )
            }) {
                saw_parked = true;
            }
            match vm.step_instruction() {
                StepOutcome::Finished { exit_code } => {
                    exit = Some(exit_code);
                    break;
                }
                StepOutcome::Error { message } => panic!("stepping errored: {}", message),
                _ => {}
            }
        }

        assert_eq!(exit, Some(0), "program finished via stepping");
        assert_eq!(
            vm.get_output(),
            &["42".to_string()],
            "correct output via stepping"
        );
        assert!(
            seen_fibers.len() >= 2,
            "stepping switched across >= 2 fibers, saw {:?}",
            seen_fibers
        );
        assert!(
            saw_parked,
            "observed a parked (yielded/blocked) fiber at some step (real concurrency)"
        );
    }

    /// ADVERSARIAL PROBE (mixing run() and stepping on the same VM): step a few
    /// instructions in fiber mode (which bootstraps main as a fiber + sets the
    /// `fiber_runtime_started` guard), THEN call run() on the SAME VM. The guard
    /// must prevent a second main-fiber spawn. We record the fiber count before
    /// and after run() and whether output is duplicated/corrupted.
    #[test]
    fn probe_mix_step_then_run_no_double_spawn() {
        const W: u16 = 15;
        let make = || {
            make_program(
                vec![Value::Int(1), Value::Int(42)],
                vec![
                    // --- main @0 ---
                    Opcode::LoadConst as u8,
                    0,
                    0,
                    Opcode::ChanNew as u8,
                    Opcode::Dup as u8,
                    Opcode::Spawn as u8,
                    (W & 0xff) as u8,
                    (W >> 8) as u8,
                    1,
                    Opcode::Pop as u8,
                    Opcode::Yield as u8,
                    Opcode::ChanRecv as u8,
                    Opcode::Pop as u8,
                    Opcode::Print as u8,
                    Opcode::Halt as u8,
                    // --- worker @15 ---
                    Opcode::LoadLocal as u8,
                    0,
                    0,
                    Opcode::LoadConst as u8,
                    1,
                    0,
                    Opcode::ChanSend as u8,
                    Opcode::LoadConst as u8,
                    0,
                    0,
                    Opcode::Return as u8,
                ],
            )
        };

        let mut vm = VM::new(make());
        vm.set_fiber_mode(true);
        vm.set_capture_output(true);
        vm.prepare();

        // Step a handful of instructions (bootstraps main fiber + guard).
        for _ in 0..3 {
            let _ = vm.step_instruction();
        }
        let fibers_after_stepping = vm.scheduler.fibers.len();
        assert!(
            vm.fiber_runtime_started,
            "guard set after stepping bootstrapped the runtime"
        );

        // Now drive run() on the SAME VM. ensure_fiber_runtime_started must be a
        // no-op (guard already true), so no second main spawn.
        let result = vm.run();
        let fibers_after_run = vm.scheduler.fibers.len();

        eprintln!(
            "MIX step-then-run: fibers_after_stepping={}, fibers_after_run={}, result={:?}, output={:?}",
            fibers_after_stepping,
            fibers_after_run,
            result,
            vm.get_output()
        );

        // Anti-double-spawn invariant: the guard means run() does NOT bootstrap
        // a SECOND main fiber. With this program, fibers_after_stepping==1 (main
        // only; Spawn not yet executed at step 3) and fibers_after_run==2 (the
        // legitimate worker spawned by the Spawn opcode). The program completes
        // with the correct, non-duplicated output and exit 0 — proving no
        // double-main-spawn / no state corruption from mixing the two paths.
        assert_eq!(result, Ok(0), "mixed step-then-run completes cleanly");
        assert_eq!(
            vm.get_output(),
            &["42".to_string()],
            "mixed step-then-run output is correct and not duplicated"
        );
        // Exactly one fiber carries the main id (no second main bootstrap).
        let main_count = vm
            .scheduler
            .fibers
            .keys()
            .filter(|&&id| id == vm.main_fiber_id)
            .count();
        assert_eq!(main_count, 1, "exactly one main fiber (no double-spawn)");
    }

    // ── Bug C1: Jump target integer overflow ──

    #[test]
    fn test_jump_negative_overflow_is_bounds_checked() {
        // A Jump instruction at offset 0 with i16::MIN (-32768) that would
        // wrap around additively if computed as (ip as i32 + offset) as usize.
        let program = make_program(
            vec![],
            vec![
                Opcode::Jump as u8,
                0x00,
                0x80, // little-endian i16::MIN = -32768
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_err(), "jump with wraparound offset must error");
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("jump target out of bounds"),
            "error must mention jump target out of bounds, got: {:?}",
            result
        );
    }

    #[test]
    fn test_jump_valid_offset_still_works() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 42
                Opcode::Jump as u8,
                1,
                0,                 // jump +1 from byte 6 -> byte 7 (Halt), skipping Pop
                Opcode::Pop as u8, // skipped
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "valid jump must still work");
    }

    #[test]
    fn test_jump_if_true_overflow_is_checked() {
        let program = make_program(
            vec![Value::Bool(true)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load true
                Opcode::JumpIfTrue as u8,
                0x00,
                0x80, // little-endian i16::MIN = -32768
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(
            result.is_err(),
            "JumpIfTrue with wraparound offset must error"
        );
    }

    #[test]
    fn test_jump_if_false_overflow_is_checked() {
        let program = make_program(
            vec![Value::Bool(false)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load false
                Opcode::JumpIfFalse as u8,
                0x00,
                0x80, // little-endian i16::MIN = -32768
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(
            result.is_err(),
            "JumpIfFalse with wraparound offset must error"
        );
    }

    // ── Bug C2: Bit shift UB ──

    #[test]
    fn test_shl_excessive_shift_clamped_not_ub() {
        let program = make_program(
            vec![Value::Int(1), Value::Int(1000)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 1
                Opcode::LoadConst as u8,
                1,
                0,                 // Load 1000
                Opcode::Shl as u8, // 1 << 1000 (clamped to << 63)
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "shl by 1000 must not panic/UB");
    }

    #[test]
    fn test_shr_excessive_shift_clamped_not_ub() {
        let program = make_program(
            vec![Value::Int(42), Value::Int(999)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 42
                Opcode::LoadConst as u8,
                1,
                0,                 // Load 999
                Opcode::Shr as u8, // 42 >> 999 (clamped to >> 63)
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "shr by 999 must not panic/UB");
    }

    #[test]
    fn test_ushr_excessive_shift_clamped_not_ub() {
        let program = make_program(
            vec![Value::Int(-1_i64), Value::Int(2000)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load -1 (all 1s bits)
                Opcode::LoadConst as u8,
                1,
                0,                  // Load 2000
                Opcode::UShr as u8, // -1 >>> 2000 (clamped to >>> 63)
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "ushr by 2000 must not panic/UB");
    }

    #[test]
    fn test_shift_by_zero_is_identity() {
        let program = make_program(
            vec![Value::Int(42), Value::Int(0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::Shl as u8, // 42 << 0 = 42
                Opcode::Pop as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "shift by zero must work");
    }

    // ── Bug C6: sys_exit graceful halt ──

    #[test]
    fn test_sys_exit_graceful() {
        // Push exit code 42, then call sys_exit (syscall 0).
        // After the fix, this must NOT call std::process::exit.
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 42 (exit code) from constant pool index 0
                Opcode::Syscall as u8,
                0, // syscall number 0 = sys_exit
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(
            result.is_ok(),
            "sys_exit must not kill process; expected Ok exit"
        );
        assert_eq!(result.unwrap(), 42, "sys_exit(42) must return exit code 42");
    }

    #[test]
    fn test_sys_exit_default_code() {
        // sys_exit with a non-int value should default to exit code 1.
        let program = make_program(
            vec![Value::Bool(true)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load true (non-int)
                Opcode::Syscall as u8,
                0, // syscall number 0 = sys_exit
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "sys_exit non-int must not kill process");
        assert_eq!(
            result.unwrap(),
            1,
            "sys_exit(non-int) must return default exit code 1"
        );
    }

    // ── Bug M1: Negative Array/String Index ──

    #[test]
    fn test_array_get_negative_index_errors() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![
                    Value::Int(1),
                    Value::Int(2),
                    Value::Int(3),
                ]))),
                Value::Int(-1),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load array
                Opcode::LoadConst as u8,
                1,
                0,                      // Load -1 index
                Opcode::ArrayGet as u8, // arr[-1]
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_err(), "negative array index must error");
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("Index out of bounds: negative index"),
            "error must mention negative index, got: {:?}",
            result
        );
    }

    #[test]
    fn test_array_get_out_of_bounds_index_errors() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![Value::Int(1), Value::Int(2)]))),
                Value::Int(5),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load array [1, 2]
                Opcode::LoadConst as u8,
                1,
                0,                      // Load index 5
                Opcode::ArrayGet as u8, // arr[5]
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_err(), "out-of-bounds array index must error");
        assert!(
            result.as_ref().unwrap_err().contains("Index out of bounds"),
            "error must mention out of bounds, got: {:?}",
            result
        );
    }

    #[test]
    fn test_array_get_valid_index_still_works() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![
                    Value::Int(10),
                    Value::Int(20),
                    Value::Int(30),
                ]))),
                Value::Int(1),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load array
                Opcode::LoadConst as u8,
                1,
                0,                      // Load index 1
                Opcode::ArrayGet as u8, // arr[1] = 20
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "valid array index must still work");
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, 20),
            other => panic!("Expected Int(20), got {:?}", other),
        }
    }

    #[test]
    fn test_string_index_negative_errors() {
        let program = make_program(
            vec![Value::String(Rc::new("hello".to_string())), Value::Int(-2)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load "hello"
                Opcode::LoadConst as u8,
                1,
                0,                      // Load -2 index
                Opcode::ArrayGet as u8, // "hello"[-2]
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_err(), "negative string index must error");
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("Index out of bounds: negative index"),
            "error must mention negative index, got: {:?}",
            result
        );
    }

    // ── Bug M2: ArraySet OOB writes ──

    #[test]
    fn test_array_set_oob_errors() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![Value::Int(1), Value::Int(2)]))),
                Value::Int(5),
                Value::Int(99),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load array [1, 2]
                Opcode::LoadConst as u8,
                1,
                0, // Load index 5 (OOB)
                Opcode::LoadConst as u8,
                2,
                0,                      // Load value 99
                Opcode::ArraySet as u8, // arr[5] = 99
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_err(), "OOB array set must error");
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("Array index out of bounds"),
            "error must mention Array index out of bounds, got: {:?}",
            result
        );
    }

    #[test]
    fn test_array_set_valid_still_works() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![
                    Value::Int(1),
                    Value::Int(2),
                    Value::Int(3),
                ]))),
                Value::Int(1),
                Value::Int(99),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load array [1, 2, 3]
                Opcode::LoadConst as u8,
                1,
                0, // Load index 1
                Opcode::LoadConst as u8,
                2,
                0,                      // Load value 99
                Opcode::ArraySet as u8, // arr[1] = 99
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "valid array set must still work");
    }

    #[test]
    fn test_array_set_rejects_tuple_operand() {
        let program = make_program(
            vec![
                Value::Tuple(Gc::new(GcCell::new(TupleData::sealed(vec![Value::Int(1)])))),
                Value::Int(0),
                Value::Int(99),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::ArraySet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let error = vm.run().expect_err("ArraySet must reject tuples");
        assert!(
            error.contains("Invalid array/index types"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_tuple_set_rejects_array_operand() {
        let program = make_program(
            vec![
                Value::Array(Gc::new(GcCell::new(vec![Value::Int(1)]))),
                Value::Int(0),
                Value::Int(99),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let error = vm.run().expect_err("TupleSet must reject arrays");
        assert!(
            error.contains("Invalid tuple/index types"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn test_tuple_set_rejects_negative_and_out_of_bounds_indices() {
        for index in [-1, 1] {
            let program = make_program(
                vec![Value::Int(1), Value::Int(index), Value::Int(99)],
                vec![
                    Opcode::LoadConst as u8,
                    0,
                    0,
                    Opcode::NewTuple as u8,
                    Opcode::LoadConst as u8,
                    1,
                    0,
                    Opcode::LoadConst as u8,
                    2,
                    0,
                    Opcode::TupleSet as u8,
                    Opcode::Halt as u8,
                ],
            );
            let mut vm = VM::new(program);
            let error = vm.run().expect_err("invalid tuple index must fail");
            assert!(
                error.contains("Tuple index out of bounds"),
                "index {index} produced unexpected error: {error}"
            );
        }
    }

    #[test]
    fn test_tuple_set_initializes_new_tuple_once_in_order() {
        let program = make_program(
            vec![Value::Int(1), Value::Int(0), Value::Int(99)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewTuple as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        assert!(vm.run().is_ok(), "ordered tuple initialization must work");
        let Some(Value::Tuple(tuple)) = vm.stack.last() else {
            panic!("constructed tuple was not left on the stack");
        };
        let tuple = tuple.borrow();
        match tuple.elements.first() {
            Some(Value::Int(value)) => assert_eq!(*value, 99),
            other => panic!("TupleSet wrote unexpected value: {other:?}"),
        };
    }

    #[test]
    fn test_tuple_set_enforces_order_reads_elements_and_rejects_sealed_tuple() {
        let out_of_order = make_program(
            vec![Value::Int(3), Value::Int(1), Value::Int(20)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewTuple as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(out_of_order);
        let error = vm
            .run()
            .expect_err("tuple elements must be initialized in order");
        assert_eq!(
            error,
            "Tuple elements must be initialized in order: expected index 0, got 1"
        );

        let ordered = make_program(
            vec![
                Value::Int(3),
                Value::Int(0),
                Value::Int(10),
                Value::Int(1),
                Value::Int(20),
                Value::Int(2),
                Value::Int(30),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewTuple as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                3,
                0,
                Opcode::LoadConst as u8,
                4,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                5,
                0,
                Opcode::LoadConst as u8,
                6,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::ArrayGet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(ordered);
        assert!(vm.run().is_ok(), "ordered tuple initialization must work");
        assert_eq!(vm.stack.len(), 2);
        let Value::Tuple(tuple) = &vm.stack[0] else {
            panic!("constructed tuple was not left on the stack");
        };
        {
            let tuple = tuple.borrow();
            assert!(tuple.next_initializer.is_none());
            assert!(matches!(
                tuple.elements.as_slice(),
                [Value::Int(10), Value::Int(20), Value::Int(30)]
            ));
        }
        assert!(matches!(vm.stack.get(1), Some(Value::Int(10))));

        let sealed = make_program(
            vec![
                Value::Int(2),
                Value::Int(0),
                Value::Int(10),
                Value::Int(1),
                Value::Int(20),
                Value::Int(99),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewTuple as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                3,
                0,
                Opcode::LoadConst as u8,
                4,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                5,
                0,
                Opcode::TupleSet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(sealed);
        let error = vm.run().expect_err("sealed tuple must reject TupleSet");
        assert_eq!(error, "TupleSet is only valid while constructing a tuple");
    }

    #[test]
    fn test_tuple_set_rejects_mutation_after_construction() {
        let program = make_program(
            vec![
                Value::Int(1),
                Value::Int(0),
                Value::Int(99),
                Value::Int(100),
            ],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,
                Opcode::NewTuple as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                2,
                0,
                Opcode::TupleSet as u8,
                Opcode::Dup as u8,
                Opcode::LoadConst as u8,
                1,
                0,
                Opcode::LoadConst as u8,
                3,
                0,
                Opcode::TupleSet as u8,
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let error = vm
            .run()
            .expect_err("TupleSet must reject mutation after construction");
        assert!(
            error.contains("only valid while constructing a tuple"),
            "unexpected error: {error}"
        );
    }

    // ── Bug M3: Neg on Int::MIN wraps (spec: integer overflow is wrapping) ──

    #[test]
    fn test_neg_int_min_wraps() {
        let program = make_program(
            vec![Value::Int(i64::MIN)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,                 // Load Int::MIN
                Opcode::Neg as u8, // neg(Int::MIN) wraps to Int::MIN
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "neg(Int::MIN) must wrap, not error");
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, i64::MIN, "neg(Int::MIN) wraps to Int::MIN"),
            other => panic!("Expected Int(i64::MIN), got {:?}", other),
        }
    }

    #[test]
    fn test_neg_normal_still_works() {
        let program = make_program(
            vec![Value::Int(42)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0,                 // Load 42
                Opcode::Neg as u8, // -42
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok(), "normal negation must still work");
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Int(n) => assert_eq!(*n, -42),
            other => panic!("Expected Int(-42), got {:?}", other),
        }
    }

    // ── Bug M4: NaN float comparison ──

    fn assert_stack_bool(vm: &VM, expected: bool, msg: &str) {
        assert_eq!(vm.stack.len(), 1, "{}", msg);
        match &vm.stack[0] {
            Value::Bool(b) => assert_eq!(*b, expected, "{}", msg),
            other => panic!("{} — expected Bool({}), got {:?}", msg, expected, other),
        }
    }

    fn make_nan_test_program(opcode: Opcode) -> Program {
        make_program(
            vec![Value::Float(f64::NAN), Value::Float(1.0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load NaN
                Opcode::LoadConst as u8,
                1,
                0, // Load 1.0
                opcode as u8,
                Opcode::Halt as u8,
            ],
        )
    }

    fn make_nan_nan_test_program(opcode: Opcode) -> Program {
        make_program(
            vec![Value::Float(f64::NAN), Value::Float(f64::NAN)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load NaN
                Opcode::LoadConst as u8,
                1,
                0, // Load NaN
                opcode as u8,
                Opcode::Halt as u8,
            ],
        )
    }

    #[test]
    fn test_nan_lt_one_is_false() {
        let program = make_nan_test_program(Opcode::Lt);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN < 1 should be false");
    }

    #[test]
    fn test_nan_gt_one_is_false() {
        let program = make_nan_test_program(Opcode::Gt);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN > 1 should be false");
    }

    #[test]
    fn test_nan_le_one_is_false() {
        let program = make_nan_test_program(Opcode::Le);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN <= 1 should be false");
    }

    #[test]
    fn test_nan_ge_one_is_false() {
        let program = make_nan_test_program(Opcode::Ge);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN >= 1 should be false");
    }

    #[test]
    fn test_nan_eq_one_is_false() {
        let program = make_nan_test_program(Opcode::Eq);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN == 1 should be false");
    }

    #[test]
    fn test_nan_ne_one_is_true() {
        let program = make_nan_test_program(Opcode::Ne);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, true, "NaN != 1 should be true");
    }

    #[test]
    fn test_nan_eq_nan_is_false() {
        let program = make_nan_nan_test_program(Opcode::Eq);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "NaN == NaN should be false");
    }

    #[test]
    fn test_nan_ne_nan_is_true() {
        let program = make_nan_nan_test_program(Opcode::Ne);
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, true, "NaN != NaN should be true");
    }

    #[test]
    fn test_normal_float_comparisons_still_work() {
        let program = make_program(
            vec![Value::Float(3.0), Value::Float(5.0)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 3.0
                Opcode::LoadConst as u8,
                1,
                0,                // Load 5.0
                Opcode::Lt as u8, // 3.0 < 5.0 = true
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, true, "3.0 < 5.0 should be true");
    }

    #[test]
    fn test_one_gt_nan_is_false() {
        let program = make_program(
            vec![Value::Float(1.0), Value::Float(f64::NAN)],
            vec![
                Opcode::LoadConst as u8,
                0,
                0, // Load 1.0
                Opcode::LoadConst as u8,
                1,
                0,                // Load NaN
                Opcode::Gt as u8, // 1.0 > NaN should be false
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(result.is_ok());
        assert_stack_bool(&vm, false, "1 > NaN should be false");
    }

    #[test]
    fn test_make_closure_u16_limit_at_boundary() {
        // Verify MakeClosure with offset at u16::MAX does not panic.
        // This is a bytecode-layer test: we verify the u16 read does not
        // overflow, and that the VM handles the maximum representable offset.
        // Real closures at this offset are not practically reachable because
        // MAX_CODE_LENGTH (10_000_000) fits in u32 but codegen truncates to u16.
        let program = make_program(
            vec![],
            vec![
                Opcode::MakeClosure as u8,
                0xFF,
                0xFF, // code_offset = u16::MAX
                0x00, // capture_count = 0
                Opcode::Halt as u8,
            ],
        );
        let mut vm = VM::new(program);
        let result = vm.run();
        assert!(
            result.is_ok(),
            "MakeClosure at u16::MAX should not panic: {:?}",
            result
        );
        // Should have a closure on the stack with the expected offset
        assert_eq!(vm.stack.len(), 1);
        match &vm.stack[0] {
            Value::Closure(c) => assert_eq!(c.code_offset, u16::MAX as usize),
            other => panic!("Expected Closure, got {:?}", other),
        }
    }
}

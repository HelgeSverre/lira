//! Debug Support Types
//!
//! Types for debugging execution: stepping, state inspection, and pause/resume.

use crate::value::Value;
use gc::Gc;
use std::collections::{BinaryHeap, HashMap, HashSet};
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
    Paused { line: u32, column: u32, ip: usize },
    /// Suspended by user pause request
    Suspended { line: u32, column: u32, ip: usize },
    /// Program finished normally
    Finished { exit_code: i32 },
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
    Breakpoint { line: u32, column: u32, ip: usize },
    /// Pause requested by user
    Paused { line: u32, column: u32, ip: usize },
    /// Step operation completed (for line/into/over/out)
    StepCompleted { line: u32, column: u32, ip: usize },
    /// Program finished normally
    Finished { exit_code: i32 },
    /// Runtime error occurred
    Error { message: String },
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
    /// Rich structured value (for frontend inspection)
    pub rich_value: Option<RichValue>,
}

impl ValueInfo {
    pub fn new(display: String, type_name: String) -> Self {
        Self {
            display,
            type_name,
            rich_value: None,
        }
    }

    pub fn with_rich_value(display: String, type_name: String, rich_value: RichValue) -> Self {
        Self {
            display,
            type_name,
            rich_value: Some(rich_value),
        }
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

/// Rich value representation for serialization
/// This is a serializable representation of Value that doesn't use Rc/RefCell
#[derive(Debug, Clone)]
pub enum RichValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<RichValue>),
    Tuple(Vec<RichValue>),
    Object(HashMap<String, RichValue>),
    Function {
        code_offset: usize,
    },
    Closure {
        code_offset: usize,
        captures: Vec<RichValue>,
    },
    Fiber {
        id: u64,
    },
    Channel {
        id: u64,
    },
}

impl RichValue {
    /// Convert a VM Value to a RichValue
    pub fn from_value(value: &Value) -> Self {
        RichValueBuilder::new().convert(value)
    }

    /// Compact, allocation-bounded label for debugger tables.
    ///
    /// Aggregate contents remain available through the structured value; the
    /// table label deliberately avoids rendering them recursively a second
    /// time.
    pub(crate) fn display_summary(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::Array(elements) => format!("[{} elements]", elements.len()),
            Self::Tuple(elements) => format!("({} elements)", elements.len()),
            Self::Object(fields) => format!("{{{} fields}}", fields.len()),
            Self::Function { code_offset } => format!("<function@{code_offset}>"),
            Self::Closure { code_offset, .. } => format!("<closure@{code_offset}>"),
            Self::Fiber { id } => format!("<fiber#{id}>"),
            Self::Channel { id } => format!("<channel#{id}>"),
        }
    }
}

const RICH_VALUE_DEPTH_LIMIT: usize = 64;
const RICH_VALUE_NODE_LIMIT: usize = 10_000;
const RICH_VALUE_TEXT_BYTE_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum HeapId {
    Array(usize),
    Tuple(usize),
    Object(usize),
    Struct(usize),
    Closure(usize),
}

/// Bounded converter shared by one debugger snapshot.
///
/// The active-path set breaks cycles without collapsing repeated values in an
/// acyclic DAG. A snapshot-wide node budget also prevents a wide aggregate—or
/// thousands of separate stack roots—from duplicating unbounded data into the
/// debugger protocol.
pub(crate) struct RichValueBuilder {
    active: HashSet<HeapId>,
    remaining_nodes: usize,
    remaining_text_bytes: usize,
}

impl RichValueBuilder {
    pub(crate) fn new() -> Self {
        Self {
            active: HashSet::new(),
            remaining_nodes: RICH_VALUE_NODE_LIMIT,
            remaining_text_bytes: RICH_VALUE_TEXT_BYTE_LIMIT,
        }
    }

    pub(crate) fn convert(&mut self, value: &Value) -> RichValue {
        self.convert_at(value, 0)
    }

    fn marker(text: &str) -> RichValue {
        RichValue::String(text.to_string())
    }

    fn convert_text(&mut self, text: &str) -> String {
        if text.len() <= self.remaining_text_bytes {
            self.remaining_text_bytes -= text.len();
            return text.to_string();
        }

        let mut end = self.remaining_text_bytes.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        self.remaining_text_bytes = 0;
        if end == 0 {
            "<text-limit>".to_string()
        } else {
            format!("{}<text-limit>", &text[..end])
        }
    }

    fn convert_values(&mut self, values: &[Value], depth: usize) -> Vec<RichValue> {
        let mut converted = Vec::new();
        for value in values {
            if self.remaining_nodes == 0 {
                converted.push(Self::marker("<node-limit>"));
                break;
            }
            converted.push(self.convert_at(value, depth));
        }
        converted
    }

    fn convert_fields(
        &mut self,
        fields: &HashMap<String, Value>,
        depth: usize,
    ) -> HashMap<String, RichValue> {
        // Select at most the node budget's lexicographically first field names.
        // A bounded max-heap preserves deterministic selection without cloning
        // every key/value from a potentially huge dynamic object first.
        let field_limit = self.remaining_nodes.min(fields.len());
        let mut selected = BinaryHeap::with_capacity(field_limit);
        for name in fields.keys().map(String::as_str) {
            if selected.len() < field_limit {
                selected.push(name);
            } else if selected.peek().is_some_and(|largest| name < *largest) {
                selected.pop();
                selected.push(name);
            }
        }

        let mut converted = HashMap::new();
        for name in selected.into_sorted_vec() {
            if self.remaining_nodes == 0 || self.remaining_text_bytes == 0 {
                converted.insert(
                    "<truncated>".to_string(),
                    Self::marker(if self.remaining_nodes == 0 {
                        "<node-limit>"
                    } else {
                        "<text-limit>"
                    }),
                );
                break;
            }
            let output_name = self.convert_text(name);
            converted.insert(output_name, self.convert_at(&fields[name], depth));
        }
        if fields.len() > field_limit {
            converted.insert("<truncated>".to_string(), Self::marker("<node-limit>"));
        }
        converted
    }

    fn convert_at(&mut self, value: &Value, depth: usize) -> RichValue {
        if depth >= RICH_VALUE_DEPTH_LIMIT {
            return Self::marker("<max-depth>");
        }
        if self.remaining_nodes == 0 {
            return Self::marker("<node-limit>");
        }
        self.remaining_nodes -= 1;

        match value {
            Value::Null => RichValue::Null,
            Value::Bool(b) => RichValue::Bool(*b),
            Value::Int(n) => RichValue::Int(*n),
            Value::Float(f) => RichValue::Float(*f),
            Value::String(s) => RichValue::String(self.convert_text(s)),
            Value::Array(arr) => {
                let identity = HeapId::Array(Gc::as_ptr(arr) as usize);
                if !self.active.insert(identity) {
                    return Self::marker("<cycle>");
                }
                let source = arr.borrow();
                let elements = self.convert_values(&source, depth + 1);
                self.active.remove(&identity);
                RichValue::Array(elements)
            }
            Value::Tuple(tuple) => {
                let identity = HeapId::Tuple(Gc::as_ptr(tuple) as usize);
                if !self.active.insert(identity) {
                    return Self::marker("<cycle>");
                }
                let source = tuple.borrow();
                let elements = self.convert_values(&source.elements, depth + 1);
                self.active.remove(&identity);
                RichValue::Tuple(elements)
            }
            Value::Object(obj) => {
                let identity = HeapId::Object(Gc::as_ptr(obj) as usize);
                if !self.active.insert(identity) {
                    return Self::marker("<cycle>");
                }
                let source = obj.borrow();
                let fields = self.convert_fields(&source, depth + 1);
                self.active.remove(&identity);
                RichValue::Object(fields)
            }
            Value::Struct(obj) => {
                let identity = HeapId::Struct(Gc::as_ptr(obj) as usize);
                if !self.active.insert(identity) {
                    return Self::marker("<cycle>");
                }
                let source = obj.borrow();
                let fields = self.convert_fields(&source, depth + 1);
                self.active.remove(&identity);
                // The debug protocol currently exposes aggregate fields through
                // one object shape; retain the fields while preserving the
                // VM's distinct struct representation internally.
                RichValue::Object(fields)
            }
            Value::Function(offset) => RichValue::Function {
                code_offset: *offset,
            },
            Value::Closure(closure) => {
                let identity = HeapId::Closure(Gc::as_ptr(closure) as usize);
                if !self.active.insert(identity) {
                    return Self::marker("<cycle>");
                }
                let captures = self.convert_values(&closure.captures, depth + 1);
                self.active.remove(&identity);
                RichValue::Closure {
                    code_offset: closure.code_offset,
                    captures,
                }
            }
            Value::Interface(interface) => {
                RichValue::String(format!("<interface {} methods>", interface.methods.len()))
            }
            Value::Fiber(id) => RichValue::Fiber { id: *id },
            Value::Channel(id) => RichValue::Channel { id: *id },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{ClosureData, TupleData};
    use gc::GcCell;

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
    fn rich_value_conversion_breaks_array_tuple_object_and_closure_cycles() {
        let array = Gc::new(GcCell::new(Vec::new()));
        array.borrow_mut().push(Value::Array(array.clone()));
        match RichValue::from_value(&Value::Array(array)) {
            RichValue::Array(elements) => {
                assert!(
                    matches!(elements.as_slice(), [RichValue::String(marker)] if marker == "<cycle>")
                );
            }
            other => panic!("unexpected cyclic array rendering: {other:?}"),
        }

        let tuple = Gc::new(GcCell::new(TupleData::sealed(Vec::new())));
        tuple
            .borrow_mut()
            .elements
            .push(Value::Tuple(tuple.clone()));
        match RichValue::from_value(&Value::Tuple(tuple)) {
            RichValue::Tuple(elements) => {
                assert!(
                    matches!(elements.as_slice(), [RichValue::String(marker)] if marker == "<cycle>")
                );
            }
            other => panic!("unexpected cyclic tuple rendering: {other:?}"),
        }

        let object = Gc::new(GcCell::new(HashMap::new()));
        object
            .borrow_mut()
            .insert("self".to_string(), Value::Object(object.clone()));
        match RichValue::from_value(&Value::Object(object)) {
            RichValue::Object(fields) => {
                assert!(
                    matches!(fields.get("self"), Some(RichValue::String(marker)) if marker == "<cycle>")
                );
            }
            other => panic!("unexpected cyclic object rendering: {other:?}"),
        }

        let array = Gc::new(GcCell::new(Vec::new()));
        let closure = Gc::new(ClosureData {
            code_offset: 7,
            captures: vec![Value::Array(array.clone())],
        });
        array.borrow_mut().push(Value::Closure(closure));
        match RichValue::from_value(&Value::Array(array)) {
            RichValue::Array(elements) => match elements.as_slice() {
                [RichValue::Closure { captures, .. }] => {
                    assert!(
                        matches!(captures.as_slice(), [RichValue::String(marker)] if marker == "<cycle>")
                    );
                }
                other => panic!("unexpected closure cycle elements: {other:?}"),
            },
            other => panic!("unexpected closure cycle rendering: {other:?}"),
        }
    }

    #[test]
    fn rich_value_conversion_bounds_depth_and_width() {
        let mut value = Value::Int(1);
        for _ in 0..=RICH_VALUE_DEPTH_LIMIT {
            value = Value::Array(Gc::new(GcCell::new(vec![value])));
        }
        let mut rendered = RichValue::from_value(&value);
        loop {
            match rendered {
                RichValue::Array(mut elements) if elements.len() == 1 => {
                    rendered = elements.remove(0);
                }
                RichValue::String(marker) => {
                    assert_eq!(marker, "<max-depth>");
                    break;
                }
                other => panic!("unexpected depth-limited rendering: {other:?}"),
            }
        }

        let wide = Value::Array(Gc::new(GcCell::new(vec![
            Value::Null;
            RICH_VALUE_NODE_LIMIT + 100
        ])));
        match RichValue::from_value(&wide) {
            RichValue::Array(elements) => {
                assert!(elements.len() <= RICH_VALUE_NODE_LIMIT);
                assert!(
                    matches!(elements.last(), Some(RichValue::String(marker)) if marker == "<node-limit>")
                );
            }
            other => panic!("unexpected width-limited rendering: {other:?}"),
        }

        let huge_text = "x".repeat(RICH_VALUE_TEXT_BYTE_LIMIT + 100);
        match RichValue::from_value(&Value::String(huge_text.into())) {
            RichValue::String(text) => {
                assert!(text.len() <= RICH_VALUE_TEXT_BYTE_LIMIT + "<text-limit>".len());
                assert!(text.ends_with("<text-limit>"));
            }
            other => panic!("unexpected text-limited rendering: {other:?}"),
        }
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

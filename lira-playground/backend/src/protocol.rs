//! WebSocket Protocol Type Definitions
//!
//! Defines the messages exchanged between frontend and backend.
//! These match the TypeScript definitions in frontend/src/types/protocol.ts
//!
//! Note: Some types are prepared for future debugging features and are not yet used.

#![allow(dead_code)]

use liravm::{
    ChannelStateSnapshot, FiberState, FiberStateSnapshot, RichValue, SchedulerSnapshot, Value,
};
use serde::{Deserialize, Serialize};

/// Messages sent from client to server
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// Type check source code
    Check { source: String },
    /// Compile and run source code
    Run {
        source: String,
        /// Run with the concurrent fiber scheduler enabled. When true, the
        /// program is driven to completion and a `VmStateJson` with the final
        /// fiber/channel state is returned.
        #[serde(default)]
        fiber_mode: bool,
    },
    /// Compile and run in debug mode (breakpoints included to avoid race condition)
    Debug {
        source: String,
        #[serde(default)]
        breakpoints: Vec<u32>,
        /// Enable the concurrent fiber scheduler (see `Run::fiber_mode`).
        #[serde(default)]
        fiber_mode: bool,
    },
    /// Set breakpoints (for updating during debug session)
    SetBreakpoints { breakpoints: Vec<u32> },
    /// Continue execution
    Continue,
    /// Step one instruction
    StepInstruction,
    /// Step one line
    StepLine,
    /// Step into function
    StepInto,
    /// Step over function call
    StepOver,
    /// Step out of function
    StepOut,
    /// Inspect a variable
    InspectVariable { name: String },
    /// Request current locals
    InspectLocals,
    /// Request current stack
    InspectStack,
    /// Pause execution
    Pause,
    /// Stop execution
    Stop,
    /// Get AST for source
    GetAst { source: String },
    /// Ping for keepalive
    Ping,
}

/// Messages sent from server to client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    /// Compilation succeeded
    CompileSuccess {
        ast: Option<serde_json::Value>,
        #[serde(rename = "bytecodeSize")]
        bytecode_size: usize,
    },
    /// Compilation failed
    CompileError { errors: Vec<CompileError> },
    /// Program output
    Output { text: String },
    /// Program finished
    Finished {
        #[serde(rename = "exitCode")]
        exit_code: i32,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
    },
    /// Runtime error
    RuntimeError {
        message: String,
        location: Option<SourceLocation>,
    },
    /// Breakpoint hit
    BreakpointHit { line: u32, column: u32, ip: usize },
    /// Execution paused
    Paused { line: u32, column: u32, ip: usize },
    /// Variable value
    Variable {
        name: String,
        value: String,
        #[serde(rename = "valueType")]
        value_type: String,
    },
    /// Local variables
    Locals { locals: Vec<VariableInfo> },
    /// Stack state
    Stack { stack: Vec<String> },
    /// AST response
    Ast { ast: serde_json::Value },
    /// VM state update
    VmStateUpdate { state: VmState },
    /// Full fiber/channel scheduler state (fiber-mode runs)
    VmStateJson { state: VmStateJson },
    /// Fiber spawned
    FiberSpawned { fiber: FiberInfo },
    /// Fiber state changed
    FiberStateChanged {
        #[serde(rename = "fiberId")]
        fiber_id: u64,
        #[serde(rename = "newState")]
        new_state: String,
    },
    /// Channel created
    ChannelCreated { channel: ChannelInfo },
    /// Channel message
    ChannelMessage {
        #[serde(rename = "channelId")]
        channel_id: u64,
        operation: String,
        value: String,
    },
    /// Pong response
    Pong,
    /// Timeout warning
    TimeoutWarning {
        #[serde(rename = "secondsRemaining")]
        seconds_remaining: u32,
    },
    /// Execution stopped
    Stopped,
    /// Breakpoints were set successfully
    BreakpointsSet {
        /// Breakpoint lines that were successfully applied
        breakpoints: Vec<u32>,
    },
    /// Step completed (for stepping operations)
    StepCompleted { line: u32, column: u32, ip: usize },
    /// Variable value response
    VariableValue {
        name: String,
        value: String,
        #[serde(rename = "typeName")]
        type_name: String,
    },
}

/// Compilation error
#[derive(Debug, Clone, Serialize)]
pub struct CompileError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub severity: ErrorSeverity,
}

/// Error severity levels
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorSeverity {
    Error,
    Warning,
    Hint,
}

/// Source location
#[derive(Debug, Clone, Serialize)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
    pub file: Option<String>,
}

/// Variable info for inspection
#[derive(Debug, Clone, Serialize)]
pub struct VariableInfo {
    pub name: String,
    /// Rich structured value
    pub value: ValueJson,
    #[serde(rename = "typeName")]
    pub type_name: String,
}

/// VM state for debug updates
#[derive(Debug, Clone, Serialize)]
pub struct VmState {
    #[serde(rename = "executionState")]
    pub execution_state: String,
    pub ip: usize,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Rich structured stack values
    pub stack: Vec<ValueJson>,
    pub locals: Vec<VariableInfo>,
    #[serde(rename = "callStack")]
    pub call_stack: Vec<String>,
    pub output: Vec<String>,
}

/// Call frame info for stack trace
#[derive(Debug, Clone, Serialize)]
pub struct CallFrameInfo {
    #[serde(rename = "functionName")]
    pub function_name: Option<String>,
    pub line: Option<u32>,
    pub ip: usize,
}

/// Fiber info for state updates
#[derive(Debug, Clone, Serialize)]
pub struct FiberInfo {
    pub id: u64,
    pub state: String,
    pub ip: usize,
}

/// Channel info for state updates
#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    pub id: u64,
    pub capacity: usize,
    #[serde(rename = "bufferSize")]
    pub buffer_size: usize,
    pub closed: bool,
}

/// VM state in JSON format
#[derive(Debug, Clone, Serialize)]
pub struct VmStateJson {
    pub fibers: Vec<FiberStateJson>,
    pub channels: Vec<ChannelStateJson>,
    #[serde(rename = "currentFiberId")]
    pub current_fiber_id: Option<u64>,
    #[serde(rename = "readyQueue")]
    pub ready_queue: Vec<u64>,
    pub output: Vec<String>,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
}

/// Fiber state in JSON format
#[derive(Debug, Clone, Serialize)]
pub struct FiberStateJson {
    pub id: u64,
    pub state: FiberStateValue,
    pub ip: usize,
    pub stack: Vec<ValueJson>,
    pub locals: Vec<ValueJson>,
    #[serde(rename = "callStack")]
    pub call_stack: Vec<FiberCallFrameJson>,
    pub result: Option<ValueJson>,
}

/// Fiber state value
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum FiberStateValue {
    Ready,
    Running,
    BlockedReceive {
        #[serde(rename = "channelId")]
        channel_id: u64,
    },
    BlockedSend {
        #[serde(rename = "channelId")]
        channel_id: u64,
    },
    BlockedSelect,
    Yielded,
    Finished,
    Failed {
        error: String,
    },
}

/// Call frame in JSON format
#[derive(Debug, Clone, Serialize)]
pub struct FiberCallFrameJson {
    #[serde(rename = "returnAddr")]
    pub return_addr: usize,
    #[serde(rename = "localsBase")]
    pub locals_base: usize,
    #[serde(rename = "functionName")]
    pub function_name: Option<String>,
}

/// Channel state in JSON format
#[derive(Debug, Clone, Serialize)]
pub struct ChannelStateJson {
    pub id: u64,
    pub buffer: Vec<ValueJson>,
    pub capacity: usize,
    pub receivers: Vec<u64>,
    pub senders: Vec<SenderJson>,
    pub closed: bool,
}

/// Sender info in JSON format
#[derive(Debug, Clone, Serialize)]
pub struct SenderJson {
    #[serde(rename = "fiberId")]
    pub fiber_id: u64,
    pub value: ValueJson,
}

impl VmStateJson {
    /// Build the wire-format VM state from a liravm [`SchedulerSnapshot`].
    ///
    /// `output` and `exit_code` are left empty here; they are delivered via the
    /// separate `Output`/`Finished` messages.
    pub fn from_snapshot(snap: &SchedulerSnapshot) -> Self {
        VmStateJson {
            fibers: snap
                .fibers
                .iter()
                .map(FiberStateJson::from_snapshot)
                .collect(),
            channels: snap
                .channels
                .iter()
                .map(ChannelStateJson::from_snapshot)
                .collect(),
            current_fiber_id: snap.current_fiber_id,
            ready_queue: snap.ready_queue.clone(),
            output: Vec::new(),
            exit_code: None,
        }
    }
}

impl FiberStateJson {
    fn from_snapshot(f: &FiberStateSnapshot) -> Self {
        FiberStateJson {
            id: f.id,
            state: FiberStateValue::from_fiber_state(&f.state),
            ip: f.ip,
            stack: f.stack.iter().map(ValueJson::from_rich_value).collect(),
            locals: f.locals.iter().map(ValueJson::from_rich_value).collect(),
            call_stack: f
                .call_stack
                .iter()
                .map(|fr| FiberCallFrameJson {
                    return_addr: fr.return_addr,
                    locals_base: fr.locals_base,
                    function_name: fr.function_name.clone(),
                })
                .collect(),
            result: f.result.as_ref().map(ValueJson::from_rich_value),
        }
    }
}

impl ChannelStateJson {
    fn from_snapshot(c: &ChannelStateSnapshot) -> Self {
        ChannelStateJson {
            id: c.id,
            buffer: c.buffer.iter().map(ValueJson::from_rich_value).collect(),
            capacity: c.capacity,
            receivers: c.receivers.clone(),
            senders: c
                .senders
                .iter()
                .map(|(id, v)| SenderJson {
                    fiber_id: *id,
                    value: ValueJson::from_rich_value(v),
                })
                .collect(),
            closed: c.closed,
        }
    }
}

impl FiberStateValue {
    fn from_fiber_state(state: &FiberState) -> Self {
        match state {
            FiberState::Ready => FiberStateValue::Ready,
            FiberState::Running => FiberStateValue::Running,
            FiberState::BlockedReceive(ch) => FiberStateValue::BlockedReceive { channel_id: *ch },
            FiberState::BlockedSend(ch) => FiberStateValue::BlockedSend { channel_id: *ch },
            FiberState::BlockedSelect => FiberStateValue::BlockedSelect,
            FiberState::Yielded => FiberStateValue::Yielded,
            FiberState::Finished => FiberStateValue::Finished,
            FiberState::Failed(e) => FiberStateValue::Failed { error: e.clone() },
        }
    }
}

/// Value in JSON format
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ValueJson {
    Null,
    Bool {
        value: bool,
    },
    Int {
        value: i64,
    },
    Float {
        value: f64,
    },
    String {
        value: String,
    },
    Array {
        elements: Vec<ValueJson>,
    },
    Object {
        fields: std::collections::HashMap<String, ValueJson>,
    },
    Function {
        #[serde(rename = "codeOffset")]
        code_offset: usize,
    },
    Closure {
        #[serde(rename = "codeOffset")]
        code_offset: usize,
        captures: Vec<ValueJson>,
    },
    Fiber {
        id: u64,
    },
    Channel {
        id: u64,
    },
}

impl ValueJson {
    /// Convert a VM Value to ValueJson
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => ValueJson::Null,
            Value::Bool(b) => ValueJson::Bool { value: *b },
            Value::Int(n) => ValueJson::Int { value: *n },
            Value::Float(f) => ValueJson::Float { value: *f },
            Value::String(s) => ValueJson::String {
                value: (**s).clone(),
            },
            Value::Array(arr) => {
                let elements = arr.borrow().iter().map(ValueJson::from_value).collect();
                ValueJson::Array { elements }
            }
            Value::Object(obj) => {
                let fields = obj
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueJson::from_value(v)))
                    .collect();
                ValueJson::Object { fields }
            }
            Value::Function(offset) => ValueJson::Function {
                code_offset: *offset,
            },
            Value::Closure(c) => ValueJson::Closure {
                code_offset: c.code_offset,
                captures: c.captures.iter().map(ValueJson::from_value).collect(),
            },
            Value::Fiber(id) => ValueJson::Fiber { id: *id },
            Value::Channel(id) => ValueJson::Channel { id: *id },
        }
    }

    /// Convert a RichValue to ValueJson
    pub fn from_rich_value(value: &RichValue) -> Self {
        match value {
            RichValue::Null => ValueJson::Null,
            RichValue::Bool(b) => ValueJson::Bool { value: *b },
            RichValue::Int(n) => ValueJson::Int { value: *n },
            RichValue::Float(f) => ValueJson::Float { value: *f },
            RichValue::String(s) => ValueJson::String { value: s.clone() },
            RichValue::Array(elements) => ValueJson::Array {
                elements: elements.iter().map(ValueJson::from_rich_value).collect(),
            },
            RichValue::Object(fields) => ValueJson::Object {
                fields: fields
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueJson::from_rich_value(v)))
                    .collect(),
            },
            RichValue::Function { code_offset } => ValueJson::Function {
                code_offset: *code_offset,
            },
            RichValue::Closure {
                code_offset,
                captures,
            } => ValueJson::Closure {
                code_offset: *code_offset,
                captures: captures.iter().map(ValueJson::from_rich_value).collect(),
            },
            RichValue::Fiber { id } => ValueJson::Fiber { id: *id },
            RichValue::Channel { id } => ValueJson::Channel { id: *id },
        }
    }
}

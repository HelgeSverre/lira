//! WebSocket Protocol Type Definitions
//!
//! Defines the messages exchanged between frontend and backend.
//! These match the TypeScript definitions in frontend/src/types/protocol.ts
//!
//! Note: Some types are prepared for future debugging features and are not yet used.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Messages sent from client to server
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    /// Type check source code
    Check { source: String },
    /// Compile and run source code
    Run { source: String },
    /// Compile and run in debug mode
    Debug { source: String },
    /// Set breakpoints
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
    pub value: String,
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
    pub stack: Vec<String>,
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
    Failed { error: String },
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

/// Value in JSON format
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ValueJson {
    Null,
    Bool { value: bool },
    Int { value: i64 },
    Float { value: f64 },
    String { value: String },
    Array { elements: Vec<ValueJson> },
    Object { fields: std::collections::HashMap<String, ValueJson> },
    Function {
        #[serde(rename = "codeOffset")]
        code_offset: usize,
    },
    Closure {
        #[serde(rename = "codeOffset")]
        code_offset: usize,
        captures: Vec<ValueJson>,
    },
    Fiber { id: u64 },
    Channel { id: u64 },
}

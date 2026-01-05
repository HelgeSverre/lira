//! Dedicated VM Thread
//!
//! Spawns a dedicated std::thread per WebSocket session that owns the non-Send VM.
//! Communication happens via tokio::sync::mpsc channels, allowing the async WebSocket
//! handler to interact with the synchronous VM execution.

use std::panic::AssertUnwindSafe;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use liravm::{DebugEvent, DebugSession, DebugSnapshot};
use tokio::sync::{mpsc, oneshot};

use crate::handlers::compile_source;
use crate::protocol::{
    ClientMessage, CompileError, ErrorSeverity, ServerMessage, VariableInfo, VmState,
};

/// Channel buffer size for commands
const CHANNEL_BUFFER_SIZE: usize = 32;

/// Default timeout for command responses (30 seconds)
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Commands sent from WebSocket handler to VM thread
pub enum VmCommand {
    /// Execute a client message and get response
    Execute {
        message: ClientMessage,
        response_tx: oneshot::Sender<VmResponse>,
    },
    /// Shutdown the VM thread
    Shutdown,
}

/// Response from VM thread
pub struct VmResponse {
    /// Server messages to send back to client
    pub messages: Vec<ServerMessage>,
    /// Whether the WebSocket connection should terminate
    pub terminate: bool,
}

/// Errors that can occur when communicating with VM thread
#[derive(Debug)]
pub enum VmError {
    /// VM thread died unexpectedly
    ThreadDied,
    /// Command timed out
    Timeout,
    /// Response channel was closed
    ResponseChannelClosed,
    /// Channel is full (backpressure)
    ChannelFull,
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::ThreadDied => write!(f, "VM thread died unexpectedly"),
            VmError::Timeout => write!(f, "VM command timed out"),
            VmError::ResponseChannelClosed => write!(f, "Response channel closed"),
            VmError::ChannelFull => write!(f, "Command channel full"),
        }
    }
}

impl std::error::Error for VmError {}

/// Handle to communicate with the VM thread
pub struct VmThreadHandle {
    /// Channel to send commands to the VM thread
    command_tx: mpsc::Sender<VmCommand>,
    /// Thread join handle (used for cleanup)
    join_handle: Option<JoinHandle<()>>,
}

impl VmThreadHandle {
    /// Spawn a new VM thread and return a handle to communicate with it
    pub fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

        let join_handle = thread::spawn(move || {
            vm_thread_main(command_rx);
        });

        Self {
            command_tx,
            join_handle: Some(join_handle),
        }
    }

    /// Send a command to the VM thread and wait for response
    pub async fn send_command(&self, message: ClientMessage) -> Result<VmResponse, VmError> {
        let (response_tx, response_rx) = oneshot::channel();

        // Send command to VM thread
        self.command_tx
            .send(VmCommand::Execute {
                message,
                response_tx,
            })
            .await
            .map_err(|_| VmError::ThreadDied)?;

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(VmError::ResponseChannelClosed),
            Err(_) => Err(VmError::Timeout),
        }
    }

    /// Shutdown the VM thread gracefully
    pub async fn shutdown(&mut self) {
        // Send shutdown command (ignore errors if thread is already dead)
        let _ = self.command_tx.send(VmCommand::Shutdown).await;

        // Wait for thread to finish
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for VmThreadHandle {
    fn drop(&mut self) {
        // Best-effort shutdown - use try_send since we can't await in Drop
        let _ = self.command_tx.try_send(VmCommand::Shutdown);

        // Don't block in Drop, let the thread finish on its own
        // The thread will exit when it receives Shutdown or when the channel is dropped
    }
}

/// Main loop for the VM thread
fn vm_thread_main(mut command_rx: mpsc::Receiver<VmCommand>) {
    // Create a single-threaded tokio runtime for channel operations
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime in VM thread");

    // Create the debug session (owns the non-Send VM)
    let mut session = DebugSession::new();

    rt.block_on(async {
        loop {
            match command_rx.recv().await {
                Some(VmCommand::Execute {
                    message,
                    response_tx,
                }) => {
                    // Handle the message with panic recovery
                    let response =
                        match std::panic::catch_unwind(AssertUnwindSafe(|| {
                            handle_client_message(&mut session, message)
                        })) {
                            Ok(response) => response,
                            Err(panic_info) => {
                                // Extract panic message
                                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "Unknown panic".to_string()
                                };

                                tracing::error!("VM panic: {}", panic_msg);

                                // Reset session after panic
                                session = DebugSession::new();

                                VmResponse {
                                    messages: vec![ServerMessage::RuntimeError {
                                        message: format!("Internal VM error: {}", panic_msg),
                                        location: None,
                                    }],
                                    terminate: false,
                                }
                            }
                        };

                    // Send response (ignore error if receiver dropped)
                    let _ = response_tx.send(response);
                }
                Some(VmCommand::Shutdown) | None => {
                    // Shutdown requested or channel closed
                    break;
                }
            }
        }
    });
}

/// Handle a client message and produce server responses
fn handle_client_message(session: &mut DebugSession, msg: ClientMessage) -> VmResponse {
    match msg {
        ClientMessage::Ping => VmResponse {
            messages: vec![ServerMessage::Pong],
            terminate: false,
        },

        ClientMessage::Check { source } => handle_check(&source),

        ClientMessage::GetAst { source } => handle_get_ast(&source),

        ClientMessage::Run { source } => handle_run(session, &source, false),

        ClientMessage::Debug { source } => handle_run(session, &source, true),

        ClientMessage::Stop => {
            session.stop();
            VmResponse {
                messages: vec![ServerMessage::Stopped],
                terminate: false,
            }
        }

        ClientMessage::Pause => {
            session.pause();
            VmResponse {
                messages: vec![], // Pause is async - response comes when VM actually pauses
                terminate: false,
            }
        }

        ClientMessage::Continue => handle_continue(session),

        ClientMessage::StepInstruction => handle_step_instruction(session),

        ClientMessage::StepLine => handle_step_line(session),

        ClientMessage::StepInto => handle_step_into(session),

        ClientMessage::StepOut => handle_step_out(session),

        ClientMessage::SetBreakpoints { breakpoints } => {
            session.set_breakpoints(breakpoints);
            VmResponse {
                messages: vec![],
                terminate: false,
            }
        }

        ClientMessage::InspectVariable { name } => handle_inspect_variable(session, &name),

        ClientMessage::InspectLocals => handle_inspect_locals(session),

        ClientMessage::InspectStack => handle_inspect_stack(session),
    }
}

/// Handle type check request
fn handle_check(source: &str) -> VmResponse {
    match lirac::check(source) {
        Ok(()) => VmResponse {
            messages: vec![ServerMessage::CompileSuccess {
                ast: None,
                bytecode_size: 0,
            }],
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::CompileError {
                errors: parse_error(&e),
            }],
            terminate: false,
        },
    }
}

/// Handle get AST request
fn handle_get_ast(source: &str) -> VmResponse {
    match compile_source(source) {
        Ok((ast, bytecode)) => {
            let ast_json = serde_json::to_value(&ast).ok();
            VmResponse {
                messages: vec![
                    ServerMessage::CompileSuccess {
                        ast: ast_json.clone(),
                        bytecode_size: bytecode.len(),
                    },
                    ServerMessage::Ast {
                        ast: ast_json.unwrap_or(serde_json::Value::Null),
                    },
                ],
                terminate: false,
            }
        }
        Err(errors) => VmResponse {
            messages: vec![ServerMessage::CompileError { errors }],
            terminate: false,
        },
    }
}

/// Handle run/debug request
fn handle_run(session: &mut DebugSession, source: &str, debug_mode: bool) -> VmResponse {
    let mut messages = Vec::new();
    let start = std::time::Instant::now();

    // Compile source
    let bytecode = match compile_source(source) {
        Ok((ast, bytecode)) => {
            let ast_json = serde_json::to_value(&ast).ok();
            messages.push(ServerMessage::CompileSuccess {
                ast: if debug_mode { ast_json } else { None },
                bytecode_size: bytecode.len(),
            });
            bytecode
        }
        Err(errors) => {
            return VmResponse {
                messages: vec![ServerMessage::CompileError { errors }],
                terminate: false,
            };
        }
    };

    // Load bytecode into session
    if let Err(e) = session.load(source, bytecode) {
        return VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        };
    }

    // Start execution
    let snapshot = match session.start() {
        Ok(snapshot) => snapshot,
        Err(e) => {
            return VmResponse {
                messages: vec![ServerMessage::RuntimeError {
                    message: e,
                    location: None,
                }],
                terminate: false,
            };
        }
    };

    // Send initial state if in debug mode
    if debug_mode {
        messages.push(ServerMessage::VmStateUpdate {
            state: snapshot_to_vm_state(&snapshot),
        });
    }

    // Continue execution until breakpoint or completion
    match session.continue_execution() {
        Ok(event) => {
            messages.extend(debug_event_to_messages(event, session, start));
        }
        Err(e) => {
            messages.push(ServerMessage::RuntimeError {
                message: e,
                location: None,
            });
        }
    }

    VmResponse {
        messages,
        terminate: false,
    }
}

/// Handle continue execution
fn handle_continue(session: &mut DebugSession) -> VmResponse {
    let start = std::time::Instant::now();

    match session.continue_execution() {
        Ok(event) => VmResponse {
            messages: debug_event_to_messages(event, session, start),
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        },
    }
}

/// Handle step instruction
fn handle_step_instruction(session: &mut DebugSession) -> VmResponse {
    match session.step_instruction() {
        Ok(event) => VmResponse {
            messages: debug_event_to_messages(event, session, std::time::Instant::now()),
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        },
    }
}

/// Handle step line
fn handle_step_line(session: &mut DebugSession) -> VmResponse {
    match session.step_line() {
        Ok(event) => VmResponse {
            messages: debug_event_to_messages(event, session, std::time::Instant::now()),
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        },
    }
}

/// Handle step into
fn handle_step_into(session: &mut DebugSession) -> VmResponse {
    match session.step_into() {
        Ok(event) => VmResponse {
            messages: debug_event_to_messages(event, session, std::time::Instant::now()),
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        },
    }
}

/// Handle step out
fn handle_step_out(session: &mut DebugSession) -> VmResponse {
    match session.step_out() {
        Ok(event) => VmResponse {
            messages: debug_event_to_messages(event, session, std::time::Instant::now()),
            terminate: false,
        },
        Err(e) => VmResponse {
            messages: vec![ServerMessage::RuntimeError {
                message: e,
                location: None,
            }],
            terminate: false,
        },
    }
}

/// Handle variable inspection
fn handle_inspect_variable(session: &DebugSession, name: &str) -> VmResponse {
    if let Some(snapshot) = session.get_snapshot() {
        // Look for variable in locals
        for local in &snapshot.locals {
            if local.name.as_deref() == Some(name) {
                return VmResponse {
                    messages: vec![ServerMessage::VariableValue {
                        name: name.to_string(),
                        value: local.value.display.clone(),
                        type_name: local.value.type_name.clone(),
                    }],
                    terminate: false,
                };
            }
        }
    }

    VmResponse {
        messages: vec![ServerMessage::VariableValue {
            name: name.to_string(),
            value: "<not found>".to_string(),
            type_name: "unknown".to_string(),
        }],
        terminate: false,
    }
}

/// Handle locals inspection
fn handle_inspect_locals(session: &DebugSession) -> VmResponse {
    let locals = if let Some(snapshot) = session.get_snapshot() {
        snapshot
            .locals
            .iter()
            .map(|l| VariableInfo {
                name: l.name.clone().unwrap_or_else(|| format!("local_{}", l.slot)),
                value: l.value.display.clone(),
                type_name: l.value.type_name.clone(),
            })
            .collect()
    } else {
        vec![]
    };

    VmResponse {
        messages: vec![ServerMessage::Locals { locals }],
        terminate: false,
    }
}

/// Handle stack inspection
fn handle_inspect_stack(session: &DebugSession) -> VmResponse {
    let stack = if let Some(snapshot) = session.get_snapshot() {
        snapshot.stack.iter().map(|v| v.display.clone()).collect()
    } else {
        vec![]
    };

    VmResponse {
        messages: vec![ServerMessage::Stack { stack }],
        terminate: false,
    }
}

/// Convert DebugEvent to ServerMessages
fn debug_event_to_messages(
    event: DebugEvent,
    session: &DebugSession,
    start: std::time::Instant,
) -> Vec<ServerMessage> {
    let mut messages = Vec::new();

    // Always send any accumulated output
    for line in session.get_output() {
        messages.push(ServerMessage::Output { text: line });
    }

    match event {
        DebugEvent::StateChanged(snapshot) => {
            messages.push(ServerMessage::VmStateUpdate {
                state: snapshot_to_vm_state(&snapshot),
            });
        }
        DebugEvent::Output(text) => {
            messages.push(ServerMessage::Output { text });
        }
        DebugEvent::BreakpointHit { line, column, ip } => {
            messages.push(ServerMessage::BreakpointHit { line, column, ip });
            // Also send current state
            if let Some(snapshot) = session.get_snapshot() {
                messages.push(ServerMessage::VmStateUpdate {
                    state: snapshot_to_vm_state(&snapshot),
                });
            }
        }
        DebugEvent::StepCompleted { line, column, ip } => {
            messages.push(ServerMessage::StepCompleted { line, column, ip });
            // Also send current state
            if let Some(snapshot) = session.get_snapshot() {
                messages.push(ServerMessage::VmStateUpdate {
                    state: snapshot_to_vm_state(&snapshot),
                });
            }
        }
        DebugEvent::Paused { line, column, ip } => {
            messages.push(ServerMessage::Paused { line, column, ip });
            if let Some(snapshot) = session.get_snapshot() {
                messages.push(ServerMessage::VmStateUpdate {
                    state: snapshot_to_vm_state(&snapshot),
                });
            }
        }
        DebugEvent::Finished { exit_code } => {
            messages.push(ServerMessage::Finished {
                exit_code,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        DebugEvent::Error { message, location } => {
            messages.push(ServerMessage::RuntimeError {
                message,
                location: location.map(|(line, column)| crate::protocol::SourceLocation {
                    line,
                    column,
                    file: None,
                }),
            });
        }
    }

    messages
}

/// Convert DebugSnapshot to VmState for protocol
fn snapshot_to_vm_state(snapshot: &DebugSnapshot) -> VmState {
    let (line, column) = snapshot.location.unzip();
    VmState {
        execution_state: format!("{:?}", snapshot.state),
        ip: snapshot.ip,
        line,
        column,
        stack: snapshot.stack.iter().map(|v| v.display.clone()).collect(),
        locals: snapshot
            .locals
            .iter()
            .map(|l| VariableInfo {
                name: l.name.clone().unwrap_or_else(|| format!("local_{}", l.slot)),
                value: l.value.display.clone(),
                type_name: l.value.type_name.clone(),
            })
            .collect(),
        call_stack: snapshot
            .call_stack
            .iter()
            .map(|f| {
                format!(
                    "{}@{}",
                    f.function_name.as_deref().unwrap_or("<anonymous>"),
                    f.return_addr
                )
            })
            .collect(),
        output: session_output_placeholder(),
    }
}

/// Placeholder for output - actual output is sent separately
fn session_output_placeholder() -> Vec<String> {
    vec![]
}

/// Parse error string into CompileError list
fn parse_error(error: &str) -> Vec<CompileError> {
    vec![CompileError {
        message: error.to_string(),
        line: None,
        column: None,
        severity: ErrorSeverity::Error,
    }]
}

//! WebSocket Session Management
//!
//! Handles stateful WebSocket sessions for real-time code execution.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::handlers::compile_source;
use crate::protocol::{
    ChannelInfo, ClientMessage, CompileError, ErrorSeverity, FiberInfo, ServerMessage,
};

/// A WebSocket session for executing Lira code
pub struct Session {
    /// Whether execution is currently running
    running: bool,
    /// Start time of current execution
    start_time: Option<Instant>,
    /// Stop flag for cooperative VM stopping (shared with VM)
    stop_flag: Arc<AtomicBool>,
    /// Breakpoint line numbers (1-based)
    breakpoints: Vec<u32>,
}

impl Session {
    /// Create a new session
    pub fn new() -> Self {
        Self {
            running: false,
            start_time: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            breakpoints: Vec::new(),
        }
    }

    /// Handle a client message and return responses
    pub fn handle_message(&mut self, msg: ClientMessage) -> Vec<ServerMessage> {
        match msg {
            ClientMessage::Ping => vec![ServerMessage::Pong],

            ClientMessage::Check { source } => self.handle_check(&source),

            ClientMessage::GetAst { source } => self.handle_get_ast(&source),

            ClientMessage::Run { source } => self.handle_run(&source),

            ClientMessage::Debug { source } => {
                // For now, debug mode is same as run
                self.handle_run(&source)
            }

            ClientMessage::Stop => {
                // Set stop flag so VM will stop on next instruction check
                self.stop_flag.store(true, Ordering::Relaxed);
                self.running = false;
                vec![ServerMessage::Stopped]
            }

            ClientMessage::Pause => {
                // Not implemented yet
                vec![]
            }

            ClientMessage::Continue
            | ClientMessage::StepInstruction
            | ClientMessage::StepLine
            | ClientMessage::StepInto
            | ClientMessage::StepOut => {
                // Debug stepping not implemented yet
                vec![]
            }

            ClientMessage::SetBreakpoints { breakpoints } => {
                self.breakpoints = breakpoints;
                vec![] // No response needed, just store them
            }

            ClientMessage::InspectVariable { .. }
            | ClientMessage::InspectLocals
            | ClientMessage::InspectStack => {
                // Inspection not implemented yet
                vec![]
            }
        }
    }

    /// Stop the session
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Handle check (type check only)
    fn handle_check(&self, source: &str) -> Vec<ServerMessage> {
        match lirac::check(source) {
            Ok(()) => vec![ServerMessage::CompileSuccess {
                ast: None,
                bytecode_size: 0,
            }],
            Err(e) => vec![ServerMessage::CompileError {
                errors: parse_error(&e),
            }],
        }
    }

    /// Handle get AST request
    fn handle_get_ast(&self, source: &str) -> Vec<ServerMessage> {
        match compile_source(source) {
            Ok((ast, bytecode)) => {
                let ast_json = serde_json::to_value(&ast).ok();
                vec![
                    ServerMessage::CompileSuccess {
                        ast: ast_json.clone(),
                        bytecode_size: bytecode.len(),
                    },
                    ServerMessage::Ast {
                        ast: ast_json.unwrap_or(serde_json::Value::Null),
                    },
                ]
            }
            Err(errors) => vec![ServerMessage::CompileError { errors }],
        }
    }

    /// Handle run request
    fn handle_run(&mut self, source: &str) -> Vec<ServerMessage> {
        self.running = true;
        self.start_time = Some(Instant::now());
        // Reset stop flag for new execution
        self.stop_flag.store(false, Ordering::Relaxed);
        let mut responses = Vec::new();

        // Compile
        let bytecode = match compile_source(source) {
            Ok((ast, bytecode)) => {
                let ast_json = serde_json::to_value(&ast).ok();
                responses.push(ServerMessage::CompileSuccess {
                    ast: ast_json,
                    bytecode_size: bytecode.len(),
                });
                bytecode
            }
            Err(errors) => {
                self.running = false;
                return vec![ServerMessage::CompileError { errors }];
            }
        };

        // Create VM and execute
        match liravm::create_vm(&bytecode) {
            Ok(mut vm) => {
                vm.set_capture_output(true);

                // Set up cooperative stop check
                let stop_flag = self.stop_flag.clone();
                vm.set_stop_check(move || stop_flag.load(Ordering::Relaxed));

                // Set breakpoints if any are configured
                if !self.breakpoints.is_empty() {
                    vm.set_breakpoints(self.breakpoints.clone());
                }

                match vm.run() {
                    Ok(exit_code) => {
                        // Send each output line
                        for line in vm.get_output() {
                            responses.push(ServerMessage::Output {
                                text: line.clone(),
                            });
                        }

                        // Get VM snapshot and send fiber/channel info
                        let snapshot = vm.get_snapshot();

                        // Send fiber info if any spawned
                        for fiber in &snapshot.fibers {
                            responses.push(ServerMessage::FiberSpawned {
                                fiber: FiberInfo {
                                    id: fiber.id,
                                    state: fiber.state.clone(),
                                    ip: fiber.ip,
                                },
                            });
                        }

                        // Send channel info if any created
                        for channel in &snapshot.channels {
                            responses.push(ServerMessage::ChannelCreated {
                                channel: ChannelInfo {
                                    id: channel.id,
                                    capacity: channel.capacity,
                                    buffer_size: channel.buffer_size,
                                    closed: channel.closed,
                                },
                            });
                        }

                        // Send finished
                        let duration_ms = self
                            .start_time
                            .map(|t| t.elapsed().as_millis() as u64)
                            .unwrap_or(0);

                        responses.push(ServerMessage::Finished {
                            exit_code,
                            duration_ms,
                        });
                    }
                    Err(e) => {
                        // Check if this is a breakpoint hit
                        if e.starts_with("Breakpoint hit at line ") {
                            // Parse: "Breakpoint hit at line X, column Y (ip=Z)"
                            if let Some((line, column, ip)) = parse_breakpoint_message(&e) {
                                // Send output captured so far
                                for line_text in vm.get_output() {
                                    responses.push(ServerMessage::Output {
                                        text: line_text.clone(),
                                    });
                                }
                                responses.push(ServerMessage::BreakpointHit { line, column, ip });
                            } else {
                                responses.push(ServerMessage::RuntimeError {
                                    message: e,
                                    location: None,
                                });
                            }
                        } else if e == "Execution stopped by user" {
                            // User-initiated stop - already handled
                            responses.push(ServerMessage::Stopped);
                        } else {
                            responses.push(ServerMessage::RuntimeError {
                                message: e,
                                location: None,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                responses.push(ServerMessage::RuntimeError {
                    message: e,
                    location: None,
                });
            }
        }

        self.running = false;
        responses
    }
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

/// Parse breakpoint message: "Breakpoint hit at line X, column Y (ip=Z)"
fn parse_breakpoint_message(msg: &str) -> Option<(u32, u32, usize)> {
    // Format: "Breakpoint hit at line X, column Y (ip=Z)"
    let msg = msg.strip_prefix("Breakpoint hit at line ")?;

    // Find "column " and parse line number before it
    let comma_idx = msg.find(", column ")?;
    let line: u32 = msg[..comma_idx].parse().ok()?;

    let rest = &msg[comma_idx + ", column ".len()..];

    // Find "(ip=" and parse column before it
    let paren_idx = rest.find(" (ip=")?;
    let column: u32 = rest[..paren_idx].parse().ok()?;

    // Parse ip
    let ip_start = paren_idx + " (ip=".len();
    let ip_end = rest.find(')')?;
    let ip: usize = rest[ip_start..ip_end].parse().ok()?;

    Some((line, column, ip))
}

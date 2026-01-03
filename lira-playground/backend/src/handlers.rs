//! HTTP and WebSocket Handlers
//!
//! Handles compilation, execution, and WebSocket connections.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::{IntoResponse, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::protocol::{ClientMessage, CompileError, ErrorSeverity};
use crate::session::Session;

/// Health check endpoint
pub async fn health() -> &'static str {
    "OK"
}

/// Request body for compile/check endpoints
#[derive(Debug, Deserialize)]
pub struct SourceRequest {
    pub source: String,
}

/// Request body for run endpoint (with optional breakpoints)
#[derive(Debug, Deserialize)]
pub struct RunRequest {
    pub source: String,
    #[serde(default)]
    pub breakpoints: Vec<u32>,
}

/// Request body for step endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRequest {
    pub source: String,
    pub current_line: u32,
    pub step_type: StepType,
    #[serde(default)]
    pub breakpoints: Vec<u32>,
}

/// Step type for debugging
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepType {
    Into,     // Step to next line
    Over,     // Step over (same as into for now)
    Out,      // Step out of current context
    Continue, // Continue to next breakpoint
}

/// Response for compile endpoint
#[derive(Debug, Serialize)]
pub struct CompileResponse {
    pub success: bool,
    pub ast: Option<serde_json::Value>,
    pub errors: Vec<CompileError>,
    #[serde(rename = "bytecodeSize")]
    pub bytecode_size: usize,
    #[serde(rename = "compileTimeMs")]
    pub compile_time_ms: u64,
}

/// Response for run endpoint
#[derive(Debug, Serialize)]
pub struct RunResponse {
    pub success: bool,
    pub output: Vec<String>,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    pub errors: Vec<CompileError>,
    #[serde(rename = "executionTimeMs")]
    pub execution_time_ms: u64,
    /// If execution paused at a breakpoint, this contains the location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakpoint: Option<BreakpointInfo>,
}

/// Breakpoint hit information
#[derive(Debug, Serialize)]
pub struct BreakpointInfo {
    pub line: u32,
    pub column: u32,
    pub ip: usize,
}

/// Response for check endpoint
#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub success: bool,
    pub errors: Vec<CompileError>,
}

/// Compile endpoint - returns AST and any errors
pub async fn compile(Json(req): Json<SourceRequest>) -> impl IntoResponse {
    let start = Instant::now();

    // Parse and type check
    match compile_source(&req.source) {
        Ok((ast, bytecode)) => {
            let ast_json = match serde_json::to_value(&ast) {
                Ok(json) => Some(json),
                Err(e) => {
                    tracing::error!("AST serialization failed: {}", e);
                    None
                }
            };
            Json(CompileResponse {
                success: true,
                ast: ast_json,
                errors: vec![],
                bytecode_size: bytecode.len(),
                compile_time_ms: start.elapsed().as_millis() as u64,
            })
        }
        Err(errors) => Json(CompileResponse {
            success: false,
            ast: None,
            errors,
            bytecode_size: 0,
            compile_time_ms: start.elapsed().as_millis() as u64,
        }),
    }
}

/// Run endpoint - compiles and executes, returns output
pub async fn run(Json(req): Json<RunRequest>) -> impl IntoResponse {
    let start = Instant::now();

    // Compile
    let bytecode = match compile_source(&req.source) {
        Ok((_, bytecode)) => bytecode,
        Err(errors) => {
            return Json(RunResponse {
                success: false,
                output: vec![],
                exit_code: None,
                errors,
                execution_time_ms: start.elapsed().as_millis() as u64,
                breakpoint: None,
            });
        }
    };

    // Execute with optional breakpoints
    if req.breakpoints.is_empty() {
        // Fast path: no breakpoints, use simple run
        match liravm::run_with_capture(&bytecode) {
            Ok((exit_code, output)) => Json(RunResponse {
                success: true,
                output,
                exit_code: Some(exit_code),
                errors: vec![],
                execution_time_ms: start.elapsed().as_millis() as u64,
                breakpoint: None,
            }),
            Err(e) => Json(RunResponse {
                success: false,
                output: vec![],
                exit_code: None,
                errors: vec![CompileError {
                    message: e,
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                }],
                execution_time_ms: start.elapsed().as_millis() as u64,
                breakpoint: None,
            }),
        }
    } else {
        // With breakpoints: create VM manually and set them
        match liravm::create_vm(&bytecode) {
            Ok(mut vm) => {
                vm.set_capture_output(true);
                vm.set_breakpoints(req.breakpoints);

                match vm.run() {
                    Ok(exit_code) => Json(RunResponse {
                        success: true,
                        output: vm.get_output().to_vec(),
                        exit_code: Some(exit_code),
                        errors: vec![],
                        execution_time_ms: start.elapsed().as_millis() as u64,
                        breakpoint: None,
                    }),
                    Err(e) => {
                        // Check if this is a breakpoint hit
                        if let Some(bp) = parse_breakpoint_error(&e) {
                            // Breakpoint hit - return output so far and breakpoint info
                            Json(RunResponse {
                                success: true, // Not an error, just paused
                                output: vm.get_output().to_vec(),
                                exit_code: None,
                                errors: vec![],
                                execution_time_ms: start.elapsed().as_millis() as u64,
                                breakpoint: Some(bp),
                            })
                        } else {
                            // Actual runtime error
                            Json(RunResponse {
                                success: false,
                                output: vm.get_output().to_vec(),
                                exit_code: None,
                                errors: vec![CompileError {
                                    message: e,
                                    line: None,
                                    column: None,
                                    severity: ErrorSeverity::Error,
                                }],
                                execution_time_ms: start.elapsed().as_millis() as u64,
                                breakpoint: None,
                            })
                        }
                    }
                }
            }
            Err(e) => Json(RunResponse {
                success: false,
                output: vec![],
                exit_code: None,
                errors: vec![CompileError {
                    message: e,
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                }],
                execution_time_ms: start.elapsed().as_millis() as u64,
                breakpoint: None,
            }),
        }
    }
}

/// Parse breakpoint error message: "Breakpoint hit at line X, column Y (ip=Z)"
fn parse_breakpoint_error(msg: &str) -> Option<BreakpointInfo> {
    let msg = msg.strip_prefix("Breakpoint hit at line ")?;
    let comma_idx = msg.find(", column ")?;
    let line: u32 = msg[..comma_idx].parse().ok()?;
    let rest = &msg[comma_idx + ", column ".len()..];
    let paren_idx = rest.find(" (ip=")?;
    let column: u32 = rest[..paren_idx].parse().ok()?;
    let ip_start = paren_idx + " (ip=".len();
    let ip_end = rest.find(')')?;
    let ip: usize = rest[ip_start..ip_end].parse().ok()?;
    Some(BreakpointInfo { line, column, ip })
}

/// Check endpoint - type checks without generating bytecode
pub async fn check(Json(req): Json<SourceRequest>) -> impl IntoResponse {
    match lirac::check(&req.source) {
        Ok(()) => Json(CheckResponse {
            success: true,
            errors: vec![],
        }),
        Err(e) => Json(CheckResponse {
            success: false,
            errors: parse_error_message(&e),
        }),
    }
}

/// Step endpoint - re-run with temporary breakpoint for stepping
pub async fn step(Json(req): Json<StepRequest>) -> impl IntoResponse {
    let start = Instant::now();

    // Compile
    let bytecode = match compile_source(&req.source) {
        Ok((_, bytecode)) => bytecode,
        Err(errors) => {
            return Json(RunResponse {
                success: false,
                output: vec![],
                exit_code: None,
                errors,
                execution_time_ms: start.elapsed().as_millis() as u64,
                breakpoint: None,
            });
        }
    };

    // Calculate effective breakpoints based on step type
    // Key insight: since we re-run from scratch, we must only set breakpoints
    // at lines AFTER the current line to avoid stopping at the same place again.
    let effective_breakpoints: Vec<u32> = match req.step_type {
        StepType::Into | StepType::Over => {
            // Step to next executable line after current
            // Add temporary breakpoints at next few lines (in case some are empty/comments)
            let mut bps: Vec<u32> = vec![
                req.current_line + 1,
                req.current_line + 2,
                req.current_line + 3,
            ];
            // Also include user breakpoints that are after current line
            for &bp in &req.breakpoints {
                if bp > req.current_line && !bps.contains(&bp) {
                    bps.push(bp);
                }
            }
            bps
        }
        StepType::Out => {
            // Skip all breakpoints at or before current line
            // This effectively runs to the next user breakpoint after current position
            req.breakpoints
                .iter()
                .filter(|&&l| l > req.current_line)
                .copied()
                .collect()
        }
        StepType::Continue => {
            // Run to next user breakpoint after current line
            req.breakpoints
                .iter()
                .filter(|&&l| l > req.current_line)
                .copied()
                .collect()
        }
    };

    // Run with calculated breakpoints
    match liravm::create_vm(&bytecode) {
        Ok(mut vm) => {
            vm.set_capture_output(true);
            vm.set_breakpoints(effective_breakpoints);

            match vm.run() {
                Ok(exit_code) => Json(RunResponse {
                    success: true,
                    output: vm.get_output().to_vec(),
                    exit_code: Some(exit_code),
                    errors: vec![],
                    execution_time_ms: start.elapsed().as_millis() as u64,
                    breakpoint: None,
                }),
                Err(e) => {
                    if let Some(bp) = parse_breakpoint_error(&e) {
                        Json(RunResponse {
                            success: true,
                            output: vm.get_output().to_vec(),
                            exit_code: None,
                            errors: vec![],
                            execution_time_ms: start.elapsed().as_millis() as u64,
                            breakpoint: Some(bp),
                        })
                    } else {
                        Json(RunResponse {
                            success: false,
                            output: vm.get_output().to_vec(),
                            exit_code: None,
                            errors: vec![CompileError {
                                message: e,
                                line: None,
                                column: None,
                                severity: ErrorSeverity::Error,
                            }],
                            execution_time_ms: start.elapsed().as_millis() as u64,
                            breakpoint: None,
                        })
                    }
                }
            }
        }
        Err(e) => Json(RunResponse {
            success: false,
            output: vec![],
            exit_code: None,
            errors: vec![CompileError {
                message: e,
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
            }],
            execution_time_ms: start.elapsed().as_millis() as u64,
            breakpoint: None,
        }),
    }
}

/// WebSocket upgrade handler
pub async fn websocket(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_websocket)
}

/// Handle WebSocket connection
async fn handle_websocket(socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();

    // Create session
    let mut session = Session::new();

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        };

        // Parse client message
        let client_msg: ClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Invalid message: {}", e);
                continue;
            }
        };

        // Handle message
        let responses = session.handle_message(client_msg);

        // Send responses
        for response in responses {
            let json = serde_json::to_string(&response).unwrap();
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    }

    // Clean up session
    session.stop();
}

/// Compile source code and return AST + bytecode
pub fn compile_source(source: &str) -> Result<(lirac::ast::Program, Vec<u8>), Vec<CompileError>> {
    // Tokenize
    let tokens = lirac::lexer::tokenize(source).map_err(|e| parse_error_message(&e))?;

    // Parse
    let ast = lirac::parser::parse(&tokens).map_err(|e| parse_error_message(&e))?;

    // Type check
    let typed_ast = lirac::checker::check(&ast).map_err(|e| parse_error_message(&e))?;

    // Generate bytecode
    let bytecode = lirac::codegen::generate(&typed_ast).map_err(|e| parse_error_message(&e))?;

    Ok((ast, bytecode))
}

/// Parse error message into structured errors
fn parse_error_message(error: &str) -> Vec<CompileError> {
    // Try to parse line:column from error message
    // Format: "Error at line X, column Y: message" or just plain message
    let mut errors = Vec::new();

    for line in error.lines() {
        let (line_num, col_num, message) = if let Some(rest) = line.strip_prefix("Error at line ") {
            // Parse "Error at line X, column Y: message"
            if let Some((loc, msg)) = rest.split_once(": ") {
                let parts: Vec<&str> = loc.split(", column ").collect();
                let line_num = parts.first().and_then(|s| s.parse().ok());
                let col_num = parts.get(1).and_then(|s| s.parse().ok());
                (line_num, col_num, msg.to_string())
            } else {
                (None, None, line.to_string())
            }
        } else if let Some((loc, msg)) = line.split_once(": ") {
            // Try "line:column: message" format
            let parts: Vec<&str> = loc.split(':').collect();
            if parts.len() >= 2 {
                let line_num = parts.first().and_then(|s| s.parse().ok());
                let col_num = parts.get(1).and_then(|s| s.parse().ok());
                (line_num, col_num, msg.to_string())
            } else {
                (None, None, line.to_string())
            }
        } else {
            (None, None, line.to_string())
        };

        if !message.is_empty() {
            errors.push(CompileError {
                message,
                line: line_num,
                column: col_num,
                severity: ErrorSeverity::Error,
            });
        }
    }

    if errors.is_empty() {
        errors.push(CompileError {
            message: error.to_string(),
            line: None,
            column: None,
            severity: ErrorSeverity::Error,
        });
    }

    errors
}

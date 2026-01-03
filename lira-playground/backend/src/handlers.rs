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
                    }),
                    Err(e) => Json(RunResponse {
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
                    }),
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
            }),
        }
    }
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

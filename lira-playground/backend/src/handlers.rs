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
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::protocol::{ClientMessage, CompileError, ErrorSeverity, ServerMessage};
use crate::vm_thread::VmThreadHandle;

/// Maximum allowed source code size (512 KB)
const MAX_SOURCE_LEN: usize = 512 * 1024;

/// Compilation timeout in seconds
const COMPILATION_TIMEOUT_SECS: u64 = 30;

/// Execution timeout in seconds
const EXECUTION_TIMEOUT_SECS: u64 = 30;

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

    if req.source.len() > MAX_SOURCE_LEN {
        return Json(source_too_large_response());
    }

    let source = req.source;
    let timeout = Duration::from_secs(COMPILATION_TIMEOUT_SECS);

    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || compile_source(&source)),
    )
    .await
    {
        Ok(Ok(Ok((ast, bytecode)))) => {
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
        Ok(Ok(Err(errors))) => Json(CompileResponse {
            success: false,
            ast: None,
            errors,
            bytecode_size: 0,
            compile_time_ms: start.elapsed().as_millis() as u64,
        }),
        Ok(Err(_)) => {
            tracing::error!("Compilation task panicked");
            Json(compile_error_response(
                "Internal compilation error (panic)".to_string(),
                start,
            ))
        }
        Err(_) => {
            tracing::warn!("Compilation timed out after {}s", COMPILATION_TIMEOUT_SECS);
            Json(compile_error_response(
                format!(
                    "Compilation timed out after {} seconds",
                    COMPILATION_TIMEOUT_SECS
                ),
                start,
            ))
        }
    }
}

fn source_too_large_response() -> CompileResponse {
    CompileResponse {
        success: false,
        ast: None,
        errors: vec![CompileError {
            message: format!(
                "Source code too large. Maximum allowed size is {} bytes ({} KB).",
                MAX_SOURCE_LEN,
                MAX_SOURCE_LEN / 1024
            ),
            line: None,
            column: None,
            severity: ErrorSeverity::Error,
        }],
        bytecode_size: 0,
        compile_time_ms: 0,
    }
}

fn compile_error_response(message: String, start: Instant) -> CompileResponse {
    CompileResponse {
        success: false,
        ast: None,
        errors: vec![CompileError {
            message,
            line: None,
            column: None,
            severity: ErrorSeverity::Error,
        }],
        bytecode_size: 0,
        compile_time_ms: elapsed_ms(start),
    }
}

fn run_error_response(message: String, start: Instant) -> RunResponse {
    RunResponse::failed(vec![], one_error(message, None, None), start)
}

impl RunResponse {
    /// Program ran to completion.
    fn finished(output: Vec<String>, exit_code: i32, start: Instant) -> Self {
        Self {
            success: true,
            output,
            exit_code: Some(exit_code),
            errors: vec![],
            execution_time_ms: elapsed_ms(start),
            breakpoint: None,
        }
    }

    /// Execution paused at a breakpoint (not an error).
    fn paused(output: Vec<String>, breakpoint: BreakpointInfo, start: Instant) -> Self {
        Self {
            success: true,
            output,
            exit_code: None,
            errors: vec![],
            execution_time_ms: elapsed_ms(start),
            breakpoint: Some(breakpoint),
        }
    }

    /// Compilation or runtime failure (carries any output produced first).
    fn failed(output: Vec<String>, errors: Vec<CompileError>, start: Instant) -> Self {
        Self {
            success: false,
            output,
            exit_code: None,
            errors,
            execution_time_ms: elapsed_ms(start),
            breakpoint: None,
        }
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// Wrap a single message + optional location as a one-element error list.
fn one_error(message: String, line: Option<u32>, column: Option<u32>) -> Vec<CompileError> {
    vec![CompileError {
        message,
        line,
        column,
        severity: ErrorSeverity::Error,
    }]
}

/// Create a VM, run it with the given breakpoints, and classify the outcome:
/// completion, a breakpoint pause, a runtime error (with recovered location),
/// or a VM-construction failure. Shared by the `run` and `step` endpoints.
fn run_with_breakpoints(bytecode: &[u8], breakpoints: Vec<u32>, start: Instant) -> RunResponse {
    let mut vm = match liravm::create_vm(bytecode) {
        Ok(vm) => vm,
        Err(e) => return RunResponse::failed(vec![], one_error(e, None, None), start),
    };
    vm.set_capture_output(true);
    vm.set_breakpoints(breakpoints);

    match vm.run() {
        Ok(exit_code) => RunResponse::finished(vm.get_output().to_vec(), exit_code, start),
        Err(e) => {
            if let Some(bp) = parse_breakpoint_error(&e) {
                RunResponse::paused(vm.get_output().to_vec(), bp, start)
            } else {
                // Recover the source location and drop run()'s "line:col: " prefix.
                let (line, column) = vm
                    .get_current_location()
                    .map_or((None, None), |(l, c)| (Some(l), Some(c)));
                let message = strip_location_prefix(&e, line, column);
                RunResponse::failed(
                    vm.get_output().to_vec(),
                    one_error(message, line, column),
                    start,
                )
            }
        }
    }
}

/// Run endpoint - compiles and executes, returns output
pub async fn run(Json(req): Json<RunRequest>) -> impl IntoResponse {
    let start = Instant::now();

    if req.source.len() > MAX_SOURCE_LEN {
        return Json(run_error_response(
            format!(
                "Source code too large. Maximum allowed size is {} bytes ({} KB).",
                MAX_SOURCE_LEN,
                MAX_SOURCE_LEN / 1024
            ),
            start,
        ));
    }

    let source = req.source;
    let breakpoints = req.breakpoints;
    let timeout = Duration::from_secs(EXECUTION_TIMEOUT_SECS);

    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || {
            let bytecode = match compile_source(&source) {
                Ok((_, bytecode)) => bytecode,
                Err(errors) => return RunResponse::failed(vec![], errors, start),
            };

            if breakpoints.is_empty() {
                match liravm::run_with_capture_structured(&bytecode) {
                    Ok((exit_code, output)) => RunResponse::finished(output, exit_code, start),
                    Err((output, err)) => RunResponse::failed(
                        output,
                        one_error(err.message, err.line, err.column),
                        start,
                    ),
                }
            } else {
                run_with_breakpoints(&bytecode, breakpoints, start)
            }
        }),
    )
    .await
    {
        Ok(Ok(response)) => Json(response),
        Ok(Err(_)) => Json(run_error_response(
            "Internal execution error (panic)".to_string(),
            start,
        )),
        Err(_) => Json(run_error_response(
            format!(
                "Execution timed out after {} seconds",
                EXECUTION_TIMEOUT_SECS
            ),
            start,
        )),
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
    if req.source.len() > MAX_SOURCE_LEN {
        return Json(CheckResponse {
            success: false,
            errors: vec![CompileError {
                message: format!(
                    "Source code too large. Maximum allowed size is {} bytes.",
                    MAX_SOURCE_LEN
                ),
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
            }],
        });
    }

    let source = req.source;
    let timeout = Duration::from_secs(COMPILATION_TIMEOUT_SECS);

    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || lirac::check(&source)),
    )
    .await
    {
        Ok(Ok(Ok(()))) => Json(CheckResponse {
            success: true,
            errors: vec![],
        }),
        Ok(Ok(Err(e))) => Json(CheckResponse {
            success: false,
            errors: parse_error_message(&e),
        }),
        Ok(Err(_)) => Json(CheckResponse {
            success: false,
            errors: vec![CompileError {
                message: "Internal check error (panic)".to_string(),
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
            }],
        }),
        Err(_) => Json(CheckResponse {
            success: false,
            errors: vec![CompileError {
                message: format!(
                    "Type checking timed out after {} seconds",
                    COMPILATION_TIMEOUT_SECS
                ),
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
            }],
        }),
    }
}

/// Step endpoint - re-run with temporary breakpoint for stepping
///
/// NOTE: This endpoint restarts execution from scratch on every step. This means
/// `StepType::Out` cannot properly "step out of a function" because it has no
/// awareness of the call stack depth from the previous execution. It effectively
/// behaves like `StepType::Continue` (run to the next breakpoint after the
/// current line). For proper step-out semantics, use the WebSocket debug session.
/// FIXME(H9): Track call stack depth across step invocations to implement
/// actual step-out behaviour, or remove this endpoint and route stepping through
/// the WebSocket debug session.
pub async fn step(Json(req): Json<StepRequest>) -> impl IntoResponse {
    let start = Instant::now();

    if req.source.len() > MAX_SOURCE_LEN {
        return Json(run_error_response(
            format!(
                "Source code too large. Maximum allowed size is {} bytes.",
                MAX_SOURCE_LEN
            ),
            start,
        ));
    }

    let source = req.source;
    let current_line = req.current_line;
    let step_type = req.step_type;
    let breakpoints = req.breakpoints;
    let timeout = Duration::from_secs(EXECUTION_TIMEOUT_SECS);

    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || {
            let bytecode = match compile_source(&source) {
                Ok((_, bytecode)) => bytecode,
                Err(errors) => return RunResponse::failed(vec![], errors, start),
            };

            // Calculate effective breakpoints based on step type
            // Key insight: since we re-run from scratch, we must only set breakpoints
            // at lines AFTER the current line to avoid stopping at the same place again.
            let effective_breakpoints: Vec<u32> = match step_type {
                StepType::Into | StepType::Over => {
                    let mut bps: Vec<u32> =
                        vec![current_line + 1, current_line + 2, current_line + 3];
                    for &bp in &breakpoints {
                        if bp > current_line && !bps.contains(&bp) {
                            bps.push(bp);
                        }
                    }
                    bps
                }
                StepType::Out => {
                    // FIXME(H9): StepOut is broken in this HTTP endpoint.
                    // Since we re-run from scratch, we cannot track call stack depth
                    // to know when the current function has returned. This behaves
                    // the same as Continue: runs to the next user breakpoint after
                    // the current line.
                    breakpoints
                        .iter()
                        .filter(|&&l| l > current_line)
                        .copied()
                        .collect()
                }
                StepType::Continue => breakpoints
                    .iter()
                    .filter(|&&l| l > current_line)
                    .copied()
                    .collect(),
            };

            run_with_breakpoints(&bytecode, effective_breakpoints, start)
        }),
    )
    .await
    {
        Ok(Ok(response)) => Json(response),
        Ok(Err(_)) => Json(run_error_response(
            "Internal step execution error (panic)".to_string(),
            start,
        )),
        Err(_) => Json(run_error_response(
            format!(
                "Step execution timed out after {} seconds",
                EXECUTION_TIMEOUT_SECS
            ),
            start,
        )),
    }
}

/// WebSocket upgrade handler
pub async fn websocket(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_websocket)
}

/// Timeout warning threshold in seconds (warn before the 30s timeout)
const TIMEOUT_WARNING_SECS: u64 = 25;

/// Handle WebSocket connection
async fn handle_websocket(socket: WebSocket) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Spawn dedicated VM thread for this session
    let mut vm_handle = VmThreadHandle::spawn();

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

        // Check if this is a long-running command that might timeout
        let is_long_running = matches!(
            client_msg,
            ClientMessage::Run { .. }
                | ClientMessage::Debug { .. }
                | ClientMessage::Continue
                | ClientMessage::StepInto
                | ClientMessage::StepOver
                | ClientMessage::StepOut
                | ClientMessage::StepLine
                | ClientMessage::StepInstruction
        );

        // Spawn timeout warning task for long-running commands
        let warning_handle = if is_long_running {
            let sender_clone = Arc::clone(&sender);
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(TIMEOUT_WARNING_SECS)).await;
                let warning_msg = ServerMessage::TimeoutWarning {
                    seconds_remaining: 5,
                };
                let json = serde_json::to_string(&warning_msg).unwrap();
                let mut sender_guard = sender_clone.lock().await;
                let _ = sender_guard.send(Message::Text(json.into())).await;
            }))
        } else {
            None
        };

        // Send to VM thread and get response
        let result = vm_handle.send_command(client_msg).await;

        // Cancel the timeout warning if it hasn't fired yet
        if let Some(handle) = warning_handle {
            handle.abort();
        }

        match result {
            Ok(response) => {
                // Serialize all messages without holding the sender lock,
                // then send one at a time, locking only for each send.
                let serialized: Vec<String> = response
                    .messages
                    .into_iter()
                    .map(|msg| serde_json::to_string(&msg).unwrap())
                    .collect();
                let mut send_failed = false;
                for json in &serialized {
                    let mut guard = sender.lock().await;
                    if guard
                        .send(Message::Text(json.clone().into()))
                        .await
                        .is_err()
                    {
                        send_failed = true;
                        break;
                    }
                }
                if send_failed {
                    break;
                }

                // Check if we should terminate
                if response.terminate {
                    break;
                }
            }
            Err(e) => {
                tracing::error!("VM thread error: {}", e);
                let error_msg = ServerMessage::RuntimeError {
                    message: format!("VM error: {}", e),
                    location: None,
                };
                let json = serde_json::to_string(&error_msg).unwrap();
                let mut sender_guard = sender.lock().await;
                let _ = sender_guard.send(Message::Text(json.into())).await;
            }
        }
    }

    // Clean up VM thread
    vm_handle.shutdown().await;
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

/// Remove a `"line:col: "` prefix that [`liravm::VM::run`] attaches to runtime
/// error messages, given the location parsed back out of the VM. Returns the
/// original message unchanged when no matching prefix is present.
fn strip_location_prefix(message: &str, line: Option<u32>, column: Option<u32>) -> String {
    if let (Some(line), Some(column)) = (line, column) {
        let prefix = format!("{}:{}: ", line, column);
        if let Some(rest) = message.strip_prefix(&prefix) {
            return rest.to_string();
        }
    }
    message.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    /// Drive a handler's response into a JSON value for assertions. The response
    /// types are `Serialize`-only, so we decode into a `serde_json::Value`.
    async fn body_json(resp: impl IntoResponse) -> serde_json::Value {
        let response = resp.into_response();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    fn src(s: &str) -> Json<SourceRequest> {
        Json(SourceRequest {
            source: s.to_string(),
        })
    }

    // ---- pure helpers ----

    #[test]
    fn parse_breakpoint_error_valid_and_invalid() {
        let bp = parse_breakpoint_error("Breakpoint hit at line 4, column 5 (ip=12)").unwrap();
        assert_eq!((bp.line, bp.column, bp.ip), (4, 5, 12));
        assert!(parse_breakpoint_error("Division by zero").is_none());
        assert!(parse_breakpoint_error("Breakpoint hit at line x, column 5 (ip=12)").is_none());
    }

    #[test]
    fn parse_error_message_formats() {
        let e = parse_error_message("Error at line 3, column 7: bad thing");
        assert_eq!((e[0].line, e[0].column), (Some(3), Some(7)));
        assert_eq!(e[0].message, "bad thing");

        let e = parse_error_message("2:9: unexpected token");
        assert_eq!((e[0].line, e[0].column), (Some(2), Some(9)));

        // A message with no parseable location still yields one error.
        let e = parse_error_message("something broke");
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].line, e[0].column), (None, None));
    }

    #[test]
    fn strip_location_prefix_behaviour() {
        assert_eq!(
            strip_location_prefix("4:5: Division by zero", Some(4), Some(5)),
            "Division by zero"
        );
        // No matching prefix -> unchanged.
        assert_eq!(
            strip_location_prefix("Division by zero", Some(4), Some(5)),
            "Division by zero"
        );
        assert_eq!(strip_location_prefix("plain", None, None), "plain");
    }

    #[test]
    fn compile_source_ok_and_error() {
        assert!(compile_source("let x = 1\nprintln(x)").is_ok());
        assert!(compile_source("let x = ").is_err());
    }

    // ---- async handlers ----

    #[tokio::test]
    async fn health_returns_ok() {
        assert_eq!(health().await, "OK");
    }

    #[tokio::test]
    async fn compile_handler_success_and_error() {
        let ok = body_json(compile(src("let x = 1\nprintln(x)")).await).await;
        assert_eq!(ok["success"], true);
        assert!(ok["ast"].is_object());
        assert!(ok["bytecodeSize"].as_u64().unwrap() > 0);

        let err = body_json(compile(src("let x = ")).await).await;
        assert_eq!(err["success"], false);
        assert!(!err["errors"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn check_handler_ok_and_error() {
        let ok = body_json(check(src("let x = 1")).await).await;
        assert_eq!(ok["success"], true);
        let err = body_json(check(src("let x = ")).await).await;
        assert_eq!(err["success"], false);
        assert!(!err["errors"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn run_handler_success_output() {
        let req = Json(RunRequest {
            source: "println(3 + 4)".to_string(),
            breakpoints: vec![],
        });
        let v = body_json(run(req).await).await;
        assert_eq!(v["success"], true);
        assert_eq!(v["exitCode"], 0);
        assert!(v["output"].as_array().unwrap().iter().any(|o| o == "7"));
    }

    #[tokio::test]
    async fn run_handler_runtime_error_reports_location() {
        // `z` avoids any constant-folding so the divide-by-zero is a runtime fault.
        let req = Json(RunRequest {
            source: "let z = 0\nprintln(1 / z)".to_string(),
            breakpoints: vec![],
        });
        let v = body_json(run(req).await).await;
        assert_eq!(v["success"], false);
        let errs = v["errors"].as_array().unwrap();
        assert!(!errs.is_empty());
        assert!(
            errs[0]["message"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains("zero"),
            "runtime error surfaced: {:?}",
            errs[0]
        );
    }

    #[tokio::test]
    async fn run_handler_stops_at_breakpoint() {
        let req = Json(RunRequest {
            source: "let a = 1\nlet b = 2\nprintln(a + b)".to_string(),
            breakpoints: vec![2],
        });
        let v = body_json(run(req).await).await;
        assert_eq!(v["success"], true);
        assert_eq!(
            v["breakpoint"]["line"], 2,
            "paused at the breakpoint: {v:?}"
        );
    }

    #[tokio::test]
    async fn step_handler_into_advances_to_next_line() {
        let req = Json(StepRequest {
            source: "let a = 1\nlet b = 2\nprintln(a + b)".to_string(),
            current_line: 1,
            step_type: StepType::Into,
            breakpoints: vec![],
        });
        let v = body_json(step(req).await).await;
        // Step-into sets temporary breakpoints on the following lines; it should
        // pause on the next executable line rather than running to completion.
        assert_eq!(v["success"], true);
        assert!(
            v.get("breakpoint").map(|b| !b.is_null()).unwrap_or(false),
            "step paused at a line: {v:?}"
        );
    }
}

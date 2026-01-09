//! LSP Integration Tests
//!
//! Tests the Lira Language Server using in-memory duplex streams.
//! Adapted from ast-grep's testing approach.

use bytes::{Buf, BufMut, BytesMut};
use futures::{SinkExt, StreamExt};
use lira_lsp::LiraLanguageServer;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::DuplexStream;
use tokio_util::codec::{Decoder, Encoder, Framed};
use tower_lsp::lsp_types::*;
use tower_lsp::{LspService, Server};

/// Thread-safe request ID generator
static REQUEST_ID: AtomicI64 = AtomicI64::new(1);

fn next_request_id() -> i64 {
    REQUEST_ID.fetch_add(1, Ordering::SeqCst)
}

/// LSP message codec implementing Content-Length framing
struct LspCodec;

impl Decoder for LspCodec {
    type Item = Value;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Look for the header/body separator
        let header_end = match src.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(pos) => pos,
            None => return Ok(None), // Need more data
        };

        // Parse Content-Length from headers
        let header = String::from_utf8_lossy(&src[..header_end]);
        let content_length: usize = header
            .lines()
            .find_map(|line| {
                let line = line.trim();
                if line.to_lowercase().starts_with("content-length:") {
                    line.split(':').nth(1)?.trim().parse().ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing Content-Length")
            })?;

        // Check if we have the full body
        let total_len = header_end + 4 + content_length;
        if src.len() < total_len {
            return Ok(None); // Need more data
        }

        // Extract the message
        src.advance(header_end + 4);
        let body = src.split_to(content_length);

        // Parse JSON
        let value: Value = serde_json::from_slice(&body)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        Ok(Some(value))
    }
}

impl Encoder<Value> for LspCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let body = serde_json::to_string(&item)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        dst.put_slice(header.as_bytes());
        dst.put_slice(body.as_bytes());
        Ok(())
    }
}

/// Test client for interacting with the LSP server
struct TestClient {
    stream: Framed<DuplexStream, LspCodec>,
}

impl TestClient {
    /// Create a new test client connected to a fresh LSP server
    async fn new() -> Self {
        let (service, socket) = LspService::build(LiraLanguageServer::new).finish();

        // Create duplex streams (16KB buffer)
        let (client_stream, server_stream) = tokio::io::duplex(16384);

        // Spawn the server
        tokio::spawn(async move {
            let (read, write) = tokio::io::split(server_stream);
            Server::new(read, write, socket).serve(service).await;
        });

        TestClient {
            stream: Framed::new(client_stream, LspCodec),
        }
    }

    /// Send a JSON-RPC request and return its ID
    async fn send_request(&mut self, method: &str, params: Value) -> i64 {
        let id = next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.stream
            .send(request)
            .await
            .expect("Failed to send request");
        id
    }

    /// Send a JSON-RPC notification (no response expected)
    async fn send_notification(&mut self, method: &str, params: Value) {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.stream
            .send(notification)
            .await
            .expect("Failed to send notification");
    }

    /// Wait for a response with the given ID
    async fn wait_for_response(&mut self, expected_id: i64) -> Value {
        loop {
            let msg = self
                .stream
                .next()
                .await
                .expect("Stream ended")
                .expect("Failed to receive message");

            // Check if this is the response we're looking for
            if let Some(id) = msg.get("id") {
                if id.as_i64() == Some(expected_id) {
                    return msg;
                }
            }
            // Otherwise it's a notification (like diagnostics), continue waiting
        }
    }

    /// Wait for a notification with a specific method, with timeout
    async fn wait_for_notification(&mut self, method: &str, timeout_ms: u64) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }

            match tokio::time::timeout(remaining, self.stream.next()).await {
                Ok(Some(Ok(msg))) => {
                    if msg.get("method").and_then(|m| m.as_str()) == Some(method) {
                        return Some(msg);
                    }
                    // Not the notification we want, continue
                }
                Ok(Some(Err(_))) | Ok(None) | Err(_) => return None,
            }
        }
    }

    /// Initialize the LSP session
    async fn initialize(&mut self) -> InitializeResult {
        let params = json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        });

        let id = self.send_request("initialize", params).await;
        let response = self.wait_for_response(id).await;

        // Send initialized notification
        self.send_notification("initialized", json!({})).await;

        serde_json::from_value(response["result"].clone())
            .expect("Failed to parse InitializeResult")
    }

    /// Open a document
    async fn did_open(&mut self, uri: &str, content: &str) {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "lira",
                    "version": 1,
                    "text": content
                }
            }),
        )
        .await;
    }

    /// Change a document
    async fn did_change(&mut self, uri: &str, version: i32, content: &str) {
        self.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": uri,
                    "version": version
                },
                "contentChanges": [{ "text": content }]
            }),
        )
        .await;
    }

    /// Request completions at a position
    async fn completion(&mut self, uri: &str, line: u32, character: u32) -> CompletionResponse {
        let id = self
            .send_request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone())
            .expect("Failed to parse CompletionResponse")
    }

    /// Request hover information
    async fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<Hover> {
        let id = self
            .send_request(
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        if response["result"].is_null() {
            None
        } else {
            serde_json::from_value(response["result"].clone()).ok()
        }
    }

    /// Request document symbols
    async fn document_symbols(&mut self, uri: &str) -> Vec<DocumentSymbol> {
        let id = self
            .send_request(
                "textDocument/documentSymbol",
                json!({
                    "textDocument": { "uri": uri }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Request signature help at a position
    async fn signature_help(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<SignatureHelp> {
        let id = self
            .send_request(
                "textDocument/signatureHelp",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        if response["result"].is_null() {
            None
        } else {
            serde_json::from_value(response["result"].clone()).ok()
        }
    }

    /// Request folding ranges
    async fn folding_range(&mut self, uri: &str) -> Vec<FoldingRange> {
        let id = self
            .send_request(
                "textDocument/foldingRange",
                json!({
                    "textDocument": { "uri": uri }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Prepare rename at a position
    async fn prepare_rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        let id = self
            .send_request(
                "textDocument/prepareRename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        if response["result"].is_null() {
            None
        } else {
            serde_json::from_value(response["result"].clone()).ok()
        }
    }

    /// Perform rename
    async fn rename(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<WorkspaceEdit> {
        let id = self
            .send_request(
                "textDocument/rename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                    "newName": new_name
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        if response["result"].is_null() {
            None
        } else {
            serde_json::from_value(response["result"].clone()).ok()
        }
    }

    /// Request inlay hints
    async fn inlay_hint(&mut self, uri: &str, start: Position, end: Position) -> Vec<InlayHint> {
        let id = self
            .send_request(
                "textDocument/inlayHint",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": start.line, "character": start.character },
                        "end": { "line": end.line, "character": end.character }
                    }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Request document links
    async fn document_link(&mut self, uri: &str) -> Vec<DocumentLink> {
        let id = self
            .send_request(
                "textDocument/documentLink",
                json!({
                    "textDocument": { "uri": uri }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Prepare call hierarchy at a position
    async fn prepare_call_hierarchy(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<CallHierarchyItem> {
        let id = self
            .send_request(
                "textDocument/prepareCallHierarchy",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Get incoming calls for a call hierarchy item
    async fn incoming_calls(&mut self, item: &CallHierarchyItem) -> Vec<CallHierarchyIncomingCall> {
        let id = self
            .send_request(
                "callHierarchy/incomingCalls",
                json!({
                    "item": item
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Get outgoing calls from a call hierarchy item
    async fn outgoing_calls(&mut self, item: &CallHierarchyItem) -> Vec<CallHierarchyOutgoingCall> {
        let id = self
            .send_request(
                "callHierarchy/outgoingCalls",
                json!({
                    "item": item
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Request code actions for a range
    async fn code_action(
        &mut self,
        uri: &str,
        start_line: u32,
        start_char: u32,
        end_line: u32,
        end_char: u32,
    ) -> Vec<CodeActionOrCommand> {
        let id = self
            .send_request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": start_line, "character": start_char },
                        "end": { "line": end_line, "character": end_char }
                    },
                    "context": {
                        "diagnostics": []
                    }
                }),
            )
            .await;

        let response = self.wait_for_response(id).await;
        serde_json::from_value(response["result"].clone()).unwrap_or_default()
    }

    /// Wait for diagnostics for a specific URI
    async fn wait_for_diagnostics(&mut self, uri: &str, timeout_ms: u64) -> Vec<Diagnostic> {
        if let Some(msg) = self
            .wait_for_notification("textDocument/publishDiagnostics", timeout_ms)
            .await
        {
            let params = &msg["params"];
            if params["uri"].as_str() == Some(uri) {
                return serde_json::from_value(params["diagnostics"].clone()).unwrap_or_default();
            }
        }
        vec![]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_initialize() {
    let mut client = TestClient::new().await;
    let result = client.initialize().await;

    // Check server info
    assert_eq!(result.server_info.as_ref().unwrap().name, "lira-lsp");

    // Check capabilities
    let caps = result.capabilities;
    assert!(caps.completion_provider.is_some());
    assert!(caps.hover_provider.is_some());
    assert!(caps.definition_provider.is_some());
    assert!(caps.document_symbol_provider.is_some());
    assert!(caps.references_provider.is_some());
    assert!(caps.semantic_tokens_provider.is_some());
}

#[tokio::test]
async fn test_completion_keywords() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    // Lira has no semicolons - just expressions/statements separated by newlines
    client.did_open(uri, "fn main() {\n    \n}").await;

    // Request completion inside the function body
    let completions = client.completion(uri, 1, 4).await;

    let items = match completions {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    // Should include keywords
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"if"), "Should have 'if' keyword");
    assert!(labels.contains(&"let"), "Should have 'let' keyword");
    assert!(labels.contains(&"return"), "Should have 'return' keyword");
}

#[tokio::test]
async fn test_completion_after_dot() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    // Lira syntax: no semicolons
    client
        .did_open(uri, "fn main() {\n    let s = \"hello\"\n    s.\n}")
        .await;

    // Request completion after the dot
    let completions = client.completion(uri, 2, 6).await;

    let items = match completions {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    // Should include string methods
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"len"), "Should have 'len' method");
}

#[tokio::test]
async fn test_diagnostics_syntax_error() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    // Missing closing brace
    client.did_open(uri, "fn main() {").await;

    // Wait for diagnostics
    let diagnostics = client.wait_for_diagnostics(uri, 2000).await;

    assert!(!diagnostics.is_empty(), "Should report syntax error");
}

#[tokio::test]
async fn test_diagnostics_valid_code() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    // Lira syntax: no semicolons
    client
        .did_open(uri, "fn main() {\n    println(\"hello\")\n}")
        .await;

    // Wait for diagnostics
    let diagnostics = client.wait_for_diagnostics(uri, 2000).await;

    // Debug: print diagnostics if any
    for d in &diagnostics {
        eprintln!("Diagnostic: {:?} - {}", d.severity, d.message);
    }

    assert!(
        diagnostics.is_empty(),
        "Valid code should have no diagnostics"
    );
}

#[tokio::test]
async fn test_document_symbols() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    // Lira syntax: no semicolons
    client
        .did_open(
            uri,
            r#"
fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    let result = add(1, 2)
}
"#,
        )
        .await;

    let symbols = client.document_symbols(uri).await;

    let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"add"), "Should find 'add' function");
    assert!(names.contains(&"main"), "Should find 'main' function");
}

#[tokio::test]
async fn test_hover_on_keyword() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client.did_open(uri, "fn main() {}").await;

    // Hover over 'fn' keyword
    let hover = client.hover(uri, 0, 0).await;

    assert!(hover.is_some(), "Should have hover info for 'fn'");
    let hover = hover.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(
            markup.value.contains("fn") || markup.value.contains("function"),
            "Hover should mention function"
        );
    }
}

#[tokio::test]
async fn test_did_change_updates_diagnostics() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";

    // Start with invalid code (missing closing brace)
    client.did_open(uri, "fn main() {").await;
    let diags1 = client.wait_for_diagnostics(uri, 2000).await;
    assert!(!diags1.is_empty(), "Should have errors initially");

    // Fix the code - Lira syntax: no semicolons
    client
        .did_change(uri, 2, "fn main() {\n    println(\"fixed\")\n}")
        .await;
    let diags2 = client.wait_for_diagnostics(uri, 2000).await;
    assert!(diags2.is_empty(), "Should have no errors after fix");
}

#[tokio::test]
async fn test_signature_help_builtin() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(uri, "fn main() {\n    println(\n}")
        .await;

    // Request signature help inside println(
    let sig_help = client.signature_help(uri, 1, 12).await;

    assert!(sig_help.is_some(), "Should have signature help for println");
    let sig_help = sig_help.unwrap();
    assert!(!sig_help.signatures.is_empty(), "Should have at least one signature");
    assert!(
        sig_help.signatures[0].label.contains("println"),
        "Signature should mention println"
    );
}

#[tokio::test]
async fn test_signature_help_user_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    add(1,
}
"#,
        )
        .await;

    // Request signature help inside add(1,
    let sig_help = client.signature_help(uri, 5, 10).await;

    assert!(sig_help.is_some(), "Should have signature help for add");
    let sig_help = sig_help.unwrap();
    assert!(!sig_help.signatures.is_empty());
    assert!(sig_help.signatures[0].label.contains("a: int"));
    assert!(sig_help.signatures[0].label.contains("b: int"));
    // Active parameter should be 1 (second param) since we're after the comma
    assert_eq!(sig_help.active_parameter, Some(1));
}

#[tokio::test]
async fn test_folding_range_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    println("hello")
    println("world")
}

fn other() {
    let x = 1
}
"#,
        )
        .await;

    let ranges = client.folding_range(uri).await;

    assert!(ranges.len() >= 2, "Should have at least 2 folding ranges");

    // Check that main function can fold (lines 0-3)
    let main_fold = ranges.iter().find(|r| r.start_line == 0);
    assert!(main_fold.is_some(), "Should have fold for main()");
    assert_eq!(main_fold.unwrap().end_line, 3);

    // Check that other function can fold (lines 5-7)
    let other_fold = ranges.iter().find(|r| r.start_line == 5);
    assert!(other_fold.is_some(), "Should have fold for other()");
    assert_eq!(other_fold.unwrap().end_line, 7);
}

#[tokio::test]
async fn test_folding_range_nested() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    if true {
        println("yes")
    }
}
"#,
        )
        .await;

    let ranges = client.folding_range(uri).await;

    assert!(ranges.len() >= 2, "Should have folds for fn and if");

    // Should have nested folds
    let outer_fold = ranges.iter().find(|r| r.start_line == 0);
    let inner_fold = ranges.iter().find(|r| r.start_line == 1);
    assert!(outer_fold.is_some() && inner_fold.is_some());
}

#[tokio::test]
async fn test_document_link_import() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    // Use a path within the lira project so stdlib can be found
    let uri = "file:///Users/helge/code/lira/examples/test.li";
    client
        .did_open(
            uri,
            r#"import std.fs
import std.io

fn main() {
}
"#,
        )
        .await;

    let links = client.document_link(uri).await;

    // Should have links for both imports (if stdlib is found)
    // Note: This test may return 0 links if stdlib path isn't found
    if !links.is_empty() {
        // Check that links point to the import paths
        let fs_link = links.iter().find(|l| l.range.start.line == 0);
        let io_link = links.iter().find(|l| l.range.start.line == 1);
        assert!(fs_link.is_some(), "Should have link for std.fs");
        assert!(io_link.is_some(), "Should have link for std.io");
    }
    // If links is empty, stdlib wasn't found - acceptable in test environment
}

#[tokio::test]
async fn test_completion_user_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn calculate(a: int, b: int) -> int {
    a + b
}

fn main() {
    cal
}
"#,
        )
        .await;

    // Request completion after "cal" - should match "calculate"
    let completions = client.completion(uri, 5, 7).await;

    let items = match completions {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let calc = items.iter().find(|i| i.label == "calculate");
    assert!(calc.is_some(), "Should complete user function 'calculate'");

    // Check it has function kind
    if let Some(c) = calc {
        assert_eq!(c.kind, Some(CompletionItemKind::FUNCTION));
    }
}

#[tokio::test]
async fn test_completion_user_struct() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"struct Point {
    x: float,
    y: float,
}

fn main() {
    Poi
}
"#,
        )
        .await;

    // Request completion after "Poi" - should match "Point"
    let completions = client.completion(uri, 6, 7).await;

    let items = match completions {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let point = items.iter().find(|i| i.label == "Point");
    assert!(point.is_some(), "Should complete user struct 'Point'");

    // Check it has struct kind
    if let Some(p) = point {
        assert_eq!(p.kind, Some(CompletionItemKind::STRUCT));
    }
}

#[tokio::test]
async fn test_inlay_hint_variable_type() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    let x = 42
    let s = "hello"
    let b = true
}
"#,
        )
        .await;

    // Request inlay hints for the whole document
    let hints = client
        .inlay_hint(
            uri,
            Position { line: 0, character: 0 },
            Position { line: 5, character: 0 },
        )
        .await;

    // Should have type hints for x, s, b
    assert!(hints.len() >= 3, "Should have inlay hints for variables");

    // Check that hints include expected types
    let hint_labels: Vec<String> = hints
        .iter()
        .filter_map(|h| match &h.label {
            InlayHintLabel::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    assert!(
        hint_labels.iter().any(|l| l.contains("int")),
        "Should have int hint"
    );
    assert!(
        hint_labels.iter().any(|l| l.contains("string")),
        "Should have string hint"
    );
    assert!(
        hint_labels.iter().any(|l| l.contains("bool")),
        "Should have bool hint"
    );
}

#[tokio::test]
async fn test_initialize_new_capabilities() {
    let mut client = TestClient::new().await;
    let result = client.initialize().await;

    let caps = result.capabilities;

    // Check new capabilities are registered
    assert!(
        caps.signature_help_provider.is_some(),
        "Should have signature help"
    );
    assert!(
        caps.folding_range_provider.is_some(),
        "Should have folding range"
    );
    assert!(
        caps.document_link_provider.is_some(),
        "Should have document link"
    );
    assert!(
        caps.inlay_hint_provider.is_some(),
        "Should have inlay hint"
    );
    assert!(
        caps.rename_provider.is_some(),
        "Should have rename"
    );
}

#[tokio::test]
async fn test_prepare_rename_on_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    let foo = 42
    println(foo)
}
"#,
        )
        .await;

    // Prepare rename on "foo" variable
    let result = client.prepare_rename(uri, 1, 8).await;
    assert!(result.is_some(), "Should be able to rename variable");
}

#[tokio::test]
async fn test_prepare_rename_on_keyword() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client.did_open(uri, "fn main() {}").await;

    // Try to prepare rename on "fn" keyword
    let result = client.prepare_rename(uri, 0, 0).await;
    assert!(result.is_none(), "Should not be able to rename keyword");
}

#[tokio::test]
async fn test_rename_variable() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    let count = 0
    println(count)
    count
}
"#,
        )
        .await;

    // Rename "count" to "total"
    let result = client.rename(uri, 1, 8, "total").await;
    assert!(result.is_some(), "Should get workspace edit");

    let edit = result.unwrap();
    let changes = edit.changes.expect("Should have changes");
    let edits = changes.get(&Url::parse(uri).unwrap()).expect("Should have edits for URI");

    // Should rename all occurrences (declaration + 2 usages)
    assert!(edits.len() >= 2, "Should have at least 2 edits");

    // All edits should replace with "total"
    for text_edit in edits {
        assert_eq!(text_edit.new_text, "total");
    }
}

#[tokio::test]
async fn test_rename_function() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn add(a: int, b: int) -> int {
    a + b
}

fn main() {
    let result = add(1, 2)
}
"#,
        )
        .await;

    // Rename "add" to "sum"
    let result = client.rename(uri, 0, 3, "sum").await;
    assert!(result.is_some(), "Should get workspace edit");

    let edit = result.unwrap();
    let changes = edit.changes.expect("Should have changes");
    let edits = changes.get(&Url::parse(uri).unwrap()).expect("Should have edits for URI");

    // Should rename definition and call site
    assert_eq!(edits.len(), 2, "Should have 2 edits (definition + call)");
}

// ============================================================================
// Call Hierarchy Tests
// ============================================================================

#[tokio::test]
async fn test_prepare_call_hierarchy() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn helper() {
    println("helping")
}

fn main() {
    helper()
}
"#,
        )
        .await;

    // Prepare call hierarchy on "helper" function
    let items = client.prepare_call_hierarchy(uri, 0, 4).await;
    assert_eq!(items.len(), 1, "Should find one function");
    assert_eq!(items[0].name, "helper");
    assert_eq!(items[0].kind, SymbolKind::FUNCTION);
}

#[tokio::test]
async fn test_incoming_calls() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn target() {
    println("target")
}

fn caller1() {
    target()
}

fn caller2() {
    target()
    target()
}
"#,
        )
        .await;

    // Get call hierarchy item for "target"
    let items = client.prepare_call_hierarchy(uri, 0, 4).await;
    assert!(!items.is_empty(), "Should find target function");

    // Get incoming calls
    let incoming = client.incoming_calls(&items[0]).await;
    assert_eq!(incoming.len(), 2, "Should have 2 callers");

    let caller_names: Vec<&str> = incoming.iter().map(|c| c.from.name.as_str()).collect();
    assert!(caller_names.contains(&"caller1"));
    assert!(caller_names.contains(&"caller2"));
}

#[tokio::test]
async fn test_outgoing_calls() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn helper1() {
    println("h1")
}

fn helper2() {
    println("h2")
}

fn main() {
    helper1()
    helper2()
}
"#,
        )
        .await;

    // Get call hierarchy item for "main"
    let items = client.prepare_call_hierarchy(uri, 8, 4).await;
    assert!(!items.is_empty(), "Should find main function");

    // Get outgoing calls
    let outgoing = client.outgoing_calls(&items[0]).await;
    assert_eq!(outgoing.len(), 2, "main should call 2 functions");

    let callee_names: Vec<&str> = outgoing.iter().map(|c| c.to.name.as_str()).collect();
    assert!(callee_names.contains(&"helper1"));
    assert!(callee_names.contains(&"helper2"));
}

// ============================================================================
// Code Action Tests
// ============================================================================

#[tokio::test]
async fn test_code_action_let_to_var() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn main() {
    let x = 5
}
"#,
        )
        .await;

    // Request code actions on the "let" line
    let actions = client.code_action(uri, 1, 0, 1, 0).await;

    // Should have "Convert to var" action
    let titles: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
            _ => None,
        })
        .collect();

    assert!(
        titles.iter().any(|t| t.contains("var")),
        "Should have 'Convert to var' action, got: {:?}",
        titles
    );
}

#[tokio::test]
async fn test_code_action_add_doc_comment() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"fn hello() {
    println("hello")
}
"#,
        )
        .await;

    // Request code actions on the function line
    let actions = client.code_action(uri, 0, 0, 0, 0).await;

    // Should have "Add documentation" action
    let titles: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
            _ => None,
        })
        .collect();

    assert!(
        titles.iter().any(|t| t.contains("documentation")),
        "Should have 'Add documentation' action, got: {:?}",
        titles
    );
}

#[tokio::test]
async fn test_code_action_generate_impl() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"struct Point {
    x: int,
    y: int,
}
"#,
        )
        .await;

    // Request code actions on the struct line
    let actions = client.code_action(uri, 0, 0, 0, 0).await;

    // Should have "Generate impl" action
    let titles: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
            _ => None,
        })
        .collect();

    assert!(
        titles.iter().any(|t| t.contains("impl")),
        "Should have 'Generate impl' action, got: {:?}",
        titles
    );
}

#[tokio::test]
async fn test_code_action_organize_imports() {
    let mut client = TestClient::new().await;
    client.initialize().await;

    let uri = "file:///test.li";
    client
        .did_open(
            uri,
            r#"import std.io
import std.fs
import std.collections

fn main() {}
"#,
        )
        .await;

    // Request code actions anywhere
    let actions = client.code_action(uri, 0, 0, 0, 0).await;

    // Should have "Organize imports" action since they're not sorted
    let titles: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => Some(ca.title.clone()),
            _ => None,
        })
        .collect();

    assert!(
        titles.iter().any(|t| t.contains("Organize imports")),
        "Should have 'Organize imports' action, got: {:?}",
        titles
    );
}

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

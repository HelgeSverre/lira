//! Lira Language Server Library
//!
//! Implements the Language Server Protocol for Lira, providing:
//! - Diagnostics (syntax and type errors)
//! - Completion (keywords, built-ins, snippets, user symbols)
//! - Hover (type info and documentation)
//! - Go-to-definition, find references, type definition
//! - Signature help (parameter hints)
//! - Folding ranges (code folding)
//! - Document links (clickable imports)
//! - Call hierarchy (incoming/outgoing calls)
//! - Code actions (quick fixes, refactoring)
//! - Document highlight (highlight symbol occurrences)
//! - Selection range (smart selection expand/shrink)
//! - Workspace symbols (search across files)

mod call_hierarchy;
mod code_actions;
mod completion;
mod definition;
mod diagnostics;
mod document_highlight;
mod document_links;
mod folding;
mod hover;
mod inlay_hints;
mod references;
mod rename;
mod selection_range;
mod sema_index;
mod sema_refs;
mod semantic_tokens;
mod signature_help;
mod symbols;
mod type_definition;
mod utils;
mod workspace_symbols;

use dashmap::DashMap;
use ropey::Rope;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// Document state tracked by the server
#[derive(Debug)]
pub struct Document {
    /// Document URI
    pub uri: Url,
    /// Document content as a rope for efficient editing
    pub content: Rope,
    /// Document version
    pub version: i32,
    /// Cached error-tolerant analysis for this version. `None` until first
    /// computed (or after an edit invalidates it). Shared so diagnostics,
    /// hover and completion reuse a single parse+check per version.
    pub analysis: Option<Arc<lirac::Analysis>>,
}

/// The Lira Language Server
pub struct LiraLanguageServer {
    /// LSP client handle
    client: Client,
    /// Open documents
    documents: DashMap<Url, Document>,
}

impl LiraLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
        }
    }

    /// Return the cached error-tolerant analysis for a document, computing and
    /// memoizing it on first use (or after an edit cleared the cache).
    ///
    /// Returns `None` only when the document is unknown or the source fails to
    /// even parse (no AST/sema to work with).
    fn analyzed(&self, uri: &Url) -> Option<Arc<lirac::Analysis>> {
        let mut doc = self.documents.get_mut(uri)?;
        if let Some(analysis) = &doc.analysis {
            return Some(analysis.clone());
        }
        let content = doc.content.to_string();
        let analysis = Arc::new(lirac::analyze(&content).ok()?);
        doc.analysis = Some(analysis.clone());
        Some(analysis)
    }

    /// Validate a document and publish diagnostics
    async fn validate_document(&self, uri: &Url) {
        let content = match self.documents.get(uri) {
            Some(doc) => doc.content.to_string(),
            None => return,
        };

        let diagnostics = diagnostics::check_document(uri, &content);
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LiraLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Full document sync - receive entire content on change
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                // Completion support
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                // Hover support
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // Go to definition
                definition_provider: Some(OneOf::Left(true)),
                // Document symbols (outline view)
                document_symbol_provider: Some(OneOf::Left(true)),
                // Find references
                references_provider: Some(OneOf::Left(true)),
                // Semantic tokens for enhanced highlighting
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens::get_legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            ..Default::default()
                        },
                    ),
                ),
                // Signature help (parameter hints)
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![")".to_string()]),
                    ..Default::default()
                }),
                // Folding ranges (code folding)
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                // Document links (clickable imports)
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                // Inlay hints (inline type annotations)
                inlay_hint_provider: Some(OneOf::Left(true)),
                // Rename support
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                // Call hierarchy (callers/callees)
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                // Code actions (quick fixes, refactoring)
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::REFACTOR,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::REFACTOR_REWRITE,
                            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                        ]),
                        ..Default::default()
                    },
                )),
                // Document highlight (highlight all occurrences of symbol)
                document_highlight_provider: Some(OneOf::Left(true)),
                // Selection range (smart expand/shrink selection)
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                // Workspace symbols (search across all files)
                workspace_symbol_provider: Some(OneOf::Left(true)),
                // Go to type definition
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "lira-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Lira language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = Rope::from_str(&params.text_document.text);
        let version = params.text_document.version;

        self.documents.insert(
            uri.clone(),
            Document {
                uri: uri.clone(),
                content,
                version,
                analysis: None,
            },
        );

        self.validate_document(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        // With full sync, we get the entire content
        if let Some(change) = params.content_changes.into_iter().next() {
            if let Some(mut doc) = self.documents.get_mut(&uri) {
                doc.content = Rope::from_str(&change.text);
                doc.version = params.text_document.version;
                // Invalidate the cached analysis; recomputed lazily on demand.
                doc.analysis = None;
            }
        }

        self.validate_document(&uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);

        // Clear diagnostics for closed document
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Re-validate on save
        self.validate_document(&params.text_document.uri).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let completions = completion::get_completions(&content, position);
        Ok(Some(CompletionResponse::Array(completions)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let analysis = self.analyzed(&uri);
        Ok(hover::get_hover_with(
            &content,
            position,
            analysis.as_deref(),
        ))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let location = definition::find_definition(&uri, &content, position);
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let symbols = symbols::get_document_symbols(&content);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let refs = references::find_references(&uri, &content, position, include_declaration);
        if refs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(refs))
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let analysis = self.analyzed(&uri);
        let tokens = semantic_tokens::get_semantic_tokens(&content, analysis.as_deref());
        Ok(Some(SemanticTokensResult::Tokens(tokens)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        Ok(signature_help::get_signature_help(&content, position))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let ranges = folding::get_folding_ranges(&content);
        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let links = document_links::get_document_links(&uri, &content);
        if links.is_empty() {
            Ok(None)
        } else {
            Ok(Some(links))
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let range = params.range;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let hints = inlay_hints::get_inlay_hints(&content, range);
        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri;
        let position = params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        Ok(rename::prepare_rename(&content, position))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        Ok(rename::rename(&uri, &content, position, &new_name))
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        Ok(call_hierarchy::prepare_call_hierarchy(
            &uri, &content, position,
        ))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let item = &params.item;
        let uri = &item.uri;

        let content = match self.documents.get(uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let calls = call_hierarchy::incoming_calls(uri, &content, item);
        if calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(calls))
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let item = &params.item;
        let uri = &item.uri;

        let content = match self.documents.get(uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let calls = call_hierarchy::outgoing_calls(uri, &content, item);
        if calls.is_empty() {
            Ok(None)
        } else {
            Ok(Some(calls))
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let range = params.range;
        let diagnostics = &params.context.diagnostics;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let actions = code_actions::get_code_actions(&uri, &content, range, diagnostics);
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let highlights = document_highlight::get_document_highlights(&content, position);
        if highlights.is_empty() {
            Ok(None)
        } else {
            Ok(Some(highlights))
        }
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = params.text_document.uri;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let ranges = selection_range::get_selection_ranges(&content, &params.positions);
        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = &params.query;

        // Collect symbols from all open documents
        let mut all_symbols = Vec::new();

        for entry in self.documents.iter() {
            let uri = entry.key();
            let content = entry.value().content.to_string();
            let symbols = workspace_symbols::extract_symbols(uri, &content);
            all_symbols.extend(symbols);
        }

        let filtered = workspace_symbols::filter_symbols(&all_symbols, query);
        if filtered.is_empty() {
            Ok(None)
        } else {
            Ok(Some(filtered))
        }
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let content = match self.documents.get(&uri) {
            Some(doc) => doc.content.to_string(),
            None => return Ok(None),
        };

        let location = type_definition::find_type_definition(&uri, &content, position);
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }
}

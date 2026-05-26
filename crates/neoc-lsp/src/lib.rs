//! Language server for neoc, over stdio.
//!
//! Capabilities: diagnostics, document + workspace symbols, goto (definition,
//! type-definition, implementation), find-all-references, document highlight,
//! folding ranges, selection ranges, inlay hints, call hierarchy, formatting
//! (via `neoc-fmt`), signature help, rename, hover, and completion (with lazy
//! per-item resolve).
//! Diagnostics come from [`neoc_compiler::transpile`];
//! syntax errors carry precise byte ranges from the parser, while type and emit
//! diagnostics fall back to the document's first line because those stages do
//! not track spans yet (see `tasks/lsp-spans.md`). The IDE features are built on
//! the tree-sitter CST (`neoc-grammar`) for position→node mapping and on the
//! `neoc-types` symbol table for declaration signatures, so each feature
//! handler is a pure, unit-tested function over the source text.

mod call_hierarchy;
mod completion;
mod cst;
mod folding;
mod format;
mod goto;
mod highlight;
mod hover;
mod inlay;
mod position;
mod references;
mod rename;
mod selection;
mod signature;
mod signature_help;
mod symbols;

use neoc_compiler::{ByteSpan, Diagnostic as CompilerDiagnostic, Stage, Target, transpile};

// Public handler facade. The pure feature functions the IDE-UX spec
// (`tests/cucumber.rs` over `tooling/ide/`) drives directly; `byte_to_position`
// builds the `Position` they take from a byte offset into the source.
pub use call_hierarchy::{incoming_calls, outgoing_calls};
pub use completion::{completions, resolve as resolve_completion};
pub use folding::folding_ranges;
pub use format::formatting;
pub use goto::{goto_definition, goto_implementation, goto_type_definition};
pub use highlight::document_highlight;
pub use hover::hover;
pub use inlay::inlay_hints;
pub use position::{byte_to_position, position_to_byte};
pub use references::references;
pub use rename::{prepare_rename, rename};
pub use selection::selection_range;
pub use signature_help::signature_help;
use std::collections::HashMap;
use std::sync::Mutex;
pub use symbols::workspace_symbols;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CallHierarchyServerCapability, CompletionItem, CompletionOptions, CompletionParams,
    CompletionResponse, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    DocumentHighlight, DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse,
    FoldingRange, FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    ImplementationProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    InlayHint, InlayHintParams, Location, LocationLink, MarkupContent, MarkupKind, MessageType,
    OneOf, Position, PrepareRenameResponse, Range, ReferenceParams, RenameOptions, RenameParams,
    SelectionRange, SelectionRangeParams, SelectionRangeProviderCapability, ServerCapabilities,
    ServerInfo, SignatureHelp, SignatureHelpOptions, SignatureHelpParams, SymbolInformation,
    SymbolKind, TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, TypeDefinitionProviderCapability, Url, WorkspaceEdit, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

/// The range a diagnostic with no recorded span points at: the document's
/// first line. The message still surfaces in the editor's problem list.
#[must_use]
fn fallback_range(source: &str) -> Range {
    let end = source.find('\n').unwrap_or(source.len());
    Range::new(Position::new(0, 0), byte_to_position(source, end))
}

#[must_use]
fn span_range(source: &str, span: ByteSpan) -> Range {
    Range::new(
        byte_to_position(source, span.start),
        byte_to_position(source, span.end),
    )
}

/// The `source` label an editor shows beside a diagnostic.
#[must_use]
fn stage_label(stage: Stage) -> &'static str {
    match stage {
        Stage::Parse => "syntax",
        Stage::Type => "type",
        Stage::Emit => "codegen",
    }
}

fn to_lsp(source: &str, diagnostic: &CompilerDiagnostic) -> Diagnostic {
    let range = diagnostic
        .span
        .map_or_else(|| fallback_range(source), |s| span_range(source, s));
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(format!("neoc:{}", stage_label(diagnostic.stage))),
        message: diagnostic.message.clone(),
        ..Default::default()
    }
}

/// Transpiles `source` and returns its problems as LSP diagnostics.
///
/// Pure and free of I/O, so it can be unit-tested without a running server.
#[must_use]
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    // Parse and type diagnostics are target-independent; emit against the
    // default target so the editor still surfaces any backend complaint.
    transpile(source, Target::default())
        .diagnostics
        .iter()
        .map(|d| to_lsp(source, d))
        .collect()
}

/// Encodes a document URI for `CompletionItem.data`, the opaque round-trip slot
/// the editor returns on `completionItem/resolve`. Paired with [`stashed_uri`].
fn stash_uri(uri: &Url) -> serde_json::Value {
    serde_json::Value::String(uri.to_string())
}

/// Decodes the document URI a completion item carries in its `data` slot, the
/// inverse of [`stash_uri`]. `None` if the slot is absent or not a valid URI.
fn stashed_uri(item: &CompletionItem) -> Option<Url> {
    Url::parse(item.data.as_ref()?.as_str()?).ok()
}

/// Wraps a pure [`call_hierarchy::CallItem`] as the LSP `CallHierarchyItem` for
/// `uri`. The name and URI are what the incoming/outgoing requests round-trip.
fn call_hierarchy_item(uri: Url, item: call_hierarchy::CallItem) -> CallHierarchyItem {
    CallHierarchyItem {
        name: item.name,
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri,
        range: item.full_range,
        selection_range: item.name_range,
        data: None,
    }
}

struct Backend {
    client: Client,
    /// Open documents by URI. Feature requests carry only a URI and position,
    /// so the server keeps the latest full text of each open document to answer
    /// them. Full-sync means every change replaces the stored text.
    documents: Mutex<HashMap<Url, String>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    async fn publish(&self, uri: Url, source: &str, version: Option<i32>) {
        self.client
            .publish_diagnostics(uri, diagnose(source), version)
            .await;
    }

    /// The current source of an open document, if the server has it.
    fn source(&self, uri: &Url) -> Option<String> {
        self.documents.lock().ok()?.get(uri).cloned()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    // prepareRename validates the cursor and highlights the name.
                    prepare_provider: Some(true),
                    work_done_progress_options:
                        tower_lsp::lsp_types::WorkDoneProgressOptions::default(),
                })),
                signature_help_provider: Some(SignatureHelpOptions {
                    // `(` opens the help; `,` advances to the next parameter.
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    ..SignatureHelpOptions::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `.` and `:` open member- and path-completion.
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    // Detail and docs are filled lazily, per focused item.
                    resolve_provider: Some(true),
                    ..CompletionOptions::default()
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "neoc".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "neoc language server ready")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        if let Ok(mut docs) = self.documents.lock() {
            docs.insert(doc.uri.clone(), doc.text.clone());
        }
        self.publish(doc.uri, &doc.text, Some(doc.version)).await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // Full document sync: the final change event carries the whole text.
        if let Some(change) = params.content_changes.pop() {
            let uri = params.text_document.uri;
            if let Ok(mut docs) = self.documents.lock() {
                docs.insert(uri.clone(), change.text.clone());
            }
            self.publish(uri, &change.text, Some(params.text_document.version))
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Ok(mut docs) = self.documents.lock() {
            docs.remove(&params.text_document.uri);
        }
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(DocumentSymbolResponse::Nested(
            symbols::document_symbols(&source),
        )))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let Ok(documents) = self.documents.lock() else {
            return Ok(None);
        };
        let mut out = Vec::new();
        for (uri, source) in documents.iter() {
            for symbol in symbols::workspace_symbols(source, &params.query) {
                #[allow(deprecated)] // `deprecated` field is required by the struct literal
                out.push(SymbolInformation {
                    name: symbol.name,
                    kind: symbol.kind,
                    tags: None,
                    deprecated: None,
                    location: Location::new(uri.clone(), symbol.range),
                    container_name: None,
                });
            }
        }
        Ok(Some(out))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(goto::goto_definition(&source, tdp.position).map(|def| {
            GotoDefinitionResponse::Link(vec![LocationLink {
                origin_selection_range: None,
                target_uri: tdp.text_document.uri,
                target_range: def.full_range,
                target_selection_range: def.name_range,
            }])
        }))
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(
            goto::goto_type_definition(&source, tdp.position).map(|def| {
                GotoDefinitionResponse::Link(vec![LocationLink {
                    origin_selection_range: None,
                    target_uri: tdp.text_document.uri,
                    target_range: def.full_range,
                    target_selection_range: def.name_range,
                }])
            }),
        )
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        let links: Vec<LocationLink> = goto::goto_implementation(&source, tdp.position)
            .into_iter()
            .map(|def| LocationLink {
                origin_selection_range: None,
                target_uri: tdp.text_document.uri.clone(),
                target_range: def.full_range,
                target_selection_range: def.name_range,
            })
            .collect();
        if links.is_empty() {
            return Ok(None);
        }
        Ok(Some(GotoDefinitionResponse::Link(links)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let tdp = params.text_document_position;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        let locations =
            references::references(&source, tdp.position, params.context.include_declaration)
                .into_iter()
                .map(|range| Location::new(tdp.text_document.uri.clone(), range))
                .collect();
        Ok(Some(locations))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(inlay::inlay_hints(&source)))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(format::formatting(&source))
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(call_hierarchy::prepare(&source, tdp.position)
            .map(|item| vec![call_hierarchy_item(tdp.text_document.uri, item)]))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let uri = params.item.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        let calls = call_hierarchy::incoming_calls(&source, &params.item.name)
            .into_iter()
            .map(|edge| CallHierarchyIncomingCall {
                from: call_hierarchy_item(uri.clone(), edge.item),
                from_ranges: edge.ranges,
            })
            .collect();
        Ok(Some(calls))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let uri = params.item.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        let calls = call_hierarchy::outgoing_calls(&source, &params.item.name)
            .into_iter()
            .map(|edge| CallHierarchyOutgoingCall {
                to: call_hierarchy_item(uri.clone(), edge.item),
                from_ranges: edge.ranges,
            })
            .collect();
        Ok(Some(calls))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        // One selection range per requested position; a position outside every
        // node degenerates to an empty range so the response stays aligned.
        let ranges = params
            .positions
            .into_iter()
            .map(|position| {
                selection::selection_range(&source, position).unwrap_or(SelectionRange {
                    range: Range::new(position, position),
                    parent: None,
                })
            })
            .collect();
        Ok(Some(ranges))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(folding::folding_ranges(&source)))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(Some(highlight::document_highlight(&source, tdp.position)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(signature_help::signature_help(&source, tdp.position))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(rename::prepare_rename(&source, params.position).map(PrepareRenameResponse::Range))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let tdp = params.text_document_position;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        let edits = rename::rename(&source, tdp.position, &params.new_name);
        if edits.is_empty() {
            return Ok(None);
        }
        let changes: HashMap<Url, Vec<TextEdit>> = [(tdp.text_document.uri, edits)].into();
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let tdp = params.text_document_position_params;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        Ok(hover::hover(&source, tdp.position).map(|markdown| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let tdp = params.text_document_position;
        let Some(source) = self.source(&tdp.text_document.uri) else {
            return Ok(None);
        };
        // Stash the document URI on each item so `completion_resolve` can find
        // the source again — resolve receives only the item the user focused.
        let stashed = stash_uri(&tdp.text_document.uri);
        let items = completion::completions(&source, tdp.position)
            .into_iter()
            .map(|item| CompletionItem {
                data: Some(stashed.clone()),
                ..item
            })
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        // Without the stashed URI (or its source) there is nothing to resolve
        // against, so pass the item back untouched.
        let source = stashed_uri(&item).and_then(|uri| self.source(&uri));
        Ok(match source {
            Some(source) => completion::resolve(&source, item),
            None => item,
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Runs the language server over stdio until the client closes the connection.
///
/// # Errors
///
/// Returns an error if the async runtime fails to start.
pub fn run() -> std::io::Result<()> {
    // `tokio::io::stdin`/`stdout` run on the blocking pool, so the
    // current-thread runtime needs no I/O driver.
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_the_first_line() {
        assert_eq!(
            fallback_range("abc\ndef"),
            Range::new(Position::new(0, 0), Position::new(0, 3))
        );
        assert_eq!(
            fallback_range(""),
            Range::new(Position::new(0, 0), Position::new(0, 0))
        );
    }

    #[test]
    fn clean_program_has_no_diagnostics() {
        assert!(diagnose("fn main() {}").is_empty());
    }

    #[test]
    fn type_error_is_reported_on_the_first_line() {
        let diags = diagnose("fn main() { nope(); }");
        let type_diag = diags
            .iter()
            .find(|d| d.source.as_deref() == Some("neoc:type"))
            .expect("a type diagnostic for the unknown name");
        assert_eq!(type_diag.severity, Some(DiagnosticSeverity::ERROR));
        // No span yet for type errors, so it lands on line 0.
        assert_eq!(type_diag.range.start.line, 0);
    }
}

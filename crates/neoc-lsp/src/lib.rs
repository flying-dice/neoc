//! Language server for neoc, over stdio.
//!
//! Capabilities: diagnostics, document symbols (outline), goto-definition,
//! find-all-references, signature help, rename, hover, and completion (with
//! lazy per-item resolve). Diagnostics come from [`neoc_compiler::transpile`];
//! syntax errors carry precise byte ranges from the parser, while type and emit
//! diagnostics fall back to the document's first line because those stages do
//! not track spans yet (see `tasks/lsp-spans.md`). The IDE features are built on
//! the tree-sitter CST (`neoc-grammar`) for position→node mapping and on the
//! `neoc-types` symbol table for declaration signatures, so each feature
//! handler is a pure, unit-tested function over the source text.

mod completion;
mod cst;
mod goto;
mod hover;
mod position;
mod references;
mod rename;
mod signature;
mod signature_help;
mod symbols;

use neoc_compiler::{ByteSpan, Diagnostic as CompilerDiagnostic, Stage, Target, transpile};

// Public handler facade. The pure feature functions the IDE-UX spec
// (`tests/cucumber.rs` over `tooling/ide/`) drives directly; `byte_to_position`
// builds the `Position` they take from a byte offset into the source.
pub use completion::{completions, resolve as resolve_completion};
pub use hover::hover;
pub use position::{byte_to_position, position_to_byte};
pub use references::references;
pub use rename::{prepare_rename, rename};
pub use signature_help::signature_help;
use std::collections::HashMap;
use std::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionOptions, CompletionParams, CompletionResponse, Diagnostic,
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, LocationLink, MarkupContent,
    MarkupKind, MessageType, OneOf, Position, PrepareRenameResponse, Range, ReferenceParams,
    RenameOptions, RenameParams, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit,
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
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
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

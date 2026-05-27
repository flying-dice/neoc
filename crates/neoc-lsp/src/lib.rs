//! Language server for neoc, over stdio.
//!
//! Capabilities: diagnostics (compiler errors + lint warnings), document +
//! workspace symbols, goto (definition, type-definition, implementation),
//! find-all-references, document highlight, folding ranges, selection ranges,
//! inlay hints (parameter + type), semantic tokens, call hierarchy, code actions
//! (organize imports, remove unused import), formatting (via `neoc-fmt`),
//! signature help, rename, hover, and completion (with lazy per-item resolve).
//! Diagnostics are project-wide: each file is checked against the workspace's
//! other modules ([`neoc_compiler::check_module_in_project`]), so a non-entry
//! module's `use` imports resolve instead of reporting phantom errors. An edit
//! that moves a module's exported surface re-diagnoses its importers; closed
//! but on-disk modules are diagnosed too. Syntax errors carry parser byte
//! ranges and type errors carry AST-node spans, so both land on the offending
//! source. (An untitled buffer with no project falls back to single-file
//! [`diagnose`].) The IDE features are built on the tree-sitter CST
//! (`neoc-grammar`) for position→node mapping, the `neoc-types` symbol table for
//! declaration signatures, and `neoc_types::expression_types` for inferred types
//! by position — so each feature handler is a pure, unit-tested function over
//! the source text.

mod call_hierarchy;
mod code_action;
mod completion;
mod cst;
mod diagnostics;
mod folding;
mod format;
mod goto;
mod highlight;
mod hover;
mod inlay;
mod lints;
mod position;
mod references;
mod rename;
mod selection;
mod semantic_tokens;
mod signature;
mod signature_help;
mod symbols;
// Phase 2 of cross-file resolution: the index is built and unit-tested in
// isolation here; the live handlers read it from Phase 3 on
// (see tasks/cross-file-resolution.md).
mod workspace;

// Public handler facade. The pure feature functions the IDE-UX spec
// (`tests/cucumber.rs` over `tooling/ide/`) drives directly; `byte_to_position`
// builds the `Position` they take from a byte offset into the source.
pub use crate::workspace::{DeclLoc, WorkspaceIndex};
pub use call_hierarchy::{incoming_calls, outgoing_calls};
pub use code_action::{organize_imports, unused_imports};
pub use completion::{completions, resolve as resolve_completion};
pub use diagnostics::{diagnose, diagnose_in_project};
pub use folding::folding_ranges;
pub use format::formatting;
pub use goto::{goto_definition, goto_implementation, goto_type_definition, name_at};
pub use highlight::document_highlight;
pub use hover::hover;
pub use inlay::inlay_hints;
pub use position::{byte_to_position, position_to_byte};
pub use references::{item_references, references, references_by_name};
pub use rename::{prepare_rename, rename, rename_by_name};
pub use selection::selection_range;
pub use semantic_tokens::{TOKEN_TYPES, semantic_tokens};
pub use signature_help::signature_help;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
pub use symbols::workspace_symbols;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CallHierarchyServerCapability, CodeAction, CodeActionKind, CodeActionOrCommand,
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse, CompletionItem,
    CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, Diagnostic,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWatchedFilesRegistrationOptions, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, DocumentHighlight,
    DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse, FileChangeType,
    FileSystemWatcher, FoldingRange, FoldingRangeParams, FoldingRangeProviderCapability,
    GlobPattern, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, ImplementationProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, InlayHint, InlayHintParams, Location, LocationLink, MarkupContent,
    MarkupKind, MessageType, OneOf, Position, PrepareRenameResponse, Range, ReferenceParams,
    Registration, RenameOptions, RenameParams, SelectionRange, SelectionRangeParams,
    SelectionRangeProviderCapability, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SymbolInformation, SymbolKind,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit,
    TypeDefinitionProviderCapability, Url, WorkspaceEdit, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

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

/// Whether two ranges overlap, comparing positions as `(line, character)`.
fn ranges_overlap(a: Range, b: Range) -> bool {
    let le = |x: Position, y: Position| (x.line, x.character) <= (y.line, y.character);
    le(a.start, b.end) && le(b.start, a.end)
}

/// The workspace root from an `initialize` request, preferring the first
/// `workspace_folders` entry and falling back to the (deprecated but still
/// widely sent) `root_uri`. `None` when the client opened a single file with no
/// workspace, in which case the index keeps its working-directory root.
fn workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(folder) = params.workspace_folders.as_ref().and_then(|f| f.first()) {
        if let Ok(path) = folder.uri.to_file_path() {
            return Some(path);
        }
    }
    #[allow(deprecated)] // `root_uri` is deprecated in LSP but clients still send it
    params
        .root_uri
        .as_ref()
        .and_then(|uri| uri.to_file_path().ok())
}

/// Seeds `index` with every `.neoc` file under its root, skipping any URI in
/// `open` (the open buffer is authoritative and already indexed). Runs on a
/// blocking task off the request path; per-file locking keeps query latency low
/// rather than holding the index for the whole walk.
fn discover_workspace(index: &Mutex<WorkspaceIndex>, open: &HashSet<Url>) {
    let Some(root) = index.lock().ok().map(|i| i.root().to_path_buf()) else {
        return;
    };
    for path in neoc_files(&root) {
        let Ok(uri) = Url::from_file_path(&path) else {
            continue;
        };
        if open.contains(&uri) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(mut index) = index.lock() {
            // Version 0 marks disk-sourced content; a later `did_open` replaces
            // it with the editor buffer at the buffer's own version.
            index.set_document(uri, 0, text);
        }
    }
}

/// Every reference to `name` (used in `from_uri`) across the project, as
/// `(file, range)` pairs, reading each file's text from the index.
///
/// Visibility-scoped through [`WorkspaceIndex::reference_scope`]: a `pub` name
/// reaches its defining file and every importer, a private one stays in-file.
/// `include_declaration` drops the declaring name in the file that declares it.
/// The pure core both the live handler and the IDE-UX spec drive.
#[must_use]
pub fn project_references(
    index: &WorkspaceIndex,
    from_uri: &Url,
    name: &str,
    include_declaration: bool,
) -> Vec<(Url, Range)> {
    let mut out = Vec::new();
    for file in index.reference_scope(from_uri, name) {
        let Some(text) = index.document_text(&file) else {
            continue;
        };
        // Item path: text match minus any local shadowing the name in that file.
        for range in references::item_references(text, name, include_declaration) {
            out.push((file.clone(), range));
        }
    }
    out
}

/// The edits renaming `name` (used in `from_uri`) to `new_name` across the
/// project, grouped per file. Same visibility scope as [`project_references`];
/// rewrites the `use` path segment and the call sites in each file.
#[must_use]
pub fn project_rename(
    index: &WorkspaceIndex,
    from_uri: &Url,
    name: &str,
    new_name: &str,
) -> HashMap<Url, Vec<TextEdit>> {
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for file in index.reference_scope(from_uri, name) {
        let Some(text) = index.document_text(&file) else {
            continue;
        };
        let edits = rename::rename_by_name(text, name, new_name);
        if !edits.is_empty() {
            changes.insert(file, edits);
        }
    }
    changes
}

/// Auto-import completion items: every `pub` declaration in another file that
/// `from_uri` does not already import, each carrying a `use` insertion edit so
/// accepting it both writes the name and brings it into scope. The editor
/// filters the list by the typed prefix. Sorted by name; deduped by
/// (name, use-path) so two modules exporting the same name both appear.
#[must_use]
pub fn auto_import_completions(
    index: &WorkspaceIndex,
    from_uri: &Url,
    source: &str,
) -> Vec<CompletionItem> {
    let already = code_action::imported_names(source);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out = Vec::new();
    for (name, loc) in index.all_declarations() {
        if !loc.exported || &loc.uri == from_uri || already.contains(name) {
            continue;
        }
        let Some(use_path) = index.use_path_for(&loc.uri, name) else {
            continue;
        };
        if !seen.insert((name.to_string(), use_path.clone())) {
            continue;
        }
        out.push(CompletionItem {
            label: name.to_string(),
            kind: Some(completion_kind(loc.kind)),
            detail: Some(format!("auto-import `{use_path}`")),
            // Inserted at the top of the file; `organize imports` can reflow it.
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                new_text: format!("use {use_path};\n"),
            }]),
            ..CompletionItem::default()
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// Maps a declaration's [`SymbolKind`] to the matching [`CompletionItemKind`].
fn completion_kind(kind: SymbolKind) -> CompletionItemKind {
    match kind {
        SymbolKind::STRUCT => CompletionItemKind::STRUCT,
        SymbolKind::ENUM => CompletionItemKind::ENUM,
        SymbolKind::INTERFACE => CompletionItemKind::INTERFACE,
        SymbolKind::CONSTANT => CompletionItemKind::CONSTANT,
        _ => CompletionItemKind::FUNCTION,
    }
}

/// Whether the cursor sits after a `.` or `::` — a member or path position,
/// where an unqualified auto-import would be noise. Scans left over the
/// identifier being typed to the character that precedes it.
fn is_member_or_path_context(source: &str, position: Position) -> bool {
    let bytes = source.as_bytes();
    let mut i = position_to_byte(source, position).min(bytes.len());
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    i > 0 && (bytes[i - 1] == b'.' || bytes[i - 1] == b':')
}

/// Every `.neoc` file under `root`, recursing into subdirectories but skipping
/// build and vcs directories (`target`, `node_modules`, `.git`) so discovery
/// stays `O(source)` rather than walking generated trees.
fn neoc_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let skip = matches!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("target" | "node_modules" | ".git")
                );
                if !skip {
                    walk(&path, out);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("neoc") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

struct Backend {
    client: Client,
    /// Open documents by URI. Feature requests carry only a URI and position,
    /// so the server keeps the latest full text of each open document to answer
    /// them. Full-sync means every change replaces the stored text.
    documents: Mutex<HashMap<Url, String>>,
    /// Project-wide index backing the cross-file features (workspace symbols,
    /// goto/references/rename across modules). Maintained from the document
    /// lifecycle; rooted from `initialize` so its `use`-path resolution matches
    /// the compiler's. Behind an `Arc` so background discovery can share it.
    /// See `tasks/cross-file-resolution.md`.
    index: Arc<Mutex<WorkspaceIndex>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        // Rooted at the working directory until `initialize` supplies the real
        // workspace root; replaced there before any document is indexed.
        let root = std::env::current_dir().unwrap_or_default();
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
            index: Arc::new(Mutex::new(WorkspaceIndex::new(root))),
        }
    }

    /// Re-indexes a document from the lifecycle callbacks, keeping the
    /// project-wide index in step with the open-buffer set. Returns whether the
    /// document's exported surface changed — the signal to re-diagnose its
    /// importers, not just itself.
    fn index_document(&self, uri: &Url, version: i32, text: &str) -> bool {
        match self.index.lock() {
            Ok(mut index) => {
                index
                    .set_document(uri.clone(), version, text.to_string())
                    .changed
            }
            Err(_) => false,
        }
    }

    async fn publish(&self, uri: Url, source: &str, version: Option<i32>) {
        // Resolve diagnostics against the project (lock released before the
        // await — never hold the index mutex across `.await`).
        let diags = match self.index.lock() {
            Ok(index) => diagnose_in_project(&index, &uri, source),
            Err(_) => diagnose(source),
        };
        self.client.publish_diagnostics(uri, diags, version).await;
    }

    /// Publishes one file's project-resolved diagnostics, reading its text from
    /// the index. A no-op if the file is not indexed.
    async fn publish_from_index(&self, uri: &Url) {
        let diags = {
            let Ok(index) = self.index.lock() else {
                return;
            };
            let Some(src) = index.document_text(uri).map(str::to_string) else {
                return;
            };
            diagnose_in_project(&index, uri, &src)
        };
        self.client
            .publish_diagnostics(uri.clone(), diags, None)
            .await;
    }

    /// Re-publishes diagnostics for `changed` and every module that
    /// transitively imports it — an exported-surface edit can introduce or fix
    /// errors in dependents. All diagnostics are computed under the index lock
    /// (no nested locks, no `.await`); the lock is released before publishing.
    async fn publish_affected(&self, changed: &Url) {
        let jobs: Vec<(Url, Vec<Diagnostic>)> = {
            let Ok(index) = self.index.lock() else {
                return;
            };
            index
                .dependents_closure(changed)
                .into_iter()
                .filter_map(|uri| {
                    let src = index.document_text(&uri)?.to_string();
                    let diags = diagnose_in_project(&index, &uri, &src);
                    Some((uri, diags))
                })
                .collect()
        };
        for (uri, diags) in jobs {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    /// The current source of an open document, if the server has it.
    fn source(&self, uri: &Url) -> Option<String> {
        self.documents.lock().ok()?.get(uri).cloned()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Root the index at the workspace so `use`-path resolution matches the
        // compiler's. Runs before any document is opened, so replacing the
        // placeholder index discards nothing.
        if let Some(root) = workspace_root(&params) {
            if let Ok(mut index) = self.index.lock() {
                *index = WorkspaceIndex::new(root);
            }
        }
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
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES.to_vec(),
                                token_modifiers: vec![],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options:
                                tower_lsp::lsp_types::WorkDoneProgressOptions::default(),
                        },
                    ),
                ),
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
        // Lazy/background discovery: seed the index with the workspace's `.neoc`
        // files so cross-file queries reach unopened files. Off the request path
        // (a `spawn_blocking` task, never a synchronous startup scan) and skipping
        // any file already open, since the open buffer is authoritative.
        let open: HashSet<Url> = self
            .documents
            .lock()
            .map(|docs| docs.keys().cloned().collect())
            .unwrap_or_default();
        let index = Arc::clone(&self.index);
        let client = self.client.clone();
        tokio::spawn(async move {
            // Discover (blocking I/O), then publish project-wide diagnostics for
            // every module — closed-but-on-disk files included, so a project's
            // errors surface without each file being opened first.
            let discover_index = Arc::clone(&index);
            let _ = tokio::task::spawn_blocking(move || {
                discover_workspace(&discover_index, &open);
            })
            .await;
            let jobs: Vec<(Url, Vec<Diagnostic>)> = match index.lock() {
                Ok(guard) => guard
                    .document_uris()
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .filter_map(|uri| {
                        let src = guard.document_text(&uri)?.to_string();
                        Some((uri.clone(), diagnose_in_project(&guard, &uri, &src)))
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            for (uri, diags) in jobs {
                client.publish_diagnostics(uri, diags, None).await;
            }
        });

        // Watch `.neoc` files on disk so an edit made outside the editor (a
        // branch switch, a generated file) re-indexes without the file being
        // opened. Dynamic registration — the client drives the watch.
        let registration = Registration {
            id: "neoc-watch-source".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![FileSystemWatcher {
                    glob_pattern: GlobPattern::String("**/*.neoc".to_string()),
                    kind: None,
                }],
            })
            .ok(),
        };
        if let Err(e) = self.client.register_capability(vec![registration]).await {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!("file-watch registration failed: {e}"),
                )
                .await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // The editor's open buffer is authoritative; a disk event for an open
        // file is ignored (its `did_change` already maintains the index).
        let open: HashSet<Url> = self
            .documents
            .lock()
            .map(|docs| docs.keys().cloned().collect())
            .unwrap_or_default();
        for change in params.changes {
            if open.contains(&change.uri) {
                continue;
            }
            if change.typ == FileChangeType::DELETED {
                // Capture the importers before dropping the file, clear the
                // deleted file's diagnostics, then re-diagnose its former
                // importers — they now have a real `unresolved module` error.
                let importers: Vec<Url> = match self.index.lock() {
                    Ok(mut index) => {
                        let mut a = index.dependents_closure(&change.uri);
                        index.remove_document(&change.uri);
                        a.retain(|u| *u != change.uri);
                        a
                    }
                    Err(_) => Vec::new(),
                };
                self.client
                    .publish_diagnostics(change.uri, Vec::new(), None)
                    .await;
                for uri in importers {
                    self.publish_from_index(&uri).await;
                }
                continue;
            }
            // Created or changed on disk: re-read, re-index at version 0, and
            // re-diagnose it plus everything that imports it.
            let Ok(path) = change.uri.to_file_path() else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            self.index_document(&change.uri, 0, &text);
            self.publish_affected(&change.uri).await;
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        if let Ok(mut docs) = self.documents.lock() {
            docs.insert(doc.uri.clone(), doc.text.clone());
        }
        self.index_document(&doc.uri, doc.version, &doc.text);
        self.publish(doc.uri, &doc.text, Some(doc.version)).await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // Full document sync: the final change event carries the whole text.
        if let Some(change) = params.content_changes.pop() {
            let uri = params.text_document.uri;
            if let Ok(mut docs) = self.documents.lock() {
                docs.insert(uri.clone(), change.text.clone());
            }
            let version = params.text_document.version;
            let surface_changed = self.index_document(&uri, version, &change.text);
            if surface_changed {
                // The exported surface moved — re-diagnose every dependent too.
                self.publish_affected(&uri).await;
            } else {
                // A body-only edit can only affect this file's own diagnostics.
                self.publish(uri, &change.text, Some(version)).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Ok(mut docs) = self.documents.lock() {
            docs.remove(&uri);
        }
        // The buffer is gone, but the file is still part of the project. Re-seed
        // the index from disk (version 0) so it keeps reporting; only when the
        // file no longer exists on disk is it dropped and its diagnostics
        // cleared. This makes close self-correcting rather than trusting the
        // ordering of close-vs-delete events.
        if let Ok(path) = uri.to_file_path() {
            if let Ok(disk) = std::fs::read_to_string(&path) {
                self.index_document(&uri, 0, &disk);
                self.publish(uri, &disk, None).await;
                return;
            }
        }
        if let Ok(mut index) = self.index.lock() {
            index.remove_document(&uri);
        }
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
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
        let Ok(index) = self.index.lock() else {
            return Ok(None);
        };
        // Project-wide: the name index spans every indexed file, not just the
        // document the request happens to touch.
        let mut out = Vec::new();
        for (name, loc) in index.workspace_symbols(&params.query) {
            #[allow(deprecated)] // `deprecated` field is required by the struct literal
            out.push(SymbolInformation {
                name,
                kind: loc.kind,
                tags: None,
                deprecated: None,
                location: Location::new(loc.uri, loc.range),
                container_name: None,
            });
        }
        Ok(Some(out))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let tdp = params.text_document_position_params;
        let uri = tdp.text_document.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        // A definition in the same file wins.
        if let Some(def) = goto::goto_definition(&source, tdp.position) {
            return Ok(Some(GotoDefinitionResponse::Link(vec![LocationLink {
                origin_selection_range: None,
                target_uri: uri,
                target_range: def.full_range,
                target_selection_range: def.name_range,
            }])));
        }
        // Otherwise follow `use` across files: jump to a `pub` declaration of the
        // name in a module this file imports. Scoping to the file's own imports
        // (not every project-wide declaration of the name) keeps the jump from
        // landing in an unrelated module that happens to reuse the name.
        let Some(name) = goto::name_at(&source, tdp.position) else {
            return Ok(None);
        };
        let Ok(index) = self.index.lock() else {
            return Ok(None);
        };
        let target = index
            .imported_definition(&uri, &name)
            .map(|loc| GotoDefinitionResponse::Scalar(Location::new(loc.uri.clone(), loc.range)));
        Ok(target)
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
        let uri = tdp.text_document.uri;
        let include_decl = params.context.include_declaration;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        let Some(name) = goto::name_at(&source, tdp.position) else {
            return Ok(Some(Vec::new()));
        };
        let Ok(index) = self.index.lock() else {
            // No index: file-local references.
            let locations = references::references(&source, tdp.position, include_decl)
                .into_iter()
                .map(|range| Location::new(uri.clone(), range))
                .collect();
            return Ok(Some(locations));
        };
        // Project-wide, visibility-scoped. Reads each file (including this one)
        // from the index, which the document lifecycle keeps in step with the
        // open buffers.
        let locations = project_references(&index, &uri, &name, include_decl)
            .into_iter()
            .map(|(file, range)| Location::new(file, range))
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

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        let mut actions = Vec::new();
        if let Some(edits) = code_action::organize_imports(&source) {
            let changes = [(uri.clone(), edits)].into();
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Organize imports".to_string(),
                kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                ..CodeAction::default()
            }));
        }
        // Offer to remove each unused import the request's range touches.
        for unused in code_action::unused_imports(&source) {
            if !ranges_overlap(params.range, unused.removal) {
                continue;
            }
            let edit = TextEdit {
                range: unused.removal,
                new_text: String::new(),
            };
            let changes = [(uri.clone(), vec![edit])].into();
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Remove unused import `{}`", unused.name),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                ..CodeAction::default()
            }));
        }
        Ok(Some(actions))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let Some(source) = self.source(&params.text_document.uri) else {
            return Ok(None);
        };
        let data = semantic_tokens::encode(&semantic_tokens::semantic_tokens(&source));
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
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
        let uri = tdp.text_document.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        let Some(name) = goto::name_at(&source, tdp.position) else {
            return Ok(None);
        };
        let Ok(index) = self.index.lock() else {
            // No index: file-local rename.
            let edits = rename::rename(&source, tdp.position, &params.new_name);
            if edits.is_empty() {
                return Ok(None);
            }
            return Ok(Some(WorkspaceEdit {
                changes: Some([(uri, edits)].into()),
                ..WorkspaceEdit::default()
            }));
        };
        // Project-wide, visibility-scoped: rewrite the defining file and every
        // module that imports it (the `use` path segment and the call sites).
        let changes = project_rename(&index, &uri, &name, &params.new_name);
        if changes.is_empty() {
            return Ok(None);
        }
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
        let uri = tdp.text_document.uri;
        let Some(source) = self.source(&uri) else {
            return Ok(None);
        };
        // Stash the document URI on each item so `completion_resolve` can find
        // the source again — resolve receives only the item the user focused.
        let stashed = stash_uri(&uri);
        let mut items: Vec<CompletionItem> = completion::completions(&source, tdp.position)
            .into_iter()
            .map(|item| CompletionItem {
                data: Some(stashed.clone()),
                ..item
            })
            .collect();
        // Auto-import: importable `pub` names from other files, each with a `use`
        // edit. Only in an unqualified position — not after `.`/`::`.
        if !is_member_or_path_context(&source, tdp.position) {
            if let Ok(index) = self.index.lock() {
                items.extend(
                    auto_import_completions(&index, &uri, &source)
                        .into_iter()
                        .map(|item| CompletionItem {
                            data: Some(stashed.clone()),
                            ..item
                        }),
                );
            }
        }
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
    use tower_lsp::lsp_types::DiagnosticSeverity;

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

    #[test]
    fn reference_scope_respects_visibility() {
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        index.set_document(f("util.neoc"), 1, "pub fn add() {}\n".to_string());
        index.set_document(
            f("main.neoc"),
            1,
            "use util::add;\nfn main() { add() }\n".to_string(),
        );
        index.set_document(f("solo.neoc"), 1, "fn secret() { secret() }\n".to_string());

        // A `pub` decl used across files: its file plus every importer.
        let mut pub_scope = index.reference_scope(&f("main.neoc"), "add");
        pub_scope.sort();
        let mut want = vec![f("util.neoc"), f("main.neoc")];
        want.sort();
        assert_eq!(pub_scope, want, "pub: defining file + importers");

        // A private, file-local name stays in its own file.
        assert_eq!(
            index.reference_scope(&f("solo.neoc"), "secret"),
            vec![f("solo.neoc")],
            "private: file-local only"
        );

        // An unknown name does not reach beyond the current file.
        assert_eq!(
            index.reference_scope(&f("main.neoc"), "nonesuch"),
            vec![f("main.neoc")],
            "unknown: file-local only"
        );

        // `imported_definition` is the goto half: the `pub` decl in the imported
        // module, and nothing for a private or unknown name.
        assert_eq!(
            index
                .imported_definition(&f("main.neoc"), "add")
                .map(|d| &d.uri),
            Some(&f("util.neoc")),
            "goto resolves the imported pub decl"
        );
        assert!(
            index
                .imported_definition(&f("main.neoc"), "secret")
                .is_none(),
            "a non-imported name has no cross-file definition"
        );
    }

    #[test]
    fn project_diagnose_resolves_imported_names() {
        // The headline fix: a module that `use`s a sibling is clean in the
        // project, where single-file `diagnose` reports phantom errors.
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        index.set_document(
            f("util.neoc"),
            1,
            "pub fn add(a: number, b: number) -> number { a + b }\n".to_string(),
        );
        let main_src = "use util::add;\nfn go() -> number { add(1, 2) }\n";
        index.set_document(f("main.neoc"), 1, main_src.to_string());

        let diags = diagnose_in_project(&index, &f("main.neoc"), main_src);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("unresolved module")),
            "imported `util` resolves in the project: {diags:?}"
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "no errors in the project (an unused-fn warning is fine): {diags:?}"
        );
    }

    #[test]
    fn project_diagnose_reports_a_real_cross_module_error() {
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        index.set_document(
            f("util.neoc"),
            1,
            "pub fn add(a: number, b: number) -> number { a + b }\n".to_string(),
        );
        let main_src = "use util::add;\nfn go() -> string { add(1, 2) }\n";
        index.set_document(f("main.neoc"), 1, main_src.to_string());
        let diags = diagnose_in_project(&index, &f("main.neoc"), main_src);
        assert!(
            diags.iter().any(|d| d.message.contains("expects `string`")),
            "a genuine type error still surfaces: {diags:?}"
        );
    }

    #[test]
    fn diagnostic_labels_become_related_information() {
        // A trait-conformance mismatch carries a "declared here" label that the
        // facade maps to LSP related-information pointing into the same file.
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let uri = f("shapes.neoc");
        let src = "trait Area { fn area(self) -> number; }\n\
                   struct Sq;\n\
                   impl Area for Sq { fn area(self) -> string { \"big\" } }\n";
        let mut index = WorkspaceIndex::new(root);
        index.set_document(uri.clone(), 1, src.to_string());
        let diags = diagnose_in_project(&index, &uri, src);
        let mismatch = diags
            .iter()
            .find(|d| d.message.contains("does not match trait `Area`"))
            .expect("conformance mismatch");
        let related = mismatch
            .related_information
            .as_ref()
            .expect("mismatch carries related information");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].location.uri, uri);
        assert!(
            related[0].message.contains("declared here in trait `Area`"),
            "{:?}",
            related[0]
        );
    }

    #[test]
    fn a_closed_on_disk_module_is_checked_in_the_project() {
        // A module seeded from disk (version 0, never opened) is still part of
        // the project and reports its own errors — the whole-project publish
        // pass focuses every indexed file, not just open ones.
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        let util = "pub fn bad() -> number {\n  \"x\"\n}\n";
        index.set_document(f("util.neoc"), 0, util.to_string());
        let diags = diagnose_in_project(&index, &f("util.neoc"), util);
        assert!(
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "a closed module's type error still surfaces: {diags:?}"
        );
    }

    #[test]
    fn untitled_buffer_falls_back_to_single_file() {
        // A buffer with no file path can't be placed in the project, so it
        // diagnoses exactly as the single-file path would.
        let index = WorkspaceIndex::new(Path::new("/proj"));
        let uri = Url::parse("untitled:Untitled-1").unwrap();
        let src = "fn m() -> number { 1 }\n";
        assert_eq!(diagnose_in_project(&index, &uri, src), diagnose(src));
    }

    #[test]
    fn auto_import_offers_unimported_pub_decls_with_a_use_edit() {
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        index.set_document(
            f("util.neoc"),
            1,
            "pub fn add() {}\nfn secret() {}\n".to_string(),
        );
        let main_src = "fn main() -> number { 0 }\n";
        index.set_document(f("main.neoc"), 1, main_src.to_string());

        let items = auto_import_completions(&index, &f("main.neoc"), main_src);
        let add = items
            .iter()
            .find(|i| i.label == "add")
            .expect("`add` offered");
        assert_eq!(add.kind, Some(CompletionItemKind::FUNCTION));
        assert_eq!(
            add.additional_text_edits.as_ref().unwrap()[0].new_text,
            "use util::add;\n",
            "accepting the item also writes the import"
        );
        assert!(
            !items.iter().any(|i| i.label == "secret"),
            "a private decl is not importable"
        );
    }

    #[test]
    fn auto_import_skips_already_imported_and_local_names() {
        let root = Path::new("/proj");
        let f = |rel: &str| Url::from_file_path(root.join(rel)).unwrap();
        let mut index = WorkspaceIndex::new(root);
        index.set_document(f("util.neoc"), 1, "pub fn add() {}\n".to_string());
        let main_src = "use util::add;\npub fn helper() {}\n";
        index.set_document(f("main.neoc"), 1, main_src.to_string());

        let items = auto_import_completions(&index, &f("main.neoc"), main_src);
        assert!(
            !items.iter().any(|i| i.label == "add"),
            "already imported — not offered again"
        );
        assert!(
            !items.iter().any(|i| i.label == "helper"),
            "a local declaration needs no import"
        );
    }

    #[test]
    fn member_and_path_context_suppress_auto_import() {
        let at = |src: &str| is_member_or_path_context(src, byte_to_position(src, src.len()));
        assert!(at("foo.ba"), "after a `.` member access");
        assert!(at("Color::Re"), "after a `::` path");
        assert!(!at("ba"), "a bare identifier prefix is fair game");
    }

    #[test]
    fn discovery_finds_neoc_files_and_skips_build_dirs() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "neoc_disc_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let write = |rel: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "fn main() {}\n").unwrap();
            path
        };
        let top = write("main.neoc");
        let nested = write("util/helpers.neoc");
        write("target/generated.neoc"); // build dir — must be skipped
        write("README.md"); // not .neoc — ignored

        let mut found = neoc_files(&root);
        found.sort();
        let mut want = vec![top, nested];
        want.sort();
        assert_eq!(
            found, want,
            "recurse into subdirs, skip target/, ignore non-.neoc"
        );

        std::fs::remove_dir_all(&root).ok();
    }
}

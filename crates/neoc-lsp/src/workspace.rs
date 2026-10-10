//! Project-wide index for cross-file features — Phase 2 of the cross-file
//! resolution plan.
//!
//! Single-file handlers (references, rename, workspace symbols, goto) stop at
//! the file boundary. This index is the foundation that lets them reach across
//! a project without re-analysing the world on a keystroke. The design targets
//! (from the major servers): per-file work keyed by content, a `name → location`
//! index so project-wide queries are a lookup rather than a scan, and a
//! reverse-dependency map so an edit's blast radius stays bounded.
//!
//! It is deliberately built and unit-tested in isolation here; wiring it into
//! the live handlers is Phase 3. The index reuses the compiler's
//! [`module_file`] rule so the LSP's file graph matches what compiles.
//!
//! Invariants:
//! - One parse + declaration extraction per `(uri, content)`; an unchanged edit
//!   (same text) does no work beyond bumping the stored version.
//! - The `decls` and `importers` maps always reflect the current document set;
//!   queries read them directly, never re-parsing.
//! - [`WorkspaceIndex::set_document`] reports whether the file's *exported
//!   surface* (its `pub` declarations and its imports) changed, so a later
//!   debounce layer can skip recomputing dependents on a body-only edit — the
//!   early-cutoff that keeps an edit from cascading.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use neoc_compiler::{module_file, resolution_root};
use tower_lsp::lsp_types::{Range, SymbolKind, Url};

use crate::cst::{self, node_text};
use crate::position::byte_range;

/// Canonicalises a file `Url` so the index keys match the compiler's, which
/// canonicalises its resolution keys (mirrors `neoc_compiler`'s private
/// `canonical_or`: `std::fs::canonicalize`, falling back to the path itself when
/// it does not yet exist on disk). A symlinked, `..`-containing, or
/// case-differing spelling of the same file therefore lands on one key, so
/// references/rename do not miss. A non-file `Url` (an untitled buffer) is
/// returned unchanged. Already-canonical paths round-trip identically.
fn canonical_uri(uri: Url) -> Url {
    let Ok(path) = uri.to_file_path() else {
        return uri;
    };
    match std::fs::canonicalize(&path) {
        Ok(canonical) => Url::from_file_path(canonical).unwrap_or(uri),
        Err(_) => uri,
    }
}

/// Where a declaration lives, for a project-wide name lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclLoc {
    /// The file the declaration is in.
    pub uri: Url,
    /// The declaration's symbol kind (function, struct, enum, …).
    pub kind: SymbolKind,
    /// The range of the declaration's name, for an editor to jump to.
    pub range: Range,
    /// Whether the declaration is `pub` (visible to other modules).
    pub exported: bool,
}

/// Whether a [`WorkspaceIndex::set_document`] call changed the file's exported
/// surface — the set of `pub` declarations and the set of imports a dependent
/// module resolves against. When unchanged, no dependent needs recomputing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceChange {
    /// `true` when the exported surface differs from the previous version.
    pub changed: bool,
}

/// A single top-level declaration extracted from a file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Decl {
    name: String,
    kind: SymbolKind,
    range: Range,
    exported: bool,
    /// The declaration's externally-relevant text — its signature — so a change
    /// a dependent can observe (a parameter type, a field, a variant) registers
    /// as a surface change. Excludes a function's body; see [`decl_signature`].
    signature: String,
}

/// The cached, version-keyed summary of one file: enough to answer queries and
/// to maintain the project-wide maps, without retaining the parse tree.
#[derive(Debug)]
struct FileSummary {
    /// Hash of the source the summary was built from; an edit with the same
    /// text is a no-op.
    hash: u64,
    /// Every top-level declaration in the file.
    decls: Vec<Decl>,
    /// Each `use` path, as its `::`-separated segments.
    imports: Vec<Vec<String>>,
    /// The directory this file's `use` paths resolve against: its project source
    /// root (nearest `neoc.toml`) or, with no manifest, its own parent. Computed
    /// once per indexed file via [`resolution_root`], so cross-file navigation
    /// resolves each file against its own project — a monorepo opened at its top
    /// with several `neoc.toml` resolves every file correctly, not against one
    /// index-wide root. Caching it here keeps every later query (goto,
    /// references, rename, completion) off the filesystem.
    root: PathBuf,
}

impl FileSummary {
    /// The exported surface: a sorted key per `pub` declaration (its name, kind,
    /// and signature) and the sorted import paths, so the comparison is
    /// order-insensitive. Two summaries with equal surfaces are interchangeable
    /// to every dependent. `SymbolKind` is not `Ord`, so each export is rendered
    /// to a `name:kind:signature` string that sorts and compares cleanly;
    /// folding the signature in means a signature-only edit (the same name and
    /// kind, a changed parameter or return type) still moves the surface, so
    /// dependents are re-diagnosed.
    fn surface(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let mut exports: Vec<String> = self
            .decls
            .iter()
            .filter(|d| d.exported)
            .map(|d| format!("{}:{:?}:{}", d.name, d.kind, d.signature))
            .collect();
        exports.sort();
        let mut imports = self.imports.clone();
        imports.sort();
        (exports, imports)
    }
}

/// An open or on-disk document and the version it was last set at.
#[derive(Debug)]
struct Document {
    text: String,
    /// Tracked for the open-buffer-authoritative model; read once the debounce
    /// layer (Phase 6) keys cancellation off document versions.
    #[allow(dead_code)]
    version: i32,
}

/// The project-wide index. Construct with [`WorkspaceIndex::new`], feed it
/// documents with [`WorkspaceIndex::set_document`], and query the `decls` /
/// `importers` views.
#[derive(Debug)]
pub struct WorkspaceIndex {
    /// The opened workspace folder. Background discovery walks this directory to
    /// seed unopened files. It is **not** the `use`-resolution root: each file
    /// resolves its imports against its own project root, stored per file in
    /// [`FileSummary::root`] (computed via [`resolution_root`]). A monorepo opened
    /// at its top therefore resolves every file against its own nearest
    /// `neoc.toml`, not against this single folder.
    root: PathBuf,
    /// Document text by URI; an open buffer is authoritative over disk.
    documents: HashMap<Url, Document>,
    /// Per-file cached summary, keyed by URI.
    summaries: HashMap<Url, FileSummary>,
    /// Project-wide name index: declaration name → every place it is declared.
    decls: HashMap<String, Vec<DeclLoc>>,
    /// Reverse-dependency map: a file → the files whose `use` imports resolve to
    /// it. Bounds the blast radius of an exported-surface change.
    importers: HashMap<Url, Vec<Url>>,
}

impl WorkspaceIndex {
    /// Creates an empty index over the opened workspace folder `root`. The folder
    /// seeds background discovery; it is not the `use`-resolution root. Each file's
    /// imports resolve against that file's own project root (its nearest
    /// `neoc.toml`), computed per file when it is indexed — so a monorepo opened at
    /// its top resolves every file against its own project.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            documents: HashMap::new(),
            summaries: HashMap::new(),
            decls: HashMap::new(),
            importers: HashMap::new(),
        }
    }

    /// Inserts or updates a document and returns whether its exported surface
    /// changed.
    ///
    /// Re-parses and re-summarises only when `text` differs from the cached
    /// version; an identical edit just records `version`. The project-wide
    /// `decls` and `importers` maps are always brought up to date for this file,
    /// so queries stay correct. The returned [`SurfaceChange`] is the
    /// early-cutoff signal: `changed == false` means no dependent module's view
    /// of this file changed.
    pub fn set_document(&mut self, uri: Url, version: i32, text: String) -> SurfaceChange {
        // Key every document under its canonical path so an importer edge (keyed
        // off the compiler's canonical resolution) and a query by a differently
        // spelled `Url` both land on the same entry.
        let uri = canonical_uri(uri);
        let hash = hash_text(&text);
        if let Some(existing) = self.summaries.get(&uri) {
            if existing.hash == hash {
                self.documents.insert(uri, Document { text, version });
                return SurfaceChange { changed: false };
            }
        }
        let old_surface = self.summaries.get(&uri).map(FileSummary::surface);
        let new_surface = self.resummarize(&uri, &text, hash);
        let changed = old_surface.as_ref() != Some(&new_surface);
        self.documents.insert(uri, Document { text, version });
        SurfaceChange { changed }
    }

    /// Retracts `uri`'s stale entries, re-summarises it from `text`/`hash`, and
    /// re-registers its `decls` and `importers` edges. Returns the file's new
    /// exported surface so a caller can diff it. Does not touch `documents` — the
    /// caller owns the text/version bookkeeping.
    fn resummarize(&mut self, uri: &Url, text: &str, hash: u64) -> (Vec<String>, Vec<Vec<String>>) {
        self.retract(uri);
        let summary = summarize(text, hash, uri);
        for decl in &summary.decls {
            self.decls
                .entry(decl.name.clone())
                .or_default()
                .push(DeclLoc {
                    uri: uri.clone(),
                    kind: decl.kind,
                    range: decl.range,
                    exported: decl.exported,
                });
        }
        for use_path in &summary.imports {
            if let Some(target) = Self::resolve_import(&summary.root, use_path) {
                self.importers.entry(target).or_default().push(uri.clone());
            }
        }
        let surface = summary.surface();
        self.summaries.insert(uri.clone(), summary);
        surface
    }

    /// Re-summarises every indexed file whose path lies under `dir`, recomputing
    /// each file's cached resolution root ([`FileSummary::root`], via
    /// [`resolution_root`]) and rebuilding its `decls` and `importers` edges.
    /// Returns the URIs touched.
    ///
    /// Called when a `neoc.toml` under `dir` is created, edited, or deleted: the
    /// manifest is what `resolution_root` keys off, so the cached root of every
    /// sibling module is now stale even though the module's own text is
    /// unchanged. A plain re-`set_document` would short-circuit on the identical
    /// text hash and keep the stale root, so this path re-summarises directly
    /// from the stored text.
    pub fn reroot_under(&mut self, dir: &Path) -> Vec<Url> {
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let affected: Vec<Url> = self
            .summaries
            .keys()
            .filter(|uri| uri.to_file_path().is_ok_and(|p| p.starts_with(&dir)))
            .cloned()
            .collect();
        for uri in &affected {
            let Some(text) = self.documents.get(uri).map(|d| d.text.clone()) else {
                continue;
            };
            let hash = hash_text(&text);
            self.resummarize(uri, &text, hash);
        }
        affected
    }

    /// Removes a document and all its contributions to the maps. Returns whether
    /// the file had been indexed.
    pub fn remove_document(&mut self, uri: &Url) -> bool {
        let uri = canonical_uri(uri.clone());
        let had = self.summaries.contains_key(&uri);
        self.retract(&uri);
        self.documents.remove(&uri);
        self.summaries.remove(&uri);
        had
    }

    /// Withdraws `uri`'s declaration entries and its edges in the importer map,
    /// so a re-summarise can re-add only what the new text contributes.
    fn retract(&mut self, uri: &Url) {
        if let Some(summary) = self.summaries.get(uri) {
            for decl in &summary.decls {
                if let Some(locs) = self.decls.get_mut(&decl.name) {
                    locs.retain(|l| &l.uri != uri);
                    if locs.is_empty() {
                        self.decls.remove(&decl.name);
                    }
                }
            }
            for use_path in &summary.imports {
                if let Some(target) = Self::resolve_import(&summary.root, use_path) {
                    if let Some(importers) = self.importers.get_mut(&target) {
                        importers.retain(|u| u != uri);
                        if importers.is_empty() {
                            self.importers.remove(&target);
                        }
                    }
                }
            }
        }
    }

    /// Resolves a `use` path's segments to the URI of the file that defines it,
    /// via the compiler's [`module_file`] rule, against `root` — the importing
    /// file's own project root. `None` for a single-segment path (no module
    /// qualifier) or a path that maps to no file under that root.
    fn resolve_import(root: &Path, use_path: &[String]) -> Option<Url> {
        let file = module_file(root, use_path)?;
        // Canonicalise so the importer-graph key matches the canonical `Url` the
        // target file is stored under (see [`canonical_uri`]) and the compiler's
        // own canonical resolution keys.
        Some(canonical_uri(Url::from_file_path(file).ok()?))
    }

    /// The opened workspace folder. Background discovery walks this directory to
    /// seed unopened files. Not the `use`-resolution root — see
    /// [`WorkspaceIndex`].
    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The project source root a given file's `use` paths resolve against — its
    /// nearest `neoc.toml` source root, or its own parent when unindexed or under
    /// no manifest. Cached at index time; this is a map read, no filesystem walk.
    fn file_root(&self, uri: &Url) -> Option<&Path> {
        self.summaries
            .get(&canonical_uri(uri.clone()))
            .map(|s| s.root.as_path())
    }

    /// Every declaration of `name` across the project. Empty slice when none.
    #[must_use]
    pub fn find_decls(&self, name: &str) -> &[DeclLoc] {
        self.decls.get(name).map_or(&[], Vec::as_slice)
    }

    /// The `use` path that imports `item` from the file `file` — the reverse of
    /// the compiler's `module_file` rule. `<root>/math/vec.neoc` + `dot` yields
    /// `math::vec::dot`; `<root>/util.neoc` + `add` yields `util::add`. The root
    /// stripped is `file`'s own project root (its `neoc.toml` source root), so a
    /// monorepo file's import path is relative to its own package, not the opened
    /// folder. `None` when `file` is not under that root. Used to build the `use`
    /// insertion an auto-import completion offers.
    #[must_use]
    pub fn use_path_for(&self, file: &Url, item: &str) -> Option<String> {
        let file = canonical_uri(file.clone());
        let path = file.to_file_path().ok()?;
        // Prefer the cached per-file root; fall back to computing it from disk for
        // a file the index does not hold a summary for.
        let root = self
            .file_root(&file)
            .map_or_else(|| resolution_root(&path), Path::to_path_buf);
        let rel = path.strip_prefix(&root).ok()?.with_extension("");
        let mut segments: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if segments.is_empty() {
            return None;
        }
        segments.push(item.to_string());
        Some(segments.join("::"))
    }

    /// Every declaration in the project, as `(name, location)` pairs — the whole
    /// name index. Used by auto-import to scan importable `pub` declarations.
    pub fn all_declarations(&self) -> impl Iterator<Item = (&str, &DeclLoc)> {
        self.decls
            .iter()
            .flat_map(|(name, locs)| locs.iter().map(move |loc| (name.as_str(), loc)))
    }

    /// The `pub` declaration the name `name`, used in `from_uri`, resolves to
    /// across files: a declaration in a module `from_uri` imports. `None` when no
    /// imported module declares it (the name is local, unknown, or private). The
    /// cross-file half of goto-definition.
    #[must_use]
    pub fn imported_definition(&self, from_uri: &Url, name: &str) -> Option<&DeclLoc> {
        let from_uri = canonical_uri(from_uri.clone());
        let targets = self.import_targets(&from_uri);
        self.find_decls(name)
            .iter()
            .find(|d| d.exported && targets.contains(&d.uri))
    }

    /// The files a reference or rename of `name`, used in `from_uri`, may touch —
    /// scoped by visibility so an edit's blast radius stays bounded.
    ///
    /// A name resolving to a private or purely-local declaration stays in its own
    /// file. A `pub` declaration's references span its defining file plus every
    /// file that imports that module (its importers). The match within those
    /// files is still text-level, so a same-named local can be over-included
    /// until scope-aware resolution lands; but the *file set* never reaches a
    /// module that does not import the definition.
    #[must_use]
    pub fn reference_scope(&self, from_uri: &Url, name: &str) -> Vec<Url> {
        let from_uri = canonical_uri(from_uri.clone());
        let decls = self.find_decls(name);
        let targets = self.import_targets(&from_uri);
        // The declaration this use resolves to: one in the current file, else a
        // `pub` one in a module the current file imports.
        let def = decls.iter().find(|d| d.uri == from_uri).or_else(|| {
            decls
                .iter()
                .find(|d| d.exported && targets.contains(&d.uri))
        });
        let Some(def) = def else {
            return vec![from_uri.clone()];
        };
        if !def.exported {
            return vec![from_uri.clone()];
        }
        let mut scope = vec![def.uri.clone()];
        for importer in self.importers_of(&def.uri) {
            if !scope.contains(importer) {
                scope.push(importer.clone());
            }
        }
        if !scope.contains(&from_uri) {
            scope.push(from_uri);
        }
        scope
    }

    /// The files `uri`'s `use` declarations resolve to — the modules it imports
    /// from. The forward direction of [`Self::importers_of`]; used to scope a
    /// cross-file goto to a module the current file actually imports.
    #[must_use]
    pub fn import_targets(&self, uri: &Url) -> Vec<Url> {
        let Some(summary) = self.summaries.get(&canonical_uri(uri.clone())) else {
            return Vec::new();
        };
        summary
            .imports
            .iter()
            .filter_map(|path| Self::resolve_import(&summary.root, path))
            .collect()
    }

    /// The files whose `use` imports resolve to `uri` — its direct dependents.
    #[must_use]
    pub fn importers_of(&self, uri: &Url) -> &[Url] {
        self.importers
            .get(&canonical_uri(uri.clone()))
            .map_or(&[], Vec::as_slice)
    }

    /// The current text of a document, if indexed.
    #[must_use]
    pub fn document_text(&self, uri: &Url) -> Option<&str> {
        self.documents
            .get(&canonical_uri(uri.clone()))
            .map(|d| d.text.as_str())
    }

    /// Every indexed document as `(file path, text)` — the in-memory source map
    /// a project-wide check runs against. Open buffers are authoritative (they
    /// overwrote disk text via [`Self::set_document`]). A URI that is not a file
    /// path (an untitled buffer) is skipped.
    #[must_use]
    pub fn project_sources(&self) -> HashMap<PathBuf, String> {
        self.documents
            .iter()
            .filter_map(|(uri, doc)| uri.to_file_path().ok().map(|p| (p, doc.text.clone())))
            .collect()
    }

    /// Every indexed document URI.
    pub fn document_uris(&self) -> impl Iterator<Item = &Url> {
        self.documents.keys()
    }

    /// `uri` plus every module that transitively imports it — the blast radius
    /// of an edit to `uri` (the set to re-diagnose). Cycle-safe: each URI is
    /// visited once.
    #[must_use]
    pub fn dependents_closure(&self, uri: &Url) -> Vec<Url> {
        let mut out = vec![canonical_uri(uri.clone())];
        let mut i = 0;
        while i < out.len() {
            let current = out[i].clone();
            for importer in self.importers_of(&current) {
                if !out.contains(importer) {
                    out.push(importer.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// Project-wide symbol search: every declaration whose name contains `query`
    /// (case-insensitive; an empty query matches all), paired with its name.
    /// Reads the name index, so it is a filtered scan of declarations rather
    /// than of files.
    #[must_use]
    pub fn workspace_symbols(&self, query: &str) -> Vec<(String, DeclLoc)> {
        let needle = query.to_lowercase();
        let mut out = Vec::new();
        for (name, locs) in &self.decls {
            if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                continue;
            }
            for loc in locs {
                out.push((name.clone(), loc.clone()));
            }
        }
        out
    }
}

/// The 64-bit hash of a document's text, used to skip re-summarising an
/// unchanged edit.
fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Walks the CST once, collecting each top-level declaration (name, kind,
/// range, `pub`) and each `use` path, and records the file's own project
/// resolution root so its imports resolve against its nearest `neoc.toml`. A
/// URI with no file path (an untitled buffer) gets the working directory as a
/// harmless anchor — such a buffer has no on-disk siblings to resolve anyway.
fn summarize(source: &str, hash: u64, uri: &Url) -> FileSummary {
    let file_root = uri
        .to_file_path()
        .map_or_else(|()| PathBuf::from("."), |p| resolution_root(&p));
    let tree = cst::parse(source);
    let root = tree.root_node();
    let mut decls = Vec::new();
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor) {
        if item.kind() != "item" {
            continue;
        }
        let exported = item.children(&mut item.walk()).any(|c| c.kind() == "pub");
        let mut inner = item.walk();
        for decl in item.named_children(&mut inner) {
            if decl.kind() == "use_declaration" {
                if let Some(path) = use_path_segments(&decl, source) {
                    imports.push(path);
                }
                continue;
            }
            if let Some((name, kind, range)) = decl_summary(&decl, source) {
                decls.push(Decl {
                    name,
                    kind,
                    range,
                    exported,
                    signature: decl_signature(&decl, source),
                });
            }
        }
    }
    FileSummary {
        hash,
        decls,
        imports,
        root: file_root,
    }
}

/// The `(name, kind, name-range)` of a declaration node, or `None` for a node
/// kind that declares no queryable name (e.g. an `impl` block or `ext` block).
fn decl_summary(decl: &tree_sitter::Node, source: &str) -> Option<(String, SymbolKind, Range)> {
    let kind = match decl.kind() {
        "function_declaration" => SymbolKind::FUNCTION,
        "const_declaration" => SymbolKind::CONSTANT,
        "type_alias" => SymbolKind::TYPE_PARAMETER,
        "struct_declaration" => SymbolKind::STRUCT,
        "enum_declaration" => SymbolKind::ENUM,
        "trait_declaration" => SymbolKind::INTERFACE,
        _ => return None,
    };
    let name_node = decl.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?.to_string();
    let range = byte_range(source, name_node.start_byte(), name_node.end_byte());
    Some((name, kind, range))
}

/// A declaration's signature — the text a dependent module resolves against.
///
/// For a function this is the text up to (excluding) its body block, so a
/// body-only edit leaves the signature unchanged and dependents are not
/// re-diagnosed; for every other declaration the whole node is its surface (a
/// struct's fields, an enum's variants, a const's type and value, an alias's
/// target). Whitespace is collapsed so a pure reformat does not register as a
/// surface change.
fn decl_signature(decl: &tree_sitter::Node, source: &str) -> String {
    let end = decl
        .child_by_field_name("body")
        .map_or(decl.end_byte(), |body| body.start_byte());
    source
        .get(decl.start_byte()..end)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `::`-separated identifier segments of a `use_declaration`'s path.
fn use_path_segments(decl: &tree_sitter::Node, source: &str) -> Option<Vec<String>> {
    let path = decl.child_by_field_name("path")?;
    let mut segments = Vec::new();
    for seg in path.named_children(&mut path.walk()) {
        if seg.kind() == "identifier" {
            if let Some(text) = node_text(&seg, source) {
                segments.push(text.to_string());
            }
        }
    }
    (!segments.is_empty()).then_some(segments)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn uri(root: &Path, name: &str) -> Url {
        Url::from_file_path(root.join(name)).unwrap()
    }

    /// A unique on-disk temp directory, removed on drop. Per-file `use`
    /// resolution reads `neoc.toml` from disk, so these tests need real files.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "neoc_lsp_ws_{}_{}_{}",
                tag,
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Writes `body` to `rel` under the temp dir, creating parents, and
        /// returns the file's canonical `Url` — matching the canonical keys the
        /// index now stores (the temp dir may be a symlink, e.g. `/tmp` →
        /// `/private/tmp` on macOS).
        fn write(&self, rel: &str, body: &str) -> Url {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            Url::from_file_path(canonical).unwrap()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MANIFEST: &str = "[package]\nname = \"app\"\ntarget = \"lua\"\n";

    #[test]
    fn dependents_closure_is_transitive() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "a.neoc"), 1, "pub fn a() {}\n".to_string());
        idx.set_document(
            uri(root, "b.neoc"),
            1,
            "use a::a;\npub fn b() { a() }\n".to_string(),
        );
        idx.set_document(
            uri(root, "c.neoc"),
            1,
            "use b::b;\nfn c() { b() }\n".to_string(),
        );
        let mut closure = idx.dependents_closure(&uri(root, "a.neoc"));
        closure.sort();
        let mut want = vec![
            uri(root, "a.neoc"),
            uri(root, "b.neoc"),
            uri(root, "c.neoc"),
        ];
        want.sort();
        assert_eq!(
            closure, want,
            "edit to a re-checks a, its importer b, and b's importer c"
        );
    }

    #[test]
    fn dependents_closure_handles_cycles() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(
            uri(root, "a.neoc"),
            1,
            "use b::b;\npub fn a() { b() }\n".to_string(),
        );
        idx.set_document(
            uri(root, "b.neoc"),
            1,
            "use a::a;\npub fn b() { a() }\n".to_string(),
        );
        let mut closure = idx.dependents_closure(&uri(root, "a.neoc"));
        closure.sort();
        let mut want = vec![uri(root, "a.neoc"), uri(root, "b.neoc")];
        want.sort();
        assert_eq!(
            closure, want,
            "a mutual-import cycle terminates with both files"
        );
    }

    #[test]
    fn dependents_closure_of_an_unimported_leaf_is_just_itself() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "solo.neoc"), 1, "fn s() { s() }\n".to_string());
        assert_eq!(
            idx.dependents_closure(&uri(root, "solo.neoc")),
            vec![uri(root, "solo.neoc")]
        );
    }

    #[test]
    fn indexes_declarations_for_project_wide_lookup() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(
            uri(root, "a.neoc"),
            1,
            "pub struct Point { x: number }\nfn helper() -> number { 1 }\n".to_string(),
        );

        let point = idx.find_decls("Point");
        assert_eq!(point.len(), 1);
        assert_eq!(point[0].kind, SymbolKind::STRUCT);
        assert!(point[0].exported, "`pub struct` is exported");

        let helper = idx.find_decls("helper");
        assert_eq!(helper.len(), 1);
        assert!(!helper[0].exported, "a bare `fn` is module-private");

        assert!(idx.find_decls("nope").is_empty());
    }

    #[test]
    fn same_name_in_two_files_yields_two_locations() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "a.neoc"), 1, "pub fn run() {}\n".to_string());
        idx.set_document(uri(root, "b.neoc"), 1, "pub fn run() {}\n".to_string());
        assert_eq!(idx.find_decls("run").len(), 2);
    }

    #[test]
    fn importers_map_tracks_use_edges() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "util.neoc"), 1, "pub fn add() {}\n".to_string());
        idx.set_document(
            uri(root, "main.neoc"),
            1,
            "use util::add;\nfn main() { add() }\n".to_string(),
        );
        let importers = idx.importers_of(&uri(root, "util.neoc"));
        assert_eq!(importers, &[uri(root, "main.neoc")]);
    }

    #[test]
    fn a_body_only_edit_does_not_change_the_surface() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(
            uri(root, "a.neoc"),
            1,
            "pub fn run() -> number { 1 }\n".to_string(),
        );
        // Edit the body only — same exported name, same imports.
        let change = idx.set_document(
            uri(root, "a.neoc"),
            2,
            "pub fn run() -> number { 2 }\n".to_string(),
        );
        assert!(
            !change.changed,
            "a body-only edit must not flag a surface change"
        );
    }

    #[test]
    fn a_signature_edit_changes_the_surface() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(
            uri(root, "a.neoc"),
            1,
            "pub fn add(a: number) -> number { 0 }\n".to_string(),
        );
        // Same name, kind, and body — only the parameter type changes. A
        // dependent's `add(1)` may now be ill-typed, so the surface must move.
        let change = idx.set_document(
            uri(root, "a.neoc"),
            2,
            "pub fn add(a: string) -> number { 0 }\n".to_string(),
        );
        assert!(
            change.changed,
            "a parameter-type edit changes the exported surface"
        );
    }

    #[test]
    fn adding_a_pub_declaration_changes_the_surface() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "a.neoc"), 1, "pub fn run() {}\n".to_string());
        let change = idx.set_document(
            uri(root, "a.neoc"),
            2,
            "pub fn run() {}\npub fn extra() {}\n".to_string(),
        );
        assert!(change.changed, "a new `pub` fn changes the surface");
        assert_eq!(idx.find_decls("extra").len(), 1);
    }

    #[test]
    fn an_identical_edit_is_a_no_op_surface() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        let src = "pub fn run() {}\n".to_string();
        idx.set_document(uri(root, "a.neoc"), 1, src.clone());
        let change = idx.set_document(uri(root, "a.neoc"), 2, src);
        assert!(!change.changed);
    }

    #[test]
    fn re_summarising_retracts_stale_declarations() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "a.neoc"), 1, "pub fn gone() {}\n".to_string());
        assert_eq!(idx.find_decls("gone").len(), 1);
        idx.set_document(uri(root, "a.neoc"), 2, "pub fn kept() {}\n".to_string());
        assert!(
            idx.find_decls("gone").is_empty(),
            "the removed declaration must leave the index"
        );
        assert_eq!(idx.find_decls("kept").len(), 1);
    }

    #[test]
    fn removing_a_document_clears_its_contributions() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "util.neoc"), 1, "pub fn add() {}\n".to_string());
        idx.set_document(
            uri(root, "main.neoc"),
            1,
            "use util::add;\nfn main() {}\n".to_string(),
        );
        assert!(idx.remove_document(&uri(root, "main.neoc")));
        assert!(
            idx.importers_of(&uri(root, "util.neoc")).is_empty(),
            "a removed importer must drop its edge"
        );
    }

    #[test]
    fn workspace_symbols_filters_by_substring() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(
            uri(root, "a.neoc"),
            1,
            "pub struct Config {}\npub fn configure() {}\nfn other() {}\n".to_string(),
        );
        let mut names: Vec<String> = idx
            .workspace_symbols("conf")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["Config", "configure"]);
        assert_eq!(
            idx.workspace_symbols("").len(),
            3,
            "empty query matches all"
        );
    }

    #[test]
    fn use_path_for_reverses_the_module_file_rule_within_a_project() {
        // With a `neoc.toml`, the import path is relative to the package's source
        // root (`<pkg>/src/main/neoc`), so a nested module keeps its full path.
        let tmp = TempDir::new("usepath");
        tmp.write("neoc.toml", MANIFEST);
        let util = tmp.write("src/main/neoc/util.neoc", "pub fn add() {}\n");
        let vec = tmp.write("src/main/neoc/math/vec.neoc", "pub fn dot() {}\n");
        let mut idx = WorkspaceIndex::new(tmp.path());
        idx.set_document(util.clone(), 1, "pub fn add() {}\n".to_string());
        idx.set_document(vec.clone(), 1, "pub fn dot() {}\n".to_string());
        assert_eq!(
            idx.use_path_for(&util, "add").as_deref(),
            Some("util::add"),
            "a top-level sibling, relative to the project source root"
        );
        assert_eq!(
            idx.use_path_for(&vec, "dot").as_deref(),
            Some("math::vec::dot"),
            "a nested module, relative to the project source root"
        );
    }

    #[test]
    fn use_path_for_falls_back_to_the_file_directory_without_a_manifest() {
        // No `neoc.toml`: the root is the file's own parent, matching the
        // single-file resolution rule, so the path is relative to that directory.
        let tmp = TempDir::new("usepath_loose");
        let util = tmp.write("util.neoc", "pub fn add() {}\n");
        let mut idx = WorkspaceIndex::new(tmp.path());
        idx.set_document(util.clone(), 1, "pub fn add() {}\n".to_string());
        assert_eq!(
            idx.use_path_for(&util, "add").as_deref(),
            Some("util::add"),
            "a sibling resolves relative to the file's own directory"
        );
    }

    #[test]
    fn import_targets_resolve_a_files_use_paths_to_modules() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "util.neoc"), 1, "pub fn add() {}\n".to_string());
        idx.set_document(
            uri(root, "math/vec.neoc"),
            1,
            "pub fn dot() {}\n".to_string(),
        );
        idx.set_document(
            uri(root, "main.neoc"),
            1,
            "use util::add;\nuse math::vec::dot;\nfn main() {}\n".to_string(),
        );

        let mut targets = idx.import_targets(&uri(root, "main.neoc"));
        targets.sort();
        let mut want = vec![uri(root, "util.neoc"), uri(root, "math/vec.neoc")];
        want.sort();
        assert_eq!(targets, want, "each `use` resolves to its module file");
        assert!(
            idx.import_targets(&uri(root, "util.neoc")).is_empty(),
            "a file with no `use` imports nothing"
        );
    }

    #[test]
    fn workspace_symbols_span_every_indexed_file() {
        // The handler relies on this: a project-wide symbol search reaches
        // declarations in every indexed file, each carrying its own file's URI.
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        idx.set_document(uri(root, "a.neoc"), 1, "pub fn alpha() {}\n".to_string());
        idx.set_document(uri(root, "b.neoc"), 1, "pub fn beta() {}\n".to_string());

        let mut found: Vec<(String, Url)> = idx
            .workspace_symbols("")
            .into_iter()
            .map(|(name, loc)| (name, loc.uri))
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![
                ("alpha".to_string(), uri(root, "a.neoc")),
                ("beta".to_string(), uri(root, "b.neoc")),
            ],
            "symbols from both files, each with its own URI"
        );
    }

    #[test]
    fn nested_module_import_resolves_under_a_subdirectory() {
        let root = Path::new("/proj");
        let mut idx = WorkspaceIndex::new(root);
        // `use math::vec::dot` resolves to <root>/math/vec.neoc.
        idx.set_document(
            uri(root, "math/vec.neoc"),
            1,
            "pub fn dot() {}\n".to_string(),
        );
        idx.set_document(
            uri(root, "main.neoc"),
            1,
            "use math::vec::dot;\nfn main() {}\n".to_string(),
        );
        assert_eq!(
            idx.importers_of(&uri(root, "math/vec.neoc")),
            &[uri(root, "main.neoc")]
        );
    }

    /// A two-package monorepo opened at the top: each package has its own
    /// `neoc.toml`, sources under `src/main/neoc`, and a `lib.neoc` a `main.neoc`
    /// imports by the *same* `use lib::…` path. The index must resolve each
    /// file's import against its own package, never the opened folder, so the
    /// `lib` import in package A goes to A's `lib.neoc` and B's to B's.
    fn monorepo() -> (TempDir, [Url; 4]) {
        let tmp = TempDir::new("mono");
        tmp.write("a/neoc.toml", MANIFEST);
        let a_lib = tmp.write("a/src/main/neoc/lib.neoc", "pub fn a() -> number { 1 }\n");
        let a_main = tmp.write(
            "a/src/main/neoc/main.neoc",
            "use lib::a;\nfn go() -> number { a() }\n",
        );
        tmp.write("b/neoc.toml", MANIFEST);
        let b_lib = tmp.write("b/src/main/neoc/lib.neoc", "pub fn b() -> number { 2 }\n");
        let b_main = tmp.write(
            "b/src/main/neoc/main.neoc",
            "use lib::b;\nfn go() -> number { b() }\n",
        );
        (tmp, [a_lib, a_main, b_lib, b_main])
    }

    fn index_monorepo(tmp: &TempDir, uris: &[Url; 4]) -> WorkspaceIndex {
        // Opened at the monorepo top — the single index root spans both packages.
        let mut idx = WorkspaceIndex::new(tmp.path());
        for u in uris {
            let text = std::fs::read_to_string(u.to_file_path().unwrap()).unwrap();
            idx.set_document(u.clone(), 1, text);
        }
        idx
    }

    #[test]
    fn monorepo_imports_resolve_within_each_package() {
        let (tmp, uris) = monorepo();
        let [a_lib, a_main, b_lib, b_main] = &uris;
        let idx = index_monorepo(&tmp, &uris);

        // A's `use lib::a` resolves to A's lib, not B's; and vice versa.
        assert_eq!(
            idx.import_targets(a_main),
            vec![a_lib.clone()],
            "package A resolves `lib` against its own project"
        );
        assert_eq!(
            idx.import_targets(b_main),
            vec![b_lib.clone()],
            "package B resolves `lib` against its own project"
        );
        // The importer graph keys each `lib.neoc` to its own package's `main`.
        assert_eq!(idx.importers_of(a_lib), std::slice::from_ref(a_main));
        assert_eq!(idx.importers_of(b_lib), std::slice::from_ref(b_main));
    }

    #[test]
    fn monorepo_goto_jumps_within_the_same_package() {
        let (tmp, uris) = monorepo();
        let [a_lib, a_main, b_lib, b_main] = &uris;
        let idx = index_monorepo(&tmp, &uris);

        // goto-definition on `a` (used in A's main) lands in A's lib — `imported_definition`
        // is the index half the goto handler calls.
        let a_def = idx.imported_definition(a_main, "a").map(|d| d.uri.clone());
        assert_eq!(a_def, Some(a_lib.clone()));
        let b_def = idx.imported_definition(b_main, "b").map(|d| d.uri.clone());
        assert_eq!(b_def, Some(b_lib.clone()));
        // A's main does not import `b` (that's B's symbol), so no cross-package jump.
        assert!(
            idx.imported_definition(a_main, "b").is_none(),
            "A's main must not goto B's symbol"
        );
    }

    #[test]
    fn monorepo_reference_scope_does_not_cross_packages() {
        let (tmp, uris) = monorepo();
        let [a_lib, a_main, b_lib, b_main] = &uris;
        let idx = index_monorepo(&tmp, &uris);

        // References/rename of `a` (A's `pub fn`) span A's lib and A's main only.
        let mut a_scope = idx.reference_scope(a_main, "a");
        a_scope.sort();
        let mut want_a = vec![a_lib.clone(), a_main.clone()];
        want_a.sort();
        assert_eq!(
            a_scope, want_a,
            "`a`'s references stay within package A — no bleed into B"
        );
        assert!(
            !a_scope.contains(b_lib) && !a_scope.contains(b_main),
            "package B's files must never enter A's reference scope"
        );

        let mut b_scope = idx.reference_scope(b_main, "b");
        b_scope.sort();
        let mut want_b = vec![b_lib.clone(), b_main.clone()];
        want_b.sort();
        assert_eq!(b_scope, want_b, "`b`'s references stay within package B");
    }

    #[test]
    fn lone_file_without_a_manifest_resolves_via_fallback() {
        // No `neoc.toml`: a sibling `use` resolves relative to the file's own
        // directory (the single-file fallback), so the importer edge still forms.
        let tmp = TempDir::new("loose");
        let util = tmp.write("util.neoc", "pub fn add() -> number { 1 }\n");
        let main = tmp.write("main.neoc", "use util::add;\nfn go() -> number { add() }\n");
        let mut idx = WorkspaceIndex::new(tmp.path());
        idx.set_document(
            util.clone(),
            1,
            "pub fn add() -> number { 1 }\n".to_string(),
        );
        idx.set_document(
            main.clone(),
            1,
            "use util::add;\nfn go() -> number { add() }\n".to_string(),
        );

        assert_eq!(idx.import_targets(&main), vec![util.clone()]);
        assert_eq!(idx.importers_of(&util), std::slice::from_ref(&main));
        assert_eq!(
            idx.imported_definition(&main, "add").map(|d| d.uri.clone()),
            Some(util.clone()),
            "fallback goto resolves a sibling next to the file"
        );
    }

    #[test]
    fn adding_a_manifest_mid_session_re_roots_cached_resolution() {
        // `main.neoc` lives in a subdirectory (`app/`) and imports `util::helper`
        // from the source-root level. Without a `neoc.toml`, main's root is its
        // own parent (`app/`), so `use util::helper` resolves to a nonexistent
        // `app/util.neoc` — no edge. Adding the manifest re-roots main at the
        // package source root, where `util.neoc` lives, so the edge forms. The
        // file's own text never changes; only the manifest does.
        let tmp = TempDir::new("manifest_add");
        let util = tmp.write("src/main/neoc/util.neoc", "pub fn helper() {}\n");
        let main = tmp.write(
            "src/main/neoc/app/main.neoc",
            "use util::helper;\nfn go() { helper() }\n",
        );
        let mut idx = WorkspaceIndex::new(tmp.path());
        idx.set_document(util.clone(), 1, "pub fn helper() {}\n".to_string());
        idx.set_document(
            main.clone(),
            1,
            "use util::helper;\nfn go() { helper() }\n".to_string(),
        );

        // No manifest yet: main roots at `app/`, so `use util::helper` resolves to
        // a phantom `app/util.neoc` — not the real source-root `util.neoc`, so the
        // importer edge to `util` does not form and cross-file goto misses.
        assert_ne!(
            idx.import_targets(&main),
            vec![util.clone()],
            "without a manifest, `util::helper` does not resolve to the real module"
        );
        assert!(idx.importers_of(&util).is_empty());
        assert!(
            idx.imported_definition(&main, "helper").is_none(),
            "no cross-file goto before the manifest re-roots main"
        );

        // Introduce the manifest and re-root the subtree — no `.neoc` re-edit.
        tmp.write("neoc.toml", MANIFEST);
        let affected = idx.reroot_under(tmp.path());
        assert!(
            affected.contains(&main) && affected.contains(&util),
            "both modules under the manifest are re-summarised: {affected:?}"
        );

        // Cached resolution now reflects the new root: the importer edge forms and
        // goto/refs reach across files without either file being re-edited.
        assert_eq!(
            idx.import_targets(&main),
            vec![util.clone()],
            "with the manifest, `util::helper` resolves to the source-root module"
        );
        assert_eq!(idx.importers_of(&util), std::slice::from_ref(&main));
        assert_eq!(
            idx.imported_definition(&main, "helper")
                .map(|d| d.uri.clone()),
            Some(util.clone()),
            "cross-file goto picks up the re-rooted resolution"
        );
        let mut scope = idx.reference_scope(&main, "helper");
        scope.sort();
        let mut want = vec![util.clone(), main.clone()];
        want.sort();
        assert_eq!(scope, want, "references span the re-rooted package");
    }

    #[test]
    fn importer_graph_matches_a_non_canonical_query_path() {
        // Finding B: the importer graph is keyed canonically (matching the
        // compiler). A query by a non-canonical spelling of the same file — here
        // a redundant `dir/../` segment — must still hit, so refs/rename do not
        // miss under a `..`-containing or symlinked path.
        let tmp = TempDir::new("noncanon");
        let util = tmp.write("util.neoc", "pub fn add() {}\n");
        let main = tmp.write("main.neoc", "use util::add;\nfn go() { add() }\n");
        let mut idx = WorkspaceIndex::new(tmp.path());
        idx.set_document(util.clone(), 1, "pub fn add() {}\n".to_string());
        idx.set_document(
            main.clone(),
            1,
            "use util::add;\nfn go() { add() }\n".to_string(),
        );

        // A redundant-component spelling of `util.neoc`, routed through a real
        // subdirectory: `<dir>/sub/../util.neoc` — same file, different `Url`
        // text. The `..` must traverse a directory, not a file: POSIX refuses
        // `..` through a regular file (ENOTDIR), so `<dir>/util.neoc/../util.neoc`
        // can't be canonicalized on Linux. `sub/..` resolves on every platform.
        let util_path = util.to_file_path().unwrap();
        let parent = util_path.parent().unwrap();
        let sub = parent.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let noncanonical = sub.join("..").join("util.neoc");
        let noncanonical_uri = Url::from_file_path(&noncanonical).unwrap();
        assert_ne!(
            noncanonical_uri, util,
            "the redundant-component URL differs textually from the canonical one"
        );

        // Queried by the non-canonical spelling, the importer edge and refs match.
        assert_eq!(
            idx.importers_of(&noncanonical_uri),
            std::slice::from_ref(&main),
            "importers resolve through a `..`-containing path"
        );
        let mut scope = idx.reference_scope(&noncanonical_uri, "add");
        scope.sort();
        let mut want = vec![util.clone(), main.clone()];
        want.sort();
        assert_eq!(
            scope, want,
            "references/rename scope matches via the non-canonical path"
        );
    }
}

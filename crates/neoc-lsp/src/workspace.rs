//! Project-wide index for cross-file features — Phase 2 of the cross-file
//! resolution plan (`tasks/cross-file-resolution.md`).
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
use std::path::PathBuf;

use neoc_compiler::module_file;
use tower_lsp::lsp_types::{Range, SymbolKind, Url};

use crate::cst::{self, node_text};
use crate::position::byte_range;

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
}

impl FileSummary {
    /// The exported surface: a sorted key per `pub` declaration (its name and
    /// kind) and the sorted import paths, so the comparison is order-insensitive.
    /// Two summaries with equal surfaces are interchangeable to every dependent.
    /// `SymbolKind` is not `Ord`, so each export is rendered to a `name:kind`
    /// string that sorts and compares cleanly.
    fn surface(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let mut exports: Vec<String> = self
            .decls
            .iter()
            .filter(|d| d.exported)
            .map(|d| format!("{}:{:?}", d.name, d.kind))
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
    /// The workspace root, against which `use` paths resolve to files.
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
    /// Creates an empty index rooted at `root`. `use` paths resolve to files
    /// relative to this root, matching the compiler's project resolution.
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
        let hash = hash_text(&text);
        if let Some(existing) = self.summaries.get(&uri) {
            if existing.hash == hash {
                self.documents.insert(uri, Document { text, version });
                return SurfaceChange { changed: false };
            }
        }
        let old_surface = self.summaries.get(&uri).map(FileSummary::surface);
        self.retract(&uri);

        let summary = summarize(&text, hash);
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
            if let Some(target) = self.resolve_import(use_path) {
                self.importers.entry(target).or_default().push(uri.clone());
            }
        }
        let new_surface = summary.surface();
        let changed = old_surface.as_ref() != Some(&new_surface);
        self.summaries.insert(uri.clone(), summary);
        self.documents.insert(uri, Document { text, version });
        SurfaceChange { changed }
    }

    /// Removes a document and all its contributions to the maps. Returns whether
    /// the file had been indexed.
    pub fn remove_document(&mut self, uri: &Url) -> bool {
        let had = self.summaries.contains_key(uri);
        self.retract(uri);
        self.documents.remove(uri);
        self.summaries.remove(uri);
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
                if let Some(target) = self.resolve_import(use_path) {
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
    /// via the compiler's [`module_file`] rule. `None` for a single-segment path
    /// (no module qualifier) or a path that maps to no file under the root.
    fn resolve_import(&self, use_path: &[String]) -> Option<Url> {
        let file = module_file(&self.root, use_path)?;
        Url::from_file_path(file).ok()
    }

    /// The workspace root the index resolves `use` paths against. Background
    /// discovery walks this directory to seed unopened files.
    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Every declaration of `name` across the project. Empty slice when none.
    #[must_use]
    pub fn find_decls(&self, name: &str) -> &[DeclLoc] {
        self.decls.get(name).map_or(&[], Vec::as_slice)
    }

    /// The `use` path that imports `item` from the file `file` — the reverse of
    /// the compiler's `module_file` rule. `<root>/math/vec.neoc` + `dot` yields
    /// `math::vec::dot`; `<root>/util.neoc` + `add` yields `util::add`. `None`
    /// when `file` is not under the index root. Used to build the `use` insertion
    /// an auto-import completion offers.
    #[must_use]
    pub fn use_path_for(&self, file: &Url, item: &str) -> Option<String> {
        let path = file.to_file_path().ok()?;
        let rel = path.strip_prefix(&self.root).ok()?.with_extension("");
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
        let targets = self.import_targets(from_uri);
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
        let decls = self.find_decls(name);
        let targets = self.import_targets(from_uri);
        // The declaration this use resolves to: one in the current file, else a
        // `pub` one in a module the current file imports.
        let def = decls.iter().find(|d| &d.uri == from_uri).or_else(|| {
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
        if !scope.iter().any(|u| u == from_uri) {
            scope.push(from_uri.clone());
        }
        scope
    }

    /// The files `uri`'s `use` declarations resolve to — the modules it imports
    /// from. The forward direction of [`Self::importers_of`]; used to scope a
    /// cross-file goto to a module the current file actually imports.
    #[must_use]
    pub fn import_targets(&self, uri: &Url) -> Vec<Url> {
        let Some(summary) = self.summaries.get(uri) else {
            return Vec::new();
        };
        summary
            .imports
            .iter()
            .filter_map(|path| self.resolve_import(path))
            .collect()
    }

    /// The files whose `use` imports resolve to `uri` — its direct dependents.
    #[must_use]
    pub fn importers_of(&self, uri: &Url) -> &[Url] {
        self.importers.get(uri).map_or(&[], Vec::as_slice)
    }

    /// The current text of a document, if indexed.
    // Used by the goto/references slices of Phase 3 (see cross-file-resolution.md).
    #[allow(dead_code)]
    #[must_use]
    pub fn document_text(&self, uri: &Url) -> Option<&str> {
        self.documents.get(uri).map(|d| d.text.as_str())
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
/// range, `pub`) and each `use` path.
fn summarize(source: &str, hash: u64) -> FileSummary {
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
                });
            }
        }
    }
    FileSummary {
        hash,
        decls,
        imports,
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

    use super::*;

    fn uri(root: &Path, name: &str) -> Url {
        Url::from_file_path(root.join(name)).unwrap()
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
    fn use_path_for_reverses_the_module_file_rule() {
        let root = Path::new("/proj");
        let idx = WorkspaceIndex::new(root);
        assert_eq!(
            idx.use_path_for(&uri(root, "util.neoc"), "add").as_deref(),
            Some("util::add"),
            "a top-level sibling"
        );
        assert_eq!(
            idx.use_path_for(&uri(root, "math/vec.neoc"), "dot")
                .as_deref(),
            Some("math::vec::dot"),
            "a nested module"
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
}

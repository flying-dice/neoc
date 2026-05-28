//! Diagnostic computation — the pure mapping from neoc source to LSP
//! `Diagnostic`s, both single-file and project-resolved. Free of the server
//! transport, so it is unit-testable without a running server.

use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range, Url,
};

use neoc_compiler::{
    ByteSpan, Diagnostic as CompilerDiagnostic, Stage, Target, resolution_root, transpile,
};

use crate::WorkspaceIndex;
use crate::lints;
use crate::position::byte_to_position;

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

fn to_lsp(source: &str, uri: Option<&Url>, diagnostic: &CompilerDiagnostic) -> Diagnostic {
    let range = diagnostic
        .span
        .map_or_else(|| fallback_range(source), |s| span_range(source, s));
    // Labels are byte ranges in this same source, so each related-information
    // entry points back into the document being diagnosed. Needs the document
    // URI, so an untitled single-file buffer drops them.
    let related_information = uri.filter(|_| !diagnostic.labels.is_empty()).map(|uri| {
        diagnostic
            .labels
            .iter()
            .map(|label| DiagnosticRelatedInformation {
                location: Location::new(uri.clone(), span_range(source, label.span)),
                message: label.message.clone(),
            })
            .collect()
    });
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some(format!("neoc:{}", stage_label(diagnostic.stage))),
        message: diagnostic.message.clone(),
        related_information,
        ..Default::default()
    }
}

/// The diagnostics for `source`: hard correctness errors from the compiler plus
/// warning-level lints.
///
/// Pure and free of I/O, so it can be unit-tested without a running server.
#[must_use]
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    // Parse and type diagnostics are target-independent; emit against the
    // default target so the editor still surfaces any backend complaint.
    let mut diagnostics: Vec<Diagnostic> = transpile(source, Target::default())
        .diagnostics
        .iter()
        .map(|d| to_lsp(source, None, d))
        .collect();
    diagnostics.extend(lints::lints(source));
    diagnostics
}

/// The directory a file's `use` paths resolve against: the project source root
/// when `focus` lies under a `neoc.toml`, else `focus`'s own directory — the
/// single-file fallback ([`resolution_root`]). Per-file, not index-wide, so a
/// monorepo opened at its top resolves each file against its own nearest
/// project. A malformed manifest degrades to the fallback without failing the
/// request (`resolution_root` swallows the `Err`).
#[must_use]
fn focus_root(focus: &Path) -> PathBuf {
    resolution_root(focus)
}

/// Project-aware diagnostics for `uri`: the file is checked against the
/// workspace's other modules (open buffers authoritative), so its `use`
/// imports resolve and a non-entry module no longer reports phantom
/// `unresolved module` errors. The resolution root is the file's own project
/// source root ([`focus_root`]), discovered from `neoc.toml`, not the folder
/// the editor opened. Falls back to single-file [`diagnose`] for an untitled
/// buffer (no file path) or when the focus is otherwise unresolvable.
#[must_use]
pub fn diagnose_in_project(index: &WorkspaceIndex, uri: &Url, source: &str) -> Vec<Diagnostic> {
    let Ok(focus) = uri.to_file_path() else {
        return diagnose(source);
    };
    // The handler's live buffer is authoritative and may be a step ahead of the
    // index, so the focus carries the current text explicitly.
    let mut sources = index.project_sources();
    sources.insert(focus.clone(), source.to_string());
    let root = focus_root(&focus);
    let mut diagnostics: Vec<Diagnostic> =
        neoc_compiler::check_module_in_project(&root, &sources, &focus)
            .iter()
            .map(|d| to_lsp(source, Some(uri), d))
            .collect();
    diagnostics.extend(lints::lints(source));
    diagnostics
}

/// Project-aware diagnostics for several documents at once — the batch form of
/// [`diagnose_in_project`], for re-diagnosing a changed module and all its
/// importers. The project is parsed once and shared across every focus
/// ([`neoc_compiler::check_modules_in_project`]), so the cost is one universe
/// parse rather than one per file. Each result is keyed by its `Url`; a `uri`
/// with no file path (an untitled buffer) is skipped.
#[must_use]
pub fn diagnose_many_in_project(
    index: &WorkspaceIndex,
    uris: &[Url],
) -> Vec<(Url, Vec<Diagnostic>)> {
    use std::collections::HashMap;

    let sources = index.project_sources();
    // Pair each diagnosable uri with its file-path focus, preserving the uri to
    // map results and label locations back.
    let foci: Vec<(Url, PathBuf)> = uris
        .iter()
        .filter_map(|uri| uri.to_file_path().ok().map(|path| (uri.clone(), path)))
        .collect();
    // Each file resolves against its own project root, so a monorepo with
    // several `neoc.toml` under one opened folder checks each file correctly.
    // Group foci by root: one universe parse per root, not one per file.
    let mut by_root: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (_, path) in &foci {
        by_root
            .entry(focus_root(path))
            .or_default()
            .push(path.clone());
    }
    let mut results: HashMap<PathBuf, Vec<CompilerDiagnostic>> = HashMap::new();
    for (root, focus_paths) in &by_root {
        results.extend(neoc_compiler::check_modules_in_project(
            root,
            &sources,
            focus_paths,
        ));
    }
    foci.into_iter()
        .map(|(uri, path)| {
            let source = sources.get(&path).map_or("", String::as_str);
            let mut diagnostics: Vec<Diagnostic> = results
                .remove(&path)
                .unwrap_or_default()
                .iter()
                .map(|d| to_lsp(source, Some(&uri), d))
                .collect();
            diagnostics.extend(lints::lints(source));
            (uri, diagnostics)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A unique on-disk temp directory, removed on drop. `neoc.toml` discovery
    /// reads the manifest from disk, so these tests need real files.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "neoc_lsp_{}_{}_{}",
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

        /// Writes `body` to `rel` under the temp dir, creating parents.
        fn write(&self, rel: &str, body: &str) -> PathBuf {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, body).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MANIFEST: &str = "[package]\nname = \"app\"\ntarget = \"lua\"\n";

    /// Builds an index rooted at `root` holding the two project sources, then
    /// returns `main.neoc`'s project diagnostics. The index root is the caller's
    /// choice (package root or monorepo root) — resolution must not depend on it.
    fn diagnose_main(
        index_root: &Path,
        main: &Path,
        main_src: &str,
        lib: &Path,
    ) -> Vec<Diagnostic> {
        let mut index = WorkspaceIndex::new(index_root);
        index.set_document(
            Url::from_file_path(lib).unwrap(),
            1,
            std::fs::read_to_string(lib).unwrap(),
        );
        let uri = Url::from_file_path(main).unwrap();
        index.set_document(uri.clone(), 1, main_src.to_string());
        diagnose_in_project(&index, &uri, main_src)
    }

    #[test]
    fn sibling_use_resolves_under_neoc_toml_regardless_of_index_root() {
        // A package laid out per PROJECT.md §2: sources under src/main/neoc.
        let tmp = TempDir::new("pkg");
        tmp.write("neoc.toml", MANIFEST);
        let lib = tmp.write(
            "src/main/neoc/sqlite.neoc",
            "pub fn open() -> number { 1 }\n",
        );
        let main_src = "use sqlite::open;\nfn go() -> number { open() }\n";
        let main = tmp.write("src/main/neoc/main.neoc", main_src);

        // Index rooted at the *package* root.
        let pkg = diagnose_main(tmp.path(), &main, main_src, &lib);
        assert!(
            !pkg.iter().any(|d| d.message.contains("unresolved module")),
            "sibling `use` resolves with index at package root: {pkg:?}"
        );

        // Index rooted at a *monorepo* parent above the package.
        let monorepo = tmp.path().parent().unwrap();
        let mono = diagnose_main(monorepo, &main, main_src, &lib);
        assert!(
            !mono.iter().any(|d| d.message.contains("unresolved module")),
            "sibling `use` resolves with index at monorepo root: {mono:?}"
        );
        assert!(
            !mono
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
            "no errors at all: {mono:?}"
        );
    }

    #[test]
    fn monorepo_resolves_each_file_against_its_own_project() {
        // Two packages under one monorepo root, each with its own neoc.toml and
        // its own sibling module. A batch diagnose rooted at the top must check
        // each file against its own project (per-file root).
        let tmp = TempDir::new("mono");
        tmp.write("a/neoc.toml", MANIFEST);
        let a_lib = tmp.write("a/src/main/neoc/lib.neoc", "pub fn a() -> number { 1 }\n");
        let a_main_src = "use lib::a;\nfn go() -> number { a() }\n";
        let a_main = tmp.write("a/src/main/neoc/main.neoc", a_main_src);

        tmp.write("b/neoc.toml", MANIFEST);
        let b_lib = tmp.write("b/src/main/neoc/lib.neoc", "pub fn b() -> number { 2 }\n");
        let b_main_src = "use lib::b;\nfn go() -> number { b() }\n";
        let b_main = tmp.write("b/src/main/neoc/main.neoc", b_main_src);

        let mut index = WorkspaceIndex::new(tmp.path());
        for (path, src) in [
            (&a_lib, std::fs::read_to_string(&a_lib).unwrap()),
            (&a_main, a_main_src.to_string()),
            (&b_lib, std::fs::read_to_string(&b_lib).unwrap()),
            (&b_main, b_main_src.to_string()),
        ] {
            index.set_document(Url::from_file_path(path).unwrap(), 1, src);
        }
        let uris = vec![
            Url::from_file_path(&a_main).unwrap(),
            Url::from_file_path(&b_main).unwrap(),
        ];
        let batch = diagnose_many_in_project(&index, &uris);
        for (uri, diags) in &batch {
            assert!(
                !diags
                    .iter()
                    .any(|d| d.message.contains("unresolved module")),
                "each file resolves against its own project: {uri} {diags:?}"
            );
        }
    }

    #[test]
    fn no_manifest_falls_back_to_file_directory() {
        // A lone file with no neoc.toml resolves siblings as before: the `use`
        // target lives next to the importing file.
        let tmp = TempDir::new("loose");
        let lib = tmp.write("util.neoc", "pub fn add() -> number { 1 }\n");
        let main_src = "use util::add;\nfn go() -> number { add() }\n";
        let main = tmp.write("main.neoc", main_src);

        let diags = diagnose_main(tmp.path(), &main, main_src, &lib);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("unresolved module")),
            "fallback resolves a sibling next to the file: {diags:?}"
        );
    }

    #[test]
    fn malformed_manifest_does_not_crash_and_falls_back() {
        // A broken neoc.toml must not panic the request; resolution degrades to
        // the file-directory fallback (sources sit beside the file here).
        let tmp = TempDir::new("badtoml");
        tmp.write("neoc.toml", "this is = = not toml");
        let lib = tmp.write("util.neoc", "pub fn add() -> number { 1 }\n");
        let main_src = "use util::add;\nfn go() -> number { add() }\n";
        let main = tmp.write("main.neoc", main_src);

        // Must return (not panic) and resolve via the fallback root.
        let diags = diagnose_main(tmp.path(), &main, main_src, &lib);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("unresolved module")),
            "a malformed manifest degrades to the fallback: {diags:?}"
        );
    }

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
}

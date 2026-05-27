//! Diagnostic computation — the pure mapping from neoc source to LSP
//! `Diagnostic`s, both single-file and project-resolved. Free of the server
//! transport, so it is unit-testable without a running server.

use std::path::PathBuf;

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range, Url,
};

use neoc_compiler::{ByteSpan, Diagnostic as CompilerDiagnostic, Stage, Target, transpile};

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

/// Project-aware diagnostics for `uri`: the file is checked against the
/// workspace's other modules (open buffers authoritative), so its `use`
/// imports resolve and a non-entry module no longer reports phantom
/// `unresolved module` errors. Falls back to single-file [`diagnose`] for an
/// untitled buffer (no file path) or when the focus is otherwise unresolvable.
#[must_use]
pub fn diagnose_in_project(index: &WorkspaceIndex, uri: &Url, source: &str) -> Vec<Diagnostic> {
    let Ok(focus) = uri.to_file_path() else {
        return diagnose(source);
    };
    // The handler's live buffer is authoritative and may be a step ahead of the
    // index, so the focus carries the current text explicitly.
    let mut sources = index.project_sources();
    sources.insert(focus.clone(), source.to_string());
    let mut diagnostics: Vec<Diagnostic> =
        neoc_compiler::check_module_in_project(index.root(), &sources, &focus)
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
    let sources = index.project_sources();
    // Pair each diagnosable uri with its file-path focus, preserving the uri to
    // map results and label locations back.
    let foci: Vec<(Url, PathBuf)> = uris
        .iter()
        .filter_map(|uri| uri.to_file_path().ok().map(|path| (uri.clone(), path)))
        .collect();
    let focus_paths: Vec<PathBuf> = foci.iter().map(|(_, path)| path.clone()).collect();
    let mut results = neoc_compiler::check_modules_in_project(index.root(), &sources, &focus_paths);
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

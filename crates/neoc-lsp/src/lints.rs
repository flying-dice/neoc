//! Lints: warning-level diagnostics for code that is valid but suspect —
//! neoc's equivalent of the clippy / editor-suggestion layer, distinct from the
//! hard correctness errors `neoc_compiler::transpile` reports.
//!
//! Each lint is a pure function over the source; [`lints`] runs them all. They
//! carry `WARNING` severity and the `UNNECESSARY` tag where the editor should
//! grey the code out, and pair with the matching code action for the fix.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag};

use crate::code_action;

/// The lint diagnostics for `source`.
#[must_use]
pub fn lints(source: &str) -> Vec<Diagnostic> {
    unused_import_lints(source)
}

/// One `unused import` warning per import whose bound name is never used,
/// tagged `UNNECESSARY` so the editor greys it out. The remove-unused-import
/// code action is the fix.
fn unused_import_lints(source: &str) -> Vec<Diagnostic> {
    code_action::unused_imports(source)
        .into_iter()
        .map(|unused| Diagnostic {
            range: unused.range,
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("neoc:lint".to_string()),
            message: format!("unused import `{}`", unused.name),
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_on_an_unused_import() {
        let src = "use std::fs;\nuse std::io;\nfn main() -> number { io::run() }\n";
        let diags = lints(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        let d = &diags[0];
        assert_eq!(d.message, "unused import `fs`");
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn no_lints_when_all_imports_are_used() {
        let src = "use std::io;\nfn main() -> number { io::run() }\n";
        assert!(lints(src).is_empty());
    }
}

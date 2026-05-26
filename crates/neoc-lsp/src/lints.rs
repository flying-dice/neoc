//! The lint framework and its lints.
//!
//! Lints are warning-level diagnostics for code that is valid but suspect —
//! neoc's clippy / editor-suggestion layer, distinct from the hard correctness
//! errors `neoc_compiler::transpile` reports. Each lint is a [`Lint`]: a named,
//! severity-carrying check over a [`LintContext`]. [`lints`] builds the context
//! once, runs every registered lint, and maps each [`Finding`] to an LSP
//! diagnostic carrying the lint's name as its `code`.
//!
//! Adding a lint is one `impl Lint` plus one entry in [`REGISTRY`]. The context
//! exposes the document's parsed views — CST eagerly, the AST module lazily —
//! computed once and shared, so a lint pays only for what it reads. New typed
//! views (symbol table, inferred types) join the context as the lints that need
//! them land.

use std::cell::OnceCell;

use neoc_ast::{ExprKind, ItemKind};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Range};
use tree_sitter::Tree;

use crate::code_action;
use crate::cst;
use crate::position::byte_range;

/// The parsed views of a document a lint may need, computed once per pass and
/// shared across lints. The CST is eager (cheap); the AST module is parsed
/// lazily on first use.
pub struct LintContext<'s> {
    source: &'s str,
    tree: Tree,
    module: OnceCell<neoc_ast::Module>,
}

impl<'s> LintContext<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            source,
            tree: cst::parse(source),
            module: OnceCell::new(),
        }
    }

    /// The document text.
    #[must_use]
    pub fn source(&self) -> &str {
        self.source
    }

    /// The tree-sitter concrete syntax tree.
    #[must_use]
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The typed AST module, parsed on first use and shared thereafter.
    #[must_use]
    pub fn module(&self) -> &neoc_ast::Module {
        self.module
            .get_or_init(|| neoc_parser::parse(self.source).module)
    }
}

/// One lint finding: where it is, what to say, and an optional editor tag (e.g.
/// `Unnecessary` to grey out dead code).
pub struct Finding {
    pub range: Range,
    pub message: String,
    pub tag: Option<DiagnosticTag>,
}

/// A lint: a named check over a document, run for its [`Finding`]s.
pub trait Lint {
    /// Stable identifier, surfaced as the diagnostic's `code` (e.g.
    /// `unused_import`) so users can recognise and, in future, configure it.
    fn name(&self) -> &'static str;

    /// The severity its findings report at. Warning by default.
    fn severity(&self) -> DiagnosticSeverity {
        DiagnosticSeverity::WARNING
    }

    /// The findings for `cx`.
    fn check(&self, cx: &LintContext) -> Vec<Finding>;
}

/// Every registered lint. Add a lint here once its `impl Lint` exists.
const REGISTRY: &[&dyn Lint] = &[&UnusedImport, &NeedlessReturn];

/// The lint diagnostics for `source`: every registered lint's findings, mapped
/// to LSP diagnostics tagged with the lint name.
#[must_use]
pub fn lints(source: &str) -> Vec<Diagnostic> {
    let cx = LintContext::new(source);
    let mut out = Vec::new();
    for lint in REGISTRY {
        for finding in lint.check(&cx) {
            out.push(Diagnostic {
                range: finding.range,
                severity: Some(lint.severity()),
                code: Some(NumberOrString::String(lint.name().to_string())),
                source: Some("neoc".to_string()),
                message: finding.message,
                tags: finding.tag.map(|tag| vec![tag]),
                ..Default::default()
            });
        }
    }
    out
}

/// `unused_import`: an import whose bound name is never used. Pairs with the
/// remove-unused-import code action. Works off the CST.
struct UnusedImport;

impl Lint for UnusedImport {
    fn name(&self) -> &'static str {
        "unused_import"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        code_action::unused_imports_in(cx.tree(), cx.source())
            .into_iter()
            .map(|unused| Finding {
                range: unused.range,
                message: format!("unused import `{}`", unused.name),
                tag: Some(DiagnosticTag::UNNECESSARY),
            })
            .collect()
    }
}

/// `needless_return`: a `return` in a function body's tail position, where the
/// expression is already the body's value. Works off the AST so it sees only
/// the genuine tail (a `return` inside a nested block is a real early return).
struct NeedlessReturn;

impl Lint for NeedlessReturn {
    fn name(&self) -> &'static str {
        "needless_return"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let mut out = Vec::new();
        for item in &cx.module().items {
            match &item.kind {
                ItemKind::Fn(f) => tail_return(&f.body, cx.source(), &mut out),
                ItemKind::Impl(block) => {
                    for method in &block.items {
                        tail_return(&method.body, cx.source(), &mut out);
                    }
                }
                ItemKind::Trait(t) => {
                    for trait_item in &t.items {
                        if let Some(body) = &trait_item.default {
                            tail_return(body, cx.source(), &mut out);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Pushes a finding when `body`'s tail value is a `return`.
fn tail_return(body: &neoc_ast::Block, source: &str, out: &mut Vec<Finding>) {
    if let Some(value) = &body.value
        && matches!(value.kind, ExprKind::Return(_))
    {
        out.push(Finding {
            range: byte_range(source, value.span.start, value.span.end),
            message: "unneeded `return`; a function body's last expression is its value"
                .to_string(),
            tag: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(source: &str) -> Vec<String> {
        lints(source).into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn warns_on_an_unused_import_with_a_code_and_tag() {
        let src = "use std::fs;\nuse std::io;\nfn main() -> number { io::run() }\n";
        let import_lints: Vec<_> = lints(src)
            .into_iter()
            .filter(|d| d.code == Some(NumberOrString::String("unused_import".into())))
            .collect();
        assert_eq!(import_lints.len(), 1);
        let d = &import_lints[0];
        assert_eq!(d.message, "unused import `fs`");
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn warns_on_a_tail_return() {
        let src = "fn answer() -> number { return 42 }\n";
        let msgs = messages(src);
        assert!(
            msgs.iter().any(|m| m.contains("unneeded `return`")),
            "{msgs:?}"
        );
    }

    #[test]
    fn an_early_return_in_a_nested_block_is_not_flagged() {
        // The `return` exits the function from inside the `if`; the body's tail
        // is `0`, so nothing is needless.
        let src = "fn pick(c: bool) -> number { if c { return 1 } 0 }\n";
        assert!(
            messages(src).iter().all(|m| !m.contains("unneeded")),
            "{:?}",
            messages(src)
        );
    }

    #[test]
    fn clean_source_has_no_lints() {
        let src = "use std::io;\nfn main() -> number { io::run() }\n";
        assert!(messages(src).is_empty());
    }

    #[test]
    fn context_shares_the_lazy_module() {
        let cx = LintContext::new("fn main() -> number { 1 }\n");
        assert!(std::ptr::eq(cx.module(), cx.module()));
    }
}

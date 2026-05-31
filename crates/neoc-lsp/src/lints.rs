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

use neoc_ast::{ExprKind, ItemKind, Visibility};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Range};
use tree_sitter::{Node, Tree};

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
const REGISTRY: &[&dyn Lint] = &[
    &UnusedImport,
    &UnusedDeclaration,
    &UnusedLet,
    &UnusedParam,
    &NeedlessReturn,
    &BoolComparison,
    &NeedlessParens,
];

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

/// `unused_let`: a `let` binding that is never read. Scope-aware — it asks
/// `neoc_types::resolution` whether the binding has any use, so a shadowed-then-
/// unused binding is caught precisely. A `_`-prefixed name opts out (the
/// convention for a deliberately-unused binding). Scoped to `let` for now;
/// unused parameters and shadowing wait on the RFC.
struct UnusedLet;

impl Lint for UnusedLet {
    fn name(&self) -> &'static str {
        "unused_let"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let resolution = neoc_types::resolution(cx.module());
        resolution
            .bindings()
            .iter()
            .filter(|b| b.kind == neoc_types::BindingKind::Let)
            .filter(|b| !b.name.starts_with('_'))
            .filter(|b| !resolution.is_used(b.id))
            .map(|b| Finding {
                range: byte_range(cx.source(), b.def.0, b.def.1),
                message: format!(
                    "unused `let` binding `{}`; prefix with `_` to silence",
                    b.name
                ),
                tag: Some(DiagnosticTag::UNNECESSARY),
            })
            .collect()
    }
}

/// `unused_param`: a function parameter that is never read. Like Rust's
/// `unused_variables`, it fires uniformly — free functions, inherent methods,
/// and trait-impl methods alike (a trait forces the parameter, but the idiom is
/// to silence it, not exempt it) — with a `_`-prefix opt-out. `self` is never
/// flagged (it is not recorded as a binding). Scope-aware via the resolution
/// map, so it pays no attention to spelling.
struct UnusedParam;

impl Lint for UnusedParam {
    fn name(&self) -> &'static str {
        "unused_param"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let resolution = neoc_types::resolution(cx.module());
        resolution
            .bindings()
            .iter()
            .filter(|b| b.kind == neoc_types::BindingKind::Param)
            .filter(|b| !b.name.starts_with('_'))
            .filter(|b| !resolution.is_used(b.id))
            .map(|b| Finding {
                range: byte_range(cx.source(), b.def.0, b.def.1),
                message: format!("unused parameter `{}`; prefix with `_` to silence", b.name),
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

/// `unused_declaration`: a private top-level `fn`/`const`/`struct`/`enum`/`type`
/// whose name is referenced nowhere else in the file — dead code. Only private
/// items, since a `pub` one may be used from another file. `main` is exempt.
struct UnusedDeclaration;

impl Lint for UnusedDeclaration {
    fn name(&self) -> &'static str {
        "unused_declaration"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let mut out = Vec::new();
        for item in &cx.module().items {
            if item.vis != Visibility::Private {
                continue;
            }
            let Some((kind, name)) = decl_kind_and_name(&item.kind) else {
                continue;
            };
            if name == "main" {
                continue;
            }
            // Resolution is name-based, so the only safe direction is a false
            // negative: if the name appears solely at its declaration it is
            // unused; a same-named identifier elsewhere suppresses the lint.
            if cst::identifier_nodes(cx.tree(), cx.source(), name).len() <= 1 {
                out.push(Finding {
                    range: byte_range(cx.source(), item.span.start, item.span.end),
                    message: format!("unused {kind} `{name}`"),
                    tag: Some(DiagnosticTag::UNNECESSARY),
                });
            }
        }
        out
    }
}

/// The kind label and declared name of a named top-level item, if it is one.
fn decl_kind_and_name(kind: &ItemKind) -> Option<(&'static str, &str)> {
    match kind {
        ItemKind::Fn(f) => Some(("function", &f.sig.name)),
        ItemKind::Const(c) => Some(("constant", &c.name)),
        ItemKind::Struct(s) => Some(("struct", &s.name)),
        ItemKind::Enum(e) => Some(("enum", &e.name)),
        ItemKind::TypeAlias(t) => Some(("type", &t.name)),
        _ => None,
    }
}

/// `bool_comparison`: comparing to a boolean literal (`x == true`, `x != false`)
/// — the value should be used directly. Found on the CST.
struct BoolComparison;

impl Lint for BoolComparison {
    fn name(&self) -> &'static str {
        "bool_comparison"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let mut out = Vec::new();
        let source = cx.source();
        walk_named(cx.tree().root_node(), &mut |node| {
            if node.kind() != "binary_expression" {
                return;
            }
            let is_eq = node
                .child_by_field_name("operator")
                .is_some_and(|op| matches!(op.kind(), "==" | "!="));
            let has_bool = ["left", "right"]
                .iter()
                .filter_map(|f| node.child_by_field_name(f))
                .any(|operand| operand.kind() == "boolean_literal");
            if is_eq && has_bool {
                out.push(Finding {
                    range: byte_range(source, node.start_byte(), node.end_byte()),
                    message: "comparison with a boolean literal; use the value itself".to_string(),
                    tag: None,
                });
            }
        });
        out
    }
}

/// `needless_parens`: parentheses around an expression that never needs them — a
/// name, literal, call, or already-parenthesised expression. The precedence-
/// dependent cases (`(a + b) * c`) are left alone.
struct NeedlessParens;

/// Expression kinds that are atomic — parentheses around them are always
/// redundant, regardless of the surrounding precedence.
const PRIMARY: &[&str] = &[
    "identifier",
    "path",
    "integer_literal",
    "float_literal",
    "string_literal",
    "boolean_literal",
    "unit_expression",
    "call_expression",
    "parenthesized_expression",
];

impl Lint for NeedlessParens {
    fn name(&self) -> &'static str {
        "needless_parens"
    }

    fn check(&self, cx: &LintContext) -> Vec<Finding> {
        let mut out = Vec::new();
        let source = cx.source();
        walk_named(cx.tree().root_node(), &mut |node| {
            if node.kind() == "parenthesized_expression"
                && let Some(inner) = node.named_child(0)
                && PRIMARY.contains(&inner.kind())
            {
                out.push(Finding {
                    range: byte_range(source, node.start_byte(), node.end_byte()),
                    message: "redundant parentheses".to_string(),
                    tag: None,
                });
            }
        });
        out
    }
}

/// Visits every named node in the tree, depth-first.
fn walk_named(node: Node, visit: &mut impl FnMut(Node)) {
    visit(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk_named(child, visit);
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
    fn warns_on_an_unused_let_but_not_a_used_or_underscored_one() {
        let src = "fn f() -> number {\n  let unused = 1;\n  let used = 2;\n  let _ignored = 3;\n  used\n}\n";
        let unused_lets: Vec<_> = lints(src)
            .into_iter()
            .filter(|d| d.code == Some(NumberOrString::String("unused_let".into())))
            .collect();
        assert_eq!(unused_lets.len(), 1, "only `unused` should fire");
        assert!(
            unused_lets[0]
                .message
                .contains("unused `let` binding `unused`")
        );
        assert_eq!(unused_lets[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn unused_let_is_scope_precise_for_a_shadow() {
        // The inner `x` is read; the outer `x` is shadowed before any use, so it
        // is the unused one. Name-based matching could not tell them apart.
        let src = "fn f() -> number {\n  let x = 1;\n  let x = 2;\n  x\n}\n";
        let msgs = messages(src);
        let hits: Vec<_> = msgs.iter().filter(|m| m.contains("unused `let`")).collect();
        assert_eq!(hits.len(), 1, "exactly one unused binding: {msgs:?}");
    }

    fn unused_params(source: &str) -> Vec<String> {
        lints(source)
            .into_iter()
            .filter(|d| d.code == Some(NumberOrString::String("unused_param".into())))
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn warns_on_an_unused_param_with_underscore_and_self_exempt() {
        // `unused` fires; `_skip` opts out; `used` is read; `self` is never a
        // candidate. Mirrors Rust's `unused_variables`.
        let src = "struct P {}\nimpl P {\n  fn m(self, unused: number, _skip: number, used: number) -> number {\n    used\n  }\n}\n";
        let hits = unused_params(src);
        assert_eq!(hits.len(), 1, "only `unused` fires: {hits:?}");
        assert!(hits[0].contains("unused parameter `unused`"));
    }

    #[test]
    fn warns_on_an_unused_trait_impl_param() {
        // Rust warns even when a trait forces the parameter — the fix is `_`,
        // not an exemption. So the impl's unused `x` fires.
        let src = "trait T {\n  fn f(self, x: number) -> number;\n}\nstruct P {}\nimpl T for P {\n  fn f(self, x: number) -> number { 0 }\n}\n";
        let hits = unused_params(src);
        assert_eq!(hits.len(), 1, "the impl's unused `x` fires: {hits:?}");
        assert!(hits[0].contains("unused parameter `x`"));
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
    fn flags_an_unused_private_function() {
        let src = "fn helper() -> number { 1 }\nfn main() -> number { 2 }\n";
        assert!(
            messages(src)
                .iter()
                .any(|m| m == "unused function `helper`"),
            "{:?}",
            messages(src)
        );
    }

    #[test]
    fn a_used_or_public_declaration_is_not_unused() {
        let used = "fn helper() -> number { 1 }\nfn main() -> number { helper() }\n";
        assert!(
            messages(used)
                .iter()
                .all(|m| !m.contains("unused function"))
        );
        let public = "pub fn helper() -> number { 1 }\nfn main() -> number { 2 }\n";
        assert!(messages(public).iter().all(|m| !m.contains("helper")));
    }

    #[test]
    fn flags_comparison_with_a_boolean_literal() {
        let src = "fn check(b: bool) -> bool { b == true }\nfn main() -> bool { check(true) }\n";
        assert!(
            messages(src).iter().any(|m| m.contains("boolean literal")),
            "{:?}",
            messages(src)
        );
    }

    #[test]
    fn flags_redundant_parens_but_not_a_sum() {
        let redundant = "fn one() -> number { (1) }\nfn main() -> number { one() }\n";
        assert!(
            messages(redundant)
                .iter()
                .any(|m| m == "redundant parentheses")
        );
        let needed = "fn sum(a: number, b: number) -> number { (a + b) }\nfn main() -> number { sum(1, 2) }\n";
        assert!(
            messages(needed)
                .iter()
                .all(|m| m != "redundant parentheses"),
            "{:?}",
            messages(needed)
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

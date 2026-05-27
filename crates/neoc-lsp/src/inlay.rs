//! Inlay hints: inline parameter-name and inferred-type annotations.
//!
//! Two kinds. At each free-function call, every argument gets a `name:` hint
//! naming the parameter it fills, read from the callee's declaration (omitted
//! when the argument is exactly that name). Each `let` binding without a written
//! type gets a `: type` hint after the name, from `neoc_types`' inference. Method
//! calls are not handled yet.

use neoc_types::{ExprType, Ty};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};
use tree_sitter::{Node, Tree};

use crate::cst::{self, node_text};
use crate::position::byte_to_position;

/// The inlay hints for `source`, in document order.
#[must_use]
pub fn inlay_hints(source: &str) -> Vec<InlayHint> {
    let tree = cst::parse(source);
    let mut out = Vec::new();
    collect(tree.root_node(), source, &tree, &mut out);

    let types = neoc_types::expression_types(&neoc_parser::parse(source).module);
    collect_let_types(tree.root_node(), source, &types, &mut out);

    out.sort_by_key(|hint| (hint.position.line, hint.position.character));
    out
}

/// Walks for `let` bindings without a written type, emitting a `: type` hint
/// after each binding name from the inferred type of its value.
fn collect_let_types(node: Node, source: &str, types: &[ExprType], out: &mut Vec<InlayHint>) {
    if node.kind() == "let_statement" {
        let_type_hint(node, source, types, out);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_let_types(child, source, types, out);
    }
}

fn let_type_hint(let_stmt: Node, source: &str, types: &[ExprType], out: &mut Vec<InlayHint>) {
    // Already written, or a destructuring pattern with no single name to tag.
    if let_stmt.child_by_field_name("type").is_some() {
        return;
    }
    let Some(pattern) = let_stmt.child_by_field_name("pattern") else {
        return;
    };
    if pattern.kind() != "binding_pattern" {
        return;
    }
    let Some(value) = let_stmt.child_by_field_name("value") else {
        return;
    };
    let Some(ty) = type_of_value(types, value.start_byte(), value.end_byte()) else {
        return;
    };
    // An unresolved type would only add noise.
    if matches!(ty, Ty::Unknown) {
        return;
    }
    out.push(InlayHint {
        position: byte_to_position(source, pattern.end_byte()),
        label: InlayHintLabel::String(format!(": {ty}")),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(false),
        data: None,
    });
}

/// The inferred type of the value spanning `[start, end)` — the narrowest
/// recorded expression that fully contains the value, which is the value
/// expression itself.
fn type_of_value(types: &[ExprType], start: usize, end: usize) -> Option<Ty> {
    types
        .iter()
        .filter(|t| t.start <= start && t.end >= end)
        .min_by_key(|t| t.end - t.start)
        .map(|t| t.ty.clone())
}

fn collect(node: Node, source: &str, tree: &Tree, out: &mut Vec<InlayHint>) {
    if node.kind() == "call_expression" {
        param_hints(node, source, tree, out);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect(child, source, tree, out);
    }
}

/// Emits a hint for each argument of a free-function `call` naming the
/// parameter it fills.
fn param_hints(call: Node, source: &str, tree: &Tree, out: &mut Vec<InlayHint>) {
    let Some(name) = call
        .child_by_field_name("function")
        .and_then(|callee| cst::callee_name(callee, source))
    else {
        return;
    };
    let Some(decl) = cst::find_decl(tree, source, name) else {
        return;
    };
    if decl.node.kind() != "function_declaration" {
        return;
    }
    let params = param_names(decl.node, source);
    let Some(args) = call.child_by_field_name("arguments") else {
        return;
    };
    let mut cursor = args.walk();
    for (arg, param) in args.named_children(&mut cursor).zip(params) {
        // Skip the hint when the argument already reads as the parameter name.
        if node_text(&arg, source) == Some(param) {
            continue;
        }
        out.push(InlayHint {
            position: byte_to_position(source, arg.start_byte()),
            label: InlayHintLabel::String(format!("{param}:")),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: Some(false),
            // A space after the colon so it reads `a: 1`, not `a:1`.
            padding_right: Some(true),
            data: None,
        });
    }
}

/// The parameter names of a `function_declaration`, in order. A `self` receiver
/// has no name field and is skipped, so the names line up with positional args.
fn param_names<'a>(decl: Node, source: &'a str) -> Vec<&'a str> {
    let Some(params) = decl.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter(|p| p.kind() == "parameter")
        .filter_map(|p| {
            p.child_by_field_name("name")
                .and_then(|n| node_text(&n, source))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(src: &str) -> Vec<String> {
        inlay_hints(src)
            .into_iter()
            .map(|hint| match hint.label {
                InlayHintLabel::String(text) => text,
                InlayHintLabel::LabelParts(_) => String::new(),
            })
            .collect()
    }

    #[test]
    fn names_each_argument_of_a_call() {
        let src =
            "fn add(a: number, b: number) -> number { a }\nfn main() -> number { add(1, 2) }\n";
        assert_eq!(labels(src), vec!["a:", "b:"]);
    }

    #[test]
    fn omits_the_hint_when_argument_matches_the_parameter() {
        let src = "fn id(x: number) -> number { x }\nfn m(x: number) -> number { id(x) }\n";
        assert!(labels(src).is_empty(), "{:?}", labels(src));
    }

    #[test]
    fn no_hints_for_unknown_callee() {
        let src = "fn m() -> number { nope(1, 2) }\n";
        assert!(labels(src).is_empty());
    }

    #[test]
    fn types_an_unannotated_let_binding() {
        let src = "fn main() -> number {\n  let total = 1 + 2\n  total\n}\n";
        assert!(
            labels(src).contains(&": number".to_string()),
            "{:?}",
            labels(src)
        );
    }

    #[test]
    fn skips_a_let_with_a_written_type() {
        let src = "fn main() -> number {\n  let total: number = 1\n  total\n}\n";
        assert!(
            !labels(src).iter().any(|l| l.starts_with(':')),
            "{:?}",
            labels(src)
        );
    }

    #[test]
    fn hint_position_is_the_argument_start() {
        let src = "fn inc(step: number) -> number { step }\nfn m() -> number { inc(7) }\n";
        let hints = inlay_hints(src);
        assert_eq!(hints.len(), 1);
        // The hint sits at the `7` argument.
        let seven = src.rfind('7').unwrap();
        assert_eq!(
            hints[0].position,
            crate::position::byte_to_position(src, seven)
        );
    }
}

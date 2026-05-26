//! Inlay hints: inline parameter-name annotations at call sites.
//!
//! At each free-function call, every argument gets a `name:` hint naming the
//! parameter it fills, read from the callee's declaration. A hint is omitted
//! when the argument is exactly the parameter name (it would only repeat what
//! is already on screen). Method calls and type hints are not handled yet.

use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel};
use tree_sitter::{Node, Tree};

use crate::cst::{self, node_text};
use crate::position::byte_to_position;

/// The parameter-name inlay hints for `source`, in document order.
#[must_use]
pub fn inlay_hints(source: &str) -> Vec<InlayHint> {
    let tree = cst::parse(source);
    let mut out = Vec::new();
    collect(tree.root_node(), source, &tree, &mut out);
    out
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

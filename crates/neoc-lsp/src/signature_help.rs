//! Signature help: the callee's signature and the active parameter while the
//! cursor sits inside a call's argument list.
//!
//! Resolution is name-based and file-local, like the other handlers: the call
//! under the cursor is matched to a top-level `fn` declaration by name, and the
//! signature is read from that declaration's CST so parameter names survive
//! (`fn add(a: number, b: number) -> number`). The active parameter is the
//! count of argument separators before the cursor. Free-function calls only for
//! now; method calls (`receiver.m(...)`) wait on receiver-type resolution.

use tower_lsp::lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};
use tree_sitter::{Node, Tree};

use crate::cst::{self, node_text};
use crate::position::position_to_byte;

/// Signature help for the call enclosing `position`, if the cursor is inside a
/// call to a known top-level function. `None` when the cursor is not inside a
/// call's argument list, or the callee is not a resolvable free function.
#[must_use]
pub fn signature_help(source: &str, position: Position) -> Option<SignatureHelp> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);

    let call = enclosing_call(&tree, byte)?;
    let callee = call.child_by_field_name("function")?;
    let name = callee_name(callee, source)?;
    let decl = cst::find_decl(&tree, source, name)?;
    if decl.node.kind() != "function_declaration" {
        return None;
    }

    let (label, parameters) = render_signature(decl.node, source)?;
    // The active parameter is the number of commas before the cursor, clamped
    // to the last parameter so trailing arguments don't point past the list.
    let active = if parameters.is_empty() {
        None
    } else {
        let index = active_parameter(call, byte).min(parameters.len() - 1);
        Some(u32::try_from(index).unwrap_or(u32::MAX))
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: None,
            parameters: Some(parameters),
            active_parameter: active,
        }],
        active_signature: Some(0),
        active_parameter: active,
    })
}

/// The innermost `call_expression` whose argument list contains `byte`.
fn enclosing_call(tree: &Tree, byte: usize) -> Option<Node<'_>> {
    let mut node = cst::named_node_at(tree, byte)?;
    loop {
        // The argument list spans the arguments between the parens (not the
        // parens themselves), so a cursor just inside an empty `()` or right
        // after the opening paren lands on its boundary — inclusive bounds.
        if node.kind() == "call_expression"
            && let Some(args) = node.child_by_field_name("arguments")
            && byte >= args.start_byte()
            && byte <= args.end_byte()
        {
            return Some(node);
        }
        node = node.parent()?;
    }
}

/// The callee name of a `call_expression`'s `function` field: the identifier
/// itself, or the final identifier of a path (`Mod::foo`).
fn callee_name<'a>(callee: Node, source: &'a str) -> Option<&'a str> {
    if callee.kind() == "identifier" {
        return node_text(&callee, source);
    }
    let mut last = None;
    let mut cursor = callee.walk();
    for child in callee.named_children(&mut cursor) {
        if child.kind() == "identifier" {
            last = Some(child);
        }
    }
    node_text(&last?, source)
}

/// The number of argument separators (commas) before `byte` in the call's
/// argument list — the index of the parameter the cursor is on.
fn active_parameter(call: Node, byte: usize) -> usize {
    let Some(args) = call.child_by_field_name("arguments") else {
        return 0;
    };
    let mut cursor = args.walk();
    args.children(&mut cursor)
        .filter(|c| c.kind() == "," && c.start_byte() < byte)
        .count()
}

/// Builds the signature label (`fn name<generics>(params) -> ret`) and one
/// [`ParameterInformation`] per parameter, read from the declaration's CST.
fn render_signature(decl: Node, source: &str) -> Option<(String, Vec<ParameterInformation>)> {
    let name = node_text(&decl.child_by_field_name("name")?, source)?;
    let generics = decl
        .child_by_field_name("generics")
        .and_then(|g| node_text(&g, source))
        .unwrap_or_default();

    let params_node = decl.child_by_field_name("parameters")?;
    let mut cursor = params_node.walk();
    let labels: Vec<String> = params_node
        .named_children(&mut cursor)
        .filter(|p| matches!(p.kind(), "parameter" | "self_parameter"))
        .filter_map(|p| Some(node_text(&p, source)?.trim().to_string()))
        .collect();

    let mut label = format!("fn {name}{generics}({})", labels.join(", "));
    if let Some(ret) = decl.child_by_field_name("return_type") {
        label.push_str(" -> ");
        label.push_str(node_text(&ret, source)?);
    }

    let parameters = labels
        .into_iter()
        .map(|l| ParameterInformation {
            label: ParameterLabel::Simple(l),
            documentation: None,
        })
        .collect();
    Some((label, parameters))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    /// Signature help with the cursor at the end of `marker`'s last occurrence.
    /// Last, not first: a call like `add(` also appears in the declaration
    /// `fn add(`, and these sources put the call after the declaration.
    fn help_after(src: &str, marker: &str) -> Option<SignatureHelp> {
        let byte = src.rfind(marker).expect("marker") + marker.len();
        signature_help(src, byte_to_position(src, byte))
    }

    fn label(help: &SignatureHelp) -> &str {
        &help.signatures[0].label
    }

    #[test]
    fn first_argument_is_active() {
        let src = "fn add(a: number, b: number) -> number { a }\nfn main() { add(1, 2) }\n";
        let help = help_after(src, "add(").expect("signature help");
        assert_eq!(label(&help), "fn add(a: number, b: number) -> number");
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn second_argument_is_active() {
        let src = "fn add(a: number, b: number) -> number { a }\nfn main() { add(1, 2) }\n";
        let help = help_after(src, "add(1, ").expect("signature help");
        assert_eq!(help.active_parameter, Some(1));
    }

    #[test]
    fn trailing_argument_clamps_to_last_parameter() {
        let src = "fn id(x: number) -> number { x }\nfn main() { id(1, 2, 3) }\n";
        let help = help_after(src, "id(1, 2, ").expect("signature help");
        // Two commas precede the cursor, but the signature has one parameter.
        assert_eq!(help.active_parameter, Some(0));
    }

    #[test]
    fn nested_call_resolves_to_the_inner_callee() {
        let src = "fn inner(a: number) -> number { a }\nfn outer(b: number) -> number { b }\nfn main() { outer(inner(1)) }\n";
        let help = help_after(src, "inner(").expect("signature help");
        assert_eq!(label(&help), "fn inner(a: number) -> number");
    }

    #[test]
    fn method_calls_are_not_supported_yet() {
        // `receiver.m(...)` is a method_call_expression, not a call_expression;
        // it needs receiver-type resolution. Pinned until that lands.
        let src = "struct S { x: number }\nimpl S { fn go(self, n: number) -> number { n } }\nfn main(s: S) -> number { s.go(1) }\n";
        assert!(help_after(src, "go(").is_none());
    }

    #[test]
    fn cursor_outside_any_call_is_none() {
        let src = "fn add(a: number, b: number) -> number { a }\nfn main() { add(1, 2) }\n";
        assert!(signature_help(src, byte_to_position(src, 0)).is_none());
    }

    #[test]
    fn parameterless_call_has_no_active_parameter() {
        let src = "fn now() -> number { 0 }\nfn main() { now() }\n";
        let help = help_after(src, "now(").expect("signature help");
        assert_eq!(label(&help), "fn now() -> number");
        assert_eq!(help.active_parameter, None);
    }
}

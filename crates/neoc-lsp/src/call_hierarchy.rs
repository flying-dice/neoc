//! Call hierarchy: who calls a function (incoming) and what a function calls
//! (outgoing).
//!
//! Name-based and file-local, like the rest. A function is identified by name;
//! incoming calls are the top-level functions whose body calls it, outgoing
//! calls are the top-level functions its own body calls. Call sites are grouped
//! per function, with the range of each callee reference.

use tower_lsp::lsp_types::Range;
use tree_sitter::{Node, Tree};

use crate::cst::{self, node_text};
use crate::position::{byte_range, position_to_byte};

/// A function the hierarchy refers to: its name and CST ranges (the whole
/// declaration, and the declared name the editor selects).
pub struct CallItem {
    pub name: String,
    pub full_range: Range,
    pub name_range: Range,
}

/// One edge of the hierarchy: the function at the other end and the ranges of
/// the call sites that connect to it.
pub struct CallEdge {
    pub item: CallItem,
    pub ranges: Vec<Range>,
}

/// The call-hierarchy item for the function named at `position`, if the cursor
/// is on a name that resolves to a top-level function.
#[must_use]
pub fn prepare(source: &str, position: tower_lsp::lsp_types::Position) -> Option<CallItem> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let name = cst::identifier_at(&tree, source, byte)?;
    function_item(&tree, source, name)
}

/// The top-level functions whose body calls `target`, each with the ranges of
/// the calls within it.
#[must_use]
pub fn incoming_calls(source: &str, target: &str) -> Vec<CallEdge> {
    let tree = cst::parse(source);
    let mut out = Vec::new();
    for decl in cst::top_level_decls(&tree, source) {
        if decl.node.kind() != "function_declaration" {
            continue;
        }
        let ranges: Vec<Range> = calls_within(decl.node, source)
            .into_iter()
            .filter(|(name, _)| *name == target)
            .map(|(_, range)| range)
            .collect();
        if ranges.is_empty() {
            continue;
        }
        if let Some(caller) = node_text(&decl.name_node, source)
            && let Some(item) = function_item(&tree, source, caller)
        {
            out.push(CallEdge { item, ranges });
        }
    }
    out
}

/// The top-level functions called from `subject`'s body, each with the ranges
/// of the calls — grouped per callee, in first-call order.
#[must_use]
pub fn outgoing_calls(source: &str, subject: &str) -> Vec<CallEdge> {
    let tree = cst::parse(source);
    let Some(decl) = cst::find_decl(&tree, source, subject) else {
        return Vec::new();
    };
    if decl.node.kind() != "function_declaration" {
        return Vec::new();
    }
    let mut grouped: Vec<(String, Vec<Range>)> = Vec::new();
    for (name, range) in calls_within(decl.node, source) {
        // Only edges to top-level functions; calls to unknowns are dropped.
        if function_item(&tree, source, name).is_none() {
            continue;
        }
        match grouped.iter_mut().find(|(n, _)| n == name) {
            Some((_, ranges)) => ranges.push(range),
            None => grouped.push((name.to_string(), vec![range])),
        }
    }
    grouped
        .into_iter()
        .filter_map(|(name, ranges)| {
            function_item(&tree, source, &name).map(|item| CallEdge { item, ranges })
        })
        .collect()
}

/// A [`CallItem`] for `name` when it is a top-level function declaration.
fn function_item(tree: &Tree, source: &str, name: &str) -> Option<CallItem> {
    let decl = cst::find_decl(tree, source, name)?;
    if decl.node.kind() != "function_declaration" {
        return None;
    }
    Some(CallItem {
        name: name.to_string(),
        full_range: byte_range(source, decl.node.start_byte(), decl.node.end_byte()),
        name_range: byte_range(
            source,
            decl.name_node.start_byte(),
            decl.name_node.end_byte(),
        ),
    })
}

/// Every call within `node`, as `(callee name, callee reference range)`.
fn calls_within<'a>(node: Node, source: &'a str) -> Vec<(&'a str, Range)> {
    let mut out = Vec::new();
    collect_calls(node, source, &mut out);
    out
}

fn collect_calls<'a>(node: Node, source: &'a str, out: &mut Vec<(&'a str, Range)>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "call_expression"
            && let Some(func) = child.child_by_field_name("function")
            && let Some(name) = cst::callee_name(func, source)
        {
            out.push((name, byte_range(source, func.start_byte(), func.end_byte())));
        }
        collect_calls(child, source, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    fn names(edges: &[CallEdge]) -> Vec<&str> {
        edges.iter().map(|e| e.item.name.as_str()).collect()
    }

    #[test]
    fn prepare_resolves_a_function_name() {
        let src = "fn fib(n: number) -> number { fib(n) }\n";
        let item = prepare(src, byte_to_position(src, 4)).expect("call item");
        assert_eq!(item.name, "fib");
    }

    #[test]
    fn prepare_off_a_function_is_none() {
        let src = "struct Point { x: number }\n";
        assert!(prepare(src, byte_to_position(src, 8)).is_none());
    }

    #[test]
    fn incoming_calls_lists_the_callers() {
        let src = "fn target() -> number { 1 }\nfn a() -> number { target() }\nfn b() -> number { target() + target() }\nfn c() -> number { 0 }\n";
        let callers = incoming_calls(src, "target");
        assert_eq!(names(&callers), vec!["a", "b"]);
        // `b` calls target twice.
        let b = callers.iter().find(|e| e.item.name == "b").unwrap();
        assert_eq!(b.ranges.len(), 2);
    }

    #[test]
    fn outgoing_calls_lists_the_callees_once_each() {
        let src = "fn leaf() -> number { 1 }\nfn helper() -> number { 2 }\nfn root() -> number { leaf() + helper() + leaf() }\n";
        let callees = outgoing_calls(src, "root");
        assert_eq!(names(&callees), vec!["leaf", "helper"]);
        let leaf = callees.iter().find(|e| e.item.name == "leaf").unwrap();
        assert_eq!(leaf.ranges.len(), 2, "leaf is called twice");
    }

    #[test]
    fn outgoing_ignores_unknown_callees() {
        let src = "fn root() -> number { mystery(1) }\n";
        assert!(outgoing_calls(src, "root").is_empty());
    }
}

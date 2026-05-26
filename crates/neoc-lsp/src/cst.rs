//! Concrete-syntax-tree helpers: parse a document, map a position to the node
//! under the cursor, and locate declaration nodes by name.
//!
//! The tree-sitter CST carries byte spans natively, so these functions stay
//! pure: a `&str` of source in, byte offsets and node references out. No type
//! information lives here — that comes from `neoc-types`.

use tree_sitter::{Node, Parser, Tree};

/// Parses neoc `source` into a tree-sitter [`Tree`].
///
/// The grammar load is infallible for a fixed language; the parse itself never
/// returns `None` for a `&str` input.
#[must_use]
pub fn parse(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&neoc_grammar::LANGUAGE.into())
        .expect("load neoc grammar");
    parser
        .parse(source, None)
        .expect("tree-sitter never returns None for a &str input")
}

/// The smallest named node whose byte range contains `byte`.
///
/// Returns `None` when the offset is outside every named node (e.g. trailing
/// whitespace at the end of the file).
#[must_use]
pub fn named_node_at(tree: &Tree, byte: usize) -> Option<Node<'_>> {
    let root = tree.root_node();
    let node = root.named_descendant_for_byte_range(byte, byte)?;
    Some(node)
}

/// The identifier text under `byte`, if the cursor sits on an `identifier`
/// node.
///
/// Walks up at most one level so a cursor on the `identifier` child of a
/// `path` (e.g. the callee of `fib(3)`) still resolves to the name.
#[must_use]
pub fn identifier_at<'a>(tree: &Tree, source: &'a str, byte: usize) -> Option<&'a str> {
    let node = named_node_at(tree, byte)?;
    let ident = if node.kind() == "identifier" {
        node
    } else {
        node.named_children(&mut node.walk())
            .find(|c| c.kind() == "identifier" && c.byte_range().contains(&byte))?
    };
    node_text(&ident, source)
}

/// The source text a node spans, or `None` when the range is not valid UTF-8
/// boundaries (which the tree-sitter spans always are for a `&str`).
#[must_use]
pub fn node_text<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.byte_range())
}

/// A top-level declaration node and the name it declares.
pub struct Decl<'t> {
    /// The declaration node (`function_declaration`, `struct_declaration`, …),
    /// spanning the whole item.
    pub node: Node<'t>,
    /// The `name` identifier node of the declaration. Its byte range is what
    /// goto-definition jumps to.
    pub name_node: Node<'t>,
}

/// The declaration kinds that live directly under `item` and carry a `name`
/// field the way `documentSymbol`/goto care about.
const NAMED_ITEM_KINDS: &[&str] = &[
    "function_declaration",
    "struct_declaration",
    "enum_declaration",
    "trait_declaration",
    "const_declaration",
    "type_alias",
];

/// Walks every top-level `item` and yields the named declaration it wraps.
///
/// `impl_block` and `use_declaration` carry no single declared name, so they
/// are skipped here; callers that need impl methods walk the block's children
/// directly.
#[must_use]
pub fn top_level_decls<'t>(tree: &'t Tree, source: &str) -> Vec<Decl<'t>> {
    let root = tree.root_node();
    let mut out = Vec::new();
    for item in root.named_children(&mut root.walk()) {
        if item.kind() != "item" {
            continue;
        }
        for decl in item.named_children(&mut item.walk()) {
            if NAMED_ITEM_KINDS.contains(&decl.kind())
                && let Some(name_node) = decl.child_by_field_name("name")
            {
                let _ = source; // text is read lazily by callers
                out.push(Decl {
                    node: decl,
                    name_node,
                });
            }
        }
    }
    out
}

/// The first top-level declaration named `name`, if any.
#[must_use]
pub fn find_decl<'t>(tree: &'t Tree, source: &str, name: &str) -> Option<Decl<'t>> {
    top_level_decls(tree, source)
        .into_iter()
        .find(|d| node_text(&d.name_node, source) == Some(name))
}

fn is_comment(kind: &str) -> bool {
    matches!(kind, "line_comment" | "block_comment" | "doc_comment")
}

/// The doc comment directly above a declaration, if any.
///
/// Collects the contiguous `///`-prefixed lines immediately preceding the
/// declaration (no blank line between), strips the `///` marker, and joins
/// them. Ordinary `//` comments are not documentation and end the block, so a
/// stray note above an item never leaks into hover.
///
/// Works for top-level items and members alike: comments precede the wrapping
/// `item`/`impl_item`/`trait_item` for fns and types, but precede the node
/// itself for a struct field or enum variant. The anchor is chosen to match.
#[must_use]
pub fn leading_doc_comment(decl: Node, source: &str) -> Option<String> {
    let anchor = match decl.parent() {
        Some(p) if matches!(p.kind(), "item" | "impl_item" | "trait_item") => p,
        _ => decl,
    };
    let mut lines: Vec<String> = Vec::new();
    let mut expected_row = anchor.start_position().row;
    let mut sibling = anchor.prev_sibling();
    while let Some(node) = sibling {
        // Must be a comment on the line directly above what follows it.
        if !is_comment(node.kind()) || node.end_position().row + 1 != expected_row {
            break;
        }
        let text = node_text(&node, source)?.trim_end();
        let Some(doc) = text.strip_prefix("///") else {
            break;
        };
        lines.push(doc.strip_prefix(' ').unwrap_or(doc).to_string());
        expected_row = node.start_position().row;
        sibling = node.prev_sibling();
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

/// Whether `parent`'s `name` field is exactly `ident`.
fn names(parent: Node, ident: Node) -> bool {
    parent
        .child_by_field_name("name")
        .is_some_and(|n| n.byte_range() == ident.byte_range())
}

/// Whether a `function_declaration` sits inside an `impl`/`trait` (a method)
/// rather than at module scope (a free function).
fn is_method(func: Node) -> bool {
    func.parent()
        .is_some_and(|p| matches!(p.kind(), "impl_item" | "trait_item"))
}

/// If the cursor sits on the declaring name of a member — a struct field, enum
/// variant, or `impl`/`trait` method — the member's declaration node.
#[must_use]
pub fn member_decl_at(tree: &Tree, byte: usize) -> Option<Node<'_>> {
    let node = named_node_at(tree, byte)?;
    let ident = if node.kind() == "identifier" {
        node
    } else {
        node.named_children(&mut node.walk())
            .find(|c| c.kind() == "identifier" && c.byte_range().contains(&byte))?
    };
    let parent = ident.parent()?;
    if !names(parent, ident) {
        return None;
    }
    match parent.kind() {
        "field_declaration" | "variant" => Some(parent),
        "function_declaration" if is_method(parent) => Some(parent),
        _ => None,
    }
}

/// The unique member declaration named `name` anywhere in the file — a struct
/// field, enum variant, or `impl`/`trait` method. `None` if no member matches,
/// or more than one does (ambiguous without a receiver type, so hover stays
/// silent rather than guess wrong).
#[must_use]
pub fn find_unique_member<'t>(tree: &'t Tree, source: &str, name: &str) -> Option<Node<'t>> {
    let mut found = Vec::new();
    collect_members(tree.root_node(), source, name, &mut found);
    match found.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Every `identifier` node in the document whose text is exactly `name`, in
/// source order.
///
/// The basis for find-all-references: resolution is name-based (the same model
/// goto-definition uses), so every identically-spelled identifier — declaration,
/// call, type position, field — counts as a reference. No scope analysis, so a
/// local that shadows a top-level name is included.
#[must_use]
pub fn identifier_nodes<'t>(tree: &'t Tree, source: &str, name: &str) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    collect_identifiers(tree.root_node(), source, name, &mut out);
    out
}

fn collect_identifiers<'t>(node: Node<'t>, source: &str, name: &str, out: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "identifier" && node_text(&child, source) == Some(name) {
            out.push(child);
        }
        collect_identifiers(child, source, name, out);
    }
}

fn collect_members<'t>(node: Node<'t>, source: &str, name: &str, out: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let is_member = matches!(child.kind(), "field_declaration" | "variant")
            || (child.kind() == "function_declaration" && is_method(child));
        if is_member
            && child
                .child_by_field_name("name")
                .and_then(|n| node_text(&n, source))
                == Some(name)
        {
            out.push(child);
        }
        collect_members(child, source, name, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_at_callee() {
        let src = "fn main() { fib(3); }";
        let tree = parse(src);
        let byte = src.find("fib").unwrap() + 1;
        assert_eq!(identifier_at(&tree, src, byte), Some("fib"));
    }

    #[test]
    fn identifier_at_whitespace_is_none() {
        let src = "fn main() {}\n   ";
        let tree = parse(src);
        assert_eq!(identifier_at(&tree, src, src.len()), None);
    }

    #[test]
    fn top_level_decls_lists_every_kind() {
        let src = "struct S {}\nenum E { A }\nfn f() {}\nconst C: number = 1;\ntype T = number;\ntrait Tr { fn m(self); }\n";
        let tree = parse(src);
        let names: Vec<&str> = top_level_decls(&tree, src)
            .iter()
            .filter_map(|d| node_text(&d.name_node, src))
            .collect();
        assert_eq!(names, vec!["S", "E", "f", "C", "T", "Tr"]);
    }

    #[test]
    fn find_decl_locates_struct() {
        let src = "fn a() {}\nstruct Point { x: number }\n";
        let tree = parse(src);
        let decl = find_decl(&tree, src, "Point").expect("Point decl");
        assert_eq!(decl.node.kind(), "struct_declaration");
    }
}

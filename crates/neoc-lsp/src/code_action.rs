//! Code actions. The first is organize-imports: sort the document's `use`
//! declarations and drop exact duplicates.
//!
//! Purely CST-driven — it rewrites the contiguous run of top-level `use`
//! declarations in place. When the imports are already sorted and unique, or
//! are split by other items (so a block rewrite would scramble them), there is
//! no action.

use tower_lsp::lsp_types::TextEdit;
use tree_sitter::Node;

use crate::cst::{self, node_text};
use crate::position::byte_range;

/// The edit that sorts and de-duplicates the document's `use` declarations, or
/// `None` when there is nothing to organize (fewer than two imports, already
/// canonical, or imports interleaved with other items).
#[must_use]
pub fn organize_imports(source: &str) -> Option<Vec<TextEdit>> {
    let tree = cst::parse(source);
    let uses = contiguous_use_run(&tree)?;
    if uses.len() < 2 {
        return None;
    }

    let original: Vec<&str> = uses
        .iter()
        .filter_map(|node| node_text(node, source).map(str::trim))
        .collect();
    let mut sorted = original.clone();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted == original {
        return None;
    }

    // Replace the whole run, from the first import to the last, in one edit.
    let start = uses.first()?.start_byte();
    let end = uses.last()?.end_byte();
    Some(vec![TextEdit {
        range: byte_range(source, start, end),
        new_text: sorted.join("\n"),
    }])
}

/// The top-level `use` declarations when they form one contiguous run (no other
/// item between the first and last). `None` if there are none, or they are
/// interleaved with other items — a block rewrite would lose those items.
fn contiguous_use_run(tree: &tree_sitter::Tree) -> Option<Vec<Node<'_>>> {
    let root = tree.root_node();
    let mut uses = Vec::new();
    let mut seen_non_use_after_use = false;
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor) {
        if item.kind() != "item" {
            continue;
        }
        let mut item_cursor = item.walk();
        for decl in item.named_children(&mut item_cursor) {
            if decl.kind() == "use_declaration" {
                if seen_non_use_after_use {
                    return None; // a `use` follows a non-`use`: not contiguous.
                }
                uses.push(decl);
            } else if !uses.is_empty() {
                seen_non_use_after_use = true;
            }
        }
    }
    if uses.is_empty() { None } else { Some(uses) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::position_to_byte;

    fn organized(source: &str) -> Option<String> {
        let edit = organize_imports(source)?.pop()?;
        let start = position_to_byte(source, edit.range.start);
        let end = position_to_byte(source, edit.range.end);
        let mut result = source.to_string();
        result.replace_range(start..end, &edit.new_text);
        Some(result)
    }

    #[test]
    fn sorts_and_dedupes_imports() {
        let src = "use std::io;\nuse lib::base64;\nuse std::io;\nuse std::fs;\nfn main() {}\n";
        let out = organized(src).expect("an edit");
        assert_eq!(
            out,
            "use lib::base64;\nuse std::fs;\nuse std::io;\nfn main() {}\n"
        );
    }

    #[test]
    fn already_sorted_and_unique_has_no_action() {
        let src = "use lib::base64;\nuse std::fs;\nfn main() {}\n";
        assert_eq!(organize_imports(src), None);
    }

    #[test]
    fn a_single_import_has_no_action() {
        let src = "use std::io;\nfn main() {}\n";
        assert_eq!(organize_imports(src), None);
    }

    #[test]
    fn interleaved_imports_are_left_alone() {
        let src = "use std::io;\nfn a() {}\nuse lib::base64;\n";
        assert_eq!(organize_imports(src), None);
    }
}

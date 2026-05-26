//! Code actions. The first is organize-imports: sort the document's `use`
//! declarations and drop exact duplicates.
//!
//! Purely CST-driven — it rewrites the contiguous run of top-level `use`
//! declarations in place. When the imports are already sorted and unique, or
//! are split by other items (so a block rewrite would scramble them), there is
//! no action.

use tower_lsp::lsp_types::{Range, TextEdit};
use tree_sitter::{Node, Tree};

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

/// An import whose bound name is never used in the rest of the file.
pub struct UnusedImport {
    /// The bound name (the alias, or the last segment of the path).
    pub name: String,
    /// The declaration's own range — what a lint underlines.
    pub range: Range,
    /// The range to delete to remove the import — the declaration and its
    /// trailing newline.
    pub removal: Range,
}

/// Every top-level import whose bound name appears nowhere outside the import
/// declarations. The remedy is the [`UnusedImport::removal`] range.
#[must_use]
pub fn unused_imports(source: &str) -> Vec<UnusedImport> {
    unused_imports_in(&cst::parse(source), source)
}

/// [`unused_imports`] over an already-parsed tree, so a lint pass that holds the
/// CST need not re-parse.
#[must_use]
pub fn unused_imports_in(tree: &Tree, source: &str) -> Vec<UnusedImport> {
    let mut out = Vec::new();
    for use_decl in top_level_uses(tree) {
        let Some(binding) = import_binding(use_decl, source) else {
            continue;
        };
        if !is_used_outside_imports(tree, source, binding) {
            out.push(UnusedImport {
                name: binding.to_string(),
                range: byte_range(source, use_decl.start_byte(), use_decl.end_byte()),
                removal: removal_range(use_decl, source),
            });
        }
    }
    out
}

/// The set of names the file's top-level `use` declarations bind into scope
/// (each import's alias, or its path's last segment). Auto-import consults it to
/// skip a name that is already imported.
#[must_use]
pub fn imported_names(source: &str) -> std::collections::HashSet<String> {
    let tree = cst::parse(source);
    top_level_uses(&tree)
        .iter()
        .filter_map(|u| import_binding(*u, source).map(str::to_string))
        .collect()
}

/// The name an import binds: its alias, or the last identifier of its path.
fn import_binding<'a>(use_decl: Node, source: &'a str) -> Option<&'a str> {
    if let Some(alias) = use_decl.child_by_field_name("alias") {
        return node_text(&alias, source);
    }
    let path = use_decl.child_by_field_name("path")?;
    let mut last = None;
    let mut cursor = path.walk();
    for segment in path.named_children(&mut cursor) {
        if segment.kind() == "identifier" {
            last = Some(segment);
        }
    }
    node_text(&last?, source)
}

/// Whether `name` is referenced by an identifier outside any import.
fn is_used_outside_imports(tree: &Tree, source: &str, name: &str) -> bool {
    cst::identifier_nodes(tree, source, name)
        .iter()
        .any(|node| !within_use(*node))
}

/// Whether `node` sits inside a `use` declaration.
fn within_use(node: Node) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "use_declaration" {
            return true;
        }
        current = n.parent();
    }
    false
}

/// The range that deletes a `use` declaration, including its trailing newline so
/// no blank line is left behind.
fn removal_range(use_decl: Node, source: &str) -> Range {
    let start = use_decl.start_byte();
    let mut end = use_decl.end_byte();
    if source[end..].starts_with('\n') {
        end += 1;
    }
    byte_range(source, start, end)
}

/// Every top-level `use` declaration, in document order.
fn top_level_uses(tree: &Tree) -> Vec<Node<'_>> {
    let root = tree.root_node();
    let mut uses = Vec::new();
    let mut cursor = root.walk();
    for item in root.named_children(&mut cursor) {
        if item.kind() != "item" {
            continue;
        }
        let mut item_cursor = item.walk();
        for decl in item.named_children(&mut item_cursor) {
            if decl.kind() == "use_declaration" {
                uses.push(decl);
            }
        }
    }
    uses
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

    #[test]
    fn flags_only_the_unused_import() {
        let src = "use std::fs;\nuse std::io;\nfn main() -> number { io::run() }\n";
        let found = unused_imports(src);
        let names: Vec<&str> = found.iter().map(|u| u.name.as_str()).collect();
        // `fs` is never referenced; `io` is used in `io::run()`.
        assert_eq!(names, vec!["fs"]);
    }

    #[test]
    fn a_used_import_is_not_flagged() {
        let src = "use std::io;\nfn main() -> number { io::run() }\n";
        assert!(unused_imports(src).is_empty());
    }

    #[test]
    fn removal_deletes_the_whole_import_line() {
        let src = "use std::fs;\nfn main() {}\n";
        let unused = unused_imports(src);
        assert_eq!(unused.len(), 1);
        let start = position_to_byte(src, unused[0].removal.start);
        let end = position_to_byte(src, unused[0].removal.end);
        let mut result = src.to_string();
        result.replace_range(start..end, "");
        assert_eq!(result, "fn main() {}\n");
    }
}

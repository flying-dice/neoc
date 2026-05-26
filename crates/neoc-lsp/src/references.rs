//! Find-all-references: every occurrence of the name under the cursor.
//!
//! Resolution is name-based and file-local — the same model goto-definition
//! uses. The identifier under the cursor is matched against every `identifier`
//! node in the document; each match is a reference. There is no scope analysis
//! yet, so a local that shadows a top-level name is reported alongside it
//! (consistent with goto and hover, which also resolve by name), and references
//! in other files wait on a multi-document index.

use tower_lsp::lsp_types::{Position, Range};
use tree_sitter::Tree;

use crate::cst;
use crate::position::{byte_range, position_to_byte};

/// The references to the identifier at `position`, in source order.
///
/// `include_declaration` controls whether the declaring name itself is part of
/// the result, mirroring the LSP `ReferenceContext`. Returns an empty vector
/// when the cursor is not on an identifier.
#[must_use]
pub fn references(source: &str, position: Position, include_declaration: bool) -> Vec<Range> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let Some(name) = cst::identifier_at(&tree, source, byte) else {
        return Vec::new();
    };
    let decl = declaration_name_range(&tree, source, name);
    cst::identifier_nodes(&tree, source, name)
        .into_iter()
        .map(|node| byte_range(source, node.start_byte(), node.end_byte()))
        .filter(|range| include_declaration || Some(*range) != decl)
        .collect()
}

/// The range of the declaring name for `name`, when it resolves to a top-level
/// declaration or a unique member. `None` leaves every occurrence in the
/// result, since there is no defining name to single out.
fn declaration_name_range(tree: &Tree, source: &str, name: &str) -> Option<Range> {
    let name_node = match cst::find_decl(tree, source, name) {
        Some(decl) => decl.name_node,
        None => cst::find_unique_member(tree, source, name)?.child_by_field_name("name")?,
    };
    Some(byte_range(
        source,
        name_node.start_byte(),
        name_node.end_byte(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    /// LSP position at the 1-based `occ`-th occurrence of `needle`.
    fn position_of(src: &str, needle: &str, occ: usize) -> Position {
        let mut from = 0usize;
        let mut idx = 0usize;
        for _ in 0..occ {
            let i = src[from..].find(needle).expect("occurrence") + from;
            idx = i;
            from = i + 1;
        }
        byte_to_position(src, idx + 1)
    }

    #[test]
    fn includes_declaration_and_every_use() {
        let src = "fn fib(n: number) -> number { fib(n) }\nfn main() -> number { fib(3) }\n";
        // Cursor on the recursive call; the declaration is included by default.
        let refs = references(src, position_of(src, "fib", 2), true);
        assert_eq!(
            refs.len(),
            3,
            "decl + recursive call + call in main: {refs:?}"
        );
    }

    #[test]
    fn excluding_declaration_drops_the_defining_name() {
        let src = "fn fib(n: number) -> number { fib(n) }\nfn main() -> number { fib(3) }\n";
        let with = references(src, position_of(src, "fib", 1), true);
        let without = references(src, position_of(src, "fib", 1), false);
        assert_eq!(with.len(), without.len() + 1);
        // The dropped range is the declaration name (`fib` at byte 3).
        assert!(!without.contains(&byte_range(src, 3, 6)), "{without:?}");
    }

    #[test]
    fn references_are_in_source_order() {
        let src = "fn fib(n: number) -> number { fib(n) }\nfn main() -> number { fib(3) }\n";
        let refs = references(src, position_of(src, "fib", 1), true);
        let mut sorted = refs.clone();
        sorted.sort_by_key(|r| (r.start.line, r.start.character));
        assert_eq!(refs, sorted);
    }

    #[test]
    fn type_name_references_span_declaration_and_uses() {
        let src = "struct Point { x: number }\nfn make() -> Point { Point { x: 1 } }\n";
        // decl name, return type, and struct literal — three occurrences.
        let refs = references(src, position_of(src, "Point", 2), true);
        assert_eq!(refs.len(), 3, "{refs:?}");
    }

    #[test]
    fn cursor_off_any_identifier_is_empty() {
        let src = "fn main() { 1 + 1 }\n";
        let refs = references(src, byte_to_position(src, src.find('+').unwrap()), true);
        assert!(refs.is_empty(), "{refs:?}");
    }
}

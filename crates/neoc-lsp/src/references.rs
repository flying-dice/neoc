//! Find-all-references: every occurrence the name under the cursor refers to.
//!
//! Resolution is scope-aware for *local* bindings (params, `let`s, closure and
//! pattern binds): the cursor resolves to a binding via
//! [`neoc_types::resolution`], and only that binding's occurrences are returned
//! — a shadowing inner `x` and an outer `x` no longer conflate. For a top-level
//! item (or an unknown name) it falls back to matching the identifier text
//! across the file, *minus* any occurrence the resolution map marks as a
//! shadowing local, so a rename of the item never reaches into a local's scope.
//! Cross-file is the same item path, run per file (`tasks/scope-aware-resolution.md`).

use tower_lsp::lsp_types::{Position, Range};
use tree_sitter::Tree;

use neoc_types::Resolved;

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
    let module = neoc_parser::parse(source).module;
    let resolution = neoc_types::resolution(&module);

    // A local binding: return exactly its scope occurrences (def + uses).
    if let Some(Resolved::Local(id)) = resolution.at(byte) {
        let def = resolution.binding(id).map(|b| b.def);
        return resolution
            .local_occurrences(id)
            .into_iter()
            .filter(|range| include_declaration || Some(*range) != def)
            .map(|(start, end)| byte_range(source, start, end))
            .collect();
    }

    // A top-level item or unknown name: text match minus shadowing locals.
    let Some(name) = cst::identifier_at(&tree, source, byte) else {
        return Vec::new();
    };
    item_references(source, name, include_declaration)
}

/// Every occurrence of the identifier `name` in `source`, in source order — the
/// raw name-based match, with no scope analysis.
#[must_use]
pub fn references_by_name(source: &str, name: &str, include_declaration: bool) -> Vec<Range> {
    let tree = cst::parse(source);
    let decl = declaration_name_range(&tree, source, name);
    cst::identifier_nodes(&tree, source, name)
        .into_iter()
        .map(|node| byte_range(source, node.start_byte(), node.end_byte()))
        .filter(|range| include_declaration || Some(*range) != decl)
        .collect()
}

/// Occurrences of a top-level item `name`, excluding any that the resolution map
/// marks as a *local* binding shadowing the name. This is the cross-file and
/// item path: a rename of `add` must rewrite its declaration, its `use` segment,
/// and its call sites, but never a `let add` that shadows it. The map records
/// only single-segment value uses, so type-position and struct-literal uses
/// (never locals) correctly survive the filter.
#[must_use]
pub fn item_references(source: &str, name: &str, include_declaration: bool) -> Vec<Range> {
    let module = neoc_parser::parse(source).module;
    let resolution = neoc_types::resolution(&module);
    references_by_name(source, name, include_declaration)
        .into_iter()
        .filter(|range| {
            let byte = position_to_byte(source, range.start);
            !matches!(resolution.at(byte), Some(Resolved::Local(_)))
        })
        .collect()
}

/// The declaring-name range for the identifier at `position`, if it resolves to
/// a top-level declaration or a unique member. Lets document highlight mark the
/// defining occurrence distinctly from its reads.
#[must_use]
pub(crate) fn declaration_range_at(source: &str, position: Position) -> Option<Range> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let name = cst::identifier_at(&tree, source, byte)?;
    declaration_name_range(&tree, source, name)
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
    fn by_name_finds_uses_without_a_cursor() {
        // The cross-file core: in a file that only *uses* an imported name (no
        // declaration), every occurrence is a reference and none is dropped.
        let src = "use util::add;\nfn main() -> number { add(add(1, 2), 3) }\n";
        let refs = references_by_name(src, "add", false);
        assert_eq!(refs.len(), 3, "the `use` segment and both calls: {refs:?}");
    }

    #[test]
    fn by_name_drops_the_declaration_when_excluded() {
        // In the defining file, excluding the declaration drops exactly one.
        let src = "fn add(a: number, b: number) -> number { a + b }\nfn main() { add(1, 2) }\n";
        let with = references_by_name(src, "add", true);
        let without = references_by_name(src, "add", false);
        assert_eq!(with.len(), without.len() + 1);
    }

    #[test]
    fn cursor_off_any_identifier_is_empty() {
        let src = "fn main() { 1 + 1 }\n";
        let refs = references(src, byte_to_position(src, src.find('+').unwrap()), true);
        assert!(refs.is_empty(), "{refs:?}");
    }
}

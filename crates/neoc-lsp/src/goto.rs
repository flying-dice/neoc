//! Goto-definition: resolve the identifier under the cursor to the CST range
//! of the top-level declaration that introduces it.
//!
//! Resolution is name-based against the document's own top-level items: fns,
//! structs, enums, traits, consts, and type aliases. Locals and impl methods
//! are out of scope for this pass — the function returns `None` when the name
//! does not match a top-level declaration.

use tower_lsp::lsp_types::Range;

use crate::cst;
use crate::position::{byte_range, position_to_byte};

/// A resolved definition target.
///
/// `full_range` spans the whole declaration (the item the editor highlights on
/// peek); `name_range` is the declared name the editor places the cursor on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition {
    pub full_range: Range,
    pub name_range: Range,
}

/// The definition for the identifier at `position`, if it names a top-level
/// declaration.
///
/// Resolves against the document's top-level items (fns, structs, enums,
/// traits, consts, type aliases). Returns `None` when the name is unknown or
/// the cursor already sits on the defining name itself.
#[must_use]
pub fn goto_definition(
    source: &str,
    position: tower_lsp::lsp_types::Position,
) -> Option<Definition> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let name = cst::identifier_at(&tree, source, byte)?;
    let decl = cst::find_decl(&tree, source, name)?;
    // Don't report a no-op jump when the cursor already sits on the defining
    // name itself.
    if decl.name_node.byte_range().contains(&byte) {
        return None;
    }
    Some(Definition {
        full_range: byte_range(source, decl.node.start_byte(), decl.node.end_byte()),
        name_range: byte_range(
            source,
            decl.name_node.start_byte(),
            decl.name_node.end_byte(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    fn position_of(src: &str, needle: &str, occurrence: usize) -> tower_lsp::lsp_types::Position {
        let mut from = 0usize;
        let mut idx = 0usize;
        for _ in 0..occurrence {
            let i = src[from..].find(needle).expect("occurrence") + from;
            idx = i;
            from = i + 1;
        }
        byte_to_position(src, idx + 1)
    }

    #[test]
    fn call_resolves_to_fn_decl() {
        let src = "fn fib(n: number) -> number { fib(n) }\n";
        // Cursor on the recursive call `fib`.
        let call = src.rfind("fib").unwrap();
        let pos = byte_to_position(src, call + 1);
        let def = goto_definition(src, pos).expect("definition");
        // The decl name `fib` starts at byte 3; the item spans the whole fn.
        assert_eq!(def.name_range, byte_range(src, 3, 6));
        assert_eq!(def.full_range.start, byte_to_position(src, 0));
    }

    #[test]
    fn struct_literal_resolves_to_struct() {
        let src = "struct Point { x: number }\nfn make() -> Point { Point { x: 1 } }\n";
        let pos = position_of(src, "Point", 2);
        let def = goto_definition(src, pos).expect("definition");
        assert_eq!(def.name_range, byte_range(src, 7, 12)); // "Point" in the decl
    }

    #[test]
    fn cursor_on_definition_is_none() {
        let src = "fn fib(n: number) -> number { n }\n";
        let pos = byte_to_position(src, 4); // on the `fib` of the decl
        assert_eq!(goto_definition(src, pos), None);
    }

    #[test]
    fn unknown_name_is_none() {
        let src = "fn main() { 1 + 1 }\n";
        let pos = byte_to_position(src, 0);
        assert_eq!(goto_definition(src, pos), None);
    }
}

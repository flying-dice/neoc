//! Rename: replace every occurrence of the name under the cursor.
//!
//! Built on find-all-references — rename is the same name-based, file-local set
//! of occurrences (declaration included), rewritten to the new name. The same
//! shadowing over-approximation applies, and the edits stay within one file
//! until a cross-document index lands.

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::cst;
use crate::position::{byte_range, position_to_byte};
use crate::references;

/// The range of the identifier under `position`, if the cursor is on one — the
/// span an editor highlights before prompting for the new name. `None` when the
/// cursor is not on an identifier, so the editor declines the rename.
#[must_use]
pub fn prepare_rename(source: &str, position: Position) -> Option<Range> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let node = cst::identifier_node_at(&tree, byte)?;
    Some(byte_range(source, node.start_byte(), node.end_byte()))
}

/// The edits that rename the identifier under `position` to `new_name`: one
/// replacement per reference, declaration included. Empty when the cursor is not
/// on an identifier.
#[must_use]
pub fn rename(source: &str, position: Position, new_name: &str) -> Vec<TextEdit> {
    references::references(source, position, true)
        .into_iter()
        .map(|range| TextEdit {
            range,
            new_text: new_name.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

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

    /// Applies `edits` to `source`, right-to-left so earlier offsets stay valid.
    fn apply(source: &str, edits: &[TextEdit]) -> String {
        let mut spans: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|e| {
                (
                    position_to_byte(source, e.range.start),
                    position_to_byte(source, e.range.end),
                    e.new_text.as_str(),
                )
            })
            .collect();
        spans.sort_by_key(|&(start, _, _)| start);
        let mut result = source.to_string();
        for (start, end, text) in spans.into_iter().rev() {
            result.replace_range(start..end, text);
        }
        result
    }

    #[test]
    fn renames_declaration_and_every_use() {
        let src = "fn fib(n: number) -> number { fib(n) }\nfn main() -> number { fib(3) }\n";
        let edits = rename(src, position_of(src, "fib", 2), "fibonacci");
        assert_eq!(edits.len(), 3, "{edits:?}");
        let renamed = apply(src, &edits);
        assert_eq!(
            renamed,
            "fn fibonacci(n: number) -> number { fibonacci(n) }\nfn main() -> number { fibonacci(3) }\n"
        );
    }

    #[test]
    fn renames_a_type_across_positions() {
        let src = "struct Point { x: number }\nfn make() -> Point { Point { x: 1 } }\n";
        let edits = rename(src, position_of(src, "Point", 1), "Vec2");
        let renamed = apply(src, &edits);
        assert_eq!(
            renamed,
            "struct Vec2 { x: number }\nfn make() -> Vec2 { Vec2 { x: 1 } }\n"
        );
    }

    #[test]
    fn prepare_returns_the_identifier_range() {
        let src = "fn fib(n: number) -> number { fib(n) }\n";
        let range = prepare_rename(src, position_of(src, "fib", 1)).expect("renameable");
        assert_eq!(range, byte_range(src, 3, 6));
    }

    #[test]
    fn prepare_off_identifier_is_none() {
        let src = "fn main() { 1 + 1 }\n";
        let at_plus = byte_to_position(src, src.find('+').unwrap());
        assert_eq!(prepare_rename(src, at_plus), None);
    }

    #[test]
    fn rename_off_identifier_is_empty() {
        let src = "fn main() { 1 + 1 }\n";
        let at_plus = byte_to_position(src, src.find('+').unwrap());
        assert!(rename(src, at_plus, "x").is_empty());
    }
}

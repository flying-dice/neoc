//! Document highlight: the occurrences of the name under the cursor within the
//! open file, with the defining occurrence marked distinctly from its reads.
//!
//! The occurrence set is exactly find-all-references — already file-local — so
//! this reuses it and only adds the read/write kind: the declaring name is a
//! write, every other occurrence a read.

use tower_lsp::lsp_types::{DocumentHighlight, DocumentHighlightKind, Position};

use crate::references;

/// The highlights for the identifier at `position`: one per occurrence, the
/// declaration tagged `WRITE` and its uses `READ`. Empty when the cursor is not
/// on an identifier.
#[must_use]
pub fn document_highlight(source: &str, position: Position) -> Vec<DocumentHighlight> {
    let declaration = references::declaration_range_at(source, position);
    references::references(source, position, true)
        .into_iter()
        .map(|range| DocumentHighlight {
            range,
            kind: Some(if Some(range) == declaration {
                DocumentHighlightKind::WRITE
            } else {
                DocumentHighlightKind::READ
            }),
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

    #[test]
    fn declaration_is_a_write_and_uses_are_reads() {
        let src = "fn fib(n: number) -> number { fib(n) }\nfn main() -> number { fib(3) }\n";
        let highlights = document_highlight(src, position_of(src, "fib", 2));
        assert_eq!(highlights.len(), 3);
        let writes = highlights
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
            .count();
        let reads = highlights
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
            .count();
        assert_eq!((writes, reads), (1, 2), "{highlights:?}");
    }

    #[test]
    fn cursor_off_identifier_is_empty() {
        let src = "fn main() { 1 + 1 }\n";
        let at_plus = byte_to_position(src, src.find('+').unwrap());
        assert!(document_highlight(src, at_plus).is_empty());
    }
}

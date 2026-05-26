//! Selection range: the chain of ever-larger syntactic ranges around a
//! position, for editor "expand/shrink selection".
//!
//! The innermost named node under the cursor is the head; each `parent` link
//! widens to the enclosing node, up to the whole file. Consecutive nodes that
//! share a range (a node and a single-child wrapper) collapse to one step.

use tower_lsp::lsp_types::{Position, Range, SelectionRange};

use crate::cst;
use crate::position::{byte_range, position_to_byte};

/// The expand-selection chain at `position`: a [`SelectionRange`] whose `parent`
/// links widen out to the file. A position past all content falls back to the
/// outermost node; `None` only when the document has no named nodes at all.
#[must_use]
pub fn selection_range(source: &str, position: Position) -> Option<SelectionRange> {
    let tree = cst::parse(source);
    let byte = position_to_byte(source, position);
    let node = tree
        .root_node()
        .named_descendant_for_byte_range(byte, byte)?;

    // Distinct ranges from the cursor node out to the root, innermost first.
    let mut ranges: Vec<Range> = Vec::new();
    let mut current = Some(node);
    while let Some(n) = current {
        let range = byte_range(source, n.start_byte(), n.end_byte());
        if ranges.last() != Some(&range) {
            ranges.push(range);
        }
        current = n.parent();
    }

    // Fold outermost-first so each step's `parent` is the wider range.
    let mut selection: Option<SelectionRange> = None;
    for range in ranges.into_iter().rev() {
        selection = Some(SelectionRange {
            range,
            parent: selection.map(Box::new),
        });
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::byte_to_position;

    /// The chain of covered source slices, innermost first.
    fn steps(src: &str, byte: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut node = selection_range(src, byte_to_position(src, byte));
        while let Some(sel) = node {
            let start = position_to_byte(src, sel.range.start);
            let end = position_to_byte(src, sel.range.end);
            out.push(src[start..end].to_string());
            node = sel.parent.map(|p| *p);
        }
        out
    }

    #[test]
    fn expands_from_identifier_outward() {
        let src = "fn main() -> number { add(1, 2) }\n";
        let at = src.find("add").unwrap() + 1;
        let chain = steps(src, at);
        // Innermost is the identifier; each step is a superset of the last.
        assert_eq!(chain.first().map(String::as_str), Some("add"));
        for pair in chain.windows(2) {
            assert!(
                pair[1].len() >= pair[0].len(),
                "selection should widen: {:?} then {:?}",
                pair[0],
                pair[1],
            );
        }
        // The outermost step spans the whole declaration.
        assert!(
            chain.last().is_some_and(|outer| outer.contains("fn main")),
            "{chain:?}"
        );
    }

    #[test]
    fn position_past_content_falls_back_to_the_outermost_node() {
        let src = "fn main() {}\n   ";
        let chain = steps(src, src.len());
        assert!(
            chain.last().is_some_and(|outer| outer.contains("fn main")),
            "{chain:?}"
        );
    }
}

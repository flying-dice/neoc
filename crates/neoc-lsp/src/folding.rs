//! Folding ranges: the collapsible regions of a document.
//!
//! Every brace-delimited body folds — function blocks, struct/enum/trait/impl
//! bodies, `match` blocks — found generically as any CST node with `{` and `}`
//! token children spanning more than one line, so no construct needs special
//! casing. Multi-line block comments fold too.

use tower_lsp::lsp_types::{FoldingRange, FoldingRangeKind};
use tree_sitter::Node;

use crate::cst;

/// The folding ranges for `source`, in document order.
#[must_use]
pub fn folding_ranges(source: &str) -> Vec<FoldingRange> {
    let tree = cst::parse(source);
    let mut out = Vec::new();
    collect(tree.root_node(), &mut out);
    out
}

fn collect(node: Node, out: &mut Vec<FoldingRange>) {
    if let Some(range) = brace_fold(node) {
        out.push(range);
    } else if node.kind() == "block_comment"
        && let Some(range) = line_span(node, Some(FoldingRangeKind::Comment))
    {
        out.push(range);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, out);
    }
}

/// A fold from a node's `{` to its `}` token, when both are direct children on
/// different lines. `None` for nodes with no brace pair or a single-line body.
fn brace_fold(node: Node) -> Option<FoldingRange> {
    let mut cursor = node.walk();
    let mut open = None;
    let mut close = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            "{" => open = Some(child),
            "}" => close = Some(child),
            _ => {}
        }
    }
    let (open, close) = (open?, close?);
    fold(open.start_position().row, close.start_position().row, None)
}

/// A fold spanning a node's own lines (used for block comments).
fn line_span(node: Node, kind: Option<FoldingRangeKind>) -> Option<FoldingRange> {
    fold(node.start_position().row, node.end_position().row, kind)
}

/// A `FoldingRange` from `start_row` to `end_row` (0-based), or `None` when the
/// region is a single line.
fn fold(start_row: usize, end_row: usize, kind: Option<FoldingRangeKind>) -> Option<FoldingRange> {
    if end_row <= start_row {
        return None;
    }
    Some(FoldingRange {
        start_line: u32::try_from(start_row).unwrap_or(u32::MAX),
        end_line: u32::try_from(end_row).unwrap_or(u32::MAX),
        kind,
        ..FoldingRange::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folds as 0-based `(start_line, end_line)` pairs, document order.
    fn folds(src: &str) -> Vec<(u32, u32)> {
        folding_ranges(src)
            .into_iter()
            .map(|f| (f.start_line, f.end_line))
            .collect()
    }

    #[test]
    fn folds_a_function_body() {
        let src = "fn add(a: number, b: number) -> number {\n  a + b\n}\n";
        assert_eq!(folds(src), vec![(0, 2)]);
    }

    #[test]
    fn folds_struct_and_function_bodies() {
        let src = "struct Point {\n  x: number,\n  y: number,\n}\nfn origin() -> Point {\n  Point { x: 0, y: 0 }\n}\n";
        // struct body lines 0..3, function body lines 4..6.
        assert_eq!(folds(src), vec![(0, 3), (4, 6)]);
    }

    #[test]
    fn single_line_body_does_not_fold() {
        let src = "fn id(n: number) -> number { n }\n";
        assert!(folds(src).is_empty());
    }

    #[test]
    fn folds_a_block_comment() {
        let src = "/*\n  a note\n*/\nfn main() {}\n";
        let kinds: Vec<_> = folding_ranges(src).into_iter().map(|f| f.kind).collect();
        assert_eq!(kinds, vec![Some(FoldingRangeKind::Comment)]);
    }
}

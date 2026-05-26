//! Document formatting: rewrite a document to neoc's canonical form via
//! `neoc-fmt`.
//!
//! The result is a single whole-document edit. A source with a syntax error is
//! left untouched — the formatter refuses to rewrite what it cannot parse, so
//! an in-progress edit is never mangled.

use neoc_fmt::format as format_source;
use tower_lsp::lsp_types::{Position, Range, TextEdit};

use crate::position::byte_to_position;

/// The edit that reformats `source`, or `None` when it is already canonical or
/// cannot be parsed.
#[must_use]
pub fn formatting(source: &str) -> Option<Vec<TextEdit>> {
    let formatted = format_source(source).ok()?;
    if formatted == source {
        return None;
    }
    let whole = Range::new(Position::new(0, 0), byte_to_position(source, source.len()));
    Some(vec![TextEdit {
        range: whole,
        new_text: formatted,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::position_to_byte;

    /// `source` with the formatting edit applied.
    fn formatted(source: &str) -> Option<String> {
        let edit = formatting(source)?.pop()?;
        let start = position_to_byte(source, edit.range.start);
        let end = position_to_byte(source, edit.range.end);
        let mut result = source.to_string();
        result.replace_range(start..end, &edit.new_text);
        Some(result)
    }

    #[test]
    fn reformats_a_messy_function() {
        let src = "fn  add(a:number,b:number)->number{a+b}\n";
        let out = formatted(src).expect("an edit");
        assert_eq!(
            out,
            "fn add(a: number, b: number) -> number {\n    a + b\n}\n"
        );
    }

    #[test]
    fn already_canonical_source_has_no_edit() {
        let src = "fn add(a: number, b: number) -> number {\n    a + b\n}\n";
        assert_eq!(formatting(src), None);
    }

    #[test]
    fn source_with_a_syntax_error_is_left_alone() {
        let src = "fn add(a: number) -> number { a +\n";
        assert_eq!(formatting(src), None);
    }
}

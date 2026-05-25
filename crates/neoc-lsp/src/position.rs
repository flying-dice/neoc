//! Position arithmetic between byte offsets and LSP positions.
//!
//! LSP positions are zero-based `(line, character)` pairs where `character`
//! counts UTF-16 code units within the line (`LSP` §"Position"). These
//! functions are pure and free of I/O so they can be unit-tested directly.

use tower_lsp::lsp_types::{Position, Range};

/// Converts a byte offset into the source to an LSP [`Position`].
///
/// The offset is clamped to the length of `source`.
#[must_use]
pub fn byte_to_position(source: &str, byte: usize) -> Position {
    let byte = byte.min(source.len());
    let mut line: u32 = 0;
    let mut line_start = 0usize;
    for (i, b) in source.bytes().enumerate() {
        if i >= byte {
            break;
        }
        if b == b'\n' {
            line = line.saturating_add(1);
            line_start = i + 1;
        }
    }
    let character =
        u32::try_from(source[line_start..byte].encode_utf16().count()).unwrap_or(u32::MAX);
    Position { line, character }
}

/// Converts an LSP [`Position`] to a byte offset into `source`.
///
/// The inverse of [`byte_to_position`]. A position past the end of a line
/// clamps to the line's end; a line past the end of the document clamps to the
/// document's length. `character` is interpreted as a count of UTF-16 code
/// units, matching the LSP spec.
#[must_use]
pub fn position_to_byte(source: &str, position: Position) -> usize {
    let mut line: u32 = 0;
    let mut line_start = 0usize;
    // Find the byte where the target line starts.
    for (i, b) in source.bytes().enumerate() {
        if line == position.line {
            break;
        }
        if b == b'\n' {
            line = line.saturating_add(1);
            line_start = i + 1;
        }
    }
    if line < position.line {
        // The requested line is past the end of the document.
        return source.len();
    }
    // Walk forward `character` UTF-16 code units within the line, stopping at
    // the line's newline or the document's end.
    let mut utf16: u32 = 0;
    let mut byte = line_start;
    for (offset, ch) in source[line_start..].char_indices() {
        if ch == '\n' || utf16 >= position.character {
            break;
        }
        utf16 = utf16.saturating_add(u32::try_from(ch.len_utf16()).unwrap_or(u32::MAX));
        byte = line_start + offset + ch.len_utf8();
    }
    byte
}

/// The LSP range spanning the half-open byte interval `[start, end)`.
#[must_use]
pub fn byte_range(source: &str, start: usize, end: usize) -> Range {
    Range::new(
        byte_to_position(source, start),
        byte_to_position(source, end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_to_position_ascii() {
        let src = "abc\ndef";
        assert_eq!(byte_to_position(src, 0), Position::new(0, 0));
        assert_eq!(byte_to_position(src, 3), Position::new(0, 3));
        assert_eq!(byte_to_position(src, 4), Position::new(1, 0));
        assert_eq!(byte_to_position(src, 6), Position::new(1, 2));
    }

    #[test]
    fn round_trip_ascii() {
        let src = "abc\ndef\nghi";
        for byte in 0..=src.len() {
            // Skip byte offsets that land inside the newline boundary; the
            // newline itself maps to a position past the line content, which
            // `position_to_byte` resolves to that same byte.
            let pos = byte_to_position(src, byte);
            assert_eq!(position_to_byte(src, pos), byte, "byte {byte}");
        }
    }

    #[test]
    fn round_trip_multibyte() {
        // `é` is two UTF-8 bytes, one UTF-16 unit; `🦀` is four UTF-8 bytes,
        // two UTF-16 units. Only char-boundary offsets are valid inputs.
        let src = "héllo\n🦀x";
        let bytes = [0usize, 1, 3, 4, 5, 6, 7, 11, 12];
        for &byte in &bytes {
            let pos = byte_to_position(src, byte);
            assert_eq!(position_to_byte(src, pos), byte, "byte {byte}");
        }
    }

    #[test]
    fn position_to_byte_clamps_line() {
        let src = "ab\ncd";
        // Character past the line end clamps to the newline.
        assert_eq!(position_to_byte(src, Position::new(0, 99)), 2);
    }

    #[test]
    fn position_to_byte_clamps_past_end() {
        let src = "ab";
        assert_eq!(position_to_byte(src, Position::new(9, 0)), 2);
    }
}

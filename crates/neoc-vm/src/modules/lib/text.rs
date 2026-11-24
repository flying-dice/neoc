//! String utilities and case conversion: `lib:text`.
//!
//! Counts and widths are unsigned integers (a negative one is refused as an argument), the options are typed tables
//! (`WrapOptions`, `TruncateOptions`, `ClosestOptions`) and `boundary` is a union of literals, the fill character of
//! the pad family is a validated type (refused as an argument), `closest` returns an empty list for no candidates and
//! cannot fail, `truncate` cannot fail, and the failures that remain are typed error tables.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

const ELLIPSIS: &str = "...";
const ELLIPSIS_WIDTH: usize = 3;

/// The widest a padded text may be, in columns: 1 Gi. A bigger request is an error rather than an allocation
/// failure that takes the whole process down.
const MAX_WIDTH: u32 = 1 << 30;

fn display_width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// Greedy wrap of one line of text into lines of at most `width` columns.
fn wrap_paragraph(para: &str, width: usize, break_words: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for word in para.split_whitespace() {
        let word_w = display_width(word);
        let projected = if current.is_empty() { word_w } else { current_w + 1 + word_w };

        if projected <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_w = projected;
            continue;
        }

        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }

        if word_w <= width || !break_words {
            // Fits on its own line, or `break_words` is off: overflow is allowed.
            current.push_str(word);
            current_w = word_w;
            continue;
        }

        // Break the oversize word into width-sized chunks; the last partial chunk stays in `current` so the next
        // word can join it.
        let (completed, tail, tail_w) = chunk_by_width(word, width);
        lines.extend(completed);
        current = tail;
        current_w = tail_w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join("\n")
}

/// Split `s` into chunks of at most `width` columns: the completed ones, then the last (possibly partial) one and
/// its width.
fn chunk_by_width(s: &str, width: usize) -> (Vec<String>, String, usize) {
    let mut completed = Vec::new();
    let mut buf = String::new();
    let mut buf_w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if !buf.is_empty() && buf_w + cw > width {
            completed.push(std::mem::take(&mut buf));
            buf_w = 0;
        }
        buf.push(c);
        buf_w += cw;
    }
    (completed, buf, buf_w)
}

fn wrap_text(text: &str, width: usize, break_words: bool, preserve_newlines: bool) -> String {
    if preserve_newlines {
        text.split('\n').map(|p| wrap_paragraph(p, width, break_words)).collect::<Vec<_>>().join("\n")
    } else {
        wrap_paragraph(&text.replace('\n', " "), width, break_words)
    }
}

fn take_by_width(s: &str, budget: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > budget {
            break;
        }
        out.push(c);
        used += cw;
    }
    out
}

/// The longest prefix within `budget` columns that ends at a word boundary (the whole prefix when it has none).
fn take_at_word(s: &str, budget: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    let mut last_ws_end: Option<usize> = None;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > budget {
            break;
        }
        if c.is_whitespace() {
            last_ws_end = Some(out.len());
        }
        out.push(c);
        used += cw;
    }
    match last_ws_end {
        Some(end) => out[..end].trim_end().to_string(),
        None => out,
    }
}

fn slugify_str(input: &str) -> String {
    let ascii = deunicode::deunicode(input);
    let mut out = String::with_capacity(ascii.len());
    let mut pending_hyphen = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
            pending_hyphen = false;
        } else {
            pending_hyphen = true;
        }
    }
    out
}

/// The byte offset where the grapheme at index `count` starts, or `None` when `s` has no more than `count`.
fn grapheme_offset(s: &str, count: usize) -> Option<usize> {
    s.grapheme_indices(true).nth(count).map(|(at, _)| at)
}

fn clip_str(text: &str, max: usize, ellipsis: &str) -> String {
    let Some(cut) = grapheme_offset(text, max) else {
        return text.to_owned();
    };
    // An ellipsis that does not fit in `max` is left out. Counting stops one past `max`: enough to know.
    let ellipsis_len = ellipsis.graphemes(true).take(max.saturating_add(1)).count();
    let Some(kept) = max.checked_sub(ellipsis_len) else {
        return text[..cut].to_owned();
    };
    let cut = grapheme_offset(text, kept).unwrap_or(cut);
    format!("{}{ellipsis}", &text[..cut])
}

/// What a terminal would show of the bytes `vte` parsed: the printed characters, line feeds and tabs.
struct Shown(String);

impl vte::Perform for Shown {
    fn print(&mut self, c: char) {
        // DEL and the C1 controls reach `print`; they are no more text than the C0 controls `execute` gets.
        if !c.is_control() {
            self.0.push(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if matches!(byte, b'\n' | b'\t') {
            self.0.push(char::from(byte));
        }
    }
}

fn strip_ansi_str(text: &str) -> String {
    let mut shown = Shown(String::new());
    // A parser per line: a string sequence (a window title, say) that is never ended would otherwise take every
    // line after it. The line feed is put back by hand: inside such a sequence the parser would take it as well.
    for (at, line) in text.split('\n').enumerate() {
        if at > 0 {
            shown.0.push('\n');
        }
        vte::Parser::new().advance(&mut shown, line.as_bytes());
    }
    shown.0
}

fn fill_char(text: &str) -> Result<(), String> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if UnicodeWidthChar::width(c) == Some(1) => Ok(()),
        (Some(_), None) => Err(format!("`{text}` is not a fill character: it must be one column wide")),
        _ => Err(format!("`{text}` is not a fill character: it must be exactly one character")),
    }
}

#[mlua_ir::module("lib:text")]
pub mod text {
    use heck::{
        ToKebabCase, ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase,
    };
    use mlua_ir::{Bytes, LuaError, Options, Shape, Text};
    use serde::{Deserialize, Serialize};

    use super::{
        ELLIPSIS, ELLIPSIS_WIDTH, MAX_WIDTH, clip_str, display_width, fill_char, slugify_str, strip_ansi_str,
        take_at_word, take_by_width, wrap_text,
    };

    /// Why a text function refused its arguments.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum TextError {
        /// A width of 0 leaves no room for any text.
        #[error("width must be greater than 0")]
        ZeroWidth,
        /// The width is more than one call may build.
        #[error("width {requested} is too large (the most one call may build is {max})")]
        TooLarge {
            /// The width asked for.
            requested: u32,
            /// The widest a call may build.
            max: u32,
        },
    }

    /// One character that is one display column wide: what the pad functions fill with.
    #[derive(Debug, Clone, Text)]
    #[text(validate = "fill_char")]
    pub struct Fill(pub String);

    /// Where `truncate` may cut.
    #[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Boundary {
        /// Anywhere: as many characters as fit.
        #[default]
        Char,
        /// Between words: the last whole word that fits.
        Word,
    }

    /// How `word_wrap` breaks lines.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct WrapOptions {
        /// Cut a word that is wider than the line into pieces (otherwise it overflows its line).
        pub break_words: bool,
        /// Keep the line breaks of the text (otherwise they are spaces, and the text is one paragraph).
        pub preserve_newlines: bool,
    }

    impl Default for WrapOptions {
        fn default() -> Self {
            // Newlines are kept by default: flattening them silently merges intentional paragraph breaks.
            Self { break_words: false, preserve_newlines: true }
        }
    }

    /// How `truncate` cuts.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, Options)]
    pub struct TruncateOptions {
        /// Where the text may be cut.
        pub boundary: Boundary,
    }

    /// How `closest` lists its matches.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, Options)]
    pub struct ClosestOptions {
        /// The most matches to return; all of them when not given.
        pub limit: Option<u32>,
    }

    /// A candidate and how far it is from the input.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Match {
        /// The candidate, as given.
        #[serde(rename = "match")]
        pub candidate: String,
        /// Its Levenshtein distance from the input, in characters.
        pub distance: u64,
    }

    // Case conversion: heck does the word-boundary detection and acronym handling.

    /// Convert to `camelCase`. Acronym boundaries are detected by the backing `heck` crate.
    ///
    /// ```luau
    /// assert(text.to_camel_case("hello_world") == "helloWorld")
    /// ```
    pub fn to_camel_case(text: String) -> String {
        text.to_lower_camel_case()
    }

    /// Convert to `snake_case`.
    ///
    /// ```luau
    /// assert(text.to_snake_case("HTTPSConnection") == "https_connection")
    /// ```
    pub fn to_snake_case(text: String) -> String {
        text.to_snake_case()
    }

    /// Convert to `kebab-case`.
    pub fn to_kebab_case(text: String) -> String {
        text.to_kebab_case()
    }

    /// Convert to `PascalCase`.
    pub fn to_pascal_case(text: String) -> String {
        text.to_upper_camel_case()
    }

    /// Convert to `Title Case`.
    pub fn to_title_case(text: String) -> String {
        text.to_title_case()
    }

    /// Convert to `CONSTANT_CASE`.
    pub fn to_constant_case(text: String) -> String {
        text.to_shouty_snake_case()
    }

    /// Greedy word wrap, display-width aware: `text` as lines of at most `width` columns. Wide characters count as
    /// 2, combining marks as 0.
    ///
    /// ```luau
    /// assert(assert(text.word_wrap("hello world", 5)) == "hello\nworld")
    /// ```
    pub fn word_wrap(text: String, width: u32, opts: WrapOptions) -> Result<String, TextError> {
        if width == 0 {
            return Err(TextError::ZeroWidth);
        }
        Ok(wrap_text(&text, width as usize, opts.break_words, opts.preserve_newlines))
    }

    /// Codepoint-based Levenshtein edit distance.
    ///
    /// ```luau
    /// assert(text.levenshtein("kitten", "sitting") == 3)
    /// ```
    pub fn levenshtein(a: String, b: String) -> u64 {
        strsim::levenshtein(&a, &b) as u64
    }

    /// Score each candidate against `input` by Levenshtein distance: the matches sorted nearest first, ties
    /// broken alphabetically. No candidates, no matches.
    ///
    /// ```luau
    /// local best = text.closest("teh", { "the", "tea", "ten" }, { limit = 1 })
    /// assert(best[1].match == "tea" and best[1].distance == 1)
    /// ```
    pub fn closest(input: String, candidates: Vec<String>, opts: ClosestOptions) -> Vec<Match> {
        let mut scored: Vec<Match> = candidates
            .into_iter()
            .map(|candidate| {
                let distance = strsim::levenshtein(&input, &candidate) as u64;
                Match { candidate, distance }
            })
            .collect();
        scored.sort_by(|a, b| a.distance.cmp(&b.distance).then_with(|| a.candidate.cmp(&b.candidate)));
        if let Some(limit) = opts.limit {
            scored.truncate(limit as usize);
        }
        scored
    }

    /// Truncate `text` so its display width is at most `max_len`. The `...` ellipsis counts toward the limit; when
    /// `max_len` cannot hold it, the text is cut with no ellipsis.
    ///
    /// ```luau
    /// assert(text.truncate("hello world", 8) == "hello...")
    /// ```
    pub fn truncate(text: String, max_len: u32, opts: TruncateOptions) -> String {
        let max = max_len as usize;
        if display_width(&text) <= max {
            return text;
        }
        if max < ELLIPSIS_WIDTH {
            return take_by_width(&text, max);
        }
        let budget = max - ELLIPSIS_WIDTH;
        let prefix = match opts.boundary {
            Boundary::Char => take_by_width(&text, budget),
            Boundary::Word => take_at_word(&text, budget),
        };
        format!("{prefix}{ELLIPSIS}")
    }

    fn padded(
        text: String,
        width: u32,
        fill: Option<Fill>,
        split: impl Fn(usize) -> (usize, usize),
    ) -> Result<String, TextError> {
        if width > MAX_WIDTH {
            return Err(TextError::TooLarge { requested: width, max: MAX_WIDTH });
        }
        let fill = fill.and_then(|f| f.0.chars().next()).unwrap_or(' ');
        let own = display_width(&text);
        let width = width as usize;
        if own >= width {
            return Ok(text);
        }
        let (left, right) = split(width - own);
        let mut out = String::with_capacity(text.len() + (width - own) * fill.len_utf8());
        out.extend(std::iter::repeat_n(fill, left));
        out.push_str(&text);
        out.extend(std::iter::repeat_n(fill, right));
        Ok(out)
    }

    /// Pad `text` on the left so its display width is `width` (default fill is a space).
    ///
    /// ```luau
    /// assert(assert(text.pad_left("42", 5, "0")) == "00042")
    /// ```
    pub fn pad_left(text: String, width: u32, fill: Option<Fill>) -> Result<String, TextError> {
        padded(text, width, fill, |pad| (pad, 0))
    }

    /// Pad `text` on the right so its display width is `width` (default fill is a space).
    pub fn pad_right(text: String, width: u32, fill: Option<Fill>) -> Result<String, TextError> {
        padded(text, width, fill, |pad| (0, pad))
    }

    /// Pad `text` on both sides so its display width is `width`. An uneven padding puts the extra column on the
    /// right.
    pub fn center(text: String, width: u32, fill: Option<Fill>) -> Result<String, TextError> {
        padded(text, width, fill, |pad| (pad / 2, pad - pad / 2))
    }

    /// Convert `text` to a URL-safe ASCII slug: transliterate with `deunicode`, lowercase, join words with hyphens.
    ///
    /// ```luau
    /// assert(text.slugify("Café résumé") == "cafe-resume")
    /// ```
    pub fn slugify(text: String) -> String {
        slugify_str(&text)
    }

    /// Remove Unicode white space (spaces, tabs, line breaks, U+00A0, U+3000, ...) from both ends of `text`. Text
    /// that is not valid UTF-8 is refused as an argument (repair it with `to_valid_utf8` first).
    pub fn trim(text: String) -> String {
        text.trim().to_owned()
    }

    /// Remove Unicode white space from the start of `text`.
    pub fn trim_start(text: String) -> String {
        text.trim_start().to_owned()
    }

    /// Remove Unicode white space from the end of `text`.
    pub fn trim_end(text: String) -> String {
        text.trim_end().to_owned()
    }

    /// Clip `text` to at most `max` characters. A character is an extended grapheme cluster (UAX #29, what
    /// `lib:unicode.graphemes` yields): a flag emoji or a letter with its combining accent is one, and is never cut
    /// apart. When `text` is cut and `ellipsis` is given, the ellipsis ends the result and counts toward `max`; an
    /// ellipsis longer than `max` is left out.
    ///
    /// ```luau
    /// assert(text.clip("hello world", 8, "...") == "hello...")
    /// ```
    pub fn clip(text: String, max: u64, ellipsis: Option<String>) -> String {
        clip_str(&text, usize::try_from(max).unwrap_or(usize::MAX), &ellipsis.unwrap_or_default())
    }

    /// Make any byte string valid UTF-8: bytes that are not part of a character become U+FFFD. Valid input is
    /// returned as it is.
    ///
    /// ```luau
    /// assert(text.to_valid_utf8("a\255b") == "a\u{FFFD}b")
    /// ```
    pub fn to_valid_utf8(bytes: Bytes) -> Bytes {
        match String::from_utf8_lossy(&bytes) {
            std::borrow::Cow::Borrowed(_) => bytes,
            std::borrow::Cow::Owned(repaired) => Bytes(repaired.into_bytes()),
        }
    }

    /// Remove what a terminal would act on instead of show: escape sequences (colours, cursor movement, window
    /// titles) and control characters. Line feeds and tabs stay, so `\r\n` becomes `\n`. Each line is read on its
    /// own, so a sequence that is never ended takes the rest of its line and no more.
    ///
    /// ```luau
    /// assert(text.strip_ansi("\27[31mred\27[0m") == "red")
    /// ```
    pub fn strip_ansi(text: String) -> String {
        strip_ansi_str(&text)
    }
}

#[cfg(test)]
mod tests {
    use heck::{
        ToKebabCase, ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase,
    };
    use mlua::Lua;

    use super::*;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:text")"#).eval().unwrap();
        lua.globals().set("text", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    #[test]
    fn snake_pins() {
        assert_eq!("helloWorld".to_snake_case(), "hello_world");
        assert_eq!("HTTPSConnection".to_snake_case(), "https_connection");
        assert_eq!("getHTTPSURL".to_snake_case(), "get_httpsurl");
        assert_eq!("XMLParser".to_snake_case(), "xml_parser");
        assert_eq!("".to_snake_case(), "");
        assert_eq!("a".to_snake_case(), "a");
        assert_eq!("already_snake".to_snake_case(), "already_snake");
        assert_eq!("kebab-case".to_snake_case(), "kebab_case");
    }

    #[test]
    fn case_edge_inputs_pinned() {
        assert_eq!("  spaces  ".to_snake_case(), "spaces");
        assert_eq!("  spaces  ".to_lower_camel_case(), "spaces");
        assert_eq!("  spaces  ".to_upper_camel_case(), "Spaces");
        assert_eq!("  spaces  ".to_kebab_case(), "spaces");
        assert_eq!("  spaces  ".to_shouty_snake_case(), "SPACES");
        assert_eq!("123numeric".to_snake_case(), "123numeric");
        assert_eq!("123numeric".to_lower_camel_case(), "123numeric");
        assert_eq!("123numeric".to_upper_camel_case(), "123numeric");
        assert_eq!("with.dots.in".to_snake_case(), "with_dots_in");
        assert_eq!("with.dots.in".to_lower_camel_case(), "withDotsIn");
        assert_eq!("with.dots.in".to_upper_camel_case(), "WithDotsIn");
        assert_eq!("MIXed_CASE-stuff".to_snake_case(), "mi_xed_case_stuff");
        assert_eq!("MIXed_CASE-stuff".to_lower_camel_case(), "miXedCaseStuff");
        assert_eq!("MIXed_CASE-stuff".to_upper_camel_case(), "MiXedCaseStuff");
        assert_eq!("helloWorld".to_title_case(), "Hello World");
        assert_eq!("HTTPSConnection".to_title_case(), "Https Connection");
    }

    #[test]
    fn levenshtein_pins() {
        assert_eq!(strsim::levenshtein("kitten", "sitting"), 3);
        assert_eq!(strsim::levenshtein("", "abc"), 3);
        assert_eq!(strsim::levenshtein("abc", ""), 3);
        assert_eq!(strsim::levenshtein("abc", "abc"), 0);
        assert_eq!(strsim::levenshtein("", ""), 0);
        assert_eq!(strsim::levenshtein("kitten", "sitting"), strsim::levenshtein("sitting", "kitten"));
        assert_eq!(eval::<u64>(r#"return text.levenshtein("kitten", "sitting")"#), 3);
    }

    #[test]
    fn slugify_pins() {
        assert_eq!(slugify_str("Hello, World!"), "hello-world");
        assert_eq!(slugify_str("  leading trailing  "), "leading-trailing");
        assert_eq!(slugify_str("---multiple---hyphens---"), "multiple-hyphens");
        assert_eq!(slugify_str(""), "");
        assert_eq!(slugify_str("café résumé"), "cafe-resume");
        assert_eq!(eval::<String>(r#"return text.slugify("Café résumé")"#), "cafe-resume");
    }

    #[test]
    fn word_wrap_basic() {
        assert_eq!(eval::<String>(r#"return (text.word_wrap("hello world", 5))"#), "hello\nworld");
    }

    #[test]
    fn word_wrap_break_words() {
        assert_eq!(wrap_text("abcdefghij", 5, true, true), "abcde\nfghij");
        assert_eq!(
            eval::<String>(r#"return (text.word_wrap("abcdefghij", 5, { break_words = true }))"#),
            "abcde\nfghij"
        );
    }

    #[test]
    fn word_wrap_break_words_disabled_overflows() {
        assert_eq!(wrap_text("abcdefghij", 5, false, true), "abcdefghij");
    }

    #[test]
    fn word_wrap_preserve_newlines() {
        assert_eq!(wrap_text("line1\nline2", 80, false, true), "line1\nline2");
        assert_eq!(wrap_text("line1\nline2", 80, false, false), "line1 line2");
        assert_eq!(
            eval::<String>(r#"return (text.word_wrap("line1\nline2", 80, { preserve_newlines = false }))"#),
            "line1 line2"
        );
    }

    #[test]
    fn word_wrap_counts_display_width() {
        // Each ideograph is two columns: three of them do not fit a width of 5.
        assert_eq!(eval::<String>(r#"return (text.word_wrap("世界 你", 5))"#), "世界\n你");
    }

    #[test]
    fn word_wrap_zero_width_is_a_typed_error() {
        let (val, kind, op): (Option<String>, String, String) =
            eval(r#"local v, e = text.word_wrap("hi", 0) return v, e.kind, e.op"#);
        assert!(val.is_none());
        assert_eq!((kind.as_str(), op.as_str()), ("zero_width", "lib:text.word_wrap"));
    }

    #[test]
    fn word_wrap_refuses_negative_width_and_unknown_options() {
        assert!(refused(r#"return text.word_wrap("hi", -1)"#).contains("argument #2 'width'"));
        assert!(refused(r#"return text.word_wrap("hi", 5, { break_word = true })"#).contains("opts"));
    }

    #[test]
    fn truncate_char_boundary() {
        assert_eq!(eval::<String>(r#"return text.truncate("hello world", 8)"#), "hello...");
    }

    #[test]
    fn truncate_word_boundary() {
        assert_eq!(
            eval::<String>(r#"return text.truncate("hello world", 8, { boundary = "word" })"#),
            "hello..."
        );
        assert_eq!(
            eval::<String>(r#"return text.truncate("hello big world", 13, { boundary = "word" })"#),
            "hello big..."
        );
    }

    #[test]
    fn truncate_unknown_boundary_is_refused() {
        assert!(
            refused(r#"return text.truncate("hello", 3, { boundary = "line" })"#)
                .contains("argument #3 'opts'")
        );
    }

    #[test]
    fn truncate_no_op_when_short() {
        assert_eq!(eval::<String>(r#"return text.truncate("hi", 8)"#), "hi");
    }

    #[test]
    fn truncate_max_below_ellipsis_drops_ellipsis() {
        assert_eq!(eval::<String>(r#"return text.truncate("hello", 2)"#), "he");
        assert_eq!(eval::<String>(r#"return text.truncate("hello", 3)"#), "...");
        assert_eq!(eval::<String>(r#"return text.truncate("hello", 0)"#), "");
    }

    #[test]
    fn pad_left_default_and_custom_fill() {
        assert_eq!(eval::<String>(r#"return (text.pad_left("42", 5))"#), "   42");
        assert_eq!(eval::<String>(r#"return (text.pad_left("42", 5, "0"))"#), "00042");
    }

    #[test]
    fn pad_right_and_center() {
        assert_eq!(eval::<String>(r#"return (text.pad_right("name", 10))"#), "name      ");
        assert_eq!(eval::<String>(r#"return (text.center("title", 11))"#), "   title   ");
        // 5 - 2 = 3: one column on the left, the extra one on the right.
        assert_eq!(eval::<String>(r#"return (text.center("ab", 5))"#), " ab  ");
    }

    #[test]
    fn pad_counts_display_width_and_never_shortens() {
        assert_eq!(eval::<String>(r#"return (text.pad_left("世", 4, "."))"#), "..世");
        assert_eq!(eval::<String>(r#"return (text.pad_left("long text", 3))"#), "long text");
    }

    #[test]
    fn pad_refuses_a_bad_fill_as_an_argument() {
        assert!(refused(r#"return text.pad_left("x", 5, "ab")"#).contains("exactly one character"));
        assert!(refused(r#"return text.pad_left("x", 5, "")"#).contains("exactly one character"));
        assert!(refused(r#"return text.pad_left("x", 5, "あ")"#).contains("one column wide"));
    }

    #[test]
    fn pad_too_wide_is_a_typed_error() {
        let (val, kind, requested): (Option<String>, String, u32) =
            eval(r#"local v, e = text.pad_right("x", 4000000000) return v, e.kind, e.requested"#);
        assert!(val.is_none());
        assert_eq!((kind.as_str(), requested), ("too_large", 4_000_000_000));
    }

    #[test]
    fn closest_orders_by_distance_then_alphabet() {
        // "tea" and "ten" are 1 from "teh"; "the" is 2.
        let (first, second, third, d1, d3): (String, String, String, u64, u64) = eval(
            r#"
            local r = text.closest("teh", {"the", "tea", "ten"})
            return r[1].match, r[2].match, r[3].match, r[1].distance, r[3].distance
        "#,
        );
        assert_eq!((first.as_str(), second.as_str(), third.as_str()), ("tea", "ten", "the"));
        assert_eq!((d1, d3), (1, 2));
    }

    #[test]
    fn closest_limit() {
        assert_eq!(eval::<usize>(r#"return #text.closest("teh", {"the", "tea", "ten"}, { limit = 2 })"#), 2);
        assert_eq!(eval::<usize>(r#"return #text.closest("x", {"a", "b"}, { limit = 0 })"#), 0);
    }

    #[test]
    fn closest_no_candidates_is_an_empty_list() {
        assert_eq!(eval::<usize>(r#"return #text.closest("x", {})"#), 0);
    }

    #[test]
    fn closest_refuses_non_string_candidates_and_negative_limit() {
        assert!(refused(r#"return text.closest("x", {1})"#).contains("argument #2 'candidates'"));
        assert!(refused(r#"return text.closest("x", {"a"}, { limit = -1 })"#).contains("argument #3 'opts'"));
    }

    #[test]
    fn trim_removes_unicode_white_space() {
        assert_eq!(eval::<String>("return text.trim(\"\\u{3000} a b \\u{a0}\\n\")"), "a b");
        assert_eq!(eval::<String>(r#"return text.trim_start("  a ")"#), "a ");
        assert_eq!(eval::<String>(r#"return text.trim_end("  a ")"#), "  a");
    }

    #[test]
    fn trim_refuses_invalid_utf8() {
        assert!(refused(r#"return text.trim("a\255")"#).contains("argument #1 'text'"));
    }

    #[test]
    fn clip_counts_graphemes() {
        assert_eq!(clip_str("hello world", 8, "..."), "hello...");
        assert_eq!(clip_str("hi", 8, "..."), "hi");
        // A flag is one character, never cut apart; an ellipsis longer than `max` is left out.
        assert_eq!(clip_str("🇳🇴🇳🇴🇳🇴", 2, ""), "🇳🇴🇳🇴");
        assert_eq!(clip_str("abcdef", 2, "..."), "ab");
        assert_eq!(clip_str("e\u{301}e\u{301}e\u{301}", 2, ""), "e\u{301}e\u{301}");
        assert_eq!(eval::<String>(r#"return text.clip("hello world", 8, "...")"#), "hello...");
        assert_eq!(eval::<String>(r#"return text.clip("hello", 0)"#), "");
    }

    #[test]
    fn clip_refuses_a_max_that_is_not_a_count() {
        assert!(refused(r#"return text.clip("x", -1)"#).contains("argument #2 'max'"));
        assert!(refused(r#"return text.clip("x", 1.5)"#).contains("argument #2 'max'"));
        assert!(refused(r#"return text.clip("x", 0/0)"#).contains("argument #2 'max'"));
    }

    #[test]
    fn to_valid_utf8_repairs_only_what_is_broken() {
        let lua = engine();
        let repaired: mlua::LuaString = lua.load(r#"return text.to_valid_utf8("a\255b")"#).eval().unwrap();
        assert_eq!(&*repaired.as_bytes(), "a\u{FFFD}b".as_bytes());
        assert_eq!(eval::<String>(r#"return text.to_valid_utf8("fine")"#), "fine");
    }

    #[test]
    fn strip_ansi_keeps_what_a_terminal_shows() {
        assert_eq!(strip_ansi_str("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi_str("a\r\nb\tc"), "a\nb\tc");
        assert_eq!(strip_ansi_str("a\x1b]0;title\x07b"), "ab");
        // A sequence that is never ended takes the rest of its line and no more.
        assert_eq!(strip_ansi_str("a\x1b]0;never\nb"), "a\nb");
        assert_eq!(strip_ansi_str("a\u{85}b\x7f"), "ab");
        assert_eq!(eval::<String>(r#"return text.strip_ansi("\27[1mbold\27[0m")"#), "bold");
    }

    #[test]
    fn a_non_string_argument_is_refused_with_its_place() {
        assert!(
            refused("return text.slugify(1)")
                .contains("lib:text.slugify: argument #1 'text': expected a string")
        );
    }
}

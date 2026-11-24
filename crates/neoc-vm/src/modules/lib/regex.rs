//! Regular expressions: `lib:regex`.
//!
//! A compiled, reusable `Pattern` plus flat convenience functions (`is_match`, `find`, `find_all`, `replace`,
//! `replace_all`, `split`, `escape`) that compile on every call. Backed by the `regex` crate: **linear-time, no
//! catastrophic backtracking**. Lookaround and backreferences are deliberately unsupported; that guarantee is the
//! design point. Unicode matching is on and cannot be turned off. Flags can also be written inline in the pattern
//! (`(?i)hello`).
//!
//! The flags are a typed options table (`{ case_insensitive = true }`), a compile failure is a typed error (`syntax`,
//! `too_big`), a negative split limit is refused as an argument, a `Match` is a typed record whose groups are a union
//! of `matched` (with its span) and `unmatched`, and `Pattern` has a `source` field.

/// 1-based, inclusive byte positions as `string.find` reports them: an empty match at byte `at` is
/// `(at + 1, at)`.
fn span(start: usize, end: usize) -> (u64, u64) {
    (start as u64 + 1, end as u64)
}

#[mlua_ir::module("lib:regex")]
pub mod regex {
    use std::collections::BTreeMap;

    use mlua_ir::{LuaError, Object, Options, Shape};
    use regex::{Captures, Regex, RegexBuilder};
    use serde::{Deserialize, Serialize};

    use super::span;

    /// Why a pattern could not be compiled.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum RegexError {
        /// The pattern is not valid regex syntax (an unclosed group, an unsupported lookaround, ...).
        #[error("{reason}")]
        Syntax {
            /// The compiler's explanation, with the place in the pattern.
            reason: String,
        },
        /// The compiled pattern would be larger than the compiler's size limit.
        #[error("the compiled pattern exceeds the size limit of {limit} bytes")]
        TooBig {
            /// The limit, in bytes.
            limit: u64,
        },
    }

    impl From<regex::Error> for RegexError {
        fn from(e: regex::Error) -> Self {
            match e {
                regex::Error::CompiledTooBig(limit) => RegexError::TooBig { limit: limit as u64 },
                other => RegexError::Syntax { reason: other.to_string() },
            }
        }
    }

    /// How a pattern is read. Each flag can also be switched on inside the pattern: `(?i)`, `(?m)`, `(?s)`, `(?x)`.
    #[derive(Debug, Default, Clone, Serialize, Deserialize, Options)]
    pub struct Flags {
        /// Letters match in either case.
        pub case_insensitive: bool,
        /// `^` and `$` match at line breaks, not only at the ends of the text.
        pub multi_line: bool,
        /// `.` matches a line break too.
        pub dot_matches_newline: bool,
        /// White space in the pattern is ignored and `#` starts a comment (free-spacing patterns).
        pub ignore_whitespace: bool,
    }

    /// One capture group of a match.
    #[derive(Debug, Clone, Serialize, Shape)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Group {
        /// The group took part in the match.
        Matched {
            /// What it matched.
            text: String,
            /// The first byte, counting from 1.
            start: u64,
            /// The last byte, counting from 1 (inclusive).
            finish: u64,
        },
        /// An optional group that did not take part (`(a)?` against `b`). The list stays dense: a group is never
        /// missing from `groups`.
        Unmatched,
    }

    /// A match of a pattern in a text.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Match {
        /// The whole match.
        pub text: String,
        /// The first byte of the match, counting from 1, as `string.find` counts. Byte offsets, not characters.
        pub start: u64,
        /// The last byte of the match (inclusive); `start - 1` for an empty match.
        pub finish: u64,
        /// The capture groups in order, group 0 (the whole match) left out.
        pub groups: Vec<Group>,
        /// The named groups that took part, by name.
        pub named: BTreeMap<String, String>,
    }

    fn compile(pattern: &str, flags: &Flags) -> Result<Regex, RegexError> {
        let mut builder = RegexBuilder::new(pattern);
        builder
            .case_insensitive(flags.case_insensitive)
            .multi_line(flags.multi_line)
            .dot_matches_new_line(flags.dot_matches_newline)
            .ignore_whitespace(flags.ignore_whitespace);
        Ok(builder.build()?)
    }

    fn to_match(re: &Regex, caps: &Captures<'_>) -> Match {
        let whole = caps.get(0).expect("group 0 always takes part");
        let (start, finish) = span(whole.start(), whole.end());
        let groups = (1..caps.len())
            .map(|i| match caps.get(i) {
                Some(m) => {
                    let (start, finish) = span(m.start(), m.end());
                    Group::Matched { text: m.as_str().to_owned(), start, finish }
                }
                None => Group::Unmatched,
            })
            .collect();
        let named = re
            .capture_names()
            .flatten()
            .filter_map(|name| caps.name(name).map(|m| (name.to_owned(), m.as_str().to_owned())))
            .collect();
        Match { text: whole.as_str().to_owned(), start, finish, groups, named }
    }

    fn first(re: &Regex, text: &str) -> Option<Match> {
        re.captures(text).map(|caps| to_match(re, &caps))
    }

    fn all(re: &Regex, text: &str) -> Vec<Match> {
        re.captures_iter(text).map(|caps| to_match(re, &caps)).collect()
    }

    fn parts(re: &Regex, text: &str, limit: Option<u32>) -> Vec<String> {
        match limit {
            None | Some(0) => re.split(text).map(str::to_owned).collect(),
            Some(n) => re.splitn(text, n as usize).map(str::to_owned).collect(),
        }
    }

    /// A compiled pattern, reusable. `tostring` gives its source.
    #[derive(Debug, Clone, Object)]
    pub struct Pattern {
        source: String,
        compiled: Regex,
    }

    #[mlua_ir::object]
    impl Pattern {
        /// Compile `pattern`.
        ///
        /// ```luau
        /// local p = assert(regex.Pattern.new("hello", { case_insensitive = true }))
        /// assert(p:is_match("Hello, world!"))
        /// local _, err = regex.Pattern.new("(abc")
        /// assert(err ~= nil and err.kind == "syntax")
        /// ```
        pub fn new(pattern: String, flags: Flags) -> Result<Pattern, RegexError> {
            let compiled = compile(&pattern, &flags)?;
            Ok(Pattern { source: pattern, compiled })
        }

        /// The pattern as it was written.
        #[get]
        pub fn source(&self) -> String {
            self.source.clone()
        }

        /// The pattern as it was written.
        #[to_string]
        pub fn display(&self) -> String {
            self.source.clone()
        }

        /// `true` when `text` contains at least one match.
        pub fn is_match(&self, text: String) -> bool {
            self.compiled.is_match(&text)
        }

        /// The first match in `text`, or `nil` when there is none.
        pub fn find(&self, text: String) -> Option<Match> {
            first(&self.compiled, &text)
        }

        /// Every non-overlapping match in `text`, in order; empty when nothing matches.
        pub fn find_all(&self, text: String) -> Vec<Match> {
            all(&self.compiled, &text)
        }

        /// Replace the first match in `text`. The replacement reads `$0`, `$1` and `${name}` as groups (`$$` is a
        /// dollar sign).
        pub fn replace(&self, text: String, replacement: String) -> String {
            self.compiled.replace(&text, replacement.as_str()).into_owned()
        }

        /// Replace every match; the replacement reads groups as in `replace`.
        pub fn replace_all(&self, text: String, replacement: String) -> String {
            self.compiled.replace_all(&text, replacement.as_str()).into_owned()
        }

        /// Split `text` on every match. `limit` caps the number of parts (the last holds the rest); `0` or `nil`
        /// is no cap. A match at either end leaves an empty part there.
        pub fn split(&self, text: String, limit: Option<u32>) -> Vec<String> {
            parts(&self.compiled, &text, limit)
        }
    }

    /// Compile `pattern` and test `text` in one call.
    ///
    /// ```luau
    /// assert(assert(regex.is_match("\\d+", "abc 123")))
    /// ```
    pub fn is_match(pattern: String, text: String, flags: Flags) -> Result<bool, RegexError> {
        Ok(compile(&pattern, &flags)?.is_match(&text))
    }

    /// Compile `pattern` and find its first match in `text`: the match, `nil` when there is none, or the error.
    ///
    /// ```luau
    /// local m = assert(regex.find("(\\d+)-(\\d+)", "size 12-34 px"))
    /// assert(m.text == "12-34" and m.start == 6 and m.finish == 10)
    /// assert(m.groups[1].kind == "matched" and m.groups[1].text == "12")
    /// ```
    pub fn find(pattern: String, text: String, flags: Flags) -> Result<Option<Match>, RegexError> {
        Ok(first(&compile(&pattern, &flags)?, &text))
    }

    /// Compile `pattern` and find every match in `text`.
    pub fn find_all(pattern: String, text: String, flags: Flags) -> Result<Vec<Match>, RegexError> {
        Ok(all(&compile(&pattern, &flags)?, &text))
    }

    /// Replace the first match. The replacement reads `$0`, `$1` and `${name}` as groups.
    pub fn replace(
        pattern: String,
        text: String,
        replacement: String,
        flags: Flags,
    ) -> Result<String, RegexError> {
        Ok(compile(&pattern, &flags)?.replace(&text, replacement.as_str()).into_owned())
    }

    /// Replace every match; the replacement reads groups as in `replace`.
    ///
    /// ```luau
    /// assert(assert(regex.replace_all("(\\w+)=(\\w+)", "a=1 b=2", "$2=$1")) == "1=a 2=b")
    /// ```
    pub fn replace_all(
        pattern: String,
        text: String,
        replacement: String,
        flags: Flags,
    ) -> Result<String, RegexError> {
        Ok(compile(&pattern, &flags)?.replace_all(&text, replacement.as_str()).into_owned())
    }

    /// Split `text` on every match of `pattern`; `limit` caps the number of parts (`0` or `nil` is no cap).
    pub fn split(
        pattern: String,
        text: String,
        limit: Option<u32>,
        flags: Flags,
    ) -> Result<Vec<String>, RegexError> {
        Ok(parts(&compile(&pattern, &flags)?, &text, limit))
    }

    /// Escape every regex metacharacter in `text`, so the result matches `text` literally.
    pub fn escape(text: String) -> String {
        regex::escape(&text)
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:regex")"#).eval().unwrap();
        lua.globals().set("regex", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    #[test]
    fn is_match_basic() {
        assert!(eval::<bool>(r#"return (regex.is_match("\\d+", "abc 123 def"))"#));
        assert!(!eval::<bool>(r#"return (regex.is_match("\\d+", "abc def"))"#));
    }

    #[test]
    fn invalid_pattern_is_a_typed_error() {
        let (val, kind, op, text): (Option<bool>, String, String, String) =
            eval(r#"local v, e = regex.is_match("(abc", "anything") return v, e.kind, e.op, tostring(e)"#);
        assert!(val.is_none());
        assert_eq!((kind.as_str(), op.as_str()), ("syntax", "lib:regex.is_match"));
        assert!(text.starts_with("lib:regex.is_match: syntax:"), "{text}");
    }

    #[test]
    fn lookaround_is_a_syntax_error() {
        assert_eq!(eval::<String>(r#"local _, e = regex.Pattern.new("a(?=b)") return e.kind"#), "syntax");
    }

    #[test]
    fn a_pattern_over_the_size_limit_is_too_big() {
        let (kind, limit): (String, u64) =
            eval(r#"local _, e = regex.Pattern.new("(?:(?:a{100}){100}){100}") return e.kind, e.limit"#);
        assert_eq!(kind, "too_big");
        assert!(limit > 0);
    }

    #[test]
    fn find_returns_a_match_with_positions() {
        let (text, start, finish): (String, u64, u64) = eval(
            r#"
            local m = regex.find("\\d+", "abc 123 def")
            return m.text, m.start, m.finish
        "#,
        );
        // "abc " is 4 bytes: the match starts at byte 5 (1-based) and ends at 7.
        assert_eq!((text.as_str(), start, finish), ("123", 5, 7));
    }

    #[test]
    fn find_without_a_match_is_nil_and_nil() {
        let (val, err): (mlua::Value, mlua::Value) = eval(r#"return regex.find("zzz", "abc")"#);
        assert!(val.is_nil() && err.is_nil());
    }

    #[test]
    fn positions_agree_with_string_find() {
        let same: bool = eval(
            r#"
            local s = "abc 123 def 456"
            local s1, e1 = string.find(s, "123", 1, true)
            local m = regex.find("123", s)
            return s1 == m.start and e1 == m.finish
        "#,
        );
        assert!(same);
    }

    #[test]
    fn an_empty_match_ends_before_it_starts() {
        let (start, finish): (u64, u64) = eval(r#"local m = regex.find("", "abc") return m.start, m.finish"#);
        assert_eq!((start, finish), (1, 0));
    }

    #[test]
    fn find_all_returns_every_match() {
        let texts: Vec<String> = eval(
            r#"
            local out = {}
            for i, m in ipairs(regex.find_all("\\d+", "1 22 333 4444")) do out[i] = m.text end
            return out
        "#,
        );
        assert_eq!(texts, ["1", "22", "333", "4444"]);
        assert_eq!(eval::<usize>(r#"return #regex.find_all("x", "abc")"#), 0);
    }

    #[test]
    fn positional_groups_carry_their_span() {
        let (whole, kind, first, start, finish, second): (String, String, String, u64, u64, String) = eval(
            r#"
            local m = regex.find("(\\d+)-(\\d+)", "size 12-34 px")
            local g = m.groups[1]
            return m.text, g.kind, g.text, g.start, g.finish, m.groups[2].text
        "#,
        );
        assert_eq!((whole.as_str(), kind.as_str(), first.as_str()), ("12-34", "matched", "12"));
        assert_eq!((start, finish, second.as_str()), (6, 7, "34"));
    }

    #[test]
    fn an_unmatched_optional_group_keeps_the_list_dense() {
        let (first, second, len, iterated): (String, String, usize, usize) = eval(
            r#"
            local m = regex.find("(a)?(b)", "b")
            local n = 0
            for _ in ipairs(m.groups) do n = n + 1 end
            return m.groups[1].kind, m.groups[2].text, #m.groups, n
        "#,
        );
        assert_eq!((first.as_str(), second.as_str(), len, iterated), ("unmatched", "b", 2, 2));
    }

    #[test]
    fn named_groups_hold_only_those_that_took_part() {
        let (year, month, day, count): (String, String, String, usize) = eval(
            r#"
            local m = regex.find("(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})", "date 2026-05-19 done")
            local n = 0
            for _ in m.named do n = n + 1 end
            return m.named.year, m.named.month, m.named.day, n
        "#,
        );
        assert_eq!((year.as_str(), month.as_str(), day.as_str(), count), ("2026", "05", "19", 3));
        assert_eq!(
            eval::<usize>(
                r#"local m = regex.find("(?P<a>x)?(?P<b>y)", "y") local n = 0 for _ in m.named do n += 1 end return n"#
            ),
            1
        );
    }

    #[test]
    fn flag_case_insensitive() {
        assert!(eval::<bool>(
            r#"return (regex.is_match("hello", "Hello, world!", { case_insensitive = true }))"#
        ));
        assert!(!eval::<bool>(r#"return (regex.is_match("hello", "Hello, world!"))"#));
    }

    #[test]
    fn inline_flag_is_the_same_as_the_option() {
        let (a, b): (bool, bool) = eval(
            r#"return (regex.is_match("hello", "Hello", { case_insensitive = true })), (regex.is_match("(?i)hello", "Hello"))"#,
        );
        assert!(a && b);
    }

    #[test]
    fn flag_multi_line_anchors() {
        let (no, yes): (bool, bool) = eval(
            r#"
            local s = "first\nsecond"
            return (regex.is_match("^second$", s)), (regex.is_match("^second$", s, { multi_line = true }))
        "#,
        );
        assert!(!no, "^ and $ anchor the whole text by default");
        assert!(yes);
    }

    #[test]
    fn flag_dot_matches_newline() {
        let (no, yes): (bool, bool) = eval(
            r#"
            local s = "a\nb"
            return (regex.is_match("a.b", s)), (regex.is_match("a.b", s, { dot_matches_newline = true }))
        "#,
        );
        assert!(!no && yes);
    }

    #[test]
    fn flag_ignore_whitespace() {
        assert!(eval::<bool>(r#"return (regex.is_match("a b # c", "ab", { ignore_whitespace = true }))"#));
    }

    #[test]
    fn an_unknown_flag_is_refused_as_an_argument() {
        let err = refused(r#"return regex.is_match("a", "a", { ignore_case = true })"#);
        assert!(err.contains("lib:regex.is_match: argument #3 'flags'"), "{err}");
    }

    #[test]
    fn replace_first_only_and_all() {
        assert_eq!(eval::<String>(r#"return (regex.replace("a", "aaa", "X"))"#), "Xaa");
        assert_eq!(eval::<String>(r#"return (regex.replace_all("a", "aaa", "X"))"#), "XXX");
    }

    #[test]
    fn replace_uses_capture_references() {
        let (positional, named): (String, String) = eval(
            r#"
            return (regex.replace_all("(\\w+)=(\\w+)", "a=1 b=2", "$2=$1")),
                   (regex.replace_all("(?P<k>\\w+)=(?P<v>\\w+)", "x=10", "${v}=${k}"))
        "#,
        );
        assert_eq!((positional.as_str(), named.as_str()), ("1=a 2=b", "10=x"));
    }

    #[test]
    fn split_basic_keeps_empty_ends() {
        let parts: Vec<String> = eval(r#"return (regex.split("\\s+", "  one  two   three "))"#);
        assert_eq!(parts, ["", "one", "two", "three", ""]);
    }

    #[test]
    fn split_limit() {
        let two: Vec<String> = eval(r#"return (regex.split(",", "a,b,c,d", 2))"#);
        assert_eq!(two, ["a", "b,c,d"]);
        let all: Vec<String> = eval(r#"return (regex.split(",", "a,b,c", 0))"#);
        assert_eq!(all, ["a", "b", "c"]);
    }

    #[test]
    fn a_negative_split_limit_is_refused_as_an_argument() {
        let err = refused(r#"return regex.split(",", "a,b", -1)"#);
        assert!(err.contains("lib:regex.split: argument #3 'limit'"), "{err}");
        let err = refused(r#"return regex.Pattern.new(","):split("a,b", -1)"#);
        assert!(err.contains("lib:regex.Pattern:split: argument #2 'limit'"), "{err}");
    }

    #[test]
    fn escape_neutralises_meta_chars() {
        assert!(eval::<bool>(
            r#"
            local raw = "1.2.3+build (rc)"
            return (regex.is_match(regex.escape(raw), raw))
        "#
        ));
        assert_eq!(eval::<String>(r#"return regex.escape("a.b")"#), r"a\.b");
    }

    #[test]
    fn pattern_is_reusable() {
        let total: i64 = eval(
            r#"
            local p = assert(regex.Pattern.new("\\d+"))
            if p:is_match("nope") then return -1 end
            return #p:find_all("1 22 333")
        "#,
        );
        assert_eq!(total, 3);
    }

    #[test]
    fn pattern_source_and_tostring() {
        let (source, text): (String, String) =
            eval(r#"local p = assert(regex.Pattern.new("\\d+")) return p.source, tostring(p)"#);
        assert_eq!((source.as_str(), text.as_str()), (r"\d+", r"\d+"));
    }

    #[test]
    fn pattern_new_invalid_is_a_typed_error() {
        let (val, kind, op): (mlua::Value, String, String) =
            eval(r#"local v, e = regex.Pattern.new("(abc") return v, e.kind, e.op"#);
        assert!(val.is_nil());
        assert_eq!((kind.as_str(), op.as_str()), ("syntax", "lib:regex.Pattern.new"));
    }

    #[test]
    fn pattern_find_returns_one_value() {
        let (hit, miss): (i64, i64) = eval(
            r##"
            local p = assert(regex.Pattern.new("\\d+"))
            return select("#", p:find("abc 123")), select("#", p:find("abc"))
        "##,
        );
        assert_eq!((hit, miss), (1, 1));
    }

    #[test]
    fn pattern_with_flags() {
        assert!(eval::<bool>(
            r#"return assert(regex.Pattern.new("hello", { case_insensitive = true })):is_match("HELLO")"#
        ));
    }

    #[test]
    fn unicode_matching_is_on_by_default() {
        assert!(eval::<bool>(r#"return (regex.is_match("^\\w+$", "café"))"#));
    }

    #[test]
    fn positions_are_byte_offsets() {
        // "caf" is 3 bytes, so the two-byte é is bytes 4 to 5.
        let (start, finish): (u64, u64) =
            eval(r#"local m = regex.find("é", "café X") return m.start, m.finish"#);
        assert_eq!((start, finish), (4, 5));
    }

    #[test]
    fn every_find_all_match_matches_again() {
        assert!(eval::<bool>(
            r#"
            local pat = "\\w{3,}"
            for _, m in ipairs(regex.find_all(pat, "the quick brown fox")) do
                if not regex.is_match(pat, m.text) then return false end
            end
            return true
        "#
        ));
    }
}

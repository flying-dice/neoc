//! INI files: `lib:ini`.
//!
//! Backed by the `rust-ini` crate, behind a pass that normalises the dialect first: comment and delimiter symbols
//! of the caller's choosing, bare keys (a key with no value is a flag), and the characters rust-ini would read its
//! own way (`:` and `=` as delimiters, `;` and `#` as comments) where the caller's dialect does not.
//!
//! Ported from the old VM's `lib:ini`. What changed on purpose:
//! - A document is typed: sections of keys whose values are a string or `true`; `stringify` takes strings,
//!   booleans and numbers, and anything else is refused as an argument.
//! - `stringify` cannot fail, so it returns the text alone, and it writes sections and keys in sorted order (it
//!   was the table's own order, which changes from run to run).
//! - A comment symbol or delimiter is the whole string given (it was its first character).
//! - With symbols of the caller's choosing, `;`, `#` and `=` are plain characters (a line starting with `;` was
//!   still a comment, and a `=` still split the line).
//! - A failure is a typed `ParseError` (`control_byte`, `unclosed_section`, `empty_section_name`, `syntax`) with
//!   the line as a number.

// Stand-ins the normalising pass writes for rust-ini and takes back out. Safe because `parse` refuses these bytes
// in its input.
const BARE: &str = "\u{1}BARE\u{1}";

/// The characters rust-ini gives a meaning of its own (a delimiter, a comment), and what stands in for each while
/// it parses: after normalising, the only `=` on a line is the delimiter and no line starts a comment by accident.
const STAND_INS: [(char, char); 4] = [(':', '\u{2}'), ('=', '\u{3}'), (';', '\u{4}'), ('#', '\u{5}')];

/// The highest byte `parse` refuses: the stand-ins above are among them.
const LAST_CONTROL_BYTE: u8 = 0x08;

fn escape(text: &str) -> String {
    text.chars().map(|c| STAND_INS.iter().find(|(plain, _)| *plain == c).map_or(c, |(_, stand_in)| *stand_in)).collect()
}

fn unescape(text: &str) -> String {
    text.chars().map(|c| STAND_INS.iter().find(|(_, stand_in)| *stand_in == c).map_or(c, |(plain, _)| *plain)).collect()
}

fn wide(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[mlua_ir::module("lib:ini")]
pub mod ini {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use ::ini::Ini;
    use mlua_ir::{LuaError, OneOf, Options};
    use serde::{Deserialize, Serialize};

    use super::{BARE, LAST_CONTROL_BYTE, escape, unescape, wide};

    /// Why a text is not an INI document.
    #[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ParseError {
        /// The text holds a control byte (0 to 8), which no INI file has.
        #[error("the input contains control bytes (byte {byte} at offset {offset})")]
        ControlByte {
            /// Where the first one is, counting from 0.
            offset: u32,
            /// Its value.
            byte: u8,
        },
        /// A `[` that starts a line has no `]`.
        #[error("line {line}: unclosed section header")]
        UnclosedSection {
            /// The line, counting from 1.
            line: u32,
        },
        /// A section header is `[]`.
        #[error("line {line}: empty section name")]
        EmptySectionName {
            /// The line, counting from 1.
            line: u32,
        },
        /// Anything else the parser refuses.
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line, counting from 1.
            line: u32,
            /// The column, counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
    }

    /// How `parse` reads a dialect.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct ParseOptions {
        /// A line that starts with one of these is a comment. There are no inline comments.
        pub comment_symbols: Vec<String>,
        /// The first of these on a line splits the key from the value.
        pub delimiters: Vec<String>,
        /// Lower-case every section and key name.
        pub lowercase_keys: bool,
    }

    impl Default for ParseOptions {
        fn default() -> Self {
            Self { comment_symbols: vec![";".to_owned(), "#".to_owned()], delimiters: vec!["=".to_owned()], lowercase_keys: false }
        }
    }

    /// How `stringify` writes.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, Options)]
    pub struct StringifyOptions {
        /// Write `key = value` instead of `key=value`.
        pub spacing: bool,
    }

    /// What a key holds once parsed: its text, or `true` for a bare key (a line with no delimiter).
    #[derive(Debug, Clone, PartialEq, Eq, OneOf)]
    pub enum Value {
        /// A bare key: always `true`.
        Flag(bool),
        /// The text after the delimiter, trimmed.
        Text(String),
    }

    /// What `stringify` writes for a key: `true` is a bare key, anything else is `key=value`.
    #[derive(Debug, Clone, PartialEq, OneOf)]
    pub enum Scalar {
        /// `true` writes the key alone; `false` writes `key=false`.
        Flag(bool),
        /// Written as it is.
        Text(String),
        /// Written the shortest way that reads back (`42`, `1.5`).
        Number(f64),
    }

    /// Where the first of `delimiters` is in `line`, and how long it is.
    fn find_delimiter(line: &str, delimiters: &[String]) -> Option<(usize, usize)> {
        delimiters
            .iter()
            .filter(|delimiter| !delimiter.is_empty())
            .filter_map(|delimiter| line.find(delimiter.as_str()).map(|at| (at, delimiter.len())))
            .min_by_key(|(at, _)| *at)
    }

    /// The text as rust-ini's own dialect: `;` comments, `=` delimiters, a stand-in value for a bare key, and no
    /// other character rust-ini reads its own way. Every line stays on its line, so rust-ini's line numbers are the caller's.
    fn normalise(raw: &str, opts: &ParseOptions) -> Result<String, ParseError> {
        let mut out = String::with_capacity(raw.len() + 64);
        for (number, raw_line) in (1..).zip(raw.lines()) {
            let line = raw_line.trim();
            if line.is_empty() {
                // nothing to write
            } else if opts.comment_symbols.iter().any(|symbol| !symbol.is_empty() && line.starts_with(symbol.as_str())) {
                out.push_str(";_");
            } else if let Some(header) = line.strip_prefix('[') {
                let Some((name, _)) = header.split_once(']') else {
                    return Err(ParseError::UnclosedSection { line: number });
                };
                if name.is_empty() {
                    return Err(ParseError::EmptySectionName { line: number });
                }
                let _ = write!(out, "[{}]", escape(name));
            } else if let Some((at, length)) = find_delimiter(line, &opts.delimiters) {
                let (key, value) = (line[..at].trim_end(), line[at + length..].trim());
                let _ = write!(out, "{}={}", escape(key), escape(value));
            } else {
                let _ = write!(out, "{}={BARE}", escape(line));
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// Parse INI text into sections of keys. Keys before the first section are under the `""` section; a bare key
    /// (no delimiter) is `true`; a repeated key keeps its last value and a repeated section is merged.
    ///
    /// ```luau
    /// local doc = assert(ini.parse("name=demo\n[server]\nhost=localhost\nverbose"))
    /// assert(doc[""].name == "demo" and doc.server.host == "localhost" and doc.server.verbose == true)
    /// local _, err = ini.parse("[server")
    /// assert(err ~= nil and err.kind == "unclosed_section" and err.line == 1)
    /// ```
    pub fn parse(text: String, opts: ParseOptions) -> Result<BTreeMap<String, BTreeMap<String, Value>>, ParseError> {
        if let Some((offset, byte)) = text.bytes().enumerate().find(|(_, byte)| *byte <= LAST_CONTROL_BYTE) {
            return Err(ParseError::ControlByte { offset: wide(offset), byte });
        }
        let normalised = normalise(&text, &opts)?;
        let parsed = Ini::load_from_str_noescape(&normalised).map_err(|e| ParseError::Syntax {
            line: wide(e.line),
            column: wide(e.col),
            reason: e.msg.to_string(),
        })?;

        let fold = |name: &str| if opts.lowercase_keys { unescape(name).to_lowercase() } else { unescape(name) };
        let mut document: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
        for (section, properties) in &parsed {
            if section.is_none() && properties.is_empty() {
                continue;
            }
            // Two sections can fold to one name (`[Foo]` and `[FOO]` under `lowercase_keys`): their keys add up.
            let keys = document.entry(section.map(fold).unwrap_or_default()).or_default();
            for (key, value) in properties.iter() {
                let value = if value == BARE { Value::Flag(true) } else { Value::Text(unescape(value)) };
                keys.insert(fold(key), value);
            }
        }
        Ok(document)
    }

    fn write_keys(out: &mut String, keys: &BTreeMap<String, Scalar>, delimiter: &str) {
        for (key, value) in keys {
            let _ = match value {
                Scalar::Flag(true) => writeln!(out, "{key}"),
                Scalar::Flag(false) => writeln!(out, "{key}{delimiter}false"),
                Scalar::Text(text) => writeln!(out, "{key}{delimiter}{text}"),
                Scalar::Number(number) => writeln!(out, "{key}{delimiter}{number}"),
            };
        }
    }

    /// Write sections of keys as INI text: the `""` section first, without a header, then the others in sorted
    /// order with a blank line between them.
    ///
    /// ```luau
    /// local text = ini.stringify({ [""] = { name = "demo" }, server = { port = 8080, verbose = true } })
    /// assert(text == "name=demo\n\n[server]\nport=8080\nverbose\n")
    /// assert(ini.stringify({ s = { k = "v" } }, { spacing = true }) == "[s]\nk = v\n")
    /// ```
    pub fn stringify(config: BTreeMap<String, BTreeMap<String, Scalar>>, opts: StringifyOptions) -> String {
        let delimiter = if opts.spacing { " = " } else { "=" };
        let mut out = String::new();
        // the `""` section sorts first
        for (section, keys) in &config {
            if !section.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                let _ = writeln!(out, "[{section}]");
            }
            write_keys(&mut out, keys, delimiter);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:ini")"#).eval().unwrap();
        lua.globals().set("ini", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    /// The value of `section.key` in what `ini.parse(<text>, <opts>)` gives: a string as it is, `true` as `<true>`.
    fn parsed(args: &str, section: &str, key: &str) -> String {
        let lua = engine();
        lua.globals().set("section", section).unwrap();
        lua.globals().set("key", key).unwrap();
        lua.load(format!(
            "local doc, err = ini.parse({args}); assert(err == nil); local v = doc[section][key];              return if v == true then '<true>' else v"
        ))
        .eval()
        .unwrap()
    }

    fn section_count(text: &str) -> i64 {
        eval(&format!("local n = 0; for _ in pairs((ini.parse({text}))) do n += 1 end; return n"))
    }

    /// `(kind, tostring)` of the error `ini.parse(<args>)` returns.
    fn refused(lua: &Lua, args: &str) -> (String, String) {
        lua.load(format!("local v, e = ini.parse({args}); assert(v == nil); return e.kind, tostring(e)")).eval().unwrap()
    }

    #[test]
    fn parse_basic_sections() {
        assert_eq!((parsed(r#""[section]\nkey=value""#, "section", "key")), "value");
    }

    #[test]
    fn parse_sectionless_keys() {
        assert_eq!((parsed(r#""name=world""#, "", "name")), "world");
    }

    #[test]
    fn parse_empty_value() {
        assert_eq!((parsed(r#""[s]\nkey=""#, "s", "key")), "");
    }

    #[test]
    fn parse_bare_key() {
        assert_eq!(parsed(r#""[s]\nflag""#, "s", "flag"), "<true>");
    }

    #[test]
    fn parse_duplicate_keys_last_wins() {
        assert_eq!((parsed(r#""[s]\nk=a\nk=b""#, "s", "k")), "b");
    }

    #[test]
    fn parse_duplicate_sections_merge() {
        assert_eq!((parsed(r#""[s]\na=1\n[s]\nb=2""#, "s", "a")), "1");
        assert_eq!((parsed(r#""[s]\na=1\n[s]\nb=2""#, "s", "b")), "2");
    }

    #[test]
    fn parse_empty_input() {
        assert_eq!(section_count(r#""""#), 0);
    }

    #[test]
    fn parse_comments_only() {
        assert_eq!(section_count(r#""; comment\n# another""#), 0);
    }

    #[test]
    fn parse_no_inline_comments() {
        assert_eq!((parsed(r#""[s]\nkey=value ; not a comment""#, "s", "key")), "value ; not a comment");
    }

    #[test]
    fn parse_a_section_with_no_keys_is_an_empty_section() {
        let ok: bool = eval(
            r#"
            local doc = ini.parse("[empty]\n[other]\nk=v")
            return doc.other.k == "v" and type(doc.empty) == "table" and next(doc.empty) == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_unicode_and_colons() {
        assert_eq!((parsed(r#""[日本語]\nキー=値""#, "日本語", "キー")), "値");
        assert_eq!((parsed(r#""[a:b]\nurl:x=http://h:80/""#, "a:b", "url:x")), "http://h:80/");
        assert_eq!(parsed(r#""[s]\nbare:key""#, "s", "bare:key"), "<true>");
    }

    #[test]
    fn parse_lowercase_keys_option() {
        assert_eq!((parsed(r#""[Section]\nKey=val", { lowercase_keys = true }"#, "section", "key")), "val");
        let merged: bool = eval(
            r#"
            local doc = ini.parse("[Foo]\na=1\n[FOO]\nB=2", { lowercase_keys = true })
            return doc.foo.a == "1" and doc.foo.b == "2" and doc.Foo == nil
            "#,
        );
        assert!(merged, "sections that fold to one name add up");
    }

    #[test]
    fn parse_custom_comment_symbols() {
        assert_eq!((parsed(r#""!ignored\nkey=val", { comment_symbols = { "!" } }"#, "", "key")), "val");
        // a symbol is the whole string, and `;` is no longer one
        let ok: bool = eval(
            r#"
            local doc = ini.parse("// note\n/ not=a comment\n;x=1", { comment_symbols = { "//" } })
            return doc[""]["/ not"] == "a comment" and doc[""][";x"] == "1"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn parse_custom_delimiters() {
        assert_eq!((parsed(r#""[s]\nkey:val", { delimiters = { ":" } }"#, "s", "key")), "val");
        let ok: bool = eval(
            r#"
            local doc = ini.parse("a => 1\nb: 2 => 3\nc=4", { delimiters = { "=>", ":" } })
            return doc[""].a == "1" and doc[""].b == "2 => 3" and doc[""]["c=4"] == true
            "#,
        );
        assert!(ok, "the first delimiter on the line splits it; a delimiter is the whole string");
    }

    #[test]
    fn parse_error_unclosed_section() {
        let (kind, text) = refused(&engine(), r#""[bad""#);
        assert_eq!(kind, "unclosed_section");
        assert_eq!(text, "lib:ini.parse: unclosed_section: line 1: unclosed section header");
    }

    #[test]
    fn parse_error_empty_section_name() {
        let (kind, text) = refused(&engine(), r#""a=1\n\n[]""#);
        assert_eq!(kind, "empty_section_name");
        assert!(text.contains("line 3: empty section name"), "got: {text}");
    }

    #[test]
    fn parse_error_null_bytes() {
        let (kind, text) = refused(&engine(), r#""key=val\0ue""#);
        assert_eq!(kind, "control_byte");
        assert!(text.contains("control bytes"), "got: {text}");
    }

    #[test]
    fn parse_error_sentinel_bytes() {
        let lua = engine();
        for input in [&b"[s]\nkey=val\x01ue"[..], b"[s]\nkey\x02=value"] {
            lua.globals().set("input", lua.create_string(input).unwrap()).unwrap();
            let (kind, text) = refused(&lua, "input");
            assert_eq!(kind, "control_byte");
            assert!(text.contains("control bytes"), "got: {text}");
        }
        let (offset, byte): (u32, u8) = lua.load("local _, e = ini.parse(input); return e.offset, e.byte").eval().unwrap();
        assert_eq!((offset, byte), (7, 2));
    }

    #[test]
    fn parse_refuses_arguments_of_the_wrong_type() {
        let refused: String = eval(
            r#"
            assert(not pcall(ini.parse, 42))
            assert(not pcall(ini.parse, "a=1", { delimiters = "=" }))
            assert(not pcall(ini.parse, "\255"))
            local _, e = pcall(ini.parse, "a=1", { lowercase = true })
            return tostring(e)
            "#,
        );
        assert!(refused.contains("lib:ini.parse: argument #2 'opts'"), "got: {refused}");
    }

    #[test]
    fn stringify_basic() {
        assert_eq!(eval::<String>(r#"return ini.stringify({ section = { key = "value" } })"#), "[section]\nkey=value\n");
    }

    #[test]
    fn stringify_bare_key() {
        assert_eq!(eval::<String>(r#"return ini.stringify({ section = { flag = true } })"#), "[section]\nflag\n");
    }

    #[test]
    fn stringify_sectionless_keys_first_and_sections_sorted() {
        let result: String =
            eval(r#"return ini.stringify({ zeta = { k = "v" }, [""] = { name = "world" }, alpha = { b = "2", a = "1" } })"#);
        assert_eq!(result, "name=world\n\n[alpha]\na=1\nb=2\n\n[zeta]\nk=v\n");
    }

    #[test]
    fn stringify_spacing() {
        assert_eq!(eval::<String>(r#"return ini.stringify({ s = { key = "value" } }, { spacing = true })"#), "[s]\nkey = value\n");
        assert_eq!(eval::<String>(r#"return ini.stringify({ s = { key = "value" } }, { spacing = false })"#), "[s]\nkey=value\n");
    }

    #[test]
    fn round_trip() {
        let (text, same): (String, bool) = eval(
            r#"
            local input = "[app]\ndebug\n\n[db]\nhost=localhost\nport=5432\n"
            local output = ini.stringify((assert(ini.parse(input))))
            return output, output == input
            "#,
        );
        assert!(same, "got: {text}");
    }

    #[test]
    fn stringify_scalars_and_rejects_non_scalar() {
        // false and numbers are written as their literal form
        let s: String = eval(r#"return ini.stringify({ s = { f = false, i = 42, n = 1.5, neg = -3 } })"#);
        assert_eq!(s, "[s]\nf=false\ni=42\nn=1.5\nneg=-3\n");

        // A value with no INI form (a table, a function) is refused, not written.
        let e: String = eval(
            r#"
            assert(not pcall(ini.stringify, { s = { bad = print } }))
            assert(not pcall(ini.stringify, { s = "not a section" }))
            local ok, err = pcall(ini.stringify, { s = { bad = {} } })
            assert(ok == false)
            return tostring(err)
            "#,
        );
        assert!(e.contains("lib:ini.stringify: argument #1 'config'"), "got: {e}");
    }
}

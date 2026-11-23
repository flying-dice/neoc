//! YAML decode and encode: `vnd:serde_yaml` (the `serde_yaml` crate).
//!
//! | Function              | Rust equivalent         |
//! |-----------------------|-------------------------|
//! | `decode_string(text)` | `serde_yaml::from_slice` |
//! | `encode(value)`       | `serde_yaml::to_string` |
//!
//! A document is read as plain data: mappings are tables with string keys (a number or boolean key is its text),
//! an alias is a copy of what its anchor holds, a timestamp is its text, `.nan` and `.inf` are `null`. A tagged
//! value (`!Tag x`), a key that is a sequence or a mapping, and a text with several documents are refused.
//! Mappings are written with their keys sorted.
//!
//! Ported from neoc-vm's `vnd:serde_yaml`. What changed on purpose:
//! - A YAML `null` reads as `nil` (it was mlua's `null` sentinel, a userdata): a null value in a mapping is a
//!   missing key.
//! - A failure is a typed error: `DecodeError` (`syntax` with the line and column, `invalid`) and `EncodeError`
//!   (`unsupported`).
//! - A value that is not plain data (a function, a buffer anywhere inside, a table that contains itself) is refused
//!   as an argument; some of these were returned errors.
//!
//! TODO(std:io): `decode(reader)` and `encode_to(value, writer)` wait for `std:io`'s reader and writer handles.

#[mlua_ir::module("vnd:serde_yaml")]
pub mod serde_yaml {
    use mlua_ir::{Bytes, LuaError, Unknown};
    use serde::Serialize;

    mlua_ir::unknown_reason!(
        /// Why a decoded document has no declared shape.
        YamlText = "whatever the YAML text holds: its shape is the text's, not this API's"
    );
    mlua_ir::unknown_reason!(
        /// Why a value to encode has no declared shape.
        PlainData = "any plain data a script built (tables, strings, numbers, booleans, nil): a YAML document holds whatever it is given"
    );

    /// Why a text is not a YAML document this module reads.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// The text breaks YAML's grammar at a place, or holds there what plain data cannot (a tagged value, a key
        /// that is a sequence or a mapping, an alias with no anchor, nesting past the parser's limit).
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column, counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
        /// The text as a whole is refused: it holds more than one document, say.
        #[error("{reason}")]
        Invalid {
            /// What is wrong.
            reason: String,
        },
    }

    /// Why a value cannot be written as YAML.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum EncodeError {
        /// The value holds something YAML has no form for.
        #[error("{reason}")]
        Unsupported {
            /// What the encoder said.
            reason: String,
        },
    }

    fn wide(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    fn decode_error(error: &::serde_yaml::Error) -> DecodeError {
        let text = error.to_string();
        let Some(location) = error.location() else {
            return DecodeError::Invalid { reason: text };
        };
        let (line, column) = (wide(location.line()), wide(location.column()));
        // serde_yaml often ends its text with the place; the place is in the fields
        let reason = match text.strip_suffix(&format!(" at line {line} column {column}")) {
            Some(reason) => reason.to_owned(),
            None => text,
        };
        DecodeError::Syntax { line, column, reason }
    }

    /// Parse a YAML document as plain data: whatever the text holds. An empty text and `null` are `nil` (with no
    /// error).
    ///
    /// ```luau
    /// local doc = assert(serde_yaml.decode_string("name: alice\ntags: [a, b]\n"))
    /// local config = doc :: { name: string, tags: { string } } -- this script's own knowledge of its file
    /// assert(config.name == "alice" and config.tags[2] == "b")
    /// local _, err = serde_yaml.decode_string("key: [unclosed")
    /// assert(err ~= nil and err.kind == "syntax" and err.line == 2)
    /// ```
    pub fn decode_string(text: Bytes) -> Result<Unknown<YamlText>, DecodeError> {
        ::serde_yaml::from_slice::<::serde_json::Value>(&text).map(Unknown::new).map_err(|e| decode_error(&e))
    }

    /// Write plain data as a YAML document: a table without holes is a sequence, any other table a mapping, `nil`
    /// is `null`. A number that is not finite is written `null`.
    ///
    /// ```luau
    /// assert(serde_yaml.encode({ name = "bob", tags = { "a", "b" } }) == "name: bob\ntags:\n- a\n- b\n")
    /// ```
    pub fn encode(value: Unknown<PlainData>) -> Result<String, EncodeError> {
        ::serde_yaml::to_string(&value.value)
            .map_err(|error| EncodeError::Unsupported { reason: error.to_string() })
    }
}

#[cfg(test)]
mod tests {
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("vnd:serde_yaml")"#).eval().unwrap();
        lua.globals().set("serde_yaml", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn decodes_mappings_sequences_and_scalars() {
        let ok: bool = eval(
            r#"
            local doc, err = serde_yaml.decode_string("name: alice\nage: 30\nratio: 0.5\non: true\nnums: [1, 2, 3]\ndb:\n  host: localhost\njobs:\n  - id: 1\n  - id: 2\n")
            assert(err == nil)
            assert(doc.name == "alice" and doc.age == 30 and doc.ratio == 0.5 and doc.on == true)
            assert(#doc.nums == 3 and doc.nums[3] == 3 and doc.db.host == "localhost" and doc.jobs[2].id == 2)
            local list = assert(serde_yaml.decode_string("- a\n- b\n"))
            assert(list[1] == "a" and list[2] == "b")
            assert(serde_yaml.decode_string("hello\n") == "hello")
            assert(serde_yaml.decode_string("true\n") == true and serde_yaml.decode_string("42") == 42)
            return true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn null_is_nil() {
        let ok: bool = eval(
            r##"
            local doc = assert(serde_yaml.decode_string("key: null\nother: ~\nkept: 1\nlist: [1, null, 3]\n"))
            assert(doc.key == nil and doc.other == nil and doc.kept == 1)
            assert(doc.list[1] == 1 and doc.list[2] == nil and doc.list[3] == 3)
            for _, text in { "", "~", "null", "# only a comment\n" } do
                local value, err = serde_yaml.decode_string(text)
                assert(value == nil and err == nil)
            end
            return true
            "##,
        );
        assert!(ok);
    }

    #[test]
    fn yaml_specifics_are_flattened_to_plain_data() {
        let seen: Vec<String> = eval(
            r#"
            local doc = assert(serde_yaml.decode_string('1: one\ntrue: yes\nat: 2001-12-14t21:59:43.10-05:00\nhex: 0x10\nbase: &x [1]\ncopy: *x\nnan: .nan\ntext: |\n  two\n  lines\n'))
            assert(doc.nan == nil and doc.hex == 16)
            doc.copy[1] = 2
            assert(doc.base[1] == 1, "an alias is a copy")
            return { doc["1"], doc["true"], doc.at, doc.text }
            "#,
        );
        assert_eq!(seen, ["one", "yes", "2001-12-14t21:59:43.10-05:00", "two\nlines\n"]);
    }

    #[test]
    fn invalid_yaml_is_a_typed_error() {
        let (kind, line, column, reason, text): (String, u32, u32, String, String) = eval(
            r#"
            local doc, err = serde_yaml.decode_string("ok: 1\nkey: *nope")
            assert(doc == nil and err.op == "vnd:serde_yaml.decode_string")
            return err.kind, err.line, err.column, err.reason, tostring(err)
            "#,
        );
        assert_eq!((kind.as_str(), line, column, reason.as_str()), ("syntax", 2, 6, "unknown anchor"));
        assert_eq!(text, "vnd:serde_yaml.decode_string: syntax: line 2, column 6: unknown anchor");

        let unclosed: (u32, String) =
            eval(r#"local _, err = serde_yaml.decode_string("key: [unclosed"); return err.line, err.reason"#);
        assert_eq!(unclosed.0, 2);
        assert!(unclosed.1.contains("did not find expected ',' or ']'"), "got: {}", unclosed.1);
    }

    #[test]
    fn what_plain_data_cannot_hold_is_refused() {
        let kinds: Vec<String> = eval(
            r#"
            local out = {}
            for _, text in { "!Tag x", "a: !Tag {b: 1}", "? [1, 2]\n: v", "a: \255", "a: 1\n---\nb: 2" } do
                local doc, err = serde_yaml.decode_string(text)
                assert(doc == nil and type(err.reason) == "string")
                table.insert(out, err.kind)
            end
            return out
            "#,
        );
        assert_eq!(kinds, ["syntax", "syntax", "syntax", "syntax", "invalid"]);
    }

    /// serde_yaml stops nesting at its recursion limit.
    #[test]
    fn deeply_nested_yaml_is_an_error_not_a_stack_overflow() {
        let reason: String = eval(
            r#"
            local doc, err = serde_yaml.decode_string(("["):rep(100000) .. ("]"):rep(100000))
            assert(doc == nil and err.kind == "syntax")
            return err.reason
            "#,
        );
        assert!(reason.contains("recursion limit"), "got: {reason}");
    }

    #[test]
    fn encodes_plain_data() {
        let seen: Vec<String> = eval(
            r#"
            return {
                serde_yaml.encode({ name = "bob", age = 25, ratio = 1.5, on = true, tags = { "a", "b" }, server = { host = "localhost" } }),
                serde_yaml.encode(42),
                serde_yaml.encode(true),
                serde_yaml.encode("hello"),
                serde_yaml.encode(nil),
                serde_yaml.encode({ 1, { id = 2 } }),
                serde_yaml.encode({ text = "two\nlines", n = 0 / 0 }),
            }
            "#,
        );
        assert_eq!(
            seen,
            [
                "age: 25\nname: bob\non: true\nratio: 1.5\nserver:\n  host: localhost\ntags:\n- a\n- b\n",
                "42\n",
                "true\n",
                "hello\n",
                "null\n",
                "- 1\n- id: 2\n",
                "n: null\ntext: |-\n  two\n  lines\n",
            ]
        );
    }

    #[test]
    fn round_trips() {
        let ok: bool = eval(
            r#"
            local original = { user = { name = "carol", active = true, scores = { 1, 2.5 } }, list = { { id = 1 }, { id = 2 } } }
            local decoded = assert(serde_yaml.decode_string((assert(serde_yaml.encode(original)))))
            assert(decoded.user.name == "carol" and decoded.user.active == true and decoded.user.scores[2] == 2.5)
            assert(decoded.list[2].id == 2)
            return serde_yaml.decode_string((serde_yaml.encode(42))) == 42
            "#,
        );
        assert!(ok);
    }

    /// A buffer is refused, never turned into a sequence of numbers.
    #[test]
    fn a_buffer_is_refused() {
        let refused: Vec<String> = eval(
            r#"
            local deep = { buffer.create(2) }
            for _ = 2, 100 do
                deep = { inner = deep }
            end
            local out = {}
            for _, value in { buffer.create(2), { list = { 1, buffer.create(2) } }, deep } do
                local ok, err = pcall(serde_yaml.encode, value)
                assert(not ok)
                table.insert(out, tostring(err))
            end
            return out
            "#,
        );
        assert_eq!(refused.len(), 3);
        assert!(
            refused.iter().all(|e| e.contains("vnd:serde_yaml.encode: argument #1 'value'") && e.contains("byte array")),
            "{refused:#?}"
        );
    }

    #[test]
    fn what_is_not_plain_data_is_refused_as_an_argument() {
        let ok: bool = eval(
            r#"
            local cyclic = {}
            cyclic.self = cyclic
            local deep = {}
            for _ = 1, 100000 do deep = { deep } end
            for _, value in { print, { f = print }, cyclic, deep } do
                local ok, err = pcall(serde_yaml.encode, value)
                assert(not ok and tostring(err):find("vnd:serde_yaml.encode: argument #1 'value'", 1, true))
            end
            return not pcall(serde_yaml.decode_string, 42) and not pcall(serde_yaml.decode_string, nil)
            "#,
        );
        assert!(ok);
    }
}

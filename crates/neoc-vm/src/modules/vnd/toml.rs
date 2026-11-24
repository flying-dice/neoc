//! TOML decode and encode: `vnd:toml` (the `toml` crate).
//!
//! | Function                   | Rust equivalent                       |
//! |----------------------------|---------------------------------------|
//! | `decode(reader)`           | `toml::from_str` on all of the reader |
//! | `decode_string(text)`      | `toml::from_str`                      |
//! | `encode(value)`            | `toml::to_string`                     |
//! | `encode_pretty(value)`     | `toml::to_string_pretty`              |
//! | `encode_to(value, writer)` | `toml::to_string`, written out        |
//!
//! The `toml` 1.x crate implements TOML 1.1: inline tables may span lines and end with a comma, `\e` is a string
//! escape, and times may omit seconds (`07:32`).
//!
//! Ported from the old VM's `vnd:toml`. What changed on purpose:
//! - A date or time decodes to its text (`"1979-05-27T07:32:00Z"`); it was a table with one private key.
//! - A failure is a typed error: `DecodeError` (`syntax`, with the line and column) and `EncodeError`
//!   (`unsupported`).
//! - A document is a table with string keys, so `encode` refuses anything else as an argument (a list, a string, a
//!   buffer anywhere inside); these were returned errors, and a raised one for the buffer.
//! - `decode` and `encode_to` take `std:io`'s `Reader` and `Writer` and refuse anything else as an argument (it
//!   was a returned error); their failures are `ReadError` and `WriteError`, which add `io` (and `invalid_utf8`)
//!   to the kinds above.

/// The line and column (both from 1) of byte `offset` in `text`.
fn place(text: &str, offset: usize) -> (u32, u32) {
    let before = &text[..text.floor_char_boundary(offset.min(text.len()))];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count()) + 1;
    (u32::try_from(line).unwrap_or(u32::MAX), u32::try_from(column).unwrap_or(u32::MAX))
}

/// A TOML value as plain data: a date or time is its text.
fn plain(value: ::toml::Value) -> serde_json::Value {
    use serde_json::Value as Json;
    match value {
        ::toml::Value::String(text) => Json::String(text),
        ::toml::Value::Integer(number) => Json::from(number),
        // `nan` and `inf` have no place in plain data: they are left out, as JSON leaves them out
        ::toml::Value::Float(number) => Json::from(number),
        ::toml::Value::Boolean(flag) => Json::Bool(flag),
        ::toml::Value::Datetime(datetime) => Json::String(datetime.to_string()),
        ::toml::Value::Array(items) => Json::Array(items.into_iter().map(plain).collect()),
        ::toml::Value::Table(entries) => {
            Json::Object(entries.into_iter().map(|(key, item)| (key, plain(item))).collect())
        }
    }
}

#[mlua_ir::module("vnd:toml")]
pub mod toml {
    use std::collections::BTreeMap;

    use mlua_ir::{LuaError, Unknown};
    use serde::Serialize;

    use super::{place, plain};
    use crate::modules::std::io::io::{Reader, Writer};

    mlua_ir::unknown_reason!(
        /// Why a decoded value has no declared shape.
        TomlText = "whatever the TOML text holds under this key: its shape is the text's, not this API's"
    );
    mlua_ir::unknown_reason!(
        /// Why a value to encode has no declared shape.
        PlainData = "any plain data a script built (tables, strings, numbers, booleans): a TOML document holds whatever it is given"
    );

    /// Why a text is not TOML.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// The text breaks TOML's grammar, or says something twice (a key, a table).
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column, counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
    }

    /// Why a value cannot be written as TOML.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum EncodeError {
        /// The document holds something TOML has no form for (a number that is not finite, say).
        #[error("{reason}")]
        Unsupported {
            /// What the encoder said.
            reason: String,
        },
    }

    /// Why a reader does not give a TOML document.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ReadError {
        /// The reader failed.
        #[error("{reason}")]
        Io {
            /// What the reader said.
            reason: String,
        },
        /// What was read is not UTF-8.
        #[error("the text is not UTF-8 from byte {offset}")]
        InvalidUtf8 {
            /// How many bytes from the start are valid UTF-8.
            offset: u32,
        },
        /// The text breaks TOML's grammar, or says something twice (a key, a table).
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column, counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
    }

    impl From<DecodeError> for ReadError {
        fn from(error: DecodeError) -> Self {
            match error {
                DecodeError::Syntax { line, column, reason } => ReadError::Syntax { line, column, reason },
            }
        }
    }

    /// Why a value was not written to a writer as TOML.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum WriteError {
        /// The writer failed.
        #[error("{reason}")]
        Io {
            /// What the writer said.
            reason: String,
        },
        /// The document holds something TOML has no form for (a number that is not finite, say).
        #[error("{reason}")]
        Unsupported {
            /// What the encoder said.
            reason: String,
        },
    }

    impl From<EncodeError> for WriteError {
        fn from(error: EncodeError) -> Self {
            match error {
                EncodeError::Unsupported { reason } => WriteError::Unsupported { reason },
            }
        }
    }

    /// Read `reader` to its end and parse what it gave as a TOML document, as `decode_string` does.
    ///
    /// ```luau
    /// local io = require("std:io")
    /// local doc = assert(toml.decode(io.from_string('name = "demo"\n')))
    /// assert(doc.name == "demo")
    /// ```
    pub fn decode(reader: Reader) -> Result<BTreeMap<String, Unknown<TomlText>>, ReadError> {
        let bytes = reader.remaining().map_err(|error| ReadError::Io { reason: error.to_string() })?;
        let text = String::from_utf8(bytes).map_err(|error| ReadError::InvalidUtf8 {
            offset: u32::try_from(error.utf8_error().valid_up_to()).unwrap_or(u32::MAX),
        })?;
        Ok(decode_string(text)?)
    }

    /// Parse a TOML document: its top-level keys, each holding whatever the text gives it. A date or time is its
    /// text.
    ///
    /// ```luau
    /// local doc = assert(toml.decode_string('[server]\nhost = "localhost"\nport = 8080\n'))
    /// local server = doc.server :: { host: string, port: number } -- this script's own knowledge of its file
    /// assert(server.host == "localhost" and server.port == 8080)
    /// local _, err = toml.decode_string("key = [unclosed")
    /// assert(err ~= nil and err.kind == "syntax" and err.line == 1)
    /// ```
    pub fn decode_string(text: String) -> Result<BTreeMap<String, Unknown<TomlText>>, DecodeError> {
        let document: ::toml::Table = ::toml::from_str(&text).map_err(|error| {
            let (line, column) = place(&text, error.span().map_or(0, |span| span.start));
            DecodeError::Syntax { line, column, reason: error.message().to_owned() }
        })?;
        Ok(document.into_iter().map(|(key, value)| (key, Unknown::new(plain(value)))).collect())
    }

    fn document(value: BTreeMap<String, Unknown<PlainData>>) -> serde_json::Value {
        serde_json::Value::Object(value.into_iter().map(|(key, item)| (key, item.value)).collect())
    }

    fn unsupported(error: &::toml::ser::Error) -> EncodeError {
        EncodeError::Unsupported { reason: error.to_string() }
    }

    /// Write a document (a table with string keys) as TOML, compactly.
    ///
    /// ```luau
    /// assert(toml.encode({ name = "demo", port = 8080 }) == 'name = "demo"\nport = 8080\n')
    /// ```
    pub fn encode(value: BTreeMap<String, Unknown<PlainData>>) -> Result<String, EncodeError> {
        ::toml::to_string(&document(value)).map_err(|e| unsupported(&e))
    }

    /// Write a document (a table with string keys) as TOML, with arrays one item to a line.
    pub fn encode_pretty(value: BTreeMap<String, Unknown<PlainData>>) -> Result<String, EncodeError> {
        ::toml::to_string_pretty(&document(value)).map_err(|e| unsupported(&e))
    }

    /// Write a document to `writer` as TOML, as `encode` writes it. Nothing is written when the document cannot
    /// be encoded.
    pub fn encode_to(value: BTreeMap<String, Unknown<PlainData>>, writer: Writer) -> Result<(), WriteError> {
        let text = encode(value)?;
        writer.write_all(text.as_bytes()).map_err(|error| WriteError::Io { reason: error.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::place;

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("vnd:toml")"#).eval().unwrap();
        lua.globals().set("toml", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn a_place_is_a_line_and_a_column() {
        assert_eq!(place("", 0), (1, 1));
        assert_eq!(place("ab\ncd", 1), (1, 2));
        assert_eq!(place("ab\ncd", 3), (2, 1));
        assert_eq!(place("ab\ncé!", 6), (2, 3), "columns count characters");
        assert_eq!(place("ab", 99), (1, 3));
    }

    #[test]
    fn decodes_scalars_tables_and_arrays() {
        let ok: bool = eval(
            r#"
            local doc, err = toml.decode_string('name = "alice"\nage = 30\nratio = 0.5\nenabled = true\nnums = [1, 2, 3]\n\n[db]\nhost = "localhost"\n\n[[jobs]]\nid = 1\n\n[[jobs]]\nid = 2\n')
            assert(err == nil)
            return doc.name == "alice" and doc.age == 30 and doc.ratio == 0.5 and doc.enabled == true
                and #doc.nums == 3 and doc.nums[1] == 1 and doc.nums[3] == 3
                and doc.db.host == "localhost" and #doc.jobs == 2 and doc.jobs[2].id == 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn an_empty_text_is_an_empty_document() {
        let ok: bool =
            eval(r#"local doc, err = toml.decode_string(""); return err == nil and next(doc) == nil"#);
        assert!(ok);
    }

    #[test]
    fn decodes_toml_1_1() {
        let ok: bool = eval(
            r#"
            local tables = assert(toml.decode_string("x = {\n  a = 1,\n  b = 2,\n}\n"))
            local escape = assert(toml.decode_string('x = "\\e"'))
            return tables.x.a == 1 and tables.x.b == 2 and escape.x == "\27"
            "#,
        );
        assert!(ok, "inline tables over several lines with a trailing comma, and the \\e escape");
    }

    #[test]
    fn a_date_or_time_is_its_text() {
        let seen: Vec<String> = eval(
            r#"
            local doc = assert(toml.decode_string("t = 07:32\nd = 1979-05-27\nat = 1979-05-27T07:32:00Z\nlocal_at = 1979-05-27T07:32:00.5\nlist = [ 1979-05-27 ]"))
            return { doc.t, doc.d, doc.at, doc.local_at, doc.list[1] }
            "#,
        );
        assert_eq!(
            seen,
            ["07:32", "1979-05-27", "1979-05-27T07:32:00Z", "1979-05-27T07:32:00.5", "1979-05-27"]
        );
    }

    #[test]
    fn invalid_toml_is_a_typed_error() {
        let (kind, line, column, text): (String, u32, u32, String) = eval(
            r#"
            local doc, err = toml.decode_string("ok = 1\nbad = [unclosed")
            assert(doc == nil and err.op == "vnd:toml.decode_string" and type(err.reason) == "string")
            return err.kind, err.line, err.column, tostring(err)
            "#,
        );
        assert_eq!((kind.as_str(), line), ("syntax", 2));
        assert!(column > 1, "the column is where the parser stopped: {column}");
        assert!(text.starts_with("vnd:toml.decode_string: syntax: line 2, column "), "got: {text}");

        let duplicate: String =
            eval(r#"local _, err = toml.decode_string("a = 1\na = 2"); return err.reason"#);
        assert!(duplicate.contains("duplicate key"), "got: {duplicate}");
    }

    /// toml's parser stops nesting arrays and inline tables at its recursion limit.
    #[test]
    fn deeply_nested_toml_is_an_error_not_a_stack_overflow() {
        let reasons: Vec<String> = eval(
            r#"
            local N = 100000
            local out = {}
            for _, text in {
                "a = " .. ("["):rep(N) .. ("]"):rep(N),
                "a = " .. ("{b = "):rep(N) .. "1" .. ("}"):rep(N),
                "a = " .. ("[1,"):rep(N),
            } do
                local doc, err = toml.decode_string(text)
                assert(doc == nil and err.kind == "syntax")
                table.insert(out, err.reason)
            end
            return out
            "#,
        );
        assert_eq!(reasons.len(), 3);
        assert!(reasons.iter().all(|reason| reason.contains("recursion")), "{reasons:#?}");
    }

    #[test]
    fn encodes_a_document() {
        let (compact, pretty): (String, String) = eval(
            r#"
            local doc = { name = "bob", age = 25, ratio = 1.5, on = true, tags = { "a", "b" }, server = { host = "localhost", port = 3000 } }
            return toml.encode(doc), toml.encode_pretty(doc)
            "#,
        );
        assert_eq!(
            compact,
            "age = 25\nname = \"bob\"\non = true\nratio = 1.5\ntags = [\"a\", \"b\"]\n\n[server]\nhost = \"localhost\"\nport = 3000\n"
        );
        assert!(pretty.contains("tags = [\n    \"a\",\n    \"b\",\n]"), "got: {pretty}");
    }

    #[test]
    fn round_trips() {
        let ok: bool = eval(
            r#"
            local original = { title = "x", server = { host = "localhost", port = 3000, tags = { "a", "b" } }, jobs = { { id = 1 }, { id = 2 } } }
            for _, encode in { toml.encode, toml.encode_pretty } do
                local decoded = assert(toml.decode_string((assert(encode(original)))))
                assert(decoded.title == "x" and decoded.server.host == "localhost" and decoded.server.port == 3000)
                assert(decoded.server.tags[2] == "b" and decoded.jobs[2].id == 2)
            end
            return toml.encode({}) == ""
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_document_is_a_table_with_string_keys() {
        let refused: String = eval(
            r#"
            assert(not pcall(toml.encode, "text"))
            assert(not pcall(toml.encode, { 1, 2, 3 }))
            assert(not pcall(toml.encode_pretty, 42))
            assert(not pcall(toml.encode, { f = print }))
            assert(not pcall(toml.decode_string, 42))
            local _, e = pcall(toml.encode, nil)
            return tostring(e)
            "#,
        );
        assert!(refused.contains("vnd:toml.encode: argument #1 'value'"), "got: {refused}");
    }

    /// A buffer is refused, never turned into an array of numbers.
    #[test]
    fn a_buffer_is_refused() {
        let refused: Vec<String> = eval(
            r#"
            local deep = { buffer.create(2) }
            for _ = 2, 100 do
                deep = { inner = deep }
            end
            local out = {}
            for _, value in { { data = buffer.create(2) }, { nested = { data = buffer.create(2) } }, { data = deep } } do
                for _, encode in { toml.encode, toml.encode_pretty } do
                    local ok, err = pcall(encode, value)
                    assert(not ok)
                    table.insert(out, tostring(err))
                end
            end
            return out
            "#,
        );
        assert_eq!(refused.len(), 6);
        assert!(
            refused.iter().all(|e| e.contains("argument #1 'value'") && e.contains("byte array")),
            "{refused:#?}"
        );
    }

    #[test]
    fn what_toml_cannot_hold_is_a_typed_error() {
        let (kind, op): (String, String) = eval(
            r#"
            local text, err = toml.encode({ n = 0 / 0 })
            assert(text == nil and type(err.reason) == "string")
            return err.kind, err.op
            "#,
        );
        assert_eq!((kind.as_str(), op.as_str()), ("unsupported", "vnd:toml.encode"));
    }

    fn with_io<T: mlua::FromLuaMulti>(code: &str) -> (T, String) {
        let (lua, sink) = crate::modules::std::io::testing::engine("vnd:toml", "toml");
        let value = lua.load(code).eval().unwrap();
        (value, sink.text())
    }

    #[test]
    fn decode_reads_a_reader_to_its_end() {
        let (ok, _): (bool, _) = with_io(
            r#"
            local reader = io.from_string('skipped\n[server]\nhost = "localhost"\nport = 8080\n')
            reader:read_line()
            local doc, err = toml.decode(reader)
            assert(err == nil and reader:read(1) == nil)
            return doc.server.host == "localhost" and doc.server.port == 8080 and next(assert(toml.decode(io.from_string("")))) == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn what_a_reader_gives_can_fail_three_ways() {
        let (seen, _): (Vec<String>, _) = with_io(
            r#"
            local out = {}
            local doc, syntax = toml.decode(io.from_string("ok = 1\nbad = [unclosed"))
            assert(doc == nil and syntax.kind == "syntax" and syntax.line == 2 and syntax.column > 1)
            local _, utf8 = toml.decode(io.from_string("a = 1\n\255"))
            assert(utf8.kind == "invalid_utf8" and utf8.offset == 6)
            local _, failed = toml.decode(broken_reader)
            assert(failed.kind == "io" and failed.reason == "the pipe is closed")
            return { syntax.op, tostring(utf8), tostring(failed) }
            "#,
        );
        assert_eq!(
            seen,
            [
                "vnd:toml.decode",
                "vnd:toml.decode: invalid_utf8: the text is not UTF-8 from byte 6",
                "vnd:toml.decode: io: the pipe is closed",
            ]
        );
    }

    #[test]
    fn encode_to_writes_what_encode_gives() {
        let (ok, written): (bool, String) = with_io(
            r#"
            local doc = { name = "bob", server = { port = 3000 } }
            local ok, err = toml.encode_to(doc, writer)
            assert(ok == true and err == nil)
            return toml.encode(doc) == 'name = "bob"\n\n[server]\nport = 3000\n'
            "#,
        );
        assert!(ok);
        assert_eq!(written, "name = \"bob\"\n\n[server]\nport = 3000\n");
    }

    #[test]
    fn encode_to_fails_typed_and_writes_nothing_it_cannot_encode() {
        let (seen, written): (Vec<String>, String) = with_io(
            r#"
            local ok, unsupported = toml.encode_to({ n = 0 / 0 }, writer)
            assert(ok == nil and unsupported.kind == "unsupported")
            local _, failed = toml.encode_to({ a = 1 }, broken_writer)
            assert(failed.kind == "io" and failed.reason == "read-only")
            local raised, refused = pcall(toml.encode_to, { data = buffer.create(2) }, writer)
            assert(not raised and not pcall(toml.encode_to, { 1, 2 }, writer))
            return { unsupported.op, tostring(failed), tostring(refused) }
            "#,
        );
        assert_eq!(written, "", "nothing reaches the writer");
        assert_eq!(seen[0], "vnd:toml.encode_to");
        assert_eq!(seen[1], "vnd:toml.encode_to: io: read-only");
        assert!(
            seen[2].contains("vnd:toml.encode_to: argument #1 'value'") && seen[2].contains("byte array"),
            "{seen:#?}"
        );
    }

    #[test]
    fn a_reader_is_a_reader_and_a_writer_a_writer() {
        let (seen, _): (Vec<String>, _) = with_io(
            r#"
            local reader = io.from_string("a = 1")
            local ok1, not_reader = pcall(toml.decode, writer)
            local ok2, not_writer = pcall(toml.encode_to, { a = 1 }, reader)
            assert(not ok1 and not ok2 and not pcall(toml.decode, "a = 1") and not pcall(toml.encode_to, { a = 1 }))
            return { tostring(not_reader), tostring(not_writer) }
            "#,
        );
        assert!(seen[0].contains("vnd:toml.decode: argument #1 'reader': expected a Reader"), "{seen:#?}");
        assert!(seen[1].contains("vnd:toml.encode_to: argument #2 'writer': expected a Writer"), "{seen:#?}");
    }
}

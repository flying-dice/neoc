//! JSON text in and out: `vnd:serde_json` (the `serde_json` crate).
//!
//! | Function                   | Rust equivalent                               |
//! |----------------------------|-----------------------------------------------|
//! | `from_reader(reader)`      | `serde_json::from_slice` on all of the reader |
//! | `from_str(text)`           | `serde_json::from_slice`                      |
//! | `to_string(value)`         | `serde_json::to_string`                       |
//! | `to_string_pretty(value)`  | `serde_json::to_string_pretty`                |
//! | `to_writer(value, writer)` | `serde_json::to_string`, written out          |
//!
//! A parsed document is handed out as `lib:json` does it: an `Object` or `Array` handle for a container, a Lua value
//! for a scalar. Objects are written with their keys sorted.
//!
//! The API's rules:
//! - A JSON `null` document reads as `nil` (there is no `json.null` sentinel, as in `lib:json`).
//! - A failure is a typed error: `DecodeError` (`syntax` or `eof`, with the line and column) and `EncodeError`
//!   (`too_deep`).
//! - A value that is not plain data (a function, a buffer anywhere inside, a table that contains itself, a handle
//!   nested in a plain table) is refused as an argument.
//! - `from_reader` and `to_writer` take `std:io`'s `Reader` and `Writer` and refuse anything else as an argument; their
//!   failures are `ReadError` and `WriteError`, which add `io` to the kinds above.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

/// Where `serde_json` stopped (line and column, both from 1) and why, without the place it ends its text with.
pub fn place_and_reason(error: &::serde_json::Error) -> (u32, u32, String) {
    let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let text = error.to_string();
    let reason = match text.rfind(" at line ") {
        Some(end) => text[..end].to_owned(),
        None => text,
    };
    (wide(error.line()), wide(error.column()), reason)
}

#[mlua_ir::module("vnd:serde_json")]
pub mod serde_json {
    use mlua_ir::{Bytes, LuaError};
    use serde::Serialize;

    use super::place_and_reason;
    use neoc_vm_lib_json::deeper_than;
    use neoc_vm_lib_json::json::{Input, MAX_DEPTH, Node};
    use neoc_vm_std::io::io::{Reader, Writer};

    /// Why a text is not JSON.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// The text breaks JSON's grammar, is not UTF-8, or nests deeper than the parser reads.
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column (in bytes), counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
        /// The text ends before the document does (an empty text, an unclosed string, array or object).
        #[error("line {line}, column {column}: {reason}")]
        Eof {
            /// The line the text ends on, counting from 1.
            line: u32,
            /// The column (in bytes), counting from 1.
            column: u32,
            /// What was still expected.
            reason: String,
        },
    }

    /// Why a value cannot be written as JSON.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum EncodeError {
        /// The value nests deeper than `from_str` reads back.
        #[error("the value nests more than {max_depth} levels deep")]
        TooDeep {
            /// The most levels a document may nest (`lib:json`'s `MAX_DEPTH`).
            max_depth: u32,
        },
    }

    fn decode_error(error: &::serde_json::Error) -> DecodeError {
        let (line, column, reason) = place_and_reason(error);
        if error.is_eof() {
            DecodeError::Eof { line, column, reason }
        } else {
            DecodeError::Syntax { line, column, reason }
        }
    }

    /// Why a reader does not give a JSON document.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ReadError {
        /// The reader failed.
        #[error("{reason}")]
        Io {
            /// What the reader said.
            reason: String,
        },
        /// The text breaks JSON's grammar, is not UTF-8, or nests deeper than the parser reads.
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column (in bytes), counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
        /// The text ends before the document does (an empty text, an unclosed string, array or object).
        #[error("line {line}, column {column}: {reason}")]
        Eof {
            /// The line the text ends on, counting from 1.
            line: u32,
            /// The column (in bytes), counting from 1.
            column: u32,
            /// What was still expected.
            reason: String,
        },
    }

    impl From<DecodeError> for ReadError {
        fn from(error: DecodeError) -> Self {
            match error {
                DecodeError::Syntax { line, column, reason } => ReadError::Syntax { line, column, reason },
                DecodeError::Eof { line, column, reason } => ReadError::Eof { line, column, reason },
            }
        }
    }

    /// Why a value was not written to a writer as JSON.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum WriteError {
        /// The writer failed.
        #[error("{reason}")]
        Io {
            /// What the writer said.
            reason: String,
        },
        /// The value nests deeper than `from_str` reads back.
        #[error("the value nests more than {max_depth} levels deep")]
        TooDeep {
            /// The most levels a document may nest (`lib:json`'s `MAX_DEPTH`).
            max_depth: u32,
        },
    }

    impl From<EncodeError> for WriteError {
        fn from(error: EncodeError) -> Self {
            match error {
                EncodeError::TooDeep { max_depth } => WriteError::TooDeep { max_depth },
            }
        }
    }

    /// Read `reader` to its end and parse what it gave as a JSON text, as `from_str` does.
    ///
    /// ```luau
    /// local io = require("std:io")
    /// local body = assert(serde_json.from_reader(io.from_string('{"name":"ada"}'))) :: lib_json_Object
    /// assert(body:get("name") == "ada")
    /// ```
    pub fn from_reader(reader: Reader) -> Result<Option<Node>, ReadError> {
        let bytes = reader.remaining().map_err(|error| ReadError::Io { reason: error.to_string() })?;
        Ok(from_str(Bytes(bytes))?)
    }

    /// Parse a JSON text. An object or an array comes back as a `lib:json` handle, a string, number or boolean as a
    /// Lua value, and `null` as `nil` (with no error).
    ///
    /// ```luau
    /// local body = assert(serde_json.from_str('{"name":"ada","tags":["a","b"]}')) :: lib_json_Object
    /// assert(body:get("name") == "ada" and body:at({ "tags", 2 }) == "b")
    /// local _, err = serde_json.from_str("{bad}")
    /// assert(err ~= nil and err.kind == "syntax" and err.line == 1)
    /// ```
    pub fn from_str(text: Bytes) -> Result<Option<Node>, DecodeError> {
        let value: ::serde_json::Value = ::serde_json::from_slice(&text).map_err(|e| decode_error(&e))?;
        Ok(Node::root(value))
    }

    fn checked(value: Input) -> Result<::serde_json::Value, EncodeError> {
        let value = value.into_value();
        if deeper_than(&value, MAX_DEPTH as usize) {
            return Err(EncodeError::TooDeep { max_depth: MAX_DEPTH });
        }
        Ok(neoc_vm_core::json::with_sorted_keys(value))
    }

    /// Write a value as compact JSON: plain data (a table without holes is an array, any other table an object,
    /// `nil` is `null`) or a `lib:json` handle. A number that is not finite is written `null`.
    ///
    /// ```luau
    /// assert(serde_json.to_string({ name = "ada", tags = { "a", "b" } }) == '{"name":"ada","tags":["a","b"]}')
    /// ```
    pub fn to_string(value: Input) -> Result<String, EncodeError> {
        Ok(checked(value)?.to_string())
    }

    /// Write a value as JSON over several lines, indented by two spaces.
    pub fn to_string_pretty(value: Input) -> Result<String, EncodeError> {
        Ok(format!("{:#}", checked(value)?))
    }

    /// Write a value to `writer` as compact JSON, as `to_string` writes it. Nothing is written when the value
    /// cannot be encoded.
    pub fn to_writer(value: Input, writer: Writer) -> Result<(), WriteError> {
        let text = to_string(value)?;
        writer.write_all(text.as_bytes()).map_err(|error| WriteError::Io { reason: error.to_string() })
    }
}

#[cfg(test)]
mod tests {
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("vnd:serde_json")"#).eval().unwrap();
        lua.globals().set("serde_json", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    #[test]
    fn a_container_is_a_json_handle() {
        let ok: bool = eval(
            r#"
            local object, err = serde_json.from_str('{"name":"alice","age":30,"tags":["a","b"],"none":null}')
            assert(err == nil)
            assert(object:get("name") == "alice" and object:get("age") == 30 and object:at({ "tags", 2 }) == "b")
            assert(object:get("none") == nil and object:kind("none") == "null")
            local array = assert(serde_json.from_str("[1,2,3]"))
            assert(array:len() == 3 and array:get(1) == 1 and array:get(3) == 3)
            -- a handle of lib:json: the same type, the same methods
            local json = require("lib:json")
            local holder = json.object()
            holder:set("inner", array)
            return holder:at({ "inner", 2 }) == 2
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_scalar_is_a_lua_value_and_null_is_nil() {
        let ok: bool = eval(
            r#"
            assert(serde_json.from_str('"hello"') == "hello")
            assert(serde_json.from_str("42") == 42 and serde_json.from_str("-1.5e3") == -1500)
            assert(serde_json.from_str("true") == true and serde_json.from_str("false") == false)
            local value, err = serde_json.from_str(" null ")
            return value == nil and err == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn invalid_json_is_a_typed_error() {
        let (kind, line, column, reason, text): (String, u32, u32, String, String) = eval(
            r#"
            local value, err = serde_json.from_str('{"ok": 1,\n bad}')
            assert(value == nil and err.op == "vnd:serde_json.from_str")
            return err.kind, err.line, err.column, err.reason, tostring(err)
            "#,
        );
        assert_eq!((kind.as_str(), line, column), ("syntax", 2, 2));
        assert_eq!(reason, "key must be a string");
        assert_eq!(text, "vnd:serde_json.from_str: syntax: line 2, column 2: key must be a string");
    }

    #[test]
    fn a_text_that_ends_early_is_eof() {
        let kinds: Vec<String> = eval(
            r#"
            local out = {}
            for _, text in { "", "  ", '{"a":', "[1, 2", '"open' } do
                local value, err = serde_json.from_str(text)
                assert(value == nil and err.line >= 1)
                table.insert(out, err.kind)
            end
            return out
            "#,
        );
        assert_eq!(kinds, ["eof"; 5]);
    }

    #[test]
    fn text_that_is_not_utf8_or_nests_too_deep_is_a_syntax_error() {
        let (bad_utf8, deep): (String, String) = eval(
            r#"
            local _, bad = serde_json.from_str('"\255"')
            local _, deep = serde_json.from_str(("["):rep(100000) .. ("]"):rep(100000))
            assert(bad.kind == "syntax" and deep.kind == "syntax")
            return bad.reason, deep.reason
            "#,
        );
        assert!(bad_utf8.contains("invalid unicode"), "got: {bad_utf8}");
        assert!(deep.contains("recursion limit"), "got: {deep}");
    }

    #[test]
    fn encodes_every_kind_of_value() {
        let seen: Vec<String> = eval(
            r#"
            return {
                serde_json.to_string(42),
                serde_json.to_string(1.5),
                serde_json.to_string("hello"),
                serde_json.to_string(true),
                serde_json.to_string(nil),
                serde_json.to_string({ 1, "two", false }),
                serde_json.to_string({ b = 1, a = { nested = { "x" } } }),
                serde_json.to_string({}),
                serde_json.to_string({ n = 0 / 0, big = math.huge }),
                serde_json.to_string('quote " and \n newline'),
            }
            "#,
        );
        assert_eq!(
            seen,
            [
                "42",
                "1.5",
                "\"hello\"",
                "true",
                "null",
                "[1,\"two\",false]",
                "{\"a\":{\"nested\":[\"x\"]},\"b\":1}",
                "{}",
                "{\"big\":null,\"n\":null}",
                "\"quote \\\" and \\n newline\"",
            ]
        );
    }

    #[test]
    fn pretty_output_is_indented_and_reads_back() {
        let (pretty, ok): (String, bool) = eval(
            r#"
            local pretty = assert(serde_json.to_string_pretty({ a = 1, list = { true } }))
            local back = assert(serde_json.from_str(pretty))
            return pretty, back:get("a") == 1 and back:at({ "list", 1 }) == true
            "#,
        );
        assert_eq!(pretty, "{\n  \"a\": 1,\n  \"list\": [\n    true\n  ]\n}");
        assert!(ok);
    }

    #[test]
    fn a_json_handle_is_written_as_what_it_holds() {
        let (text, pretty): (String, String) = eval(
            r#"
            local doc = assert(serde_json.from_str('{"users":[{"name":"ada"}],"none":null}'))
            doc:get("users"):push({ name = "grace" })
            return serde_json.to_string(doc), serde_json.to_string_pretty(doc:get("users"):get(2))
            "#,
        );
        assert_eq!(text, r#"{"none":null,"users":[{"name":"ada"},{"name":"grace"}]}"#);
        assert_eq!(pretty, "{\n  \"name\": \"grace\"\n}");
    }

    #[test]
    fn round_trips_a_nested_structure() {
        let ok: bool = eval(
            r#"
            local original = { name = "bob", scores = { 1, 2, 3 }, active = true, deep = { a = { b = "c" } } }
            local decoded = assert(serde_json.from_str((assert(serde_json.to_string(original)))))
            return decoded:get("name") == "bob" and decoded:get("active") == true
                and decoded:get("scores"):len() == 3 and decoded:at({ "deep", "a", "b" }) == "c"
            "#,
        );
        assert!(ok);
    }

    /// 127 levels are written and read back; 128 are refused by every writer, since `from_str` could not read them.
    #[test]
    fn the_nesting_limit_is_what_reads_back() {
        let (kinds, max_depth): (Vec<String>, u32) = eval(
            r#"
            local function nested(depth)
                local v = {}
                for _ = 2, depth do v = { v } end
                return v
            end
            local text = assert(serde_json.to_string(nested(127)))
            assert(serde_json.from_str(text) ~= nil)
            local out, max_depth = {}, nil
            for _, encode in { serde_json.to_string, serde_json.to_string_pretty } do
                local refused, err = encode(nested(128))
                assert(refused == nil)
                table.insert(out, err.kind)
                max_depth = err.max_depth
            end
            return out, max_depth
            "#,
        );
        assert_eq!(kinds, ["too_deep", "too_deep"]);
        assert_eq!(max_depth, 127);
    }

    /// A table that contains itself, or one nested a hundred thousand deep, is refused: no stack overflow.
    #[test]
    fn cyclic_and_very_deep_tables_are_refused_not_a_stack_overflow() {
        let refused: Vec<String> = eval(
            r#"
            local cyclic = {}
            cyclic.self = cyclic
            local deep = {}
            for _ = 1, 100000 do deep = { deep } end
            local out = {}
            for _, value in { cyclic, deep } do
                local ok, err = pcall(serde_json.to_string, value)
                assert(not ok)
                table.insert(out, tostring(err))
            end
            return out
            "#,
        );
        assert_eq!(refused.len(), 2);
        assert!(
            refused.iter().all(|e| e.contains("vnd:serde_json.to_string: argument #1 'value'")),
            "{refused:#?}"
        );
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
            for _, value in { buffer.create(2), { data = { buffer.fromstring("ab") } }, deep } do
                for _, encode in { serde_json.to_string, serde_json.to_string_pretty } do
                    local ok, err = pcall(encode, value)
                    assert(not ok)
                    table.insert(out, tostring(err))
                end
            end
            assert(serde_json.to_string({ data = buffer.tostring(buffer.fromstring("ab")) }) == '{"data":"ab"}')
            return out
            "#,
        );
        assert_eq!(refused.len(), 6);
        assert!(refused.iter().all(|e| e.contains("argument #1 'value'")), "{refused:#?}");
    }

    #[test]
    fn what_is_not_plain_data_is_refused() {
        let refused: String = eval(
            r#"
            assert(not pcall(serde_json.to_string, { f = print }))
            assert(not pcall(serde_json.to_string_pretty, print))
            assert(not pcall(serde_json.from_str, 42))
            assert(not pcall(serde_json.from_str, nil))
            local _, e = pcall(serde_json.to_string, function() end)
            return tostring(e)
            "#,
        );
        assert!(refused.contains("vnd:serde_json.to_string: argument #1 'value'"), "got: {refused}");
    }

    fn with_io<T: mlua::FromLuaMulti>(code: &str) -> (T, String) {
        let (lua, sink) = neoc_vm_std::io::testing::engine("vnd:serde_json", "serde_json");
        let value = lua.load(code).eval().unwrap();
        (value, sink.text())
    }

    #[test]
    fn from_reader_reads_a_reader_to_its_end() {
        let (ok, _): (bool, _) = with_io(
            r#"
            local reader = io.from_string('skipped\n{"name":"alice","tags":["a","b"]}\n')
            reader:read_line()
            local object, err = serde_json.from_reader(reader)
            assert(err == nil and reader:read(1) == nil)
            assert(object:get("name") == "alice" and object:at({ "tags", 2 }) == "b")
            assert(serde_json.from_reader(io.from_string("42")) == 42)
            assert(assert(serde_json.from_reader(io.from_string("[1,2,3]"))):len() == 3)
            local null, none = serde_json.from_reader(io.from_string("null"))
            return null == nil and none == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn what_a_reader_gives_can_fail_three_ways() {
        let (seen, _): (Vec<String>, _) = with_io(
            r#"
            local value, syntax = serde_json.from_reader(io.from_string('{"ok": 1,\n bad}'))
            assert(value == nil and syntax.kind == "syntax" and syntax.line == 2 and syntax.column == 2)
            local _, eof = serde_json.from_reader(io.from_string(""))
            assert(eof.kind == "eof" and eof.line == 1)
            local _, trailing = serde_json.from_reader(io.from_string("1 2"))
            assert(trailing.kind == "syntax")
            local _, failed = serde_json.from_reader(broken_reader)
            assert(failed.kind == "io" and failed.reason == "the pipe is closed")
            return { tostring(syntax), eof.op, tostring(failed) }
            "#,
        );
        assert_eq!(
            seen,
            [
                "vnd:serde_json.from_reader: syntax: line 2, column 2: key must be a string",
                "vnd:serde_json.from_reader",
                "vnd:serde_json.from_reader: io: the pipe is closed",
            ]
        );
    }

    #[test]
    fn to_writer_writes_what_to_string_gives() {
        let (ok, written): (bool, String) = with_io(
            r#"
            local ok, err = serde_json.to_writer({ result = "ok", list = { 1, 2 } }, writer)
            assert(ok == true and err == nil)
            assert(writer:write("\n"))
            return serde_json.to_writer(assert(serde_json.from_str("[true,null]")), writer)
            "#,
        );
        assert!(ok);
        assert_eq!(written, "{\"list\":[1,2],\"result\":\"ok\"}\n[true,null]");
    }

    #[test]
    fn to_writer_fails_typed_and_writes_nothing_it_cannot_encode() {
        let (seen, written): (Vec<String>, String) = with_io(
            r#"
            local deep = {}
            for _ = 2, 128 do
                deep = { deep }
            end
            local ok, too_deep = serde_json.to_writer(deep, writer)
            assert(ok == nil and too_deep.kind == "too_deep" and too_deep.max_depth == 127)
            local _, failed = serde_json.to_writer({ a = 1 }, broken_writer)
            assert(failed.kind == "io" and failed.reason == "read-only")
            local raised, refused = pcall(serde_json.to_writer, { data = buffer.create(2) }, writer)
            assert(not raised)
            return { too_deep.op, tostring(failed), tostring(refused) }
            "#,
        );
        assert_eq!(written, "", "nothing reaches the writer");
        assert_eq!(seen[0], "vnd:serde_json.to_writer");
        assert_eq!(seen[1], "vnd:serde_json.to_writer: io: read-only");
        assert!(seen[2].contains("vnd:serde_json.to_writer: argument #1 'value'"), "{seen:#?}");
    }

    #[test]
    fn a_reader_is_a_reader_and_a_writer_a_writer() {
        let (seen, _): (Vec<String>, _) = with_io(
            r#"
            local reader = io.from_string("1")
            local ok1, not_reader = pcall(serde_json.from_reader, writer)
            local ok2, not_writer = pcall(serde_json.to_writer, { x = 1 }, reader)
            assert(not ok1 and not ok2 and not pcall(serde_json.from_reader, "1") and not pcall(serde_json.to_writer, 1))
            return { tostring(not_reader), tostring(not_writer) }
            "#,
        );
        assert!(
            seen[0].contains("vnd:serde_json.from_reader: argument #1 'reader': expected a Reader"),
            "{seen:#?}"
        );
        assert!(
            seen[1].contains("vnd:serde_json.to_writer: argument #2 'writer': expected a Writer"),
            "{seen:#?}"
        );
    }
}

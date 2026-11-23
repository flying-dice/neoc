//! JSON text in and out: `vnd:serde_json` (the `serde_json` crate).
//!
//! | Function                  | Rust equivalent                |
//! |---------------------------|--------------------------------|
//! | `from_str(text)`          | `serde_json::from_slice`       |
//! | `to_string(value)`        | `serde_json::to_string`        |
//! | `to_string_pretty(value)` | `serde_json::to_string_pretty` |
//!
//! A parsed document is handed out as `lib:json` does it: an `Object` or `Array` handle for a container, a Lua value
//! for a scalar. Objects are written with their keys sorted.
//!
//! Ported from neoc-vm's `vnd:serde_json`. What changed on purpose:
//! - A JSON `null` document reads as `nil` (there is no `json.null` sentinel, as in `lib:json`).
//! - A failure is a typed error: `DecodeError` (`syntax` or `eof`, with the line and column) and `EncodeError`
//!   (`too_deep`).
//! - A value that is not plain data (a function, a buffer anywhere inside, a table that contains itself, a handle
//!   nested in a plain table) is refused as an argument; some of these were returned errors.
//!
//! TODO(std:io): `from_reader(reader)` and `to_writer(value, writer)` wait for `std:io`'s reader and writer handles.

#[mlua_ir::module("vnd:serde_json")]
pub mod serde_json {
    use mlua_ir::{Bytes, LuaError};
    use serde::Serialize;

    use crate::modules::lib::json::deeper_than;
    use crate::modules::lib::json::json::{Input, MAX_DEPTH, Node};

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
        let (line, column) = (wide(error.line()), wide(error.column()));
        // serde_json ends its text with the place; the place is in the fields
        let text = error.to_string();
        let reason = match text.rfind(" at line ") {
            Some(end) => text[..end].to_owned(),
            None => text,
        };
        if error.is_eof() {
            DecodeError::Eof { line, column, reason }
        } else {
            DecodeError::Syntax { line, column, reason }
        }
    }

    fn wide(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
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
        Ok(value)
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
}

#[cfg(test)]
mod tests {
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
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
}

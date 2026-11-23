//! XML decode and encode: `vnd:quick_xml` (the `quick-xml` crate, through serde).
//!
//! | Function                   | Rust equivalent                      |
//! |----------------------------|--------------------------------------|
//! | `decode_string(text)`      | `quick_xml::de::from_str`            |
//! | `encode(value, options?)`  | `quick_xml::se::to_string_with_root` |
//!
//! An element is a table: a child element is a key, an attribute a key that starts with `@`, text beside children
//! the key `$text`. An element that holds only text is that text, and elements of one name that repeat are a list.
//! The root element's name is not kept. All text is text: `<age>30</age>` is `"30"`.
//!
//! Ported from neoc-vm's `vnd:quick_xml`. What changed on purpose:
//! - Elements of one name that repeat decode to a list (only the last one was kept), so a list that was encoded
//!   reads back.
//! - `encode` takes the root element's name as an option (it was always `root`), and a document is a table with
//!   string keys: a scalar or a list as the whole value is refused as an argument (a list wrote several roots).
//! - An empty element or attribute name is an error (it wrote `<>`).
//! - A failure is a typed error: `DecodeError` (`syntax`, `unexpected_eof`, `too_deep`, `invalid_utf8`,
//!   `unsupported`) and `EncodeError` (`unsupported`).
//! - A value that is not plain data (a function, a buffer anywhere inside) is refused as an argument; some of these
//!   were returned errors.
//!
//! TODO(std:io): `decode(reader)` and `encode_to(value, writer)` wait for `std:io`'s reader and writer handles.

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::{Map, Value};

/// The key quick-xml gives an element's own text.
const TEXT: &str = "$text";

/// An element's content as plain data: text, or a table of attributes, text and children.
struct Content(Value);

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ContentVisitor)
    }
}

struct ContentVisitor;

impl<'de> Visitor<'de> for ContentVisitor {
    type Value = Content;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("XML text or an XML element")
    }

    fn visit_str<E>(self, text: &str) -> Result<Content, E> {
        Ok(Content(Value::String(text.to_owned())))
    }

    fn visit_string<E>(self, text: String) -> Result<Content, E> {
        Ok(Content(Value::String(text)))
    }

    fn visit_unit<E>(self) -> Result<Content, E> {
        Ok(Content(Value::Null))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Content, A::Error> {
        // A value read here is never a list itself, so a list under a key is always the repeats so far.
        let mut entries = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let Content(value) = map.next_value()?;
            match entries.get_mut(&key) {
                None => {
                    entries.insert(key, value);
                }
                Some(Value::Array(repeats)) => repeats.push(value),
                Some(first) => *first = Value::Array(vec![first.take(), value]),
            }
        }
        if entries.len() == 1
            && let Some(text) = entries.remove(TEXT)
        {
            return Ok(Content(text));
        }
        Ok(Content(Value::Object(entries)))
    }
}

/// `true` when `value` has an element or attribute with no name: quick-xml would write `<>`.
fn has_empty_name(value: &Value) -> bool {
    match value {
        Value::Object(entries) => {
            entries.iter().any(|(key, item)| key.is_empty() || key == "@" || has_empty_name(item))
        }
        Value::Array(items) => items.iter().any(has_empty_name),
        _ => false,
    }
}

#[mlua_ir::module("vnd:quick_xml")]
pub mod quick_xml {
    use std::collections::BTreeMap;

    use mlua_ir::{Bytes, LuaError, Options, Unknown};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use super::{Content, has_empty_name};

    mlua_ir::unknown_reason!(
        /// Why a decoded document has no declared shape.
        XmlText = "whatever the XML text holds: its shape is the text's, not this API's"
    );
    mlua_ir::unknown_reason!(
        /// Why a value to encode has no declared shape.
        PlainData = "any plain data a script built (tables, strings, numbers, booleans): an XML document holds whatever it is given"
    );

    /// Why a text is not an XML document this module reads.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// The text is not well-formed XML: a tag that is not closed, an end tag that does not match, an entity that
        /// is not known.
        #[error("{reason}")]
        Syntax {
            /// What is wrong.
            reason: String,
        },
        /// The text ends before any element or text.
        #[error("the text holds no XML element")]
        UnexpectedEof,
        /// The elements nest deeper than the parser reads.
        #[error("the elements nest more than {max_depth} levels deep")]
        TooDeep {
            /// The most levels the parser reads.
            max_depth: u32,
        },
        /// The text is not UTF-8.
        #[error("the text is not UTF-8 from byte {offset}")]
        InvalidUtf8 {
            /// How many bytes from the start are valid UTF-8.
            offset: u32,
        },
        /// The text is XML, but holds what plain data cannot.
        #[error("{reason}")]
        Unsupported {
            /// What the parser said.
            reason: String,
        },
    }

    /// Why a value cannot be written as XML.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum EncodeError {
        /// The value holds something XML has no form for: a key (or the root) that is not an XML name, say.
        #[error("{reason}")]
        Unsupported {
            /// What is wrong.
            reason: String,
        },
    }

    /// How `encode` writes a document.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct EncodeOptions {
        /// The root element's name.
        pub root: String,
    }

    impl Default for EncodeOptions {
        fn default() -> Self {
            Self { root: "root".to_owned() }
        }
    }

    fn wide(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    fn decode_error(error: ::quick_xml::DeError) -> DecodeError {
        match error {
            ::quick_xml::DeError::InvalidXml(invalid) => DecodeError::Syntax { reason: invalid.to_string() },
            ::quick_xml::DeError::UnexpectedEof => DecodeError::UnexpectedEof,
            ::quick_xml::DeError::TooDeeplyNested(limit) => DecodeError::TooDeep { max_depth: wide(limit) },
            other => DecodeError::Unsupported { reason: other.to_string() },
        }
    }

    /// Parse an XML document as plain data: what its root element holds (a table, or the text of a root that holds
    /// only text).
    ///
    /// ```luau
    /// local doc = assert(quick_xml.decode_string('<user id="7"><name>ada</name><tag>a</tag><tag>b</tag></user>'))
    /// local user = doc :: { ["@id"]: string, name: string, tag: { string } } -- this script's own knowledge
    /// assert(user["@id"] == "7" and user.name == "ada" and user.tag[2] == "b")
    /// local _, err = quick_xml.decode_string("<unclosed")
    /// assert(err ~= nil and err.kind == "syntax")
    /// ```
    pub fn decode_string(text: Bytes) -> Result<Unknown<XmlText>, DecodeError> {
        let text = std::str::from_utf8(&text)
            .map_err(|error| DecodeError::InvalidUtf8 { offset: wide(error.valid_up_to()) })?;
        let Content(value) = ::quick_xml::de::from_str(text).map_err(decode_error)?;
        Ok(Unknown::new(value))
    }

    /// Write a document (a table with string keys) as XML under one root element. A key is a child element, a key
    /// that starts with `@` an attribute, `$text` the element's own text; a list writes its element once per item.
    ///
    /// ```luau
    /// local user = { ["@id"] = "7", name = "ada", tag = { "a", "b" } }
    /// assert(quick_xml.encode(user, { root = "user" }) == '<user id="7"><name>ada</name><tag>a</tag><tag>b</tag></user>')
    /// assert(quick_xml.encode({ name = "ada" }) == "<root><name>ada</name></root>")
    /// ```
    pub fn encode(
        value: BTreeMap<String, Unknown<PlainData>>,
        options: Option<EncodeOptions>,
    ) -> Result<String, EncodeError> {
        let root = options.unwrap_or_default().root;
        let document = Value::Object(value.into_iter().map(|(key, item)| (key, item.value)).collect());
        if root.is_empty() || has_empty_name(&document) {
            return Err(EncodeError::Unsupported { reason: "an XML name is not empty".to_owned() });
        }
        ::quick_xml::se::to_string_with_root(&root, &document).map_err(|error| match error {
            ::quick_xml::SeError::Unsupported(reason) => {
                EncodeError::Unsupported { reason: reason.into_owned() }
            }
            other => EncodeError::Unsupported { reason: other.to_string() },
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Content, has_empty_name};

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("vnd:quick_xml")"#).eval().unwrap();
        lua.globals().set("quick_xml", m).unwrap();
        lua.load(code).eval().unwrap()
    }

    fn content(text: &str) -> serde_json::Value {
        let Content(value) = quick_xml::de::from_str(text).unwrap();
        value
    }

    #[test]
    fn content_is_text_or_a_table_and_repeats_are_a_list() {
        assert_eq!(
            content("<r><name>alice</name><age>30</age></r>"),
            json!({ "name": "alice", "age": "30" })
        );
        assert_eq!(content("<r>text</r>"), json!("text"));
        assert_eq!(content("<r/>"), json!({}));
        assert_eq!(content("<r><a/></r>"), json!({ "a": {} }));
        assert_eq!(
            content("<r><a>1</a><b>x</b><a>2</a><a>3</a></r>"),
            json!({ "a": ["1", "2", "3"], "b": "x" })
        );
        assert_eq!(
            content(r#"<r><u id="1"><n>a</n></u><u id="2"/></r>"#),
            json!({ "u": [{ "@id": "1", "n": "a" }, { "@id": "2" }] })
        );
        assert_eq!(content(r#"<r id="7">text</r>"#), json!({ "@id": "7", "$text": "text" }));
        assert_eq!(
            content("<r><a><![CDATA[<raw>]]></a><b>&lt;&amp;</b></r>"),
            json!({ "a": "<raw>", "b": "<&" })
        );
        assert_eq!(content("<r><a>x<b>y</b>z</a></r>"), json!({ "a": { "$text": ["x", "z"], "b": "y" } }));
    }

    #[test]
    fn an_empty_name_is_found_anywhere() {
        assert!(has_empty_name(&json!({ "": 1 })));
        assert!(has_empty_name(&json!({ "a": { "@": "x" } })));
        assert!(has_empty_name(&json!({ "a": [{ "b": { "": 1 } }] })));
        assert!(!has_empty_name(&json!({ "a": [{ "@b": "", "$text": "" }] })));
    }

    #[test]
    fn decodes_elements_attributes_and_text() {
        let ok: bool = eval(
            r#"
            local doc, err = quick_xml.decode_string('<?xml version="1.0"?><root><name>alice</name><age>30</age><user id="7"><name>bob</name></user></root>')
            assert(err == nil)
            assert(doc.name == "alice" and doc.age == "30", "all text is text")
            assert(doc.user["@id"] == "7" and doc.user.name == "bob")
            assert(quick_xml.decode_string("<root>only text</root>") == "only text")
            assert(next((assert(quick_xml.decode_string("<root/>")))) == nil)
            return true
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn repeated_elements_are_a_list() {
        let ok: bool = eval(
            r#"
            local doc = assert(quick_xml.decode_string("<root><item>a</item><other>x</other><item>b</item><item>c</item></root>"))
            assert(#doc.item == 3 and doc.item[1] == "a" and doc.item[3] == "c" and doc.other == "x")
            local users = assert(quick_xml.decode_string('<root><user id="1"/><user id="2"><name>bob</name></user></root>'))
            return users.user[1]["@id"] == "1" and users.user[2].name == "bob"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn invalid_xml_is_a_typed_error() {
        let seen: Vec<String> = eval(
            r#"
            local out = {}
            for _, text in { "<unclosed", "<root><a></root>", "<root><a>", "<root><a>&bogus;</a></root>", "", "<a>\255</a>" } do
                local doc, err = quick_xml.decode_string(text)
                assert(doc == nil and err.op == "vnd:quick_xml.decode_string")
                table.insert(out, err.kind)
            end
            local _, bad = quick_xml.decode_string("<a>ok\255</a>")
            assert(bad.offset == 5)
            local _, err = quick_xml.decode_string("<unclosed")
            assert(type(err.reason) == "string")
            table.insert(out, tostring(err))
            return out
            "#,
        );
        assert_eq!(seen[..6], ["syntax", "syntax", "syntax", "syntax", "unexpected_eof", "invalid_utf8"]);
        assert!(
            seen[6].starts_with("vnd:quick_xml.decode_string: syntax: syntax error: tag not closed"),
            "{}",
            seen[6]
        );
    }

    /// quick-xml stops nesting elements at its recursion limit.
    #[test]
    fn deeply_nested_xml_is_an_error_not_a_stack_overflow() {
        let (kind, max_depth): (String, u32) = eval(
            r#"
            local doc, err = quick_xml.decode_string(("<a>"):rep(100000) .. ("</a>"):rep(100000))
            assert(doc == nil)
            return err.kind, err.max_depth
            "#,
        );
        assert_eq!((kind.as_str(), max_depth), ("too_deep", 128));
    }

    #[test]
    fn encodes_a_document() {
        let seen: Vec<String> = eval(
            r#"
            return {
                quick_xml.encode({ name = "dave", age = 42, on = true }),
                quick_xml.encode({ user = { ["@id"] = "7", name = "bob", ["$text"] = "t" } }),
                quick_xml.encode({ item = { 1, 2, 3 }, user = { { n = "a" }, { n = "b" } } }),
                quick_xml.encode({ text = 'x < y & "z"' }),
                quick_xml.encode({}),
                quick_xml.encode({ name = "ada" }, { root = "user" }),
                quick_xml.encode({ ["@id"] = "7" }, { root = "ns:user" }),
                quick_xml.encode({ name = "ada" }, {}),
            }
            "#,
        );
        assert_eq!(
            seen,
            [
                "<root><age>42</age><name>dave</name><on>true</on></root>",
                r#"<root><user id="7">t<name>bob</name></user></root>"#,
                "<root><item>1</item><item>2</item><item>3</item><user><n>a</n></user><user><n>b</n></user></root>",
                r#"<root><text>x &lt; y &amp; "z"</text></root>"#,
                "<root/>",
                "<user><name>ada</name></user>",
                r#"<ns:user id="7"/>"#,
                "<root><name>ada</name></root>",
            ]
        );
    }

    #[test]
    fn round_trips() {
        let ok: bool = eval(
            r#"
            local original = { name = "eve", age = "42", tags = { "a", "b" }, user = { ["@id"] = "7", city = "x" } }
            local decoded = assert(quick_xml.decode_string((assert(quick_xml.encode(original)))))
            return decoded.name == "eve" and decoded.age == "42" and #decoded.tags == 2 and decoded.tags[2] == "b"
                and decoded.user["@id"] == "7" and decoded.user.city == "x"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_name_that_xml_cannot_write_is_a_typed_error() {
        let reasons: Vec<String> = eval(
            r#"
            local out = {}
            for _, attempt in {
                { { ["bad key"] = 1 } },
                { { ["1"] = 1 } },
                { { [""] = 1 } },
                { { a = { { ["@"] = "x" } } } },
                { { a = 1 }, { root = "" } },
                { { a = 1 }, { root = "bad name" } },
            } do
                local text, err = quick_xml.encode(attempt[1], attempt[2])
                assert(text == nil and err.kind == "unsupported" and err.op == "vnd:quick_xml.encode")
                table.insert(out, err.reason)
            end
            return out
            "#,
        );
        assert_eq!(reasons.len(), 6);
        assert!(reasons[0].contains("is not allowed in an XML name `bad key`"), "{}", reasons[0]);
        assert!(reasons[1].contains("is not allowed at the start of an XML name `1`"), "{}", reasons[1]);
        assert_eq!(reasons[2..5], ["an XML name is not empty"; 3]);
        assert!(reasons[5].contains("`bad name`"), "{}", reasons[5]);
    }

    #[test]
    fn a_document_is_a_table_with_string_keys() {
        let refused: String = eval(
            r#"
            assert(not pcall(quick_xml.encode, "text"))
            assert(not pcall(quick_xml.encode, { 1, 2, 3 }))
            assert(not pcall(quick_xml.encode, 42))
            assert(not pcall(quick_xml.encode, { f = print }))
            assert(not pcall(quick_xml.encode, { a = 1 }, { rot = "x" }), "an option that does not exist")
            assert(not pcall(quick_xml.decode_string, 42))
            local _, e = pcall(quick_xml.encode, nil)
            return tostring(e)
            "#,
        );
        assert!(refused.contains("vnd:quick_xml.encode: argument #1 'value'"), "got: {refused}");
    }

    /// A buffer is refused, never turned into a run of numbered elements.
    #[test]
    fn a_buffer_is_refused() {
        let refused: Vec<String> = eval(
            r#"
            local deep = { buffer.create(2) }
            for _ = 2, 100 do
                deep = { inner = deep }
            end
            local out = {}
            for _, value in { { data = buffer.create(2) }, { root = { data = buffer.create(2) } }, { root = deep } } do
                local ok, err = pcall(quick_xml.encode, value)
                assert(not ok)
                table.insert(out, tostring(err))
            end
            return out
            "#,
        );
        assert_eq!(refused.len(), 3);
        assert!(
            refused
                .iter()
                .all(|e| e.contains("vnd:quick_xml.encode: argument #1 'value'") && e.contains("byte array")),
            "{refused:#?}"
        );
    }
}

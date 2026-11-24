//! JSON Schema validation: `vnd:jsonschema` (the `jsonschema` crate).
//!
//! ```luau
//! local jsonschema = require("vnd:jsonschema")
//! local serde_json = require("vnd:serde_json")
//!
//! local USER = assert(jsonschema.compile([[
//!   { "type": "object",
//!     "required": ["name", "email"],
//!     "properties": { "name": { "type": "string", "maxLength": 255 }, "email": { "type": "string" } } }
//! ]]))
//!
//! local body = serde_json.from_str(request_body)
//! if not USER:is_valid(body) then
//!     return { status = 400 }
//! end
//! ```
//!
//! A schema is compiled once and checked many times. `is_valid` is the gate: it stops at the first violation and
//! builds nothing. `validate` walks the whole value and says everything that is wrong with it. A value is plain data
//! or a `lib:json` handle. References to other documents (`$ref` to a URL or a file) are not fetched.
//!
//! The API's rules:
//! - `validate` gives `true`, or `nil` and a typed error holding every violation (where in the value, which rule of the
//!   schema, which keyword, why).
//! - `compile` fails with a typed error: `syntax` (the text is not JSON, with the line and column) or `invalid_schema`
//!   (with the place in the schema).
//! - A value that is not plain data (a function, a buffer anywhere inside) is refused as an argument by both methods.

/// One line for a list of violations: the first, and how many more.
fn summary(violations: &[jsonschema::Violation]) -> String {
    let Some(first) = violations.first() else {
        return "the value does not conform to the schema".to_owned();
    };
    let place = if first.instance_path.is_empty() { "the value" } else { first.instance_path.as_str() };
    match violations.len() - 1 {
        0 => format!("{place}: {}", first.reason),
        more => format!("{place}: {} (and {more} more)", first.reason),
    }
}

#[mlua_ir::module("vnd:jsonschema")]
pub mod jsonschema {
    use std::sync::Arc;

    use mlua_ir::{Bytes, LuaError, Object, Shape};
    use serde::Serialize;

    use super::summary;
    use crate::modules::vnd::serde_json::place_and_reason;
    use neoc_lib_json::json::Input;

    /// Why a text is not a schema.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CompileError {
        /// The text is not JSON.
        #[error("line {line}, column {column}: {reason}")]
        Syntax {
            /// The line the parser stopped on, counting from 1.
            line: u32,
            /// The column (in bytes), counting from 1.
            column: u32,
            /// What is wrong.
            reason: String,
        },
        /// The text is JSON, but not a JSON Schema: a keyword holds the wrong thing, a pattern is not a regular
        /// expression, a reference leads nowhere.
        #[error("{reason}")]
        InvalidSchema {
            /// Where in the schema, as a JSON pointer (`/properties/name/type`); empty for the schema as a whole.
            path: String,
            /// What is wrong.
            reason: String,
        },
    }

    /// One thing wrong with a value.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct Violation {
        /// Where in the value, as a JSON pointer (`/users/0/name`); empty for the value as a whole.
        pub instance_path: String,
        /// The rule of the schema that was broken, as a JSON pointer (`/properties/users/items/required`).
        pub schema_path: String,
        /// The keyword of that rule (`required`, `type`, `maxLength`).
        pub keyword: String,
        /// What is wrong, for a person.
        pub reason: String,
    }

    /// Why a value does not conform to a schema.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ValidationError {
        /// The value breaks at least one rule of the schema.
        #[error("{}", summary(.violations))]
        Invalid {
            /// Everything that is wrong, in the order the validator found it.
            violations: Vec<Violation>,
        },
    }

    /// A compiled schema. Compile it once and keep it: checking a value against it is the fast part.
    #[derive(Clone, Object)]
    pub struct Schema {
        validator: Arc<::jsonschema::Validator>,
    }

    #[mlua_ir::object]
    impl Schema {
        /// Whether `value` conforms. Stops at the first violation and builds nothing: the gate for a handler.
        ///
        /// ```luau
        /// local schema = assert(jsonschema.compile('{"type":"integer","minimum":0}'))
        /// assert(schema:is_valid(5) and not schema:is_valid(-1) and not schema:is_valid("5"))
        /// ```
        pub fn is_valid(&self, value: Input) -> bool {
            self.validator.is_valid(&value.into_value())
        }

        /// Check `value` against the whole schema: `true`, or everything that is wrong with it.
        ///
        /// ```luau
        /// local schema = assert(jsonschema.compile('{"type":"object","required":["name"]}'))
        /// assert(schema:validate({ name = "ada" }))
        /// local ok, err = schema:validate({ nickname = "ada" })
        /// assert(ok == nil and err ~= nil and err.violations[1].keyword == "required")
        /// ```
        pub fn validate(&self, value: Input) -> Result<(), ValidationError> {
            let value = value.into_value();
            let violations: Vec<Violation> = self
                .validator
                .iter_errors(&value)
                .map(|error| Violation {
                    instance_path: error.instance_path().as_str().to_owned(),
                    schema_path: error.schema_path().as_str().to_owned(),
                    keyword: error.kind().keyword().to_owned(),
                    reason: error.to_string(),
                })
                .collect();
            if violations.is_empty() { Ok(()) } else { Err(ValidationError::Invalid { violations }) }
        }
    }

    /// Compile a JSON Schema from its JSON text. The draft is the one its `$schema` names (2020-12 when it names
    /// none).
    ///
    /// ```luau
    /// local schema = assert(jsonschema.compile('{"type":"string","maxLength":3}'))
    /// assert(schema:is_valid("abc") and not schema:is_valid("abcd"))
    /// local _, err = jsonschema.compile('{"type":"nonsense"}')
    /// assert(err ~= nil and err.kind == "invalid_schema" and err.path == "/type")
    /// ```
    pub fn compile(schema: Bytes) -> Result<Schema, CompileError> {
        let document: ::serde_json::Value = ::serde_json::from_slice(&schema).map_err(|error| {
            let (line, column, reason) = place_and_reason(&error);
            CompileError::Syntax { line, column, reason }
        })?;
        let validator =
            ::jsonschema::validator_for(&document).map_err(|error| CompileError::InvalidSchema {
                path: error.instance_path().as_str().to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Schema { validator: Arc::new(validator) })
    }
}

#[cfg(test)]
mod tests {
    use super::jsonschema::Violation;
    use super::summary;

    const USER_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["name", "username", "email"],
        "properties": {
            "name":     { "type": "string", "maxLength": 255 },
            "username": { "type": "string", "minLength": 1, "maxLength": 64 },
            "email":    { "type": "string", "maxLength": 255 },
            "phone":    { "type": "string" }
        },
        "additionalProperties": false
    }"#;

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("vnd:jsonschema")"#).eval().unwrap();
        lua.globals().set("jsonschema", m).unwrap();
        lua.globals().set("USER_SCHEMA", USER_SCHEMA).unwrap();
        lua.load(code).eval().unwrap()
    }

    fn violation(instance_path: &str, reason: &str) -> Violation {
        Violation {
            instance_path: instance_path.to_owned(),
            schema_path: String::new(),
            keyword: String::new(),
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn a_summary_is_the_first_violation_and_a_count() {
        assert_eq!(summary(&[]), "the value does not conform to the schema");
        assert_eq!(summary(&[violation("", "5 is not a string")]), "the value: 5 is not a string");
        assert_eq!(
            summary(&[violation("/a/0", "too long"), violation("", "x"), violation("", "y")]),
            "/a/0: too long (and 2 more)"
        );
    }

    #[test]
    fn compile_refuses_what_is_not_a_schema() {
        let (syntax_line, syntax_text, path, reason, text): (u32, String, String, String, String) = eval(
            r#"
            assert(jsonschema.compile(USER_SCHEMA))
            local none, syntax = jsonschema.compile("{\n not json")
            assert(none == nil and syntax.kind == "syntax" and syntax.column >= 1 and syntax.op == "vnd:jsonschema.compile")
            local also, invalid = jsonschema.compile('{"properties":{"name":{"type":"nonsense"}}}')
            assert(also == nil and invalid.kind == "invalid_schema")
            local _, pattern = jsonschema.compile('{"pattern":"("}')
            assert(pattern.kind == "invalid_schema")
            return syntax.line, tostring(syntax), invalid.path, invalid.reason, tostring(invalid)
            "#,
        );
        assert_eq!(syntax_line, 2);
        assert!(
            syntax_text.starts_with("vnd:jsonschema.compile: syntax: line 2, column "),
            "got: {syntax_text}"
        );
        assert_eq!(path, "/properties/name/type");
        assert!(reason.contains("nonsense"), "got: {reason}");
        assert_eq!(text, format!("vnd:jsonschema.compile: invalid_schema: {reason}"));
    }

    #[test]
    fn is_valid_is_the_gate() {
        let seen: Vec<bool> = eval(
            r#"
            local s = assert(jsonschema.compile(USER_SCHEMA))
            return {
                s:is_valid({ name = "alice", username = "al", email = "a@x" }),
                s:is_valid({ name = "alice" }),
                s:is_valid({ name = 1, username = "al", email = "a@x" }),
                s:is_valid({ name = "a", username = "u", email = "e", extra = 1 }),
                s:is_valid("text"),
                s:is_valid(nil),
            }
            "#,
        );
        assert_eq!(seen, [true, false, false, false, false, false]);
    }

    #[test]
    fn validate_says_everything_that_is_wrong() {
        let (count, keywords, places, rules, message, text): (
            u32,
            Vec<String>,
            Vec<String>,
            Vec<String>,
            String,
            String,
        ) = eval(
            r#"
            local s = assert(jsonschema.compile(USER_SCHEMA))
            local ok, none = s:validate({ name = "a", username = "u", email = "e" })
            assert(ok == true and none == nil)
            local bad, err = s:validate({ name = 1, username = "", extra = true })
            assert(bad == nil and err.kind == "invalid" and err.op == "vnd:jsonschema.Schema:validate")
            local keywords, places, rules = {}, {}, {}
            for _, violation in err.violations do
                assert(type(violation.reason) == "string" and violation.reason ~= "")
                table.insert(keywords, violation.keyword)
                table.insert(places, violation.instance_path)
                table.insert(rules, violation.schema_path)
            end
            table.sort(keywords)
            table.sort(places)
            table.sort(rules)
            return #err.violations, keywords, places, rules, err.message, tostring(err)
            "#,
        );
        assert_eq!(count, 4);
        assert_eq!(keywords, ["additionalProperties", "minLength", "required", "type"]);
        assert_eq!(places, ["", "", "/name", "/username"]);
        assert_eq!(
            rules,
            ["/additionalProperties", "/properties/name/type", "/properties/username/minLength", "/required"]
        );
        assert!(message.ends_with("(and 3 more)"), "got: {message}");
        assert_eq!(text, format!("vnd:jsonschema.Schema:validate: invalid: {message}"));
    }

    #[test]
    fn a_single_violation_is_its_place_and_reason() {
        let (message, path): (String, String) = eval(
            r#"
            local s = assert(jsonschema.compile('{"properties":{"users":{"items":{"required":["name"]}}}}'))
            local _, err = s:validate({ users = { { name = "a" }, { nick = "b" } } })
            assert(#err.violations == 1)
            return err.message, err.violations[1].instance_path
            "#,
        );
        assert_eq!(path, "/users/1");
        assert!(
            message.starts_with("/users/1: ") && message.contains("\"name\" is a required property"),
            "{message}"
        );
    }

    #[test]
    fn primitives_are_validated() {
        let seen: Vec<bool> = eval(
            r#"
            local text = assert(jsonschema.compile('{"type":"string"}'))
            local whole = assert(jsonschema.compile('{"type":"integer","minimum":0}'))
            local list = assert(jsonschema.compile('{"type":"array","items":{"type":"number"},"minItems":1}'))
            return {
                text:is_valid("hi"), text:is_valid(1),
                whole:is_valid(5), whole:is_valid(-1), whole:is_valid(1.5),
                list:is_valid({ 1, 2.5 }), list:is_valid({ 1, "two" }), list:is_valid({ a = 1 }),
            }
            "#,
        );
        assert_eq!(seen, [true, false, true, false, false, true, false, false]);
    }

    #[test]
    fn a_json_handle_is_validated_as_what_it_holds() {
        let ok: bool = eval(
            r#"
            local serde_json = require("vnd:serde_json")
            local s = assert(jsonschema.compile(USER_SCHEMA))
            local body = assert(serde_json.from_str('{"name":"alice","username":"al","email":"a@x"}'))
            assert(s:is_valid(body) and s:validate(body))
            body:set("extra", { 1, 2 })
            local _, err = s:validate(body)
            return not s:is_valid(body) and err.violations[1].keyword == "additionalProperties"
            "#,
        );
        assert!(ok);
    }

    /// `idn-hostname` and `idn-email` need jsonschema's `idna` feature; without it every value passes, so both the
    /// valid and the invalid side are pinned.
    #[test]
    fn idn_formats_are_checked() {
        let (hostname, email, keyword): (Vec<bool>, Vec<bool>, String) = eval(
            r#"
            local function format_schema(format)
                return assert(jsonschema.compile(
                    '{"$schema":"http://json-schema.org/draft-07/schema#","type":"string","format":"' .. format .. '"}'
                ))
            end
            local hostname, email = format_schema("idn-hostname"), format_schema("idn-email")
            local _, err = hostname:validate("-bad-")
            assert(err.violations[1].reason:find("idn-hostname", 1, true))
            return {
                hostname:is_valid("example.com"), hostname:is_valid("\195\188ber.example"),
                hostname:is_valid("-bad-"), hostname:is_valid("a..b"), hostname:is_valid("xn--"),
                hostname:is_valid(string.rep("a", 64) .. ".com"),
            }, {
                email:is_valid("user@example.com"), email:is_valid("\206\177@\207\128\206\177\207\129.gr"),
                email:is_valid("not an email"), email:is_valid("@nope"),
            }, err.violations[1].keyword
            "#,
        );
        assert_eq!(hostname, [true, true, false, false, false, false]);
        assert_eq!(email, [true, true, false, false]);
        assert_eq!(keyword, "format");
    }

    /// A value with no JSON form is refused by both methods; a buffer is never validated as an array of numbers.
    #[test]
    fn what_is_not_plain_data_is_refused_as_an_argument() {
        let refused: Vec<String> = eval(
            r#"
            local s = assert(jsonschema.compile('{"type":"object"}'))
            local deep = { buffer.create(2) }
            for _ = 2, 100 do
                deep = { inner = deep }
            end
            local out = {}
            for _, value in { print, { f = print }, buffer.create(2), { data = buffer.create(2) }, deep } do
                for _, method in { "is_valid", "validate" } do
                    local ok, err = pcall(s[method], s, value)
                    assert(not ok)
                    assert(tostring(err):find("vnd:jsonschema.Schema:" .. method .. ": argument #1 'value'", 1, true), tostring(err))
                    table.insert(out, tostring(err))
                end
            end
            assert(not pcall(jsonschema.compile, 42) and not pcall(jsonschema.compile, { type = "string" }))
            return out
            "#,
        );
        assert_eq!(refused.len(), 10);
    }

    #[test]
    fn a_reference_to_another_document_is_not_fetched() {
        let kind: String = eval(
            r#"
            local schema, err = jsonschema.compile('{"$ref":"https://example.invalid/schema.json"}')
            assert(schema == nil)
            return err.kind
            "#,
        );
        assert_eq!(kind, "invalid_schema");
    }
}

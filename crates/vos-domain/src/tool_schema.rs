//! Tool input schemas: compile `app.tool { input = { name = t.string {} } }` into a JSON
//! Schema object, and validate an agent's arguments against it (filling defaults) before any app code runs.
//!
//! Only the subset the `app.t` builders can express is supported: `string`, `integer`, `number`,
//! `boolean`, `array`, `object`, `enum`, with `description`, `default`, `minimum`/`maximum`,
//! `minLength`/`maxLength`.

use serde_json::{json, Map, Value};

/// Marker key a builder sets on a field that may be omitted.
pub const OPTIONAL_KEY: &str = "x-optional";

/// `{ field = fragment, ... }` -> `{"type":"object","properties":...,"required":[...]}`. A field is
/// required unless it is marked optional or has a default.
pub fn compile_fields(fields: &Map<String, Value>) -> Result<Value, String> {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, fragment) in fields {
        let mut fragment = fragment
            .as_object()
            .cloned()
            .ok_or_else(|| format!("input field `{name}` must be built with app.t (t.string {{}} ...)"))?;
        let optional = fragment.remove(OPTIONAL_KEY).and_then(|v| v.as_bool()).unwrap_or(false);
        if fragment.get("type").and_then(Value::as_str).is_none() && !fragment.contains_key("enum") {
            return Err(format!("input field `{name}` has no type: build it with app.t"));
        }
        if !optional && !fragment.contains_key("default") {
            required.push(Value::String(name.clone()));
        }
        properties.insert(name.clone(), Value::Object(fragment));
    }
    required.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    Ok(
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false }),
    )
}

/// Validate `args` against `schema`; returns the arguments with defaults filled in.
pub fn validate_args(schema: &Value, args: &Value) -> Result<Value, String> {
    let args = if args.is_null() { json!({}) } else { args.clone() };
    check(schema, &args, "input")
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn check(schema: &Value, value: &Value, at: &str) -> Result<Value, String> {
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        return if options.contains(value) {
            Ok(value.clone())
        } else {
            let list: Vec<String> = options.iter().map(Value::to_string).collect();
            Err(format!("{at} must be one of {}", list.join(", ")))
        };
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => {
            let s =
                value.as_str().ok_or_else(|| format!("{at} must be a string, got {}", type_name(value)))?;
            let len = s.chars().count();
            if schema.get("minLength").and_then(Value::as_u64).is_some_and(|m| (len as u64) < m) {
                return Err(format!("{at} is too short"));
            }
            if schema.get("maxLength").and_then(Value::as_u64).is_some_and(|m| (len as u64) > m) {
                return Err(format!("{at} is too long"));
            }
            Ok(value.clone())
        }
        Some(kind @ ("integer" | "number")) => {
            let n = value.as_f64().ok_or_else(|| format!("{at} must be {kind}, got {}", type_name(value)))?;
            if kind == "integer" && n.fract() != 0.0 {
                return Err(format!("{at} must be an integer"));
            }
            let (min, max) = (
                schema.get("minimum").and_then(Value::as_f64),
                schema.get("maximum").and_then(Value::as_f64),
            );
            if min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m) {
                let range = match (min, max) {
                    (Some(a), Some(b)) => format!("between {a} and {b}"),
                    (Some(a), None) => format!("at least {a}"),
                    (None, Some(b)) => format!("at most {b}"),
                    (None, None) => String::new(),
                };
                return Err(format!("{at} must be {range}"));
            }
            Ok(value.clone())
        }
        Some("boolean") => {
            if value.is_boolean() {
                Ok(value.clone())
            } else {
                Err(format!("{at} must be a boolean, got {}", type_name(value)))
            }
        }
        Some("array") => {
            let items =
                value.as_array().ok_or_else(|| format!("{at} must be an array, got {}", type_name(value)))?;
            match schema.get("items") {
                Some(item_schema) => items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| check(item_schema, v, &format!("{at}[{i}]")))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array),
                None => Ok(value.clone()),
            }
        }
        Some("object") => check_object(schema, value, at),
        _ => Ok(value.clone()),
    }
}

fn check_object(schema: &Value, value: &Value, at: &str) -> Result<Value, String> {
    let given =
        value.as_object().ok_or_else(|| format!("{at} must be an object, got {}", type_name(value)))?;
    let empty = Map::new();
    let properties = schema.get("properties").and_then(Value::as_object).unwrap_or(&empty);
    let mut out = Map::new();
    for (name, prop) in properties {
        let path = format!("{at}.{name}");
        if let Some(v) = given.get(name).filter(|v| !v.is_null()) {
            out.insert(name.clone(), check(prop, v, &path)?);
        } else if let Some(d) = prop.get("default") {
            out.insert(name.clone(), d.clone());
        } else if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|r| r.iter().any(|x| x.as_str() == Some(name)))
        {
            return Err(format!("{path} is required"));
        }
    }
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        if let Some(extra) = given.keys().find(|k| !properties.contains_key(*k)) {
            return Err(format!("unknown field `{extra}` in {at}"));
        }
    } else {
        for (k, v) in given {
            out.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(v: &Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn search_schema() -> Value {
        compile_fields(&fields(&json!({
            "query": {"type": "string", "description": "text"},
            "limit": {"type": "integer", "default": 10, "minimum": 1, "maximum": 50},
            "folder": {"type": "string", "x-optional": true},
        })))
        .unwrap()
    }

    #[test]
    fn fields_compile_to_an_object_schema_with_required_and_defaults() {
        let s = search_schema();
        assert_eq!(s["type"], "object");
        assert_eq!(s["required"], json!(["query"]), "a default or an optional marker makes a field optional");
        assert!(
            s["properties"]["folder"].get(OPTIONAL_KEY).is_none(),
            "the marker is not part of the schema"
        );
        assert_eq!(s["additionalProperties"], false);
    }

    #[test]
    fn compile_rejects_raw_values() {
        assert!(compile_fields(&fields(&json!({"x": 5}))).unwrap_err().contains("app.t"));
        assert!(compile_fields(&fields(&json!({"x": {"description": "no type"}})))
            .unwrap_err()
            .contains("no type"));
    }

    #[test]
    fn defaults_are_filled_and_extra_nulls_dropped() {
        let ok = validate_args(&search_schema(), &json!({"query": "milk", "folder": null})).unwrap();
        assert_eq!(ok, json!({"query": "milk", "limit": 10}));
        assert_eq!(validate_args(&search_schema(), &Value::Null).unwrap_err(), "input.query is required");
    }

    #[test]
    fn types_ranges_and_unknown_fields_are_checked_before_any_app_code_runs() {
        let s = search_schema();
        assert!(validate_args(&s, &json!({"query": 5})).unwrap_err().contains("must be a string"));
        assert!(validate_args(&s, &json!({"query": "x", "limit": 0}))
            .unwrap_err()
            .contains("between 1 and 50"));
        assert!(validate_args(&s, &json!({"query": "x", "limit": 1.5})).unwrap_err().contains("integer"));
        assert!(validate_args(&s, &json!({"query": "x", "limit": "3"}))
            .unwrap_err()
            .contains("must be integer"));
        assert!(validate_args(&s, &json!({"query": "x", "evil": 1}))
            .unwrap_err()
            .contains("unknown field `evil`"));
        assert!(validate_args(&s, &json!([1])).unwrap_err().contains("must be an object"));
    }

    #[test]
    fn enums_arrays_booleans_and_nested_objects() {
        let schema = compile_fields(&fields(&json!({
            "mode": {"enum": ["a", "b"]},
            "tags": {"type": "array", "items": {"type": "string", "maxLength": 3}},
            "on": {"type": "boolean", "default": false},
            "where": {"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]},
        })))
        .unwrap();
        let ok = validate_args(&schema, &json!({"mode": "a", "tags": ["x"], "where": {"id": 3}})).unwrap();
        assert_eq!(ok["on"], false);
        assert!(validate_args(&schema, &json!({"mode": "c", "tags": [], "where": {"id": 1}}))
            .unwrap_err()
            .contains("one of"));
        assert!(validate_args(&schema, &json!({"mode": "a", "tags": ["long"], "where": {"id": 1}}))
            .unwrap_err()
            .contains("input.tags[0]"));
        assert!(validate_args(&schema, &json!({"mode": "a", "tags": [], "where": {}}))
            .unwrap_err()
            .contains("input.where.id"));
        assert!(validate_args(&schema, &json!({"mode": "a", "tags": [], "on": "yes", "where": {"id": 1}}))
            .unwrap_err()
            .contains("boolean"));
    }
}

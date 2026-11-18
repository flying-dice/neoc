//! JSON Schema (draft 2020-12 as emitted by schemars 1.x) -> Luau `export type` declarations. Types and fields are
//! emitted in alphabetical order so the output is the same whichever crates enable `serde_json`'s `preserve_order`.

use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write as _};

pub fn schema_to_luau(root_name: &str, schema: &Value, seen: &mut BTreeSet<String>) -> String {
    let mut out = String::new();
    if let Some(defs) = schema.get("$defs").and_then(Value::as_object) {
        // sorted: the output must not depend on whether serde_json preserves insertion order in this build
        let sorted: std::collections::BTreeMap<_, _> = defs.iter().collect();
        for (n, d) in sorted {
            if seen.insert(n.clone()) {
                let _ = write!(out, "export type {n} = {}\n\n", ty(d, root_name));
            }
        }
    }
    if seen.insert(root_name.to_owned()) {
        let _ = write!(out, "export type {root_name} = {}\n\n", ty(schema, root_name));
    }
    out
}

fn ty(s: &Value, root: &str) -> String {
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        // `#` = recursive reference to the schema root
        return if r == "#" { root.to_owned() } else { r.rsplit('/').next().unwrap_or("any").to_owned() };
    }
    if let Some(c) = s.get("const") {
        return literal(c);
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array) {
        return e.iter().map(literal).collect::<Vec<_>>().join(" | ");
    }
    for k in ["oneOf", "anyOf"] {
        if let Some(v) = s.get(k).and_then(Value::as_array) {
            return v.iter().map(|x| ty(x, root)).collect::<Vec<_>>().join(" | ");
        }
    }
    match s.get("type") {
        Some(Value::Array(ts)) => {
            ts.iter().filter_map(Value::as_str).map(|t| prim(t, s, root)).collect::<Vec<_>>().join(" | ")
        }
        Some(Value::String(t)) => prim(t, s, root),
        _ => "any".to_owned(),
    }
}

fn literal(v: &Value) -> String {
    match v {
        Value::String(s) => format!("{s:?}"),
        other => other.to_string(),
    }
}

fn prim(t: &str, s: &Value, root: &str) -> String {
    match t {
        "string" => "string".into(),
        "integer" | "number" => "number".into(),
        "boolean" => "boolean".into(),
        "null" => "nil".into(),
        "array" => format!("{{ {} }}", s.get("items").map_or("any".to_owned(), |i| ty(i, root))),
        "object" => object(s, root),
        _ => "any".into(),
    }
}

fn object(s: &Value, root: &str) -> String {
    let required: Vec<&str> = s
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(props) = s.get("properties").and_then(Value::as_object) {
        let sorted: std::collections::BTreeMap<_, _> = props.iter().collect();
        let fields: Vec<String> = sorted
            .into_iter()
            .map(|(k, v)| {
                let t = ty(v, root);
                if required.contains(&k.as_str()) && !t.contains("nil") {
                    format!("{k}: {t}")
                } else {
                    format!("{k}: ({t})?")
                }
            })
            .collect();
        return format!("{{ {} }}", fields.join(", "));
    }
    match s.get("additionalProperties") {
        Some(Value::Object(_)) => format!("{{ [string]: {} }}", ty(&s["additionalProperties"], root)),
        _ => "{ [string]: any }".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn emit(name: &str, schema: &Value) -> String {
        schema_to_luau(name, schema, &mut BTreeSet::new())
    }

    #[test]
    fn objects_mark_optional_fields_and_recursion() {
        let schema = json!({
            "type": "object",
            "properties": { "key": { "type": "string" }, "children": { "type": "array", "items": { "$ref": "#" } } },
            "required": ["key"]
        });
        assert_eq!(emit("Node", &schema), "export type Node = { children: ({ Node })?, key: string }\n\n");
    }

    #[test]
    fn unions_enums_and_refs() {
        let schema = json!({
            "$defs": { "Persist": { "oneOf": [{ "const": "none" }, { "const": "app" }] } },
            "type": "object",
            "properties": { "persist": { "$ref": "#/$defs/Persist" }, "n": { "type": ["integer", "null"] } }
        });
        let out = emit("Decl", &schema);
        assert!(out.contains("export type Persist = \"none\" | \"app\""), "{out}");
        assert!(out.contains("persist: (Persist)?"), "{out}");
        assert!(out.contains("n: (number | nil)?"), "{out}");
    }

    #[test]
    fn open_objects_are_string_maps() {
        let out = emit("Bag", &json!({ "type": "object", "additionalProperties": { "type": "number" } }));
        assert_eq!(out, "export type Bag = { [string]: number }\n\n");
    }

    #[test]
    fn a_type_is_emitted_once() {
        let mut seen = BTreeSet::new();
        let schema = json!({ "type": "string" });
        assert!(!schema_to_luau("Name", &schema, &mut seen).is_empty());
        assert!(schema_to_luau("Name", &schema, &mut seen).is_empty());
    }
}

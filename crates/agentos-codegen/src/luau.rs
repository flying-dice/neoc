//! JSON Schema (draft 2020-12 as emitted by schemars 1.x) -> Luau `export type` declarations.

use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write as _};

pub fn schema_to_luau(root_name: &str, schema: &Value, seen: &mut BTreeSet<String>) -> String {
    let mut out = String::new();
    if let Some(defs) = schema.get("$defs").and_then(Value::as_object) {
        for (n, d) in defs {
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
        let fields: Vec<String> = props
            .iter()
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

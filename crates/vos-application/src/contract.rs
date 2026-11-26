//! An app's contract as the RPC bus holds it: the app's `openrpc.json` (`OpenRPC` 1.4), shipped in its app ref. The bus
//! checks every request's params and every answer against it, and every notification's params, whoever sends them (a
//! page, a backend, an agent), and shows an agent each public method as described here. So an app needs no generated
//! code to be held to its contract: what the build generates is types.
//!
//! A method with a `result` is a request; one without is a notification. Params and results are JSON Schema; a
//! `$ref` to `#/components/schemas/<Name>` is read as `#/$defs/<Name>`.

use jsonschema::Validator;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// One of the app's requests.
pub struct RequestContract {
    pub summary: Option<String>,
    pub description: Option<String>,
    /// `x-readOnly`: it changes nothing, so an agent's call runs with writes refused.
    pub read_only: bool,
    /// The JSON Schema of its params (an object, one property per param): what an agent is shown.
    pub input_schema: Value,
    params: Validator,
    result: Validator,
}

/// What the bus holds of an app's contract.
pub struct AppContract {
    requests: HashMap<String, RequestContract>,
    notifications: HashMap<String, Validator>,
}

/// `#/components/schemas/<Name>` is `#/$defs/<Name>` in a schema that stands alone.
fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get_mut("$ref") {
                if let Some(name) = r.strip_prefix("#/components/schemas/") {
                    *r = format!("#/$defs/{name}");
                }
            }
            map.values_mut().for_each(rewrite_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

/// Whether a value of this schema may be null: a Lua nil is an absent key, so such a param is never required.
fn allows_null(schema: &Value) -> bool {
    match schema.get("type") {
        Some(Value::String(t)) => t == "null",
        Some(Value::Array(types)) => types.iter().any(|t| t == "null"),
        _ => false,
    }
}

fn compile(schema: Value, defs: &Map<String, Value>) -> Result<(Value, Validator), String> {
    let mut schema = schema;
    if let (Value::Object(map), false) = (&mut schema, defs.is_empty()) {
        map.insert("$defs".to_owned(), Value::Object(defs.clone()));
    }
    let validator = jsonschema::validator_for(&schema).map_err(|e| e.to_string())?;
    Ok((schema, validator))
}

/// The first problem a validator finds, as a sentence: where, and what.
fn problem(validator: &Validator, value: &Value) -> Option<String> {
    validator.iter_errors(value).next().map(|e| {
        let at = e.instance_path().to_string();
        if at.is_empty() {
            e.to_string()
        } else {
            format!("{at}: {e}")
        }
    })
}

impl AppContract {
    /// Read `openrpc.json`. An error says which method or schema is wrong.
    pub fn parse(source: &[u8]) -> Result<Self, String> {
        let document: Value =
            serde_json::from_slice(source).map_err(|e| format!("the contract is not JSON: {e}"))?;
        let mut defs =
            document.pointer("/components/schemas").and_then(Value::as_object).cloned().unwrap_or_default();
        defs.values_mut().for_each(rewrite_refs);
        let methods =
            document.get("methods").and_then(Value::as_array).ok_or("the contract has no `methods` list")?;
        let mut requests = HashMap::new();
        let mut notifications = HashMap::new();
        for method in methods {
            let name =
                method.get("name").and_then(Value::as_str).ok_or("a method of the contract has no name")?;
            let mut properties = Map::new();
            let mut required = Vec::new();
            for param in method.get("params").and_then(Value::as_array).into_iter().flatten() {
                let Some(param_name) = param.get("name").and_then(Value::as_str) else { continue };
                let mut schema = param.get("schema").cloned().unwrap_or_else(|| json!({}));
                rewrite_refs(&mut schema);
                if let (Some(description), Value::Object(map)) = (param.get("description"), &mut schema) {
                    map.entry("description").or_insert_with(|| description.clone());
                }
                if param.get("required").and_then(Value::as_bool) == Some(true) && !allows_null(&schema) {
                    required.push(Value::String(param_name.to_owned()));
                }
                properties.insert(param_name.to_owned(), schema);
            }
            let params = json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false });
            let (input_schema, params) =
                compile(params, &defs).map_err(|e| format!("the params of `{name}`: {e}"))?;
            match method.get("result") {
                None => {
                    notifications.insert(name.to_owned(), params);
                }
                Some(result) => {
                    let mut schema = result.get("schema").cloned().unwrap_or_else(|| json!({}));
                    rewrite_refs(&mut schema);
                    let (_, result) =
                        compile(schema, &defs).map_err(|e| format!("the result of `{name}`: {e}"))?;
                    let text = |key: &str| method.get(key).and_then(Value::as_str).map(str::to_owned);
                    requests.insert(
                        name.to_owned(),
                        RequestContract {
                            summary: text("summary"),
                            description: text("description"),
                            read_only: method.get("x-readOnly").and_then(Value::as_bool) == Some(true),
                            input_schema,
                            params,
                            result,
                        },
                    );
                }
            }
        }
        Ok(Self { requests, notifications })
    }

    pub fn request(&self, method: &str) -> Option<&RequestContract> {
        self.requests.get(method)
    }

    /// The request's params, as the contract says them. `Err` names the first problem.
    pub fn check_params(&self, method: &str, params: &Value) -> Result<(), String> {
        let request = self
            .requests
            .get(method)
            .ok_or_else(|| format!("`{method}` is not a request of the contract"))?;
        problem(&request.params, &normalized(params)).map_or(Ok(()), Err)
    }

    /// A handler's answer, as the contract says it. An empty table is an empty list and an empty object alike (Lua
    /// cannot tell them apart), so either reading passes.
    pub fn check_result(&self, method: &str, result: &Value) -> Result<(), String> {
        let Some(request) = self.requests.get(method) else { return Ok(()) };
        let Some(first) = problem(&request.result, result) else { return Ok(()) };
        let other = match result {
            Value::Array(items) if items.is_empty() => Some(json!({})),
            Value::Object(map) if map.is_empty() => Some(json!([])),
            _ => None,
        };
        match other {
            Some(other) if problem(&request.result, &other).is_none() => Ok(()),
            _ => Err(first),
        }
    }

    /// A notification's params, as the contract says them.
    pub fn check_notification(&self, name: &str, params: &Value) -> Result<(), String> {
        let validator = self
            .notifications
            .get(name)
            .ok_or_else(|| format!("`{name}` is not a notification of the contract"))?;
        problem(validator, &normalized(params)).map_or(Ok(()), Err)
    }
}

/// Params as an object: none (null), or an empty table read as a list, are no params.
fn normalized(params: &Value) -> Value {
    match params {
        Value::Null => json!({}),
        Value::Array(items) if items.is_empty() => json!({}),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTRACT: &str = r##"{
      "openrpc": "1.4.0", "info": { "title": "T", "version": "1.0.0" },
      "methods": [
        { "name": "add", "summary": "Add", "x-readOnly": false,
          "params": [{ "name": "text", "required": true, "schema": { "type": "string", "minLength": 1 } },
                     { "name": "tag", "required": true, "schema": { "type": ["string", "null"] } }],
          "result": { "name": "item", "schema": { "$ref": "#/components/schemas/Item" } } },
        { "name": "_items", "params": [], "result": { "name": "items", "schema": { "type": "array", "items": { "$ref": "#/components/schemas/Item" } } } },
        { "name": "added", "params": [{ "name": "item", "required": true, "schema": { "$ref": "#/components/schemas/Item" } }] }
      ],
      "components": { "schemas": { "Item": { "type": "object", "properties": { "id": { "type": "integer" } }, "required": ["id"] } } }
    }"##;

    #[test]
    fn params_results_and_notifications_are_held_to_the_contract() {
        let c = AppContract::parse(CONTRACT.as_bytes()).unwrap();
        assert!(c.check_params("add", &json!({ "text": "milk" })).is_ok(), "a nullable param may be absent");
        assert!(c.check_params("add", &json!({})).is_err());
        assert!(c.check_params("add", &json!({ "text": "milk", "extra": 1 })).is_err());
        assert!(c.check_params("nope", &json!({})).is_err());
        assert!(c.check_params("_items", &json!([])).is_ok(), "an empty table is no params");
        assert!(c.check_result("add", &json!({ "id": 1 })).is_ok());
        assert!(c.check_result("add", &json!({ "id": "x" })).is_err());
        assert!(c.check_result("_items", &json!({})).is_ok(), "an empty table is an empty list");
        assert!(c.check_notification("added", &json!({ "item": { "id": 1 } })).is_ok());
        assert!(c.check_notification("added", &json!({ "item": {} })).is_err());
        assert!(c.check_notification("add", &json!({})).is_err(), "a request is no notification");
    }

    /// The contract of the task-list fixtures (`e2e/apps/tasks-svelte`): what a page or an agent may send them.
    const TASKS: &str = r#"{
      "openrpc": "1.4.0", "info": { "title": "Tasks", "version": "0.1.0" },
      "methods": [
        { "name": "_list", "params": [], "result": { "name": "tasks", "schema": { "type": "array", "items": { "type": "object" } } } },
        { "name": "_add", "params": [{ "name": "title", "required": true, "schema": { "type": "string", "maxLength": 200 } }],
          "result": { "name": "tasks", "schema": { "type": "array", "items": { "type": "object" } } } },
        { "name": "_toggle", "params": [{ "name": "id", "required": true, "schema": { "type": "integer", "minimum": 1 } }],
          "result": { "name": "tasks", "schema": { "type": "array", "items": { "type": "object" } } } }
      ]
    }"#;

    #[test]
    fn params_the_contract_does_not_allow_are_refused_whatever_is_wrong_with_them() {
        let c = AppContract::parse(TASKS.as_bytes()).unwrap();
        let long = "x".repeat(201);
        for (method, params, said) in [
            ("_add", json!({}), "\"title\" is a required property"),
            ("_add", Value::Null, "\"title\" is a required property"),
            ("_add", json!({ "title": 5 }), "/title: 5 is not of type \"string\""),
            ("_add", json!({ "title": long }), "is longer than 200 characters"),
            (
                "_add",
                json!({ "title": "ok", "extra": true }),
                "Additional properties are not allowed ('extra' was unexpected)",
            ),
            ("_add", json!(["ok"]), "[\"ok\"] is not of type \"object\""),
            ("_toggle", json!({ "id": 0 }), "/id: 0 is less than the minimum of 1"),
            ("_toggle", json!({ "id": 1.5 }), "/id: 1.5 is not of type \"integer\""),
            ("_toggle", json!({ "id": "1" }), "/id: \"1\" is not of type \"integer\""),
            (
                "_list",
                json!({ "unexpected": 1 }),
                "Additional properties are not allowed ('unexpected' was unexpected)",
            ),
        ] {
            let problem = c.check_params(method, &params).unwrap_err();
            assert!(problem.contains(said), "{method} {params}: {problem}");
        }
        assert!(
            c.check_params("_add", &json!({ "title": "  Buy milk " })).is_ok(),
            "blanks are the handler's business"
        );
        assert!(c.check_params("_add", &json!({ "title": "x".repeat(200) })).is_ok());
        assert!(c.check_params("_toggle", &json!({ "id": 1 })).is_ok());
        assert!(c.check_params("_list", &Value::Null).is_ok());
    }

    #[test]
    fn a_request_is_described_as_an_agent_is_shown_it() {
        let c = AppContract::parse(CONTRACT.as_bytes()).unwrap();
        let add = c.request("add").unwrap();
        assert_eq!(add.summary.as_deref(), Some("Add"));
        assert_eq!(add.input_schema["required"], json!(["text"]));
        assert_eq!(add.input_schema["$defs"]["Item"]["type"], "object");
    }
}

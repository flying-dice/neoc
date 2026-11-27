//! JSON values as the modules write them.

use serde_json::Value;

/// `value` with the keys of every object in sorted order: how a module that writes plain data (`vnd:serde_json`,
/// `vnd:serde_yaml`, `vnd:toml`, `vnd:quick_xml`) writes a table's keys, whatever order the script's table gives them
/// in. A `serde_json` map keeps its keys in the order they were added when the crate's `preserve_order` feature is on
/// (the ACP SDK under `vnd:agent_client_protocol` turns it on for the whole build), so the order is made here rather
/// than left to the map.
#[must_use]
pub fn with_sorted_keys(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut fields: Vec<(String, Value)> = fields.into_iter().collect();
            fields.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(fields.into_iter().map(|(key, value)| (key, with_sorted_keys(value))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(with_sorted_keys).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::with_sorted_keys;

    #[test]
    fn every_objects_keys_are_sorted_however_they_were_added() {
        let mut value = serde_json::Map::new();
        value.insert("b".to_owned(), json!([{ "z": 1, "a": 2 }]));
        value.insert("a".to_owned(), json!({ "y": null, "x": "keep" }));
        let written = with_sorted_keys(serde_json::Value::Object(value)).to_string();
        assert_eq!(written, r#"{"a":{"x":"keep","y":null},"b":[{"a":2,"z":1}]}"#);
    }
}

//! RFC 6902 JSON Patch, which `STATE_DELTA` and `ACTIVITY_DELTA` carry.

use serde_json::Value;

/// The most operations one patch may hold.
pub const MAX_OPERATIONS: usize = 1_000;
/// The most values (every object, array, string, number, `true`, `false` and `null`) the patched document may hold,
/// counted generously: what an operation adds is counted, what it removes is not taken off.
pub const MAX_VALUES: usize = 200_000;

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// The patch is not a list of RFC 6902 operations.
    #[error("not a JSON Patch: {0}")]
    Invalid(#[from] serde_json::Error),
    /// An operation could not be applied (a path that is not there, a failed `test`).
    #[error("{0}")]
    Failed(#[from] json_patch::PatchError),
    /// The patch has more than [`MAX_OPERATIONS`] operations, or would grow the document past [`MAX_VALUES`]
    /// values. A `copy` doubles what it copies, so a short patch can ask for a document of any size.
    #[error("the patch is too large: {0}")]
    TooLarge(&'static str),
}

/// How many values `value` holds, itself included, counted up to `limit` (a count past it is `limit + 1`).
fn count(value: &Value, limit: usize) -> usize {
    let mut seen = 0;
    let mut todo = vec![value];
    while let Some(value) = todo.pop() {
        seen += 1;
        if seen > limit {
            break;
        }
        match value {
            Value::Array(items) => todo.extend(items),
            Value::Object(fields) => todo.extend(fields.values()),
            _ => {}
        }
    }
    seen
}

/// `document` with `patch` applied. All of the patch applies or none of it: on an error the document is unchanged.
/// The patch comes from an agent, so what it may ask for is bounded ([`MAX_OPERATIONS`], [`MAX_VALUES`]).
///
/// # Errors
/// See [`PatchError`].
pub fn apply(document: &mut Value, patch: &[Value]) -> Result<(), PatchError> {
    use json_patch::PatchOperation as Op;

    if patch.len() > MAX_OPERATIONS {
        return Err(PatchError::TooLarge("more than 1000 operations"));
    }
    let patch: json_patch::Patch = serde_json::from_value(Value::Array(patch.to_vec()))?;
    let mut next = document.clone();
    let mut values = count(&next, MAX_VALUES);
    for operation in &patch.0 {
        let adds = match operation {
            Op::Add(op) => count(&op.value, MAX_VALUES),
            Op::Replace(op) => count(&op.value, MAX_VALUES),
            Op::Copy(op) => next.pointer(op.from.as_str()).map_or(0, |from| count(from, MAX_VALUES)),
            Op::Move(_) | Op::Remove(_) | Op::Test(_) => 0,
        };
        values = values.saturating_add(adds);
        if values > MAX_VALUES {
            return Err(PatchError::TooLarge("the document would hold more than 200000 values"));
        }
        json_patch::patch(&mut next, std::slice::from_ref(operation))?;
    }
    *document = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_patch_applies_whole_or_not_at_all() {
        let mut doc = json!({ "steps": ["a"], "n": 1 });
        apply(
            &mut doc,
            &[
                json!({ "op": "add", "path": "/steps/-", "value": "b" }),
                json!({ "op": "replace", "path": "/n", "value": 2 }),
            ],
        )
        .unwrap();
        assert_eq!(doc, json!({ "steps": ["a", "b"], "n": 2 }));

        let before = doc.clone();
        let failed = apply(
            &mut doc,
            &[
                json!({ "op": "replace", "path": "/n", "value": 3 }),
                json!({ "op": "remove", "path": "/missing" }),
            ],
        );
        assert!(matches!(failed, Err(PatchError::Failed(_))));
        assert_eq!(doc, before);
        assert!(matches!(apply(&mut doc, &[json!({ "op": "explode" })]), Err(PatchError::Invalid(_))));
    }

    #[test]
    fn a_patch_cannot_grow_the_document_without_end() {
        // each copy doubles `/a`: forty of them would be 2^40 values
        let mut doc = json!({ "a": { "seed": [1, 2, 3, 4, 5, 6, 7, 8] } });
        let before = doc.clone();
        let doubling: Vec<Value> =
            (0..40).map(|i| json!({ "op": "copy", "from": "/a", "path": format!("/a/k{i}") })).collect();
        assert!(matches!(apply(&mut doc, &doubling), Err(PatchError::TooLarge(_))));
        assert_eq!(doc, before);

        let many: Vec<Value> =
            (0..=MAX_OPERATIONS).map(|_| json!({ "op": "test", "path": "/a", "value": 1 })).collect();
        assert!(matches!(apply(&mut doc, &many), Err(PatchError::TooLarge(_))));
    }
}

//! RFC 6902 JSON Patch, which `STATE_DELTA` and `ACTIVITY_DELTA` carry.

use serde_json::Value;

/// The most operations one patch may hold.
pub const MAX_OPERATIONS: usize = 1_000;
/// The largest the patched document may be, as [`size`] measures it (close to its bytes as JSON).
pub const MAX_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// The patch is not a list of RFC 6902 operations.
    #[error("not a JSON Patch: {0}")]
    Invalid(#[from] serde_json::Error),
    /// An operation could not be applied (a path that is not there, a failed `test`).
    #[error("{0}")]
    Failed(#[from] json_patch::PatchError),
    /// The patch has more than [`MAX_OPERATIONS`] operations, or would grow the document past [`MAX_SIZE`]. A `copy` doubles what it copies, so a short patch can ask for a document of any size.
    #[error("the patch is too large: {0}")]
    TooLarge(&'static str),
}

/// About how many bytes `value` takes: the text of every string and key, and a few bytes for every value. Counted up
/// to `limit` (a size past it is some number over `limit`).
fn size(value: &Value, limit: usize) -> usize {
    let mut seen = 0usize;
    let mut todo = vec![value];
    while let Some(value) = todo.pop() {
        seen = seen.saturating_add(8);
        match value {
            Value::String(text) => seen = seen.saturating_add(text.len()),
            Value::Array(items) => todo.extend(items),
            Value::Object(fields) => {
                for (key, field) in fields {
                    seen = seen.saturating_add(key.len());
                    todo.push(field);
                }
            }
            _ => {}
        }
        if seen > limit {
            break;
        }
    }
    seen
}

/// `document` with `patch` applied. All of the patch applies or none of it: on an error the document is unchanged.
/// The patch comes from an agent, so what it may ask for is bounded ([`MAX_OPERATIONS`], [`MAX_SIZE`]). What an
/// operation adds is counted and what it removes is not taken off; an operation that adds nothing (`remove`, `move`,
/// `test`) is never refused for size, so a document that is already large can still be made smaller.
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
    let mut total = size(&next, MAX_SIZE);
    for operation in &patch.0 {
        let adds = match operation {
            Op::Add(op) => size(&op.value, MAX_SIZE),
            Op::Replace(op) => size(&op.value, MAX_SIZE),
            Op::Copy(op) => next.pointer(op.from.as_str()).map_or(0, |from| size(from, MAX_SIZE)),
            Op::Move(_) | Op::Remove(_) | Op::Test(_) => 0,
        };
        total = total.saturating_add(adds);
        if adds > 0 && total > MAX_SIZE {
            return Err(PatchError::TooLarge("the document would be larger than 8 MB"));
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

    #[test]
    fn a_large_text_cannot_be_copied_many_times_and_a_large_document_can_still_shrink() {
        // one value, a megabyte of it: a count of values would let a thousand copies through
        let mut doc = json!({ "big": "x".repeat(1024 * 1024) });
        let before = doc.clone();
        let copies: Vec<Value> =
            (0..100).map(|i| json!({ "op": "copy", "from": "/big", "path": format!("/k{i}") })).collect();
        assert!(matches!(apply(&mut doc, &copies), Err(PatchError::TooLarge(_))));
        assert_eq!(doc, before);

        let mut large = json!({ "a": "x".repeat(MAX_SIZE + 1), "b": 1 });
        apply(&mut large, &[json!({ "op": "remove", "path": "/a" })]).unwrap();
        assert_eq!(large, json!({ "b": 1 }));
    }
}

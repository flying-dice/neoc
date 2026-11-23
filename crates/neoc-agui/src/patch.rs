//! RFC 6902 JSON Patch, which `STATE_DELTA` and `ACTIVITY_DELTA` carry.

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    /// The patch is not a list of RFC 6902 operations.
    #[error("not a JSON Patch: {0}")]
    Invalid(#[from] serde_json::Error),
    /// An operation could not be applied (a path that is not there, a failed `test`).
    #[error("{0}")]
    Failed(#[from] json_patch::PatchError),
}

/// `document` with `patch` applied. All of the patch applies or none of it: on an error the document is unchanged.
///
/// # Errors
/// See [`PatchError`].
pub fn apply(document: &mut Value, patch: &[Value]) -> Result<(), PatchError> {
    let patch: json_patch::Patch = serde_json::from_value(Value::Array(patch.to_vec()))?;
    json_patch::patch(document, &patch)?;
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
}

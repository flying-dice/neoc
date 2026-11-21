//! Revisions: the losing side of a conflict is kept, not dropped (ARCHITECTURE §6 decision 12:
//! kept 30 days; "undo agent change" is a later shell feature).

use crate::Actor;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long a revision is kept.
pub const REVISION_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionReason {
    /// Two edits of the same text overlapped; the human's won (text merge).
    TextConflict,
    /// The human's write replaced a newer value from elsewhere (scalars: human wins; text with no merge base).
    Overwritten,
    /// The value an undo (restore) replaced, so an undo can be undone.
    Restored,
}

impl RevisionReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextConflict => "text_conflict",
            Self::Overwritten => "overwritten",
            Self::Restored => "restored",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text_conflict" => Some(Self::TextConflict),
            "overwritten" => Some(Self::Overwritten),
            "restored" => Some(Self::Restored),
            _ => None,
        }
    }
}

/// A kept value: what a bound field held before it was replaced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    /// The row source the field was bound through.
    pub source: String,
    pub key: Value,
    pub field: String,
    /// The value that lost (a string for text, any JSON for scalars).
    pub value: Value,
    pub reason: RevisionReason,
    /// Who replaced it (the human at the keyboard).
    pub replaced_by: Actor,
    pub recorded_at_ms: u64,
}

/// A revision as the store kept it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredRevision {
    pub id: i64,
    pub revision: Revision,
}

impl Revision {
    /// The cut-off before which a revision recorded at `now_ms` would already have expired.
    #[must_use]
    pub fn expiry_cutoff(now_ms: u64) -> u64 {
        now_ms.saturating_sub(REVISION_RETENTION_MS)
    }

    /// Is this revision past its retention at `now_ms`?
    #[must_use]
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.recorded_at_ms < Self::expiry_cutoff(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn revision(at: u64) -> Revision {
        Revision {
            source: "note".into(),
            key: json!(7),
            field: "body".into(),
            value: json!("milk"),
            reason: RevisionReason::TextConflict,
            replaced_by: Actor::user("You"),
            recorded_at_ms: at,
        }
    }

    #[test]
    fn revisions_expire_after_thirty_days() {
        let now = 100 * 24 * 60 * 60 * 1000;
        assert!(!revision(now).is_expired(now));
        assert!(!revision(now - REVISION_RETENTION_MS).is_expired(now), "exactly 30 days old is kept");
        assert!(revision(now - REVISION_RETENTION_MS - 1).is_expired(now));
        assert_eq!(Revision::expiry_cutoff(5), 0, "no underflow near the epoch");
    }

    #[test]
    fn reasons_round_trip() {
        for r in [RevisionReason::TextConflict, RevisionReason::Overwritten, RevisionReason::Restored] {
            assert_eq!(RevisionReason::parse(r.as_str()), Some(r));
        }
        assert_eq!(RevisionReason::parse("nope"), None);
    }
}

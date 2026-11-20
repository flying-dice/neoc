//! Two-way writes (ui-system.md §3.4): a bound field edited by the human goes back through the row
//! source's `write` callback, after a `baseRev` check.
//!
//! The host decides by the binding's kind:
//! - `baseRev` is current: **accepted**, written as is.
//! - **text**, stale: a three-way merge of `base` (what the host last sent the view), `theirs` (the row now)
//!   and `mine`. Edits that do not overlap compose (**merged**); overlapping edits go to the human
//!   (**replaced**) and the agent's text is kept as a revision. Without a `base` (the host's ring of sent
//!   values lost it, e.g. after a restart) the human's text replaces theirs and theirs is kept.
//! - **scalar**, stale: the human wins (**replaced**); the overwritten value is kept as a revision.
//! - a source declared `conflict = "reject"`: a stale write is **rejected** and the client adopts the
//!   current value.
//!
//! Nothing is silently lost: every replacement carries a revision id the client can use to undo.
//! `SourceKind` stays open to a later document source kind (`db:doc`).

use crate::{
    data::{AppData, Params},
    error::PortError,
    live::{Conflict, LiveSources, SourceId, SourceKind},
    ports::{AppRuntime, CallContext},
    AppError,
};
use agentos_domain::{merge3, Actor, AppId, Revision, RevisionReason};
use serde_json::Value;

/// How the host reconciles a stale write of this field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// `base`: the text the host last sent the view at `base_rev` (`None` if it no longer has it).
    Text {
        base: Option<String>,
    },
    Scalar,
}

/// One two-way write from a client (`row.set`).
#[derive(Clone, Debug, PartialEq)]
pub struct WriteRequest {
    pub source: SourceId,
    /// The params the view's subscription uses (they select the row).
    pub params: Params,
    pub key: Value,
    pub field: String,
    pub value: Value,
    /// The revision the client's value was based on. Ignored when the source declares no revision column.
    pub base_rev: Option<Value>,
    pub binding: BindingKind,
    pub actor: Actor,
}

/// What the host did with a write; the client needs it to tell a merge from a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Based on the current revision: written as sent.
    Accepted,
    /// Based on an older revision; the edits composed and nothing was lost.
    Merged,
    /// The human's value replaced a newer one; the replaced value is kept (see `revision_id`).
    Replaced,
    /// A strict source refused a stale write; the client adopts `value`.
    Rejected,
}

impl Resolution {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Merged => "merged",
            Self::Replaced => "replaced",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WriteAck {
    pub resolution: Resolution,
    /// The row's revision now.
    pub rev: Option<Value>,
    /// The field's authoritative value now (the client adopts it).
    pub value: Value,
    /// The kept revision of the value this write replaced, for an Undo (`restore_revision`).
    pub revision_id: Option<i64>,
    /// The replaced value itself (the client can offer Undo without a round trip).
    pub lost: Option<Value>,
}

async fn read_row(data: &dyn AppData, sql: &str, params: &Params) -> Result<Value, AppError> {
    let mut rows = data.query(sql.to_owned(), params.clone()).await?;
    rows.truncate(1);
    rows.pop().ok_or_else(|| AppError::Port(PortError::NotFound("the row no longer exists".into())))
}

/// Everything a write-back needs from the service.
pub(crate) struct Env<'a> {
    pub runtime: &'a dyn AppRuntime,
    pub data: &'a dyn AppData,
    pub live: &'a LiveSources,
    pub app: &'a AppId,
    pub now_ms: u64,
}

fn text_of(v: &Value) -> Option<&str> {
    v.as_str()
}

/// Perform a write-back.
pub(crate) async fn write_field(env: &Env<'_>, req: WriteRequest) -> Result<WriteAck, AppError> {
    write_inner(env, req, false).await
}

async fn write_inner(env: &Env<'_>, req: WriteRequest, restoring: bool) -> Result<WriteAck, AppError> {
    let reject = |m: String| AppError::Port(PortError::Rejected(m));
    let def = env
        .live
        .def(&req.source)
        .ok_or_else(|| AppError::Port(PortError::NotFound(format!("source `{}`", req.source))))?;
    let SourceKind::Row { key, rev, writable, has_write, conflict } = &def.kind else {
        return Err(reject(format!("source `{}` is not a single-row source", req.source)));
    };
    if !has_write || writable.as_ref().is_some_and(|w| !w.contains(&req.field)) {
        return Err(reject(format!("field `{}` of source `{}` is not writable", req.field, req.source)));
    }

    let current = read_row(env.data, &def.sql, &req.params).await?;
    if current.get(key) != Some(&req.key) {
        return Err(reject("the key does not match the row the source selects".into()));
    }
    let current_rev = rev.as_ref().and_then(|c| current.get(c)).cloned();
    let theirs = current.get(&req.field).cloned().unwrap_or(Value::Null);
    // without a revision column there is nothing to be stale against
    let stale = rev.is_some() && req.base_rev.is_some() && current_rev != req.base_rev;

    if stale && *conflict == Conflict::Reject {
        return Ok(WriteAck {
            resolution: Resolution::Rejected,
            rev: current_rev,
            value: theirs,
            revision_id: None,
            lost: None,
        });
    }

    // decide what to write and what (if anything) to keep
    let (to_write, resolution, keep) = if theirs == req.value {
        (req.value.clone(), Resolution::Accepted, None)
    } else if restoring {
        // an undo replaces whatever is there; keep it so the undo can be undone
        (req.value.clone(), Resolution::Replaced, Some((theirs.clone(), RevisionReason::Restored)))
    } else if !stale {
        (req.value.clone(), Resolution::Accepted, None)
    } else {
        match (&req.binding, text_of(&theirs), text_of(&req.value)) {
            (BindingKind::Text { base: Some(base) }, Some(t), Some(m)) => {
                let merged = merge3(base, t, m);
                if merged.conflict {
                    (
                        Value::String(merged.text),
                        Resolution::Replaced,
                        Some((theirs.clone(), RevisionReason::TextConflict)),
                    )
                } else {
                    (Value::String(merged.text), Resolution::Merged, None)
                }
            }
            // scalars, and text with no merge base or non-text values: the human wins, theirs is kept
            (BindingKind::Text { .. }, _, _) | (BindingKind::Scalar, ..) => {
                (req.value.clone(), Resolution::Replaced, Some((theirs.clone(), RevisionReason::Overwritten)))
            }
        }
    };

    let ctx = CallContext { actor: req.actor.clone(), now_ms: env.now_ms };
    env.runtime.write_row(env.app, &req.source, req.key.clone(), &req.field, to_write.clone(), ctx).await?;

    let mut revision_id = None;
    let mut lost = None;
    if let Some((value, reason)) = keep {
        let revision = Revision {
            source: req.source.0.clone(),
            key: req.key.clone(),
            field: req.field.clone(),
            value: value.clone(),
            reason,
            replaced_by: req.actor,
            recorded_at_ms: env.now_ms,
        };
        match env.data.record_revision(revision).await {
            Ok(id) => revision_id = Some(id),
            Err(e) => tracing::warn!(error = %e, "keeping the replaced value as a revision failed"),
        }
        lost = Some(value);
    }

    let after = read_row(env.data, &def.sql, &req.params).await?;
    Ok(WriteAck {
        resolution,
        rev: rev.as_ref().and_then(|c| after.get(c)).cloned(),
        value: after.get(&req.field).cloned().unwrap_or(to_write),
        revision_id,
        lost,
    })
}

/// Undo: put a kept value back (the human wins; what it replaces is kept in turn, so an undo can be undone).
pub(crate) async fn restore_revision(
    env: &Env<'_>,
    revision_id: i64,
    params: Params,
    actor: Actor,
) -> Result<WriteAck, AppError> {
    let stored = env
        .data
        .revision(revision_id)
        .await?
        .ok_or_else(|| AppError::Port(PortError::NotFound(format!("revision {revision_id}"))))?;
    let r = stored.revision;
    let binding = if r.value.is_string() { BindingKind::Text { base: None } } else { BindingKind::Scalar };
    let request = WriteRequest {
        source: SourceId(r.source.clone()),
        params,
        key: r.key.clone(),
        field: r.field.clone(),
        value: r.value,
        base_rev: None, // an undo is explicit: it never counts as stale
        binding,
        actor,
    };
    write_inner(env, request, true).await
}

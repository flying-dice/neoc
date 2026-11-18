//! Sources and client state (spec §3.3, §3.5, §3.7).
//!
//! A **source** is a host-owned, parameterised, live read of data. The tree references it by opaque id; the
//! SQL and the database never leave the host. A source's *params* are refs (usually to client state): when
//! `client.search` changes, the client notices that `s_notes` depends on it and asks the host for a new window.
//! List rows are a **row template** sent once (the `row` slot of a `ForEach` or `List`) plus **row data**.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Budgets of a view's sources (spec §3.7). The window size lives with the sources that serve it
/// (`agentos-application` `live::MAX_WINDOW`) and the row and byte caps of a query with the database adapter
/// (`agentos-sqlite` `MAX_QUERY_ROWS` / `MAX_QUERY_BYTES`).
pub mod budgets {
    pub const MAX_SOURCES_PER_VIEW: usize = 16;
}

/// How `rows` sources report their total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum TotalMode {
    #[default]
    Exact,
    Estimate,
    None,
}

/// What happens when a two-way write is based on a stale revision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// Merge text, human wins scalars (the default).
    #[default]
    Merge,
    /// Answer `{conflict: true, value: current}` and let the client adopt the server value.
    Reject,
}

/// A declared source: what the tree's `source` props point at by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceDecl {
    /// A window of rows: `List`, `Table`, `Picker options`, `CalendarGrid events`.
    #[serde(rename_all = "camelCase")]
    Rows {
        /// The row field that is its key.
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        page: Option<u32>,
        /// Milliseconds the client waits after a param changes before asking.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        debounce: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        total: Option<TotalMode>,
        /// Param name to ref or literal.
        #[serde(default)]
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        params: Map<String, Value>,
    },
    /// One row: a detail pane or a form.
    #[serde(rename_all = "camelCase")]
    Row {
        key: String,
        /// The fields a two-way binding may write.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[cfg_attr(feature = "codegen", ts(as = "Option<Vec<String>>", optional))]
        writable: Vec<String>,
        /// The revision column used for conflict detection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        rev: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        conflict: Option<ConflictPolicy>,
        #[serde(default)]
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        params: Map<String, Value>,
    },
    /// A scalar: a count, a badge.
    Value {
        #[serde(default)]
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        params: Map<String, Value>,
    },
    /// A large edited document (`CodeEditor`); synced with splice ops.
    Text {
        #[serde(default)]
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        params: Map<String, Value>,
    },
    /// A chat thread (`ChatTranscript`), maintained by the AG-UI bridge.
    Thread {
        #[serde(default)]
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        params: Map<String, Value>,
    },
}

impl SourceDecl {
    /// The declared params.
    #[must_use]
    pub const fn params(&self) -> &Map<String, Value> {
        match self {
            Self::Rows { params, .. }
            | Self::Row { params, .. }
            | Self::Value { params }
            | Self::Text { params }
            | Self::Thread { params } => params,
        }
    }

    /// The kind name on the wire.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Rows { .. } => "rows",
            Self::Row { .. } => "row",
            Self::Value { .. } => "value",
            Self::Text { .. } => "text",
            Self::Thread { .. } => "thread",
        }
    }
}

/// Where a client-managed value survives (spec §3.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum Persist {
    /// In memory while the view is open.
    #[default]
    None,
    /// Also in `sessionStorage`/`IndexedDB` keyed by window: survives reconnect, host restart and page reload.
    Session,
    /// Also stored on the host: survives closing the window and restarting the app.
    App,
}

/// Whether and when a client-managed value is mirrored to the host (so handlers can read it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    #[default]
    None,
    Change,
    Commit,
}

/// One entry of `client = { name = { init, persist, sync } }`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientDecl {
    /// The initial value (deep links may override it via `params`).
    #[serde(default)]
    #[cfg_attr(feature = "codegen", ts(type = "JsonValue"))]
    pub init: Value,
    #[serde(default, skip_serializing_if = "is_default")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<Persist>", optional))]
    pub persist: Persist,
    #[serde(default, skip_serializing_if = "is_default")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<SyncMode>", optional))]
    pub sync: SyncMode,
    /// A JSON-schema-like type, validated on restore and on `client.set`.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, as = "Option<Value>"))]
    pub type_: Option<Value>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl ClientDecl {
    /// Persistence implies at least `commit` sync: the host must hold the value to store it.
    #[must_use]
    pub fn effective_sync(&self) -> SyncMode {
        if self.persist != Persist::None && self.sync == SyncMode::None {
            SyncMode::Commit
        } else {
            self.sync
        }
    }
}

/// The declarations of one view: client state and sources, keyed by name and id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewDecls {
    pub client: BTreeMap<String, ClientDecl>,
    pub sources: BTreeMap<String, SourceDecl>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_decls_are_tagged_by_kind() {
        let notes = SourceDecl::Rows {
            key: "id".into(),
            page: Some(60),
            debounce: Some(250),
            total: None,
            params: json!({ "folder": { "$ref": "client.folder" } }).as_object().cloned().unwrap_or_default(),
        };
        let json = serde_json::to_value(&notes).unwrap();
        assert_eq!(json["kind"], "rows");
        assert_eq!(json["params"]["folder"]["$ref"], "client.folder");
        assert!(json.get("total").is_none());
        assert_eq!(serde_json::from_value::<SourceDecl>(json).unwrap(), notes);
        assert_eq!(notes.kind(), "rows");
    }

    #[test]
    fn the_spec_snapshot_source_parses() {
        let json = json!({ "kind": "row", "key": "id", "writable": ["body"], "params": { "id": { "$ref": "client.noteId" } } });
        let SourceDecl::Row { writable, .. } = serde_json::from_value(json).unwrap() else { unreachable!() };
        assert_eq!(writable, ["body"]);
    }

    #[test]
    fn client_decls_default_to_ephemeral() {
        let decl: ClientDecl = serde_json::from_value(json!({ "init": 0 })).unwrap();
        assert_eq!(decl.persist, Persist::None);
        assert_eq!(decl.effective_sync(), SyncMode::None);
        let json = serde_json::to_value(&decl).unwrap();
        assert_eq!(json, json!({ "init": 0 }), "defaults are omitted");
    }

    #[test]
    fn persistence_implies_commit_sync() {
        let decl: ClientDecl = serde_json::from_value(json!({ "init": "edit", "persist": "app" })).unwrap();
        assert_eq!(decl.effective_sync(), SyncMode::Commit);
        let change: ClientDecl =
            serde_json::from_value(json!({ "init": "", "persist": "session", "sync": "change" })).unwrap();
        assert_eq!(change.effective_sync(), SyncMode::Change);
    }

    #[test]
    fn unknown_client_decl_fields_are_rejected() {
        assert!(serde_json::from_value::<ClientDecl>(json!({ "init": 0, "persists": "app" })).is_err());
    }
}

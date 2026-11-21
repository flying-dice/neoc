//! The node shape, event handlers and client actions, and node addressing.
//!
//! A node's **path** is the keys from the window root, `/`-joined, with an `@slot` segment when the
//! node sits in a named slot: `main/list/@row/item/delete`. Paths are stable across renders because keys are.

use std::collections::BTreeMap;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// Limits the host enforces on every view.
pub mod limits {
    pub const MAX_DEPTH: usize = 64;
    pub const MAX_NODES: usize = 10_000;
    pub const MAX_MESSAGE_BYTES: usize = 1 << 20;
    pub const MAX_OPS_PER_BATCH: usize = 1_000;
}

/// One node of a view tree. Nodes are JSON end to end: `props` values are literals, refs or `$fn` calls.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Catalogue name: `"Stack"`, `"TextField"`, `"List"`, ...
    #[serde(rename = "type")]
    pub type_: String,
    /// Unique among the siblings of the same slot.
    pub key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<BTreeMap<String, Value>>", optional))]
    pub props: BTreeMap<String, Value>,
    /// The default slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<Vec<Node>>", optional))]
    pub children: Vec<Node>,
    /// Named slots: `"row"`, `"toolbar.trailing"`, `"sidebar"`, `"dest:note"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<BTreeMap<String, Vec<Node>>>", optional))]
    pub slots: BTreeMap<String, Vec<Node>>,
    /// The events this node reports. Nothing executable crosses the wire: `true` sends the event to the app.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(feature = "codegen", ts(as = "Option<BTreeMap<String, Handler>>", optional))]
    pub on: BTreeMap<String, Handler>,
}

impl Node {
    /// A node with a type and a key and nothing else.
    #[must_use]
    pub fn new(type_: impl Into<String>, key: impl Into<String>) -> Self {
        Self { type_: type_.into(), key: key.into(), ..Self::default() }
    }

    #[must_use]
    pub fn prop(mut self, name: &str, value: Value) -> Self {
        self.props.insert(name.to_owned(), value);
        self
    }

    #[must_use]
    pub fn child(mut self, node: Self) -> Self {
        self.children.push(node);
        self
    }

    #[must_use]
    pub fn slot(mut self, name: &str, nodes: Vec<Self>) -> Self {
        self.slots.insert(name.to_owned(), nodes);
        self
    }

    #[must_use]
    pub fn handler(mut self, event: &str, handler: Handler) -> Self {
        self.on.insert(event.to_owned(), handler);
        self
    }

    /// Every container of this node: the default slot first (`None`), then the named slots in name order.
    pub fn containers(&self) -> impl Iterator<Item = (Option<&str>, &[Self])> {
        std::iter::once((None, self.children.as_slice()))
            .chain(self.slots.iter().map(|(name, nodes)| (Some(name.as_str()), nodes.as_slice())))
    }

    /// Number of nodes in this subtree, itself included.
    #[must_use]
    pub fn count(&self) -> usize {
        1 + self.containers().flat_map(|(_, nodes)| nodes).map(Self::count).sum::<usize>()
    }
}

/// What an event does: `true` reports it to the app, a list of [`Action`]s runs on the client (and any
/// `emit` action also reports to the app).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[cfg_attr(feature = "codegen", ts(type = "true | Action[]"))]
#[cfg_attr(feature = "codegen", schemars(with = "Value"))]
pub enum Handler {
    /// `true`: send the event to the app.
    Emit,
    Actions(Vec<Action>),
}

impl Serialize for Handler {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Emit => serializer.serialize_bool(true),
            Self::Actions(actions) => actions.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Handler {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::Bool(true) => Ok(Self::Emit),
            value @ Value::Array(_) => {
                serde_json::from_value(value).map(Self::Actions).map_err(de::Error::custom)
            }
            _ => Err(de::Error::custom("a handler is `true` or a list of actions")),
        }
    }
}

/// The declarative micro-language that runs on the client with no round trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(tag = "do")]
pub enum Action {
    /// Write a client or state ref.
    #[serde(rename = "set")]
    Set {
        #[serde(rename = "ref")]
        #[cfg_attr(feature = "codegen", ts(rename = "ref"))]
        target: String,
        #[cfg_attr(feature = "codegen", ts(type = "JsonValue"))]
        value: Value,
    },
    #[serde(rename = "toggle")]
    Toggle {
        #[serde(rename = "ref")]
        #[cfg_attr(feature = "codegen", ts(rename = "ref"))]
        target: String,
    },
    #[serde(rename = "nav.push")]
    NavPush {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        stack: Option<String>,
        /// `{name, ...params}`; param values may be expressions.
        #[cfg_attr(feature = "codegen", ts(type = "{ [key: string]: JsonValue }"))]
        route: Map<String, Value>,
    },
    #[serde(rename = "nav.pop")]
    NavPop,
    #[serde(rename = "nav.popToRoot")]
    NavPopToRoot,
    #[serde(rename = "focus")]
    Focus { key: String },
    #[serde(rename = "scrollTo")]
    ScrollTo { key: String },
    #[serde(rename = "copy")]
    Copy {
        #[cfg_attr(feature = "codegen", ts(type = "JsonValue"))]
        text: Value,
    },
    /// Report to the app.
    #[serde(rename = "emit")]
    Emit {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        event: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(as = "Option<BTreeMap<String, Value>>", optional))]
        payload: Option<Map<String, Value>>,
    },
    #[serde(rename = "seq")]
    Seq { actions: Vec<Action> },
}

impl Action {
    /// Every action's wire name, in declaration order. The Lua `ui.act` builders must cover exactly these.
    pub const NAMES: [&'static str; 10] =
        ["set", "toggle", "nav.push", "nav.pop", "nav.popToRoot", "focus", "scrollTo", "copy", "emit", "seq"];

    /// The wire name of the action (`"nav.push"`).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Set { .. } => "set",
            Self::Toggle { .. } => "toggle",
            Self::NavPush { .. } => "nav.push",
            Self::NavPop => "nav.pop",
            Self::NavPopToRoot => "nav.popToRoot",
            Self::Focus { .. } => "focus",
            Self::ScrollTo { .. } => "scrollTo",
            Self::Copy { .. } => "copy",
            Self::Emit { .. } => "emit",
            Self::Seq { .. } => "seq",
        }
    }
}

/// An auto key (`_0`, `_1`, ...) is assigned by the Lua constructors to nodes the author left un-keyed.
#[must_use]
pub fn is_auto_key(key: &str) -> bool {
    key.strip_prefix('_').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The path of a node: the parent's path, then `@slot` when the node is in a named slot, then its key.
#[must_use]
pub fn join_path(parent: &str, slot: Option<&str>, key: &str) -> String {
    match slot {
        Some(slot) => format!("{parent}/@{slot}/{key}"),
        None => format!("{parent}/{key}"),
    }
}

/// Split a path into the node path of its parent container's owner and the trailing key.
#[must_use]
pub fn split_path(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once('/')
}

/// Derive the test id of the node at `path`: `app + "-" + the explicit keys on the path`, joined
/// with `-`. The window root, **auto keys** and **slot names** are skipped (wrapping a node in a stack, or
/// moving it into a toolbar slot, never renames it); the `row` slot contributes `row-<rowKey>`.
///
/// Returns `None` for a node that owns no id: an auto-keyed node (it shares its parent's id) other than a
/// row-template root.
#[must_use]
pub fn derive_test_id(app: &str, path: &str, row_key: Option<&str>) -> Option<String> {
    let mut segments = path.split('/');
    segments.next(); // the window root
    let mut parts: Vec<String> = Vec::new();
    let mut owns_id = false;
    let mut after_row = false;
    for segment in segments {
        if let Some(slot) = segment.strip_prefix('@') {
            if slot == "row" {
                parts.push(format!("row-{}", row_key.unwrap_or("*")));
                after_row = true;
            }
            continue;
        }
        if is_auto_key(segment) {
            // a row template's root owns the row container id even with an auto key
            owns_id = after_row;
        } else {
            parts.push(segment.to_owned());
            owns_id = true;
        }
        after_row = false;
    }
    owns_id.then(|| parts.iter().fold(String::from(app), |id, part| format!("{id}-{part}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_node_is_tagged_json() {
        let node = Node::new("Button", "go")
            .prop("title", json!("Go"))
            .handler("tap", Handler::Emit)
            .slot("toolbar.trailing", vec![Node::new("Spacer", "_0")]);
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["type"], "Button");
        assert_eq!(json["on"]["tap"], json!(true));
        assert_eq!(json["slots"]["toolbar.trailing"][0]["type"], "Spacer");
        assert!(json.get("children").is_none(), "empty containers are omitted");
        assert_eq!(serde_json::from_value::<Node>(json).unwrap(), node);
    }

    #[test]
    fn handlers_are_true_or_action_lists() {
        let actions = Handler::Actions(vec![
            Action::Set { target: "client.sheet".into(), value: json!(true) },
            Action::Emit { event: Some("done".into()), payload: None },
        ]);
        let json = serde_json::to_value(&actions).unwrap();
        assert_eq!(json[0], json!({ "do": "set", "ref": "client.sheet", "value": true }));
        assert_eq!(serde_json::from_value::<Handler>(json).unwrap(), actions);
        assert!(serde_json::from_value::<Handler>(json!(false)).is_err());
        assert!(serde_json::from_value::<Handler>(json!("tap")).is_err());
    }

    #[test]
    fn every_action_round_trips() {
        let all = vec![
            Action::Set { target: "client.x".into(), value: json!({ "$ref": "row.id" }) },
            Action::Toggle { target: "client.on".into() },
            Action::NavPush {
                stack: None,
                route: json!({ "name": "note", "id": 7 }).as_object().cloned().unwrap(),
            },
            Action::NavPop,
            Action::NavPopToRoot,
            Action::Focus { key: "q".into() },
            Action::ScrollTo { key: "end".into() },
            Action::Copy { text: json!("hi") },
            Action::Emit { event: None, payload: None },
            Action::Seq { actions: vec![Action::NavPop] },
        ];
        let names: Vec<&str> = all.iter().map(Action::name).collect();
        assert_eq!(names, Action::NAMES, "NAMES lists every variant, in order");
        for action in all {
            let json = serde_json::to_value(&action).unwrap();
            assert_eq!(json["do"], action.name());
            assert_eq!(serde_json::from_value::<Action>(json).unwrap(), action);
        }
    }

    #[test]
    fn unknown_node_fields_are_rejected() {
        assert!(serde_json::from_value::<Node>(json!({ "type": "Text", "key": "t", "style": "x" })).is_err());
    }

    #[test]
    fn auto_keys() {
        assert!(is_auto_key("_0"));
        assert!(is_auto_key("_12"));
        assert!(!is_auto_key("_"));
        assert!(!is_auto_key("_a"));
        assert!(!is_auto_key("key-7"));
    }

    #[test]
    fn paths_join_with_slots() {
        assert_eq!(join_path("main", None, "list"), "main/list");
        assert_eq!(join_path("main/list", Some("row"), "item"), "main/list/@row/item");
        assert_eq!(split_path("main/list/@row/item"), Some(("main/list/@row", "item")));
    }

    #[test]
    fn test_ids_follow_the_spec_table() {
        let id = |app, path, row| derive_test_id(app, path, row);
        assert_eq!(id("calculator", "main/_0/_1/_2/key-7", None).as_deref(), Some("calculator-key-7"));
        assert_eq!(id("notes", "main/actions/save", None).as_deref(), Some("notes-actions-save"));
        assert_eq!(
            id("notes", "main/list/@row/item/delete", Some("7")).as_deref(),
            Some("notes-list-row-7-item-delete")
        );
        assert_eq!(id("notes", "main/list/@row/item", Some("7")).as_deref(), Some("notes-list-row-7-item"));
        assert_eq!(
            id("notes", "main/confirm-delete/@actions/confirm", None).as_deref(),
            Some("notes-confirm-delete-confirm")
        );
        // the row template root with an auto key owns the row container id
        assert_eq!(id("notes", "main/notes/_1/@row/_0", Some("7")).as_deref(), Some("notes-notes-row-7"));
    }

    #[test]
    fn auto_keyed_nodes_own_no_id() {
        assert_eq!(derive_test_id("notes", "main/_0/_1", None), None);
        assert_eq!(derive_test_id("notes", "main/list/_1", None), None);
        assert_eq!(derive_test_id("notes", "main", None), None, "the window root owns no id");
    }

    #[test]
    fn counting_and_containers() {
        let tree = Node::new("Stack", "main")
            .child(Node::new("Text", "a"))
            .slot("row", vec![Node::new("Row", "_0").child(Node::new("Text", "t"))]);
        assert_eq!(tree.count(), 4);
        assert_eq!(tree.containers().count(), 2);
    }
}

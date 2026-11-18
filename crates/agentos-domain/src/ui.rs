//! Server-driven UI: the app describes views; the managed client renders them with the kit and
//! sends events back. These types are the single source for the TS types (ts-rs) and the Luau defs.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum UiNode {
    Stack {
        direction: Direction,
        children: Vec<UiNode>,
    },
    Text {
        text: String,
    },
    Button {
        id: String,
        label: String,
        #[serde(default)]
        disabled: bool,
    },
    TextField {
        id: String,
        value: String,
        #[serde(default)]
        placeholder: Option<String>,
    },
    List {
        id: String,
        items: Vec<ListItem>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Row,
    Column,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
pub struct ListItem {
    pub key: String,
    pub label: String,
}

/// UI patch (path = child indexes from the root).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum UiPatch {
    Replace { path: Vec<u32>, node: UiNode },
    Remove { path: Vec<u32> },
}

/// Client -> host event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS, schemars::JsonSchema))]
pub struct UiEvent {
    pub target: String,
    pub name: String,
    #[serde(default)]
    #[cfg_attr(feature = "codegen", ts(type = "Record<string, unknown>"))]
    pub data: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_format_is_tagged_snake_case() {
        let n = UiNode::Stack {
            direction: Direction::Column,
            children: vec![UiNode::Button { id: "go".into(), label: "Go".into(), disabled: false }],
        };
        let j = serde_json::to_value(&n).unwrap();
        assert_eq!(j["type"], "stack");
        assert_eq!(j["children"][0]["type"], "button");
        assert_eq!(serde_json::from_value::<UiNode>(j).unwrap(), n);
    }
}

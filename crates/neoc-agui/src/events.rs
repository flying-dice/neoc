//! What an agent answers with: `ag_ui.core.events`. One [`Event`] variant for each event type; every one also
//! carries the fields of the SDK's `BaseEvent`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{Message, RunAgentInput};

/// The tokens one model call of a run used.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_input_tokens: Option<u64>,
}

fn yes() -> bool {
    true
}

/// The event enum: each variant gets the `BaseEvent` fields after its own. (`#[serde(flatten)]` would write a
/// variant as a map, and [`crate::snake`] tells a struct's fields from a map's keys.)
macro_rules! events {
    ($( $(#[$meta:meta])* $name:ident { $( $(#[$fmeta:meta])* $field:ident : $ty:ty, )* } )*) => {
        /// One event of a run, told apart by its `type`.
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
        pub enum Event {
            $(
                $(#[$meta])*
                $name {
                    $( $(#[$fmeta])* $field: $ty, )*
                    /// When the event was made, in milliseconds since the epoch.
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    timestamp: Option<i64>,
                    /// The event of the agent's own framework this one was made from.
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    raw_event: Option<Value>,
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    metadata: Option<Value>,
                    /// The subagent run the event belongs to, when it is not the run's own.
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    subagent_run_id: Option<String>,
                },
            )*
        }

        /// Every event type, in the SDK's order.
        pub const EVENT_TYPES: &[&str] = &[ $( events!(@name $name) ),* ];
    };
    (@name $name:ident) => { stringify!($name) };
}

events! {
    TextMessageStart {
        message_id: String,
        #[serde(default = "assistant")]
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    }
    TextMessageContent { message_id: String, delta: String, }
    TextMessageEnd { message_id: String, }
    /// A start, a content and an end in one: the first chunk of a message carries its id.
    TextMessageChunk {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    }
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    }
    /// A piece of the arguments, which are JSON text once joined.
    ToolCallArgs { tool_call_id: String, delta: String, }
    ToolCallEnd { tool_call_id: String, }
    ToolCallChunk {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
    }
    ToolCallResult {
        message_id: String,
        tool_call_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    }
    StateSnapshot { snapshot: Value, }
    /// A JSON Patch to the state.
    StateDelta { delta: Vec<Value>, }
    MessagesSnapshot { messages: Vec<Message>, }
    ActivitySnapshot {
        message_id: String,
        activity_type: String,
        content: Value,
        /// `false` leaves an activity message that exists already as it is.
        #[serde(default = "yes")]
        replace: bool,
    }
    /// A JSON Patch to the content of an activity message.
    ActivityDelta { message_id: String, activity_type: String, patch: Vec<Value>, }
    Raw {
        event: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    }
    Custom {
        name: String,
        #[serde(default)]
        value: Value,
    }
    RunStarted {
        thread_id: String,
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Box<RunAgentInput>>,
    }
    RunFinished {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Vec<TokenUsage>>,
    }
    RunError {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Vec<TokenUsage>>,
    }
    StepStarted { step_name: String, }
    StepFinished { step_name: String, }
    ReasoningStart { message_id: String, }
    ReasoningMessageStart {
        message_id: String,
        #[serde(default = "reasoning")]
        role: String,
    }
    ReasoningMessageContent { message_id: String, delta: String, }
    ReasoningMessageEnd { message_id: String, }
    ReasoningMessageChunk {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delta: Option<String>,
    }
    ReasoningEnd { message_id: String, }
    ReasoningEncryptedValue {
        /// `tool-call` or `message`: what `entity_id` names.
        subtype: String,
        entity_id: String,
        encrypted_value: String,
    }
    SubagentStarted {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_subagent_run_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_tool_call_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    }
    SubagentFinished {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<Value>,
    }
    SubagentError {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    }
    /// Deprecated in the SDK for `REASONING_START`; agents still send it.
    ThinkingStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    }
    ThinkingEnd {}
    ThinkingTextMessageStart {}
    ThinkingTextMessageContent { delta: String, }
    ThinkingTextMessageEnd {}
}

fn assistant() -> String {
    "assistant".to_owned()
}

fn reasoning() -> String {
    "reasoning".to_owned()
}

impl Event {
    /// The event a `data:` payload holds. An event of a type this crate does not know is `Ok(None)`: a newer agent
    /// does not end the run of an older client.
    ///
    /// # Errors
    /// The payload is not JSON, has no `type`, or is an event of a known type with a field missing or mistyped.
    pub fn from_json(payload: &str) -> Result<Option<Self>, serde_json::Error> {
        #[derive(Deserialize)]
        struct Typed {
            r#type: String,
        }
        let typed: Typed = serde_json::from_str(payload)?;
        if !EVENT_TYPES.iter().any(|known| screaming(known) == typed.r#type) {
            return Ok(None);
        }
        serde_json::from_str(payload).map(Some)
    }
}

/// `TextMessageStart` as the wire has it: `TEXT_MESSAGE_START`.
pub fn screaming(variant: &str) -> String {
    let mut out = String::with_capacity(variant.len() + 4);
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_type_is_read_from_its_wire_name_and_written_back() {
        for name in EVENT_TYPES {
            let wire = screaming(name);
            // the fields any variant may require, under their wire names; a variant ignores the rest
            let payload = json!({
                "type": wire, "messageId": "m", "delta": "d", "toolCallId": "c", "toolCallName": "n", "content": "x",
                "snapshot": {}, "messages": [], "activityType": "PLAN", "patch": [], "event": {}, "name": "n",
                "threadId": "t", "runId": "r", "message": "m", "stepName": "s", "subtype": "message",
                "entityId": "e", "encryptedValue": "v", "subagentRunId": "s", "timestamp": 1,
            });
            let payload = if wire == "STATE_DELTA" { json!({ "type": wire, "delta": [] }) } else { payload };
            let event =
                Event::from_json(&payload.to_string()).unwrap().unwrap_or_else(|| panic!("{wire} unknown"));
            let written = serde_json::to_value(&event).unwrap();
            assert_eq!(written["type"], wire, "{name}");
            assert_eq!(serde_json::from_value::<Event>(written).unwrap(), event, "{name}");
        }
        assert_eq!(EVENT_TYPES.len(), 36);
    }

    #[test]
    fn an_unknown_type_is_none_and_a_known_one_with_a_field_missing_is_an_error() {
        assert_eq!(Event::from_json(r#"{"type":"FROM_THE_FUTURE","x":1}"#).unwrap(), None);
        assert!(Event::from_json(r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"m"}"#).is_err());
        assert!(Event::from_json(r#"{"no":"type"}"#).is_err());
        assert!(Event::from_json("not json").is_err());
    }

    #[test]
    fn an_activity_snapshot_replaces_unless_it_says_not_to() {
        let event = Event::from_json(
            r#"{"type":"ACTIVITY_SNAPSHOT","messageId":"m","activityType":"a2ui-surface","content":{"a":1}}"#,
        )
        .unwrap()
        .unwrap();
        let Event::ActivitySnapshot { replace, content, .. } = event else { panic!("wrong variant") };
        assert!(replace);
        assert_eq!(content, json!({ "a": 1 }));
    }
}

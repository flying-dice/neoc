//! What a client sends to an agent: `ag_ui.core.types`.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A list read where a script's empty table may have arrived as an empty map: `{}` is the empty list.
fn list<'de, D, T>(de: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrEmpty<T> {
        List(Vec<T>),
        Empty(serde_json::Map<String, Value>),
    }
    match ListOrEmpty::<T>::deserialize(de)? {
        ListOrEmpty::List(items) => Ok(items),
        ListOrEmpty::Empty(map) if map.is_empty() => Ok(Vec::new()),
        ListOrEmpty::Empty(_) => Err(serde::de::Error::custom("expected a list")),
    }
}

fn optional_list<'de, D, T>(de: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    list(de).map(Some)
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// The body of the POST that starts a run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    #[serde(alias = "thread_id")]
    pub thread_id: String,
    #[serde(alias = "run_id")]
    pub run_id: String,
    #[serde(default, alias = "parent_run_id", skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    /// The state the agent starts from. Always sent, `null` when there is none: the Python SDK's `RunAgentInput`
    /// has no default for it, so an agent built on it refuses an input without the key.
    #[serde(default)]
    pub state: Value,
    /// The whole conversation so far.
    #[serde(default, deserialize_with = "list")]
    pub messages: Vec<Message>,
    /// The tools the client offers: the agent asks for one with a tool call, and the client runs it.
    #[serde(default, deserialize_with = "list")]
    pub tools: Vec<Tool>,
    #[serde(default, deserialize_with = "list")]
    pub context: Vec<Context>,
    #[serde(default = "empty_object", alias = "forwarded_props")]
    pub forwarded_props: Value,
    /// The answers to the interrupts the last run ended with.
    #[serde(default, deserialize_with = "optional_list", skip_serializing_if = "Option::is_none")]
    pub resume: Option<Vec<ResumeEntry>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    #[serde(alias = "interrupt_id")]
    pub interrupt_id: String,
    /// `resolved` or `cancelled`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// One message of the conversation, told apart by its `role`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Message {
    Developer {
        id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    System {
        id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    Assistant {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(
            default,
            alias = "tool_calls",
            deserialize_with = "optional_list",
            skip_serializing_if = "Option::is_none"
        )]
        tool_calls: Option<Vec<ToolCall>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    User {
        id: String,
        content: MessageContent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    Tool {
        id: String,
        content: MessageContent,
        #[serde(alias = "tool_call_id")]
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(default, alias = "encrypted_value", skip_serializing_if = "Option::is_none")]
        encrypted_value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    /// Something the agent is doing, shown between the messages; never sent back to an agent.
    Activity {
        id: String,
        #[serde(alias = "activity_type")]
        activity_type: String,
        content: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    Reasoning {
        id: String,
        content: String,
        #[serde(default, alias = "encrypted_value", skip_serializing_if = "Option::is_none")]
        encrypted_value: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
}

/// The content of a user message or a tool result: text, or parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum ContentPart {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    Image(MediaPart),
    Audio(MediaPart),
    Video(MediaPart),
    Document(MediaPart),
    /// Deprecated in the SDK for the four kinds above; still read and written.
    Binary {
        #[serde(alias = "mime_type")]
        mime_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaPart {
    pub source: PartSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// Where the bytes of a part are: inline (`data`, base64), at a URL, or a file the provider already holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum PartSource {
    Data {
        value: String,
        #[serde(alias = "mime_type")]
        mime_type: String,
    },
    Url {
        value: String,
        #[serde(default, alias = "mime_type", skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    File {
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, alias = "mime_type", skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    /// Always `function`.
    #[serde(default = "function", rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
    #[serde(default, alias = "encrypted_value", skip_serializing_if = "Option::is_none")]
    pub encrypted_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

fn function() -> String {
    "function".to_owned()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// The arguments as JSON text.
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub description: String,
    pub value: String,
}

/// A tool the client offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// The JSON Schema of the arguments.
    #[serde(default = "empty_object")]
    pub parameters: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_input_is_written_in_camel_case_and_read_under_either_name() {
        let snake = json!({
            "thread_id": "t", "run_id": "r", "parent_run_id": "p", "forwarded_props": { "a_b": 1 },
            "messages": [
                { "role": "user", "id": "1", "content": "hi" },
                { "role": "assistant", "id": "2", "tool_calls": [
                    { "id": "c", "function": { "name": "f", "arguments": "{}" }, "encrypted_value": "x" } ] },
                { "role": "tool", "id": "3", "content": "done", "tool_call_id": "c" },
                { "role": "activity", "id": "4", "activity_type": "PLAN", "content": { "snake_key": true } },
            ],
            "tools": {}, "context": [],
        });
        let input: RunAgentInput = serde_json::from_value(snake).unwrap();
        let wire = serde_json::to_value(&input).unwrap();
        assert_eq!(
            wire,
            json!({
                "threadId": "t", "runId": "r", "parentRunId": "p", "forwardedProps": { "a_b": 1 },
                "messages": [
                    { "role": "user", "id": "1", "content": "hi" },
                    { "role": "assistant", "id": "2", "toolCalls": [
                        { "id": "c", "type": "function", "function": { "name": "f", "arguments": "{}" },
                          "encryptedValue": "x" } ] },
                    { "role": "tool", "id": "3", "content": "done", "toolCallId": "c" },
                    { "role": "activity", "id": "4", "activityType": "PLAN", "content": { "snake_key": true } },
                ],
                // always sent: an agent that reads the input strictly needs the key, and null is "no state yet"
                "tools": [], "context": [], "state": null,
            })
        );
        assert_eq!(serde_json::from_value::<RunAgentInput>(wire).unwrap(), input);
    }

    #[test]
    fn user_content_is_text_or_parts() {
        let message: Message = serde_json::from_value(json!({
            "role": "user", "id": "1", "content": [
                { "type": "text", "text": "look" },
                { "type": "image", "source": { "type": "url", "value": "https://e.example/a.png", "mimeType": "image/png" } },
                { "type": "binary", "mimeType": "application/pdf", "data": "AA==" },
            ]
        }))
        .unwrap();
        let Message::User { content: MessageContent::Parts(parts), .. } = message else {
            panic!("not parts")
        };
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn a_message_with_an_unknown_role_or_a_missing_field_is_refused() {
        assert!(serde_json::from_value::<Message>(json!({ "role": "robot", "id": "1" })).is_err());
        assert!(
            serde_json::from_value::<Message>(json!({ "role": "tool", "id": "1", "content": "x" })).is_err()
        );
        assert!(serde_json::from_value::<RunAgentInput>(json!({ "threadId": "t" })).is_err());
    }
}

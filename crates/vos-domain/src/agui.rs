//! The AG-UI wire model (<https://docs.ag-ui.com>): events and the run input. Pure data, hand-rolled: there is no
//! maintained official Rust crate. Events are `{"type":"RUN_STARTED","threadId":...}`; the HTTP edge in
//! `vos-agui` frames each as an SSE `data:` line.

use crate::thread::{Interrupt, ResumeEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Developer,
    System,
    Assistant,
    User,
    Tool,
}

/// How a run ended (`RUN_FINISHED.outcome`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunOutcome {
    Success,
    /// The run stopped for a person's input; resume with a new run carrying `resume`.
    Interrupt {
        interrupts: Vec<Interrupt>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum AgUiEvent {
    RunStarted {
        thread_id: String,
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<String>,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<RunOutcome>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    RunError {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    StepStarted {
        step_name: String,
    },
    StepFinished {
        step_name: String,
    },
    TextMessageStart {
        message_id: String,
        role: Role,
    },
    TextMessageContent {
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        message_id: String,
    },
    ReasoningStart {
        message_id: String,
    },
    ReasoningMessageStart {
        message_id: String,
        role: ReasoningRole,
    },
    ReasoningMessageContent {
        message_id: String,
        delta: String,
    },
    ReasoningMessageEnd {
        message_id: String,
    },
    ReasoningEnd {
        message_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    ToolCallResult {
        message_id: String,
        tool_call_id: String,
        content: String,
    },
    StateSnapshot {
        snapshot: Value,
    },
    /// RFC 6902 JSON Patch operations.
    StateDelta {
        delta: Vec<Value>,
    },
    Custom {
        name: String,
        value: Value,
    },
}

/// The only role of a reasoning message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningRole {
    Reasoning,
}

/// `RunAgentInput`, the POST body. Only what the host routes on is typed; messages stay raw (the last `user`
/// message is the prompt: the agent session holds the rest of the context).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    pub thread_id: String,
    pub run_id: String,
    #[serde(default)]
    pub messages: Vec<Value>,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub context: Vec<Value>,
    #[serde(default)]
    pub forwarded_props: Value,
    /// Answers to the open interrupts of the thread's last run.
    #[serde(default)]
    pub resume: Vec<ResumeEntry>,
}

impl RunAgentInput {
    /// The text of the last `user` message, if the input has one.
    #[must_use]
    pub fn prompt(&self) -> Option<String> {
        self.messages.iter().rev().find(|m| m.get("role").and_then(Value::as_str) == Some("user")).and_then(
            |m| match m.get("content")? {
                Value::String(s) => Some(s.clone()),
                // multimodal content: the text parts, in order
                Value::Array(parts) => {
                    let text: Vec<&str> = parts.iter().filter_map(|p| p.get("text")?.as_str()).collect();
                    (!text.is_empty()).then(|| text.join("\n"))
                }
                _ => None,
            },
        )
    }

    /// `forwardedProps.agentId`.
    #[must_use]
    pub fn agent_id(&self) -> Option<&str> {
        self.forwarded_props.get("agentId").and_then(Value::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_shape_matches_the_spec() {
        let e = AgUiEvent::TextMessageContent { message_id: "m1".into(), delta: "hi".into() };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"type":"TEXT_MESSAGE_CONTENT","messageId":"m1","delta":"hi"})
        );
        let e = AgUiEvent::ReasoningMessageStart { message_id: "r".into(), role: ReasoningRole::Reasoning };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"type":"REASONING_MESSAGE_START","messageId":"r","role":"reasoning"})
        );
        let e = AgUiEvent::RunFinished {
            thread_id: "t".into(),
            run_id: "r".into(),
            outcome: Some(RunOutcome::Success),
            result: None,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"type":"RUN_FINISHED","threadId":"t","runId":"r","outcome":{"type":"success"}})
        );
        let back: AgUiEvent = serde_json::from_value(json!({"type":"RUN_ERROR","message":"x"})).unwrap();
        assert_eq!(back, AgUiEvent::RunError { message: "x".into(), code: None });
    }

    #[test]
    fn an_interrupt_outcome_carries_its_interrupts() {
        let e = AgUiEvent::RunFinished {
            thread_id: "t".into(),
            run_id: "r".into(),
            outcome: Some(RunOutcome::Interrupt {
                interrupts: vec![Interrupt {
                    id: "i".into(),
                    reason: "tool_call".into(),
                    message: None,
                    tool_call_id: None,
                    response_schema: None,
                    expires_at: None,
                    metadata: None,
                }],
            }),
            result: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["outcome"]["type"], "interrupt");
        assert_eq!(v["outcome"]["interrupts"][0]["id"], "i");
        assert_eq!(serde_json::from_value::<AgUiEvent>(v).unwrap(), e);
    }

    #[test]
    fn the_prompt_is_the_last_user_message_and_the_agent_comes_from_forwarded_props() {
        let input: RunAgentInput = serde_json::from_value(json!({
            "threadId": "t", "runId": "r",
            "messages": [
                {"id": "1", "role": "user", "content": "first"},
                {"id": "2", "role": "assistant", "content": "reply"},
                {"id": "3", "role": "user", "content": [{"type": "text", "text": "last"}]}
            ],
            "forwardedProps": {"agentId": "claude"}
        }))
        .unwrap();
        assert_eq!(input.prompt().as_deref(), Some("last"));
        assert_eq!(input.agent_id(), Some("claude"));
        assert!(input.resume.is_empty());
        let none: RunAgentInput = serde_json::from_value(json!({"threadId": "t", "runId": "r"})).unwrap();
        assert!(none.prompt().is_none() && none.agent_id().is_none());
        assert!(serde_json::from_value::<RunAgentInput>(json!({"runId": "r"})).is_err());
    }
}

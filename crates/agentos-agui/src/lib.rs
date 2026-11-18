//! Driving adapter: AG-UI (<https://docs.ag-ui.com>) over HTTP + SSE, for agent chats.
//!
//! AG-UI over HTTP + SSE. Hand-rolled: the whole wire model we need is one
//! serde enum. Events are `{"type":"RUN_STARTED","threadId":...}`; each SSE frame is `data: <json>\n\n`.

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::post,
    Json, Router,
};
use futures_util::{stream::BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{convert::Infallible, sync::Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", rename_all_fields = "camelCase")]
pub enum AgUiEvent {
    RunStarted {
        thread_id: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<String>,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    RunError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
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
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
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
    StateDelta {
        delta: Vec<Value>,
    },
    Custom {
        name: String,
        value: Value,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Developer,
    System,
    Assistant,
    User,
    Tool,
}

/// `RunAgentInput`, the POST body (only the parts we route on are typed; the rest is kept raw).
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
}

/// What the application provides: given the run input, a stream of AG-UI events. In the host this
/// is a thin mapping of ACP `session/update` -> AG-UI events driven by the `AgentConnector` port.
pub trait RunHandler: Send + Sync + 'static {
    fn run(&self, input: RunAgentInput) -> BoxStream<'static, AgUiEvent>;
}

pub fn agui_router(handler: Arc<dyn RunHandler>) -> Router {
    Router::new().route("/agui", post(run_endpoint)).with_state(handler)
}

async fn run_endpoint(
    State(h): State<Arc<dyn RunHandler>>,
    body: Result<Json<RunAgentInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // structurally invalid input is refused with a plain HTTP error *before* the stream starts
    let Ok(Json(input)) = body else {
        return (axum::http::StatusCode::UNPROCESSABLE_ENTITY, "invalid RunAgentInput").into_response();
    };
    let events = h.run(input).map(|e| {
        let json = serde_json::to_string(&e)
            .unwrap_or_else(|_| r#"{"type":"RUN_ERROR","message":"serialize"}"#.into());
        Ok::<_, Infallible>(Event::default().data(json))
    });
    Sse::new(events).keep_alive(KeepAlive::default()).into_response()
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
        let e = AgUiEvent::ToolCallStart {
            tool_call_id: "t".into(),
            tool_call_name: "notes__add".into(),
            parent_message_id: None,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"type":"TOOL_CALL_START","toolCallId":"t","toolCallName":"notes__add"})
        );
        let back: AgUiEvent = serde_json::from_value(json!({"type":"RUN_ERROR","message":"x"})).unwrap();
        assert_eq!(back, AgUiEvent::RunError { message: "x".into(), code: None });
    }
}

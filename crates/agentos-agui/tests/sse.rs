#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use agentos_agui::{agui_router, AgUiEvent, Role, RunAgentInput, RunHandler};
use futures_util::{stream, stream::BoxStream, StreamExt};
use serde_json::json;
use std::sync::Arc;

struct Echo;
impl RunHandler for Echo {
    fn run(&self, i: RunAgentInput) -> BoxStream<'static, AgUiEvent> {
        let last = i.messages.last().and_then(|m| m["content"].as_str()).unwrap_or("").to_owned();
        let (t, r) = (i.thread_id, i.run_id);
        let evs = vec![
            AgUiEvent::RunStarted { thread_id: t.clone(), run_id: r.clone(), parent_run_id: None },
            AgUiEvent::TextMessageStart { message_id: "m1".into(), role: Role::Assistant },
            AgUiEvent::TextMessageContent { message_id: "m1".into(), delta: "You said: ".into() },
            AgUiEvent::TextMessageContent { message_id: "m1".into(), delta: last },
            AgUiEvent::TextMessageEnd { message_id: "m1".into() },
            AgUiEvent::RunFinished { thread_id: t, run_id: r, result: None },
        ];
        stream::iter(evs).boxed()
    }
}

#[tokio::test]
async fn post_run_input_streams_ag_ui_events_as_sse() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, agui_router(Arc::new(Echo))).await });

    let res = reqwest::Client::new()
        .post(format!("http://{addr}/agui"))
        .header("accept", "text/event-stream")
        .json(&json!({"threadId":"th1","runId":"r1","state":{},"messages":[{"id":"u1","role":"user","content":"hello"}],"tools":[],"context":[],"forwardedProps":{}}))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
    let body = res.text().await.unwrap();
    println!("--- raw SSE ---\n{body}");

    let events: Vec<AgUiEvent> = body
        .split("\n\n")
        .filter_map(|f| f.strip_prefix("data: "))
        .map(|j| serde_json::from_str(j).unwrap())
        .collect();
    assert_eq!(events.len(), 6);
    assert!(matches!(events.first(), Some(AgUiEvent::RunStarted { run_id, .. }) if run_id == "r1"));
    assert!(matches!(events.last(), Some(AgUiEvent::RunFinished { .. })));
    let text: String = events
        .iter()
        .filter_map(|e| {
            if let AgUiEvent::TextMessageContent { delta, .. } = e {
                Some(delta.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(text, "You said: hello");

    // invalid input is refused outside the stream
    let bad = reqwest::Client::new()
        .post(format!("http://{addr}/agui"))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 422);
    server.abort();
}

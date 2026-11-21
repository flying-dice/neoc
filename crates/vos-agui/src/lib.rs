//! Driving adapter: AG-UI (<https://docs.ag-ui.com>) over HTTP + SSE, for agent chats.
//!
//! The routes (all behind the host's device-session guard):
//!
//! | route                                | what                                                         |
//! |--------------------------------------|--------------------------------------------------------------|
//! | `POST /agui`                         | a run: `RunAgentInput` in, an SSE stream of AG-UI events out |
//! | `POST /agui/threads`                 | ensure a thread (starts the agent session) -> snapshot       |
//! | `GET  /agui/threads/{id}`            | the thread's snapshot (history, state, open interrupts)      |
//! | `POST /agui/threads/{id}/cancel`     | cancel the turn                                              |
//! | `POST /agui/threads/{id}/settings`   | change the session's mode or one of its settings             |
//!
//! Everything that is not HTTP lives in [`vos_application::ChatService`]. Errors found before a stream
//! starts are JSON `{"error": ...}` with a status code; once the stream is open a failure is a `RUN_ERROR`
//! event. A client that drops the stream of a run that is not waiting for an answer cancels it.

pub use vos_domain::agui::{AgUiEvent, ReasoningRole, Role, RunAgentInput, RunOutcome};

use axum::{
    body::Bytes,
    extract::{FromRef, Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Extension, Json, Router,
};
use futures_util::{Stream, StreamExt};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio_util::sync::CancellationToken;
use vos_application::{ChatError, ChatService, RunEvents};
use vos_domain::{
    thread::{json_of, SettingsChange},
    Device, DeviceId,
};

/// The open run streams by device, so that revoking a device can end them: a revoked session must not keep
/// receiving what an agent says.
#[derive(Default)]
pub struct DeviceStreams {
    open: Mutex<HashMap<DeviceId, CancellationToken>>,
}

impl DeviceStreams {
    /// A token that is cancelled when `device` is revoked.
    fn token_for(&self, device: &DeviceId) -> CancellationToken {
        self.open.lock().entry(device.clone()).or_default().child_token()
    }

    /// End every stream of this device (it was revoked).
    pub fn revoke(&self, device: &DeviceId) {
        if let Some(token) = self.open.lock().remove(device) {
            token.cancel();
        }
    }
}

#[derive(Clone, FromRef)]
struct AguiState {
    chat: Arc<ChatService>,
    streams: Arc<DeviceStreams>,
}

pub fn agui_router(chat: Arc<ChatService>) -> Router {
    agui_router_with(chat, Arc::default())
}

/// As [`agui_router`], with the registry the host calls [`DeviceStreams::revoke`] on when a device is revoked.
pub fn agui_router_with(chat: Arc<ChatService>, streams: Arc<DeviceStreams>) -> Router {
    Router::new()
        .route("/agui", post(run))
        .route("/agui/threads", post(ensure_thread))
        .route("/agui/threads/{id}", get(thread))
        .route("/agui/threads/{id}/cancel", post(cancel))
        .route("/agui/threads/{id}/settings", post(settings))
        .with_state(AguiState { chat, streams })
}

fn failure(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn chat_failure(e: &ChatError) -> Response {
    let status = match e {
        ChatError::BadInput(_) => StatusCode::UNPROCESSABLE_ENTITY,
        ChatError::NotFound(_) => StatusCode::NOT_FOUND,
        ChatError::Busy => StatusCode::CONFLICT,
        ChatError::Agent(_) => StatusCode::BAD_GATEWAY,
    };
    failure(status, &e.to_string())
}

/// The body as `T`, or a `422` that says what is wrong with it.
fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes, what: &str) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("invalid {what}: {e}"))
}

/// The events of one run as they are streamed; dropping it (the client went away) lets the chat service
/// cancel a run that has nothing pending.
struct RunStream {
    events: RunEvents,
    chat: Arc<ChatService>,
    thread_id: String,
    run_id: String,
}

impl Stream for RunStream {
    type Item = AgUiEvent;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<AgUiEvent>> {
        self.events.poll_next_unpin(cx)
    }
}

impl Drop for RunStream {
    fn drop(&mut self) {
        let (chat, thread, run) = (self.chat.clone(), self.thread_id.clone(), self.run_id.clone());
        tokio::spawn(async move { chat.client_gone(&thread, &run).await });
    }
}

#[axum::debug_handler(state = AguiState)]
async fn run(
    State(chat): State<Arc<ChatService>>,
    State(streams): State<Arc<DeviceStreams>>,
    device: Option<Extension<Device>>,
    body: Bytes,
) -> Response {
    let input: RunAgentInput = match parse(&body, "RunAgentInput") {
        Ok(i) => i,
        Err(why) => return failure(StatusCode::UNPROCESSABLE_ENTITY, &why),
    };
    let (thread_id, run_id) = (input.thread_id.clone(), input.run_id.clone());
    let events = match chat.run(input).await {
        Ok(events) => events,
        Err(e) => return chat_failure(&e),
    };
    // a stream of a revoked device ends here; dropping it lets the chat service cancel a run nothing waits on
    let revoked = device.map_or_default(|Extension(d)| streams.token_for(&d.id));
    let stream =
        RunStream { events, chat, thread_id, run_id }.take_until(revoked.cancelled_owned()).map(|e| {
            let data = serde_json::to_string(&e)
                .unwrap_or_else(|_| r#"{"type":"RUN_ERROR","message":"could not encode an event"}"#.into());
            Ok::<_, Infallible>(Event::default().data(data))
        });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnsureThread {
    thread_id: String,
    agent_id: String,
}

async fn ensure_thread(State(chat): State<Arc<ChatService>>, body: Bytes) -> Response {
    let req: EnsureThread = match parse(&body, "thread request") {
        Ok(r) => r,
        Err(why) => return failure(StatusCode::UNPROCESSABLE_ENTITY, &why),
    };
    match chat.ensure(&req.thread_id, &req.agent_id).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => chat_failure(&e),
    }
}

async fn thread(State(chat): State<Arc<ChatService>>, Path(id): Path<String>) -> Response {
    match chat.snapshot(&id).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => chat_failure(&e),
    }
}

async fn cancel(State(chat): State<Arc<ChatService>>, Path(id): Path<String>) -> Response {
    match chat.cancel(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => chat_failure(&e),
    }
}

async fn settings(State(chat): State<Arc<ChatService>>, Path(id): Path<String>, body: Bytes) -> Response {
    let change: SettingsChange = match parse(&body, "settings change") {
        Ok(c) => c,
        Err(why) => return failure(StatusCode::UNPROCESSABLE_ENTITY, &why),
    };
    match chat.settings(&id, change).await {
        Ok(state) => Json(json_of(&state)).into_response(),
        Err(e) => chat_failure(&e),
    }
}

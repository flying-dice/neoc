//! The AG-UI protocol (<https://docs.ag-ui.com>), as the official Python SDK's `ag_ui.core` has it.
//!
//! - [`types`]: what a client sends: [`RunAgentInput`], the [`Message`]s, [`Tool`], [`Context`].
//! - [`events`]: what an agent answers with: [`Event`], one variant for each event type.
//! - [`stream`]: the events of a run, read from the bytes of its `text/event-stream` response.
//! - [`patch`]: RFC 6902 JSON Patch, for `STATE_DELTA` and `ACTIVITY_DELTA`.
//! - [`snake`]: the same values with the Python SDK's attribute names (`thread_id` for `threadId`).
//!
//! On the wire every field is camelCase, as the SDKs write it; read, a field is accepted under either name (the
//! Python models are `populate_by_name`). The crate does no I/O: the caller makes the HTTP request and hands over
//! the body.

pub mod events;
pub mod patch;
pub mod snake;
pub mod stream;
pub mod types;

pub use events::{Event, TokenUsage};
pub use patch::{PatchError, apply as apply_patch};
pub use stream::{DEFAULT_MAX_EVENT, StreamError, events as event_stream};
pub use types::{
    ContentPart, Context, FunctionCall, Message, MessageContent, PartSource, ResumeEntry, RunAgentInput,
    Tool, ToolCall,
};

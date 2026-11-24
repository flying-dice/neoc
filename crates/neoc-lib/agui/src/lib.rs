//! The AG-UI protocol, client side: `lib:agui`, over the `neoc-agui` crate.
//!
//! AG-UI (<https://docs.ag-ui.com>) is how an agent and the program in front of it talk: the program POSTs a
//! `RunAgentInput` (the conversation, the tools it offers, the state) and the agent answers with a stream of events
//! (`text/event-stream`), one JSON object each. The tables a script reads and writes have the fields of the official
//! Python SDK's `ag_ui.core` under their Python names (`thread_id`, `message_id`, `tool_call_id`); the wire is
//! camelCase, and the module translates.
//!
//! ```lua
//! local agui = require("lib:agui")
//! local run, err = agui.run("http://127.0.0.1:8000/agent", {
//!     thread_id = "t1", run_id = "r1", messages = { { id = "m1", role = "user", content = "hello" } },
//! })
//! if not run then error(tostring(err)) end
//! while true do
//!     local event, err = run:next()
//!     if err then error(tostring(err)) end
//!     if not event then break end          -- the stream ended
//!     if event.type == "TEXT_MESSAGE_CONTENT" then io.write(event.delta) end
//! end
//! ```
//!
//! The request is `vnd:hyper`'s client: the same connection, TLS and limits, read as a stream instead of a whole
//! body. What differs when the options say nothing:
//!
//! - **Redirects are not followed.** The request carries the conversation and often a credential; where it goes is
//!   the URL that was given. A 3xx is a `status` error.
//! - **It waits longer for data**: `idle_timeout` is 300 s, not 60. An agent that is thinking sends nothing.
//! - **One event is bounded** (`max_event`, 8 MiB), not the stream: a run is as long as it is. `timeout`, when
//!   given, bounds the run as a whole.
//!
//! A failure is a typed `RunError`: the kinds of `vnd:hyper`'s client, `status` (the endpoint answered with
//! something other than 2xx) and `protocol` (the answer is not an event stream, or an event in it is not an AG-UI
//! event).
//!
//! The types a script sees are written here ([`agui`]), with the Python SDK's names; the protocol itself (the wire
//! names, the event stream, JSON Patch) is `neoc-agui`'s. A value crosses between the two as JSON: an input is read
//! by `neoc-agui` under its Python names, and an event is written by `neoc_agui::snake`.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use mlua_ir::ir::Ty;
use mlua_ir::{Registry, Shape, Unknown};
use serde::{Deserialize, Serialize};

mlua_ir::unknown_reason!(
    /// Why a value an agent or its caller defines has no declared shape.
    AgentData = "data whose shape the agent or its caller decides: a state, a JSON Schema, a result, an activity's content, a JSON Patch operation, an event of the agent's own framework"
);

/// JSON whose shape the agent or its caller decides, as a field of a record. JSON `null` is `nil`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Data(pub serde_json::Value);

impl Shape for Data {
    fn shape(reg: &mut Registry) -> Ty {
        Unknown::<AgentData>::shape(reg)
    }
}

/// The content of a user message or a tool result: text, or a list of parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// The text.
    Text(String),
    /// Text and media, in order.
    Parts(Vec<agui::ContentPart>),
}

impl Shape for MessageContent {
    fn shape(reg: &mut Registry) -> Ty {
        Ty::Union { members: vec![String::shape(reg), Vec::<agui::ContentPart>::shape(reg)] }
    }
}

/// The Python SDK's name of every event type, in the SDK's order: what [`agui::Event`] must have a variant for.
#[cfg(test)]
fn wire_types() -> Vec<String> {
    neoc_agui::events::EVENT_TYPES.iter().map(|name| neoc_agui::events::screaming(name)).collect()
}

#[mlua_ir::module("lib:agui")]
pub mod agui {
    use std::collections::BTreeMap;

    use futures::StreamExt;
    use mlua::Lua;
    use mlua_ir::{LuaError, Options, Shape, Stream, Unknown};
    use neoc_agui::StreamError;
    use serde::{Deserialize, Serialize};

    use super::{AgentData, Data, MessageContent};
    use neoc_vm_core::lifecycle::Life;
    use neoc_vnd_hyper::client::{HyperError, Prepared, RequestOptions, Streamed, TimeLimit};

    /// How long a run may send nothing before it is given up, unless `idle_timeout` says otherwise.
    const DEFAULT_IDLE_SECS: f64 = 300.0;
    /// How much of an error response's body is quoted in the error, in characters.
    const ERROR_BODY: usize = 2048;

    // -----------------------------------------------------------------------------------------------------------
    // What a script sends
    // -----------------------------------------------------------------------------------------------------------

    /// The body of the POST that starts a run.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct RunAgentInput {
        /// The conversation the run belongs to.
        pub thread_id: String,
        /// This run's own id.
        pub run_id: String,
        /// The run this one continues or branches from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub parent_run_id: Option<String>,
        /// The whole conversation so far. Activity messages are not sent back to an agent: leave them out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub messages: Option<Vec<Message>>,
        /// The tools the caller offers: the agent asks for one with a tool call, the run ends, and the caller
        /// answers with a `tool` message in the next run.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub tools: Option<Vec<Tool>>,
        /// Things the agent should know, each a description and a value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub context: Option<Vec<Context>>,
        /// The state the agent starts from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub state: Option<Data>,
        /// Anything else for the agent, passed through as it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub forwarded_props: Option<Data>,
        /// The answers to the interrupts the last run ended with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub resume: Option<Vec<ResumeEntry>>,
    }

    /// The answer to one interrupt.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct ResumeEntry {
        /// The interrupt's `id`.
        pub interrupt_id: String,
        /// `resolved` or `cancelled`.
        pub status: String,
        /// The answer: what the interrupt's `response_schema` describes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub payload: Option<Data>,
        /// The sender's own annotations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub metadata: Option<Data>,
    }

    /// One message of the conversation, told apart by its `role`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "role", rename_all = "lowercase")]
    pub enum Message {
        /// Instructions from whoever built the program in front of the agent.
        Developer {
            /// The message's id.
            id: String,
            /// The text.
            content: String,
            /// Who wrote it.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            name: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// The system prompt.
        System {
            /// The message's id.
            id: String,
            /// The text.
            content: String,
            /// Who wrote it.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            name: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// What the agent said, and the tools it asked for.
        Assistant {
            /// The message's id.
            id: String,
            /// The text.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            content: Option<String>,
            /// The tools the agent asked for.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            tool_calls: Option<Vec<ToolCall>>,
            /// Which agent wrote it.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            name: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// What the person said.
        User {
            /// The message's id.
            id: String,
            /// The text, or text and media.
            content: MessageContent,
            /// Who wrote it.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            name: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// The result of a tool call.
        Tool {
            /// The message's id.
            id: String,
            /// The result.
            content: MessageContent,
            /// The call this is the result of.
            tool_call_id: String,
            /// What went wrong, when the tool failed.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            error: Option<String>,
            /// The provider's opaque form, to be sent back as it is.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            encrypted_value: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// Something the agent is doing, shown between the messages; never sent back to an agent.
        Activity {
            /// The message's id.
            id: String,
            /// What kind of activity `content` describes (`a2ui-surface`, ...).
            activity_type: String,
            /// The activity, in the shape its kind has.
            content: Data,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// The agent's reasoning.
        Reasoning {
            /// The message's id.
            id: String,
            /// The text.
            content: String,
            /// The provider's opaque form, to be sent back as it is.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            encrypted_value: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
    }

    /// One part of a message's content, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "lowercase")]
    pub enum ContentPart {
        /// Text.
        Text {
            /// The text.
            text: String,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// An image.
        Image {
            /// Where its bytes are.
            source: PartSource,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// Sound.
        Audio {
            /// Where its bytes are.
            source: PartSource,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// A video.
        Video {
            /// Where its bytes are.
            source: PartSource,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// A document.
        Document {
            /// Where its bytes are.
            source: PartSource,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// The sender's own annotations.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            metadata: Option<Data>,
        },
        /// Deprecated in the SDK for the four kinds above; still read and written.
        Binary {
            /// The media type of the bytes.
            mime_type: String,
            /// The part's id.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            id: Option<String>,
            /// Where the bytes are, when they are not inline.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            url: Option<String>,
            /// The bytes, base64.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            data: Option<String>,
            /// The name of the file they came from.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            filename: Option<String>,
        },
    }

    /// Where the bytes of a part are, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "lowercase")]
    pub enum PartSource {
        /// Inline.
        Data {
            /// The bytes, base64.
            value: String,
            /// Their media type.
            mime_type: String,
        },
        /// At a URL.
        Url {
            /// The URL.
            value: String,
            /// The media type of what is there.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            mime_type: Option<String>,
        },
        /// A file the provider already holds.
        File {
            /// The provider's id of the file.
            value: String,
            /// Which provider.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            provider: Option<String>,
            /// The file's media type.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            mime_type: Option<String>,
        },
    }

    /// A tool the agent asked for.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct ToolCall {
        /// The call's id: a `tool` message answers it by this.
        pub id: String,
        /// Always `function`; that when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub r#type: Option<String>,
        /// The tool asked for, and its arguments.
        pub function: FunctionCall,
        /// The provider's opaque form, to be sent back as it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub encrypted_value: Option<String>,
        /// The sender's own annotations.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub metadata: Option<Data>,
    }

    /// The tool a call names, and what it is called with.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct FunctionCall {
        /// The tool's name.
        pub name: String,
        /// The arguments, as JSON text.
        pub arguments: String,
    }

    /// Something the agent should know.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct Context {
        /// What the value is.
        pub description: String,
        /// The value.
        pub value: String,
    }

    /// A tool the caller offers.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct Tool {
        /// The tool's name.
        pub name: String,
        /// What it does, for the model.
        pub description: String,
        /// The JSON Schema of its arguments; an object with no properties when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub parameters: Option<Data>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // What an agent answers with
    // -----------------------------------------------------------------------------------------------------------

    /// The tokens one model call of a run used.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct TokenUsage {
        /// Whose model it was.
        #[serde(default)]
        pub provider: Option<String>,
        /// The model.
        #[serde(default)]
        pub model: Option<String>,
        /// The tokens read.
        #[serde(default)]
        pub input_tokens: Option<u64>,
        /// The tokens written.
        #[serde(default)]
        pub output_tokens: Option<u64>,
        /// Both together.
        #[serde(default)]
        pub total_tokens: Option<u64>,
        /// The tokens of the output that were reasoning.
        #[serde(default)]
        pub reasoning_tokens: Option<u64>,
        /// The tokens of the input that were read from a cache.
        #[serde(default)]
        pub cached_input_tokens: Option<u64>,
        /// The tokens of the input that were written to a cache.
        #[serde(default)]
        pub cache_write_input_tokens: Option<u64>,
    }

    /// A question a run stopped at: the next run answers it with a `ResumeEntry`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct Interrupt {
        /// What a resume entry's `interrupt_id` names.
        pub id: String,
        /// Why the run stopped, in the agent's own word (`tool_approval`, ...).
        #[serde(default)]
        pub reason: Option<String>,
        /// The question, for the person.
        #[serde(default)]
        pub message: Option<String>,
        /// The tool call the question is about.
        #[serde(default)]
        pub tool_call_id: Option<String>,
        /// The JSON Schema the answer's `payload` must fit.
        #[serde(default)]
        pub response_schema: Option<Data>,
        /// When the question stops being answerable.
        #[serde(default)]
        pub expires_at: Option<String>,
        /// The sender's own annotations.
        #[serde(default)]
        pub metadata: Option<Data>,
        /// The subagent that asked.
        #[serde(default)]
        pub subagent_run_id: Option<String>,
    }

    /// How a run ended.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct RunOutcome {
        /// `success`, or `interrupt`.
        pub r#type: String,
        /// The questions the run stopped at: the next run answers every one of them in its input's `resume`.
        #[serde(default)]
        pub interrupts: Option<Vec<Interrupt>>,
    }

    /// How a subagent ended.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct SubagentOutcome {
        /// `success`, or `suspended`.
        pub r#type: String,
        /// The interrupts of the run it is suspended at.
        #[serde(default)]
        pub interrupt_ids: Option<Vec<String>>,
    }

    /// The event enum: each variant gets the fields every event has (the SDK's `BaseEvent`) after its own.
    macro_rules! events {
        ($( $(#[$meta:meta])* $name:ident { $( $(#[$fmeta:meta])* $field:ident : $ty:ty, )* } )*) => {
            /// One event of a run, told apart by its `type`. Data inside an event (`snapshot`, `content`,
            /// `raw_event`, `value`) is as the agent sent it, with JSON `null` as `nil`.
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
            #[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
            #[shape(module = "lib:agui")]
            pub enum Event {
                $(
                    $(#[$meta])*
                    $name {
                        $( $(#[$fmeta])* $field: $ty, )*
                        /// When the event was made, in milliseconds since the epoch.
                        #[serde(default)]
                        timestamp: Option<i64>,
                        /// The event of the agent's own framework this one was made from.
                        #[serde(default)]
                        raw_event: Option<Data>,
                        /// The sender's own annotations.
                        #[serde(default)]
                        metadata: Option<Data>,
                        /// The subagent run the event belongs to, when it is not the run's own.
                        #[serde(default)]
                        subagent_run_id: Option<String>,
                    },
                )*
            }
        };
    }

    events! {
        /// A message of the agent begins.
        TextMessageStart {
            /// The message.
            message_id: String,
            /// `assistant` unless the agent says otherwise.
            role: String,
            /// The author's name.
            #[serde(default)]
            name: Option<String>,
        }
        /// The next piece of a message's text.
        TextMessageContent {
            /// The message.
            message_id: String,
            /// The piece.
            delta: String,
        }
        /// A message is complete.
        TextMessageEnd {
            /// The message.
            message_id: String,
        }
        /// A start, a content and an end in one: the first chunk of a message carries its id.
        TextMessageChunk {
            /// The message, on its first chunk.
            #[serde(default)]
            message_id: Option<String>,
            /// The author's role, on the first chunk.
            #[serde(default)]
            role: Option<String>,
            /// The next piece of text.
            #[serde(default)]
            delta: Option<String>,
            /// The author's name.
            #[serde(default)]
            name: Option<String>,
        }
        /// The agent asks for a tool.
        ToolCallStart {
            /// The call.
            tool_call_id: String,
            /// The tool asked for.
            tool_call_name: String,
            /// The assistant message the call belongs to.
            #[serde(default)]
            parent_message_id: Option<String>,
        }
        /// A piece of a call's arguments, which are JSON text once joined.
        ToolCallArgs {
            /// The call.
            tool_call_id: String,
            /// The piece.
            delta: String,
        }
        /// A call's arguments are complete.
        ToolCallEnd {
            /// The call.
            tool_call_id: String,
        }
        /// A start, arguments and an end in one: the first chunk of a call carries its id and the tool's name.
        ToolCallChunk {
            /// The call, on its first chunk.
            #[serde(default)]
            tool_call_id: Option<String>,
            /// The tool asked for, on the first chunk.
            #[serde(default)]
            tool_call_name: Option<String>,
            /// The assistant message the call belongs to.
            #[serde(default)]
            parent_message_id: Option<String>,
            /// The next piece of the arguments.
            #[serde(default)]
            delta: Option<String>,
        }
        /// The result of a tool the agent ran itself.
        ToolCallResult {
            /// The `tool` message the result is.
            message_id: String,
            /// The call.
            tool_call_id: String,
            /// The result, as text.
            content: String,
            /// `tool`, when given.
            #[serde(default)]
            role: Option<String>,
        }
        /// The whole state.
        StateSnapshot {
            /// The state.
            snapshot: Option<Data>,
        }
        /// A change to the state.
        StateDelta {
            /// The JSON Patch (`apply_patch`).
            delta: Vec<Data>,
        }
        /// The whole conversation as the agent has it.
        MessagesSnapshot {
            /// The messages.
            messages: Vec<Message>,
        }
        /// An activity message, whole.
        ActivitySnapshot {
            /// The activity message.
            message_id: String,
            /// What kind of activity it is.
            activity_type: String,
            /// The activity, in the shape its kind has.
            content: Option<Data>,
            /// `false` leaves an activity message that exists already as it is.
            replace: bool,
        }
        /// A change to the content of an activity message.
        ActivityDelta {
            /// The activity message.
            message_id: String,
            /// What kind of activity it is.
            activity_type: String,
            /// The JSON Patch (`apply_patch`).
            patch: Vec<Data>,
        }
        /// An event of the agent's own framework.
        Raw {
            /// The event.
            event: Option<Data>,
            /// Where it came from.
            #[serde(default)]
            source: Option<String>,
        }
        /// An event of the application's own.
        Custom {
            /// The event's name.
            name: String,
            /// Its payload.
            #[serde(default)]
            value: Option<Data>,
        }
        /// The run began.
        RunStarted {
            /// The conversation.
            thread_id: String,
            /// The run.
            run_id: String,
            /// The version of the protocol the agent speaks.
            #[serde(default)]
            protocol_version: Option<String>,
            /// The run this one continues or branches from.
            #[serde(default)]
            parent_run_id: Option<String>,
            /// The input as the agent read it.
            #[serde(default)]
            input: Option<RunAgentInput>,
        }
        /// The run ended as the agent meant it to.
        RunFinished {
            /// The conversation.
            #[serde(default)]
            thread_id: Option<String>,
            /// The run.
            #[serde(default)]
            run_id: Option<String>,
            /// What the run produced.
            #[serde(default)]
            result: Option<Data>,
            /// How it ended; success when there is none.
            #[serde(default)]
            outcome: Option<RunOutcome>,
            /// The tokens each model call used.
            #[serde(default)]
            usage: Option<Vec<TokenUsage>>,
        }
        /// The run failed.
        RunError {
            /// What went wrong.
            message: String,
            /// The agent's own code for it.
            #[serde(default)]
            code: Option<String>,
            /// The tokens each model call used.
            #[serde(default)]
            usage: Option<Vec<TokenUsage>>,
        }
        /// A step of the run began.
        StepStarted {
            /// The step.
            step_name: String,
        }
        /// A step of the run ended.
        StepFinished {
            /// The step.
            step_name: String,
        }
        /// The agent begins to reason.
        ReasoningStart {
            /// The reasoning message.
            message_id: String,
        }
        /// A reasoning message begins.
        ReasoningMessageStart {
            /// The reasoning message.
            message_id: String,
            /// `reasoning` unless the agent says otherwise.
            role: String,
        }
        /// The next piece of the reasoning's text.
        ReasoningMessageContent {
            /// The reasoning message.
            message_id: String,
            /// The piece.
            delta: String,
        }
        /// A reasoning message is complete.
        ReasoningMessageEnd {
            /// The reasoning message.
            message_id: String,
        }
        /// A start, a content and an end of reasoning in one.
        ReasoningMessageChunk {
            /// The reasoning message, on its first chunk.
            #[serde(default)]
            message_id: Option<String>,
            /// The next piece of text.
            #[serde(default)]
            delta: Option<String>,
        }
        /// The agent stops reasoning.
        ReasoningEnd {
            /// The reasoning message.
            message_id: String,
        }
        /// The provider's opaque form of a message's or a tool call's reasoning.
        ReasoningEncryptedValue {
            /// `message` or `tool-call`: what `entity_id` names.
            subtype: String,
            /// The message or the tool call.
            entity_id: String,
            /// The opaque form, to be sent back as it is.
            encrypted_value: String,
        }
        /// A subagent began.
        SubagentStarted {
            /// The subagent's name.
            name: String,
            /// What it is for.
            #[serde(default)]
            description: Option<String>,
            /// The subagent that started this one.
            #[serde(default)]
            parent_subagent_run_id: Option<String>,
            /// The tool call that started it.
            #[serde(default)]
            parent_tool_call_id: Option<String>,
            /// The assistant message it belongs to.
            #[serde(default)]
            parent_message_id: Option<String>,
        }
        /// A subagent ended as it meant to.
        SubagentFinished {
            /// What it produced.
            #[serde(default)]
            result: Option<Data>,
            /// How it ended.
            #[serde(default)]
            outcome: Option<SubagentOutcome>,
        }
        /// A subagent failed.
        SubagentError {
            /// What went wrong.
            message: String,
            /// The agent's own code for it.
            #[serde(default)]
            code: Option<String>,
        }
        /// Deprecated in the SDK for `REASONING_START`; agents still send it.
        ThinkingStart {
            /// What the thinking is about.
            #[serde(default)]
            title: Option<String>,
        }
        /// Deprecated in the SDK for `REASONING_END`.
        ThinkingEnd {}
        /// Deprecated in the SDK for `REASONING_MESSAGE_START`.
        ThinkingTextMessageStart {}
        /// Deprecated in the SDK for `REASONING_MESSAGE_CONTENT`.
        ThinkingTextMessageContent {
            /// The next piece of text.
            delta: String,
        }
        /// Deprecated in the SDK for `REASONING_MESSAGE_END`.
        ThinkingTextMessageEnd {}
    }

    // -----------------------------------------------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------------------------------------------

    /// The start of an error response's body, for the message: `: <text>`, or nothing.
    fn said(body: &str) -> String {
        if body.is_empty() { String::new() } else { format!(": {body}") }
    }

    /// Why a run could not start, or ended before the agent ended it. The transport's kinds mean what they mean in
    /// `vnd:hyper`; `reason` is for a person or a log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum RunError {
        /// An option, a header or the input was refused before anything was sent.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// The URL is not an `http` or `https` URL with a host.
        #[error("{reason}")]
        InvalidUrl {
            /// What is wrong with it.
            reason: String,
        },
        /// No connection could be made: the name did not resolve, or the host refused or could not be reached.
        #[error("{reason}")]
        Connect {
            /// What the transport said.
            reason: String,
        },
        /// The TLS handshake failed, usually over the certificate.
        #[error("{reason}")]
        Tls {
            /// What the handshake said.
            reason: String,
        },
        /// A time limit passed: connecting, no data arriving, or the whole run.
        #[error("{reason}")]
        TimedOut {
            /// Which limit.
            limit: TimeLimit,
            /// The limit and how long it was.
            reason: String,
        },
        /// One event is longer than `max_event`.
        #[error("an event is longer than max_event ({max_event} bytes)")]
        TooLarge {
            /// The limit that was passed, in bytes.
            max_event: u64,
        },
        /// The connection ended or failed before the stream did.
        #[error("{reason}")]
        ConnectionLost {
            /// What the transport said.
            reason: String,
        },
        /// The endpoint answered with something that is not valid HTTP.
        #[error("{reason}")]
        BadResponse {
            /// What the parser said.
            reason: String,
        },
        /// The endpoint answered, and not with 2xx (a 3xx too: redirects are not followed).
        #[error("the endpoint answered {status}{}", said(.body))]
        Status {
            /// The status code.
            status: u16,
            /// The start of the response's body, as text.
            body: String,
        },
        /// The answer is not an event stream, or an event in it is not an AG-UI event (a field missing or of the
        /// wrong type).
        #[error("{reason}")]
        Protocol {
            /// What is wrong with it.
            reason: String,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl RunError {
        fn protocol(reason: impl Into<String>) -> Self {
            Self::Protocol { reason: reason.into() }
        }
    }

    impl From<HyperError> for RunError {
        fn from(e: HyperError) -> Self {
            match e {
                HyperError::InvalidInput { reason } => Self::InvalidInput { reason },
                HyperError::InvalidUrl { reason } => Self::InvalidUrl { reason },
                HyperError::Connect { reason } => Self::Connect { reason },
                HyperError::Tls { reason } => Self::Tls { reason },
                HyperError::TimedOut { limit, reason } => Self::TimedOut { limit, reason },
                HyperError::ConnectionLost { reason } => Self::ConnectionLost { reason },
                HyperError::BadResponse { reason } => Self::BadResponse { reason },
                HyperError::Other { reason } => Self::Other { reason },
                // a run follows no redirect and reads no whole body: neither limit is ever passed
                other @ (HyperError::TooManyRedirects { .. } | HyperError::TooLarge { .. }) => {
                    Self::Other { reason: other.to_string() }
                }
            }
        }
    }

    impl From<StreamError<HyperError>> for RunError {
        fn from(e: StreamError<HyperError>) -> Self {
            match e {
                StreamError::Transport(e) => e.into(),
                StreamError::TooLarge(max_event) => Self::TooLarge { max_event: max_event as u64 },
                StreamError::Framing(_) | StreamError::Event(_) => Self::protocol(e.to_string()),
            }
        }
    }

    /// Why a JSON Patch was not applied.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum PatchError {
        /// The patch is not a list of RFC 6902 operations.
        #[error("{reason}")]
        InvalidPatch {
            /// What is wrong with it.
            reason: String,
        },
        /// An operation could not be applied: a path that is not there, a failed `test`.
        #[error("{reason}")]
        Failed {
            /// The operation and why.
            reason: String,
        },
        /// The patch has more operations than one may, or would grow the document past what one may be.
        #[error("{reason}")]
        TooLarge {
            /// Which limit.
            reason: String,
        },
    }

    impl From<neoc_agui::PatchError> for PatchError {
        fn from(e: neoc_agui::PatchError) -> Self {
            let reason = e.to_string();
            match e {
                neoc_agui::PatchError::Invalid(_) => Self::InvalidPatch { reason },
                neoc_agui::PatchError::Failed(_) => Self::Failed { reason },
                neoc_agui::PatchError::TooLarge(_) => Self::TooLarge { reason },
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // A run
    // -----------------------------------------------------------------------------------------------------------

    /// What a run's request sends besides the input, and the limits it keeps to.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    #[serde(default, deny_unknown_fields)]
    pub struct RunOptions {
        /// Headers to add to the request (`Authorization` for an endpoint that wants a token).
        /// `Content-Type: application/json` and `Accept: text/event-stream` are sent unless given.
        pub headers: BTreeMap<String, String>,
        /// PEM certificates to trust instead of the platform's.
        pub ca_pem: Option<String>,
        /// Seconds the TCP connect and the TLS handshake may take.
        pub connect_timeout: f64,
        /// Seconds without data: an agent that is thinking sends nothing, so this is longer than `vnd:hyper`'s.
        pub idle_timeout: f64,
        /// Seconds the whole run may take. No limit when not given.
        pub timeout: Option<f64>,
        /// The most bytes one event may take; a longer one ends the run with `too_large`. It bounds one event,
        /// not the run.
        pub max_event: u64,
    }

    impl Default for RunOptions {
        fn default() -> Self {
            let request = RequestOptions::default();
            Self {
                headers: BTreeMap::new(),
                ca_pem: None,
                connect_timeout: request.connect_timeout,
                idle_timeout: DEFAULT_IDLE_SECS,
                timeout: None,
                max_event: neoc_agui::DEFAULT_MAX_EVENT as u64,
            }
        }
    }

    /// The request a run is: `input` as the wire has it, POSTed to `url`.
    fn prepared(url: String, input: &RunAgentInput, opts: RunOptions) -> Result<(Prepared, usize), RunError> {
        let invalid = |reason: String| RunError::InvalidInput { reason };
        if opts.max_event == 0 {
            return Err(invalid("max_event must be a positive whole number of bytes, got 0".to_owned()));
        }
        let max_event = usize::try_from(opts.max_event).unwrap_or(usize::MAX);
        // read by the protocol's own type under its Python names, and written under the wire's
        let wire: neoc_agui::RunAgentInput = serde_json::to_value(input)
            .and_then(serde_json::from_value)
            .map_err(|e| invalid(format!("input: {e}")))?;
        let body = serde_json::to_vec(&wire).map_err(|e| invalid(format!("input: {e}")))?;

        // ours unless the caller set them (a name given twice in any case is refused by `vnd:hyper`)
        let mut headers = opts.headers;
        let has = |headers: &BTreeMap<String, String>, name: &str| {
            headers.keys().any(|given| given.eq_ignore_ascii_case(name))
        };
        if !has(&headers, "content-type") {
            headers.insert("content-type".to_owned(), "application/json".to_owned());
        }
        if !has(&headers, "accept") {
            headers.insert("accept".to_owned(), "text/event-stream".to_owned());
        }
        let request = RequestOptions {
            headers,
            ca_pem: opts.ca_pem,
            connect_timeout: opts.connect_timeout,
            idle_timeout: opts.idle_timeout,
            timeout: opts.timeout,
            follow_redirects: false,
            ..RequestOptions::default()
        };
        Ok((Prepared::new("POST", url, Some(body), request)?, max_event))
    }

    /// The start of an error response's body, as text.
    async fn quote(streamed: &mut Streamed) -> String {
        let mut body = Vec::new();
        while body.len() < ERROR_BODY {
            match streamed.chunk().await {
                Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                _ => break,
            }
        }
        let text = String::from_utf8_lossy(&body);
        let text: String = text.chars().take(ERROR_BODY).collect();
        text.trim().to_owned()
    }

    /// An event of the protocol as a script reads it: the Python SDK's names.
    fn script_event(event: &neoc_agui::Event) -> Result<Event, RunError> {
        neoc_agui::snake::to_value(event)
            .and_then(serde_json::from_value)
            .map_err(|e| RunError::protocol(format!("not an AG-UI event: {e}")))
    }

    /// Start a run: POST `input` to the agent's AG-UI endpoint at `url` (`http` or `https`) and read its answer as
    /// a stream of events. Returns when the endpoint has answered: a stream whose `next()` gives the events.
    ///
    /// `next()` is `(event, nil)`, `(nil, nil)` when the stream has ended (the agent closed it, which after a
    /// `RUN_FINISHED` or a `RUN_ERROR` is the end of the run and before one is a run cut short), or `(nil, err)`
    /// when the stream failed (the connection was lost, nothing arrived for `idle_timeout`, an event is not an
    /// AG-UI event or is over `max_event`); nothing follows an error. An event of a type the module does not know
    /// is skipped. `close()` stops the run: the connection closes, which is how an agent is told to stop, and
    /// `next()` returns `(nil, nil)` from then on.
    ///
    /// Redirects are not followed: a 3xx, like any answer that is not 2xx, is a `status` error that quotes the
    /// start of the body. An answer that is not `text/event-stream` is `protocol`. The connection belongs to the
    /// script: it closes when the run is closed or collected and when the script stops.
    ///
    /// ```luau
    /// local run, err = agui.run("http://127.0.0.1:1/agent", {
    ///     thread_id = "t1",
    ///     run_id = "r1",
    ///     messages = { { id = "m1", role = "user", content = "hello" } },
    /// }, { headers = { authorization = "Bearer token" }, connect_timeout = 2 })
    /// if run == nil then
    ///     assert(err ~= nil and (err.kind == "connect" or err.kind == "timed_out"))
    /// end
    /// ```
    #[host("Wait")]
    pub async fn run(
        lua: Lua,
        url: String,
        input: RunAgentInput,
        opts: RunOptions,
    ) -> Result<Stream<Event, RunError>, RunError> {
        let (request, max_event) = prepared(url, &input, opts)?;
        let mut streamed = Streamed::open(request, Life::of(&lua)).await?;
        if !(200..300).contains(&streamed.status) {
            return Err(RunError::Status { status: streamed.status, body: quote(&mut streamed).await });
        }
        let content_type = streamed
            .headers
            .iter()
            .find_map(|(name, value)| (name == "content-type").then_some(value.as_str()))
            .unwrap_or("");
        let media = content_type.split(';').next().unwrap_or("").trim();
        if !media.eq_ignore_ascii_case("text/event-stream") {
            return Err(RunError::protocol(format!(
                "the endpoint answered with `{content_type}`, not text/event-stream"
            )));
        }
        let bytes = futures::stream::unfold(streamed, |mut streamed| async move {
            streamed.chunk().await.transpose().map(|chunk| (chunk, streamed))
        });
        // the stream ends at its first error: `event_stream` yields nothing after one
        let mut failed = false;
        let events = neoc_agui::event_stream(bytes, max_event)
            .map(|event| event.map_err(RunError::from).and_then(|event| script_event(&event)))
            .take_while(move |event| {
                let go = !failed;
                failed = event.is_err();
                std::future::ready(go)
            });
        Ok(Stream::new(events))
    }

    /// Apply a JSON Patch (RFC 6902: what `STATE_DELTA.delta` and `ACTIVITY_DELTA.patch` are) to `document` and
    /// return the new document; `document` itself is not changed. All of the patch applies or none of it. A table
    /// with no entries is an empty list; JSON `null` is `nil`, so a patch cannot store one in an array.
    ///
    /// ```luau
    /// local state = { steps = { "read" } }
    /// local next = assert(agui.apply_patch(state, { { op = "add", path = "/steps/-", value = "write" } }))
    /// local none, err = agui.apply_patch(state, { { op = "remove", path = "/missing" } })
    /// assert(none == nil and err ~= nil and err.kind == "failed")
    /// ```
    pub fn apply_patch(
        document: Unknown<AgentData>,
        patch: Vec<Unknown<AgentData>>,
    ) -> Result<Unknown<AgentData>, PatchError> {
        let mut document = document.value;
        let patch: Vec<serde_json::Value> = patch.into_iter().map(|operation| operation.value).collect();
        neoc_agui::apply_patch(&mut document, &patch)?;
        Ok(Unknown::new(document))
    }
}

#[cfg(test)]
mod tests;

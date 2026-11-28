//! The Agent Client Protocol, client side: `vnd:agent_client_protocol`, mirroring the official ACP SDK
//! (`agent-client-protocol`, its protocol v1 client role).
//!
//! ACP (<https://agentclientprotocol.com>, version 1) is how a program in front of a coding agent talks to it: the
//! program opens a session, sends a prompt, and the agent streams what it does (`session/update`), asks before it
//! acts (`session/request_permission`), and answers the prompt when the turn is over.
//!
//! The module mirrors the SDK's names: `Client.v1()` is the SDK's builder for a v1 client, `Lines` its transport of
//! one JSON-RPC message per line, the connection a `ConnectionTo` with `send_request` and `send_notification`,
//! `build_session` and `load_session` the SDK's session builders, and a session an `ActiveSession` whose
//! `read_update` gives each `SessionMessage` the agent sends for it, in order. The transport comes from `lib:websocket`
//! (one message per text frame, as `acpd` serves `GET /acp`): this module dials nothing.
//!
//! ```lua
//! local acp = require("vnd:agent_client_protocol")
//! local websocket = require("lib:websocket")
//! local ws = assert(websocket.connect("ws://127.0.0.1:3000/acp"))
//! local cx = assert(acp.Client.v1():name("my-client"):connect(acp.Lines.new(ws)))
//! local init = assert(cx:send_request({ protocol_version = 1 }))
//! local session = assert(cx:build_session("/home/me/project"):start_session())
//! assert(session:send_prompt("hi"))
//! while true do
//!     local message = assert(session:read_update())  -- an error the agent answers the prompt with is `(nil, err)`
//!     if message.type == "prompt_response" then break end
//!     if message.type == "update" and message.update.type == "agent_message_chunk" then
//!         local content = message.update.content
//!         if content.type == "text" then io.write(content.text) end
//!     elseif message.type == "request_permission" then
//!         session:respond_permission(message.id, { outcome = "selected", option_id = message.options[1].option_id })
//!     end
//! end
//! cx:close()
//! ws:close()
//! ```
//!
//! Where the module and the SDK differ, Luau is why. The SDK's `connect_with(transport, main_fn)` runs a closure with
//! the connection; a script holds the connection as a value instead (`connect`), which closes with `close()` or when
//! every handle to it is gone. A request the agent sends for a session (`session/request_permission`) is answered by
//! its id (`respond_permission`) rather than through the SDK's `Responder`, which a record cannot hold. A loaded
//! session's replayed history is dropped before `start_session` returns.
//!
//! The SDK's schema types are serde types whose tagged enums serde-reflection cannot trace, so `Foreign` cannot
//! type them: the tables a script reads and writes are declared here, with the schema's fields under snake_case
//! names (`session_id`, `tool_call_id`), and a value crosses to and from the SDK's type through serde. A field the
//! module does not declare is dropped, and an update of a kind it does not read (or one that is not what ACP v1
//! says it is) is given as an `other` update, as the agent sent it.
//!
//! The client offers the agent nothing of its own: no file system, no terminal. Any request the agent makes other
//! than `session/request_permission` is answered "method not found" (-32601), and a notification the module does not
//! know is ignored. A failure is a typed `AcpError`.

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

mod link;

use mlua_ir::ir::Ty;
use mlua_ir::{Registry, Shape, Unknown};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};

mlua_ir::unknown_reason!(
    /// Why a value an agent defines has no declared shape.
    AgentData = "data whose shape the agent decides: a tool's raw input or output, an update of a kind this module does not read"
);

/// JSON whose shape the agent decides, as a field of a record. JSON `null` is `nil`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Data(pub serde_json::Value);

impl Shape for Data {
    fn shape(reg: &mut Registry) -> Ty {
        Unknown::<AgentData>::shape(reg)
    }
}

/// A field ACP says is read leniently: a value that is not what it should be is taken as the default.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// A list ACP says is read leniently: an item that is not what it should be is left out.
fn lenient_items<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        _ => Vec::new(),
    };
    Ok(items.into_iter().filter_map(|item| serde_json::from_value(item).ok()).collect())
}

#[mlua_ir::module("vnd:agent_client_protocol")]
pub mod agent_client_protocol {
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{LuaError, Object, Shape};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};

    use super::{Data, lenient, lenient_items};
    use crate::link;
    use neoc_vm_core::lifecycle::Life;
    use neoc_vm_lib_websocket::websocket::WebSocket;

    // -----------------------------------------------------------------------------------------------------------
    // What the agent says of itself
    // -----------------------------------------------------------------------------------------------------------

    /// A program on one end of the connection: its name and version.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct Implementation {
        /// Its name, for programs.
        pub name: String,
        /// Its name, for people.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,
        /// Its version.
        pub version: String,
    }

    /// The kinds of content a prompt may hold besides text and resource links, which every agent takes.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Shape)]
    pub struct PromptCapabilities {
        /// `image` blocks.
        #[serde(default)]
        pub image: bool,
        /// `audio` blocks.
        #[serde(default)]
        pub audio: bool,
        /// `resource` blocks: context embedded in the prompt.
        #[serde(default)]
        pub embedded_context: bool,
    }

    /// The MCP servers a session may be given besides `stdio` ones, which every agent takes.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Shape)]
    pub struct McpCapabilities {
        /// `http` servers.
        #[serde(default)]
        pub http: bool,
        /// `sse` servers.
        #[serde(default)]
        pub sse: bool,
    }

    /// What the agent can do.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Shape)]
    pub struct AgentCapabilities {
        /// Whether it can load a session it had before (`load_session`).
        #[serde(default, deserialize_with = "lenient")]
        pub load_session: bool,
        /// What a prompt may hold.
        #[serde(default, deserialize_with = "lenient")]
        pub prompt_capabilities: PromptCapabilities,
        /// The MCP servers it can be given.
        #[serde(default, deserialize_with = "lenient")]
        pub mcp_capabilities: McpCapabilities,
    }

    /// A way to sign in to the agent.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct AuthMethod {
        /// Its id.
        pub id: String,
        /// Its name, for people.
        pub name: String,
        /// What it is.
        #[serde(default)]
        pub description: Option<String>,
    }

    /// The agent's answer to `initialize`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct InitializeResponse {
        /// The version of ACP the agent speaks: always 1 (the SDK refuses another with an `rpc` error, -32600).
        pub protocol_version: u16,
        /// What it can do.
        #[serde(default, deserialize_with = "lenient")]
        pub agent_capabilities: AgentCapabilities,
        /// Its name and version.
        #[serde(default, deserialize_with = "lenient")]
        pub agent_info: Option<Implementation>,
        /// The ways to sign in to it; none when it needs no sign-in.
        #[serde(default, deserialize_with = "lenient_items")]
        pub auth_methods: Vec<AuthMethod>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------------------------------------------

    /// A header sent to an MCP server.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct HttpHeader {
        /// The header's name.
        pub name: String,
        /// Its value.
        pub value: String,
    }

    /// An environment variable an MCP server is started with.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct EnvVariable {
        /// The variable's name.
        pub name: String,
        /// Its value.
        pub value: String,
    }

    /// An MCP server the agent connects to for the session, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum McpServer {
        /// A server at a URL, over streamable HTTP (when the agent's `mcp_capabilities.http` says it can).
        Http {
            /// The server's name, for the agent.
            name: String,
            /// Its URL.
            url: String,
            /// Headers to send it (`Authorization`).
            #[serde(default)]
            headers: Vec<HttpHeader>,
        },
        /// A server at a URL, over server-sent events (when the agent's `mcp_capabilities.sse` says it can).
        Sse {
            /// The server's name, for the agent.
            name: String,
            /// Its URL.
            url: String,
            /// Headers to send it.
            #[serde(default)]
            headers: Vec<HttpHeader>,
        },
        /// A program the agent starts on its own machine, which every agent can do. The wire has no `type` for it.
        Stdio {
            /// The server's name, for the agent.
            name: String,
            /// The program.
            command: String,
            /// Its arguments.
            #[serde(default)]
            args: Vec<String>,
            /// Its environment, besides the agent's.
            #[serde(default)]
            env: Vec<EnvVariable>,
        },
    }

    /// What `new_session` is given.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct NewSessionRequest {
        /// The folder the session works in: an absolute path on the agent's machine.
        pub cwd: String,
        /// The MCP servers the agent connects to for the session.
        #[serde(default)]
        pub mcp_servers: Vec<McpServer>,
    }

    /// What `load_session` is given.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct LoadSessionRequest {
        /// The session to load: what `new_session` answered.
        pub session_id: String,
        /// The folder the session works in: an absolute path on the agent's machine.
        pub cwd: String,
        /// The MCP servers the agent connects to for the session.
        #[serde(default)]
        pub mcp_servers: Vec<McpServer>,
    }

    /// A mode the agent can work in (`ask`, `code`, ...).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct SessionMode {
        /// Its id.
        pub id: String,
        /// Its name, for people.
        pub name: String,
        /// What it is.
        #[serde(default)]
        pub description: Option<String>,
    }

    /// The modes of a session.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct SessionModeState {
        /// The mode the session is in.
        pub current_mode_id: String,
        /// The modes it can be in.
        pub available_modes: Vec<SessionMode>,
    }

    /// The agent's answer to `new_session`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct NewSessionResponse {
        /// The session: what a prompt, a load and a cancel name.
        pub session_id: String,
        /// Its modes, when the agent has modes.
        #[serde(default, deserialize_with = "lenient")]
        pub modes: Option<SessionModeState>,
    }

    /// The agent's answer to `load_session`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct LoadSessionResponse {
        /// The session's modes, when the agent has modes.
        #[serde(default, deserialize_with = "lenient")]
        pub modes: Option<SessionModeState>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Content
    // -----------------------------------------------------------------------------------------------------------

    /// A piece of a prompt or of what the agent says, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum ContentBlock {
        /// Text (Markdown, for what the agent says).
        Text {
            /// The text.
            text: String,
        },
        /// An image (in a prompt, when the agent's `prompt_capabilities.image` says it takes one).
        Image {
            /// The bytes, base64.
            data: String,
            /// Their media type.
            mime_type: String,
            /// Where the image is from.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            uri: Option<String>,
        },
        /// Sound (in a prompt, when the agent's `prompt_capabilities.audio` says it takes it).
        Audio {
            /// The bytes, base64.
            data: String,
            /// Their media type.
            mime_type: String,
        },
        /// A reference to something the agent can read for itself.
        ResourceLink {
            /// Where it is.
            uri: String,
            /// Its name.
            name: String,
            /// Its media type.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            mime_type: Option<String>,
            /// Its name, for people.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            title: Option<String>,
            /// What it is.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            description: Option<String>,
            /// Its size, in bytes.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            size: Option<u64>,
        },
        /// Something embedded whole (in a prompt, when the agent's `prompt_capabilities.embedded_context` says it
        /// takes it).
        Resource {
            /// What is embedded.
            resource: EmbeddedResource,
        },
    }

    /// The content of an embedded resource: text or bytes.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct EmbeddedResource {
        /// Where it is from.
        pub uri: String,
        /// Its media type.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub mime_type: Option<String>,
        /// Its text, when it is text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub text: Option<String>,
        /// Its bytes, base64, when it is not text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub blob: Option<String>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Tool calls
    // -----------------------------------------------------------------------------------------------------------

    /// What kind of thing a tool call does, for choosing how to show it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum ToolKind {
        /// Reads files or data.
        Read,
        /// Changes files.
        Edit,
        /// Deletes files.
        Delete,
        /// Moves or renames files.
        Move,
        /// Searches.
        Search,
        /// Runs a command.
        Execute,
        /// Thinks or plans.
        Think,
        /// Fetches from the network.
        Fetch,
        /// Changes the session's mode.
        SwitchMode,
        /// Anything else.
        Other,
    }

    /// Where a tool call is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum ToolCallStatus {
        /// Not started: its input is still streaming, or it waits for permission.
        Pending,
        /// Running.
        InProgress,
        /// Done.
        Completed,
        /// Failed.
        Failed,
    }

    /// What a tool call produced, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum ToolCallContent {
        /// Content.
        Content {
            /// The content.
            content: ContentBlock,
        },
        /// A change to a file.
        Diff {
            /// The file: an absolute path.
            path: String,
            /// What it held; `nil` for a new file.
            #[serde(default)]
            old_text: Option<String>,
            /// What it holds.
            new_text: String,
        },
        /// A terminal the agent runs a command in.
        Terminal {
            /// The terminal.
            terminal_id: String,
        },
    }

    /// A place in a file a tool call is about.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct ToolCallLocation {
        /// The file: an absolute path.
        pub path: String,
        /// The line, from 1.
        #[serde(default)]
        pub line: Option<u64>,
    }

    /// A tool call as a permission request describes it: its id, and what is known of it.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct ToolCallUpdate {
        /// The call.
        pub tool_call_id: String,
        /// What it does, for people.
        #[serde(default)]
        pub title: Option<String>,
        /// What kind of thing it does.
        #[serde(default)]
        pub kind: Option<ToolKind>,
        /// Where it is.
        #[serde(default)]
        pub status: Option<ToolCallStatus>,
        /// What it produced.
        #[serde(default)]
        pub content: Option<Vec<ToolCallContent>>,
        /// The places in files it is about.
        #[serde(default)]
        pub locations: Option<Vec<ToolCallLocation>>,
        /// Its input, as the agent has it.
        #[serde(default)]
        pub raw_input: Option<Data>,
        /// Its output, as the agent has it.
        #[serde(default)]
        pub raw_output: Option<Data>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Plans and commands
    // -----------------------------------------------------------------------------------------------------------

    /// How much a step of a plan matters.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum PlanEntryPriority {
        /// Most.
        High,
        /// Some.
        Medium,
        /// Least.
        Low,
    }

    /// Where a step of a plan is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum PlanEntryStatus {
        /// Not started.
        Pending,
        /// Being done.
        InProgress,
        /// Done.
        Completed,
    }

    /// A step of a plan.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct PlanEntry {
        /// What the step is.
        pub content: String,
        /// How much it matters.
        pub priority: PlanEntryPriority,
        /// Where it is.
        pub status: PlanEntryStatus,
    }

    /// What a command takes after its name.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct AvailableCommandInput {
        /// A hint, for people, of what to write.
        pub hint: String,
    }

    /// A command a prompt can start with (`/review`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct AvailableCommand {
        /// Its name, without the `/`.
        pub name: String,
        /// What it does.
        pub description: String,
        /// What it takes after its name, when it takes something.
        #[serde(default)]
        pub input: Option<AvailableCommandInput>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // A turn
    // -----------------------------------------------------------------------------------------------------------

    /// One `session/update`: something the agent did or said, told apart by its `type` (the wire's
    /// `sessionUpdate`).
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum SessionUpdate {
        /// A piece of what the person said (a loaded session's history).
        UserMessageChunk {
            /// The piece.
            content: ContentBlock,
        },
        /// A piece of the agent's answer.
        AgentMessageChunk {
            /// The piece.
            content: ContentBlock,
        },
        /// A piece of the agent's reasoning.
        AgentThoughtChunk {
            /// The piece.
            content: ContentBlock,
        },
        /// The agent starts a tool call.
        ToolCall {
            /// The call: later updates name it.
            tool_call_id: String,
            /// What it does, for people.
            title: String,
            /// What kind of thing it does.
            #[serde(default)]
            kind: Option<ToolKind>,
            /// Where it is.
            #[serde(default)]
            status: Option<ToolCallStatus>,
            /// What it produced so far.
            #[serde(default)]
            content: Option<Vec<ToolCallContent>>,
            /// The places in files it is about.
            #[serde(default)]
            locations: Option<Vec<ToolCallLocation>>,
            /// Its input, as the agent has it.
            #[serde(default)]
            raw_input: Option<Data>,
            /// Its output, as the agent has it.
            #[serde(default)]
            raw_output: Option<Data>,
        },
        /// A change to a tool call: the fields it has replace the call's, and the rest stay as they were.
        ToolCallUpdate {
            /// The call.
            tool_call_id: String,
            /// What it does, for people.
            #[serde(default)]
            title: Option<String>,
            /// What kind of thing it does.
            #[serde(default)]
            kind: Option<ToolKind>,
            /// Where it is.
            #[serde(default)]
            status: Option<ToolCallStatus>,
            /// What it produced: the whole list, not an addition to it.
            #[serde(default)]
            content: Option<Vec<ToolCallContent>>,
            /// The places in files it is about.
            #[serde(default)]
            locations: Option<Vec<ToolCallLocation>>,
            /// Its input, as the agent has it.
            #[serde(default)]
            raw_input: Option<Data>,
            /// Its output, as the agent has it.
            #[serde(default)]
            raw_output: Option<Data>,
        },
        /// The agent's plan, whole: it replaces the one before.
        Plan {
            /// The steps.
            entries: Vec<PlanEntry>,
        },
        /// The commands a prompt can start with, whole.
        AvailableCommandsUpdate {
            /// The commands.
            available_commands: Vec<AvailableCommand>,
        },
        /// The session changed mode.
        CurrentModeUpdate {
            /// The mode it is in.
            current_mode_id: String,
        },
        /// An update this module does not read: a kind it does not know, or one that is not what ACP v1 says.
        Other {
            /// Its kind: the wire's `sessionUpdate`.
            session_update: String,
            /// The update as the agent sent it (camelCase).
            update: Data,
        },
    }

    /// What kind of answer a permission option is.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum PermissionOptionKind {
        /// Allow this once.
        AllowOnce,
        /// Allow this, and don't ask again.
        AllowAlways,
        /// Refuse this once.
        RejectOnce,
        /// Refuse this, and don't ask again.
        RejectAlways,
    }

    /// An answer the agent offers to a permission request.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct PermissionOption {
        /// What `respond_permission` answers with.
        pub option_id: String,
        /// The answer, for people.
        pub name: String,
        /// What kind of answer it is.
        pub kind: PermissionOptionKind,
    }

    /// The answer to a permission request, told apart by its `outcome`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "outcome", rename_all = "snake_case")]
    pub enum PermissionOutcome {
        /// One of the options the agent offered.
        Selected {
            /// The option's `option_id`.
            option_id: String,
        },
        /// None: the turn is being cancelled.
        Cancelled {},
    }

    /// Why the agent ended a turn.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum StopReason {
        /// It finished.
        EndTurn,
        /// It reached its limit of tokens.
        MaxTokens,
        /// It reached its limit of model requests in one turn.
        MaxTurnRequests,
        /// It refused to go on.
        Refusal,
        /// The turn was cancelled (`cancel`).
        Cancelled,
    }

    /// What a session's `read_update` gives, told apart by its `type`: the SDK's `SessionMessage`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum SessionMessage {
        /// A `session/update`: the agent did or said something.
        Update {
            /// What.
            update: SessionUpdate,
        },
        /// A `session/request_permission`: the agent asks before it goes on. Answer it with
        /// `respond_permission(id, outcome)`.
        RequestPermission {
            /// The request: what `respond_permission` names.
            id: String,
            /// The tool call it is about.
            tool_call: ToolCallUpdate,
            /// The answers the agent offers.
            options: Vec<PermissionOption>,
        },
        /// The answer to a prompt: that turn is over.
        PromptResponse {
            /// Why it ended.
            stop_reason: StopReason,
        },
    }

    /// What `send_request` sends: `initialize`, the first request of a connection.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct InitializeRequest {
        /// The protocol version the client speaks: 1. The SDK refuses an agent that answers with another.
        pub protocol_version: u16,
        /// Who the client is; the builder's `name` and neoc's version when not given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub client_info: Option<Implementation>,
    }

    /// What `send_notification` sends: `session/cancel`, which asks the agent to stop the session's turn.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct CancelNotification {
        /// The session.
        pub session_id: String,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------------------------------------------

    /// Why a call failed. `reason` is for a person or a log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum AcpError {
        /// An argument was refused before anything was sent, or a call came at the wrong time (a socket a transport
        /// took already, a permission request that is not waiting).
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// The peer is not an ACP v1 agent: an answer is not what ACP v1 says.
        #[error("{reason}")]
        Protocol {
            /// What is wrong.
            reason: String,
        },
        /// The request failed with a JSON-RPC error: the agent's answer, or the SDK's own (-32600 for an agent of
        /// another protocol version, -32603 for an answer that is not what ACP says it is).
        #[error("{code}: {reason}")]
        Rpc {
            /// The JSON-RPC error code (-32601 for a method the agent does not have, ...).
            code: i64,
            /// The error's message, and its data when it has some.
            reason: String,
        },
        /// The connection is closed: by `close()`, by the agent, or by the engine shutting down.
        #[error("{reason}")]
        Closed {
            /// Who closed it, or what broke it.
            reason: String,
        },
    }

    // -----------------------------------------------------------------------------------------------------------
    // The connection
    // -----------------------------------------------------------------------------------------------------------

    /// The roles of a connection, as the SDK names them: `Client.v1()` starts a v1 client.
    #[derive(Clone, Copy, Object)]
    pub struct Client;

    #[mlua_ir::object]
    impl Client {
        /// The builder of a protocol v1 client connection.
        pub fn v1() -> Builder {
            Builder { name: None }
        }
    }

    /// A client connection being set up: the SDK's `Builder`.
    #[derive(Clone, Object)]
    pub struct Builder {
        name: Option<String>,
    }

    #[mlua_ir::object]
    impl Builder {
        /// The same builder, with a name: the connection's, and the client's in `initialize` unless the request
        /// says who it is.
        pub fn name(&self, name: String) -> Builder {
            Builder { name: Some(name) }
        }

        /// Start the connection over `transport`: the Luau form of the SDK's `connect_with`. The connection runs
        /// until `close()`, until every handle to it is gone, until the agent hangs up, or until the script stops.
        /// Nothing is sent: the first request is `send_request(InitializeRequest)`.
        pub async fn connect(&self, lua: Lua, transport: Lines) -> Result<ConnectionTo, AcpError> {
            let lines = transport.lines.lock().take().ok_or_else(|| AcpError::InvalidInput {
                reason: "the transport was used already".to_owned(),
            })?;
            let name = self.name.clone().unwrap_or_else(|| "neoc".to_owned());
            let link = link::Link::start(&Life::of(&lua), lines, name).ready().await?;
            Ok(ConnectionTo { link: Arc::new(link) })
        }
    }

    /// A transport of one JSON-RPC message per line: the SDK's `Lines`.
    #[derive(Clone, Object)]
    pub struct Lines {
        lines: Arc<Mutex<Option<neoc_vm_lib_websocket::Lines>>>,
    }

    #[mlua_ir::object]
    impl Lines {
        /// The lines of a `lib:websocket` socket: one per text frame, as `acpd` serves `/acp`. The socket is the
        /// transport's from then on: the script's `receive` on it gives `closed`, and `close` still closes it.
        pub fn new(socket: WebSocket) -> Result<Lines, AcpError> {
            let lines = socket.take_lines().ok_or_else(|| AcpError::InvalidInput {
                reason: "the socket is being read, or a transport has it already".to_owned(),
            })?;
            Ok(Lines { lines: Arc::new(Mutex::new(Some(lines))) })
        }
    }

    /// A connection to an agent: the SDK's `ConnectionTo<Agent>`. Copies of the handle are the same connection.
    #[derive(Clone, Object)]
    pub struct ConnectionTo {
        link: Arc<link::Link>,
    }

    #[mlua_ir::object]
    impl ConnectionTo {
        /// Send `initialize` and wait for the agent's answer: its version, what it can do, its name. The client
        /// offers no file system and no terminal.
        pub async fn send_request(&self, request: InitializeRequest) -> Result<InitializeResponse, AcpError> {
            self.link.initialize(request).await
        }

        /// Send `session/cancel`. The session's permission requests that wait are answered `cancelled` first, as
        /// ACP asks; the agent then answers the prompt with `stop_reason = "cancelled"` when it has stopped.
        pub fn send_notification(&self, notification: CancelNotification) -> Result<bool, AcpError> {
            self.link.cancel(&notification.session_id).map(|()| true)
        }

        /// A new session (`session/new`) in `cwd`, an absolute path on the agent's machine.
        pub fn build_session(&self, cwd: String) -> SessionBuilder {
            SessionBuilder { link: Arc::clone(&self.link), cwd, load: None, mcp_servers: Vec::new() }
        }

        /// A session the agent had before (`session/load`), when its `agent_capabilities.load_session` says it
        /// can.
        pub fn load_session(&self, session_id: String, cwd: String) -> SessionBuilder {
            SessionBuilder { link: Arc::clone(&self.link), cwd, load: Some(session_id), mcp_servers: Vec::new() }
        }

        /// Close the connection. Calls in flight end with a `closed` error. Closing again does nothing. The socket
        /// closes when its own last handle goes, or with its `close()`.
        pub fn close(&self) {
            self.link.close();
        }
    }

    /// A session being set up: the SDK's `SessionBuilder`.
    #[derive(Clone, Object)]
    pub struct SessionBuilder {
        link: Arc<link::Link>,
        cwd: String,
        load: Option<String>,
        mcp_servers: Vec<McpServer>,
    }

    #[mlua_ir::object]
    impl SessionBuilder {
        /// The same builder, with one more MCP server for the agent to connect to in the session.
        pub fn with_mcp_server(&self, server: McpServer) -> SessionBuilder {
            let mut next = self.clone();
            next.mcp_servers.push(server);
            next
        }

        /// Send `session/new` (or `session/load`) and wait for the agent's answer: the session. What the agent sends
        /// for it from then on is read with `read_update`.
        pub async fn start_session(&self) -> Result<ActiveSession, AcpError> {
            let session = match &self.load {
                None => {
                    let request = NewSessionRequest { cwd: self.cwd.clone(), mcp_servers: self.mcp_servers.clone() };
                    self.link.new_session(&request).await?
                }
                Some(session_id) => {
                    let request = LoadSessionRequest {
                        session_id: session_id.clone(),
                        cwd: self.cwd.clone(),
                        mcp_servers: self.mcp_servers.clone(),
                    };
                    self.link.load_session(&request).await?
                }
            };
            Ok(ActiveSession { session: Arc::new(session) })
        }
    }

    /// A session on a connection: the SDK's `ActiveSession`. Copies of the handle are the same session; when the
    /// last one goes, the permission requests that wait are answered `cancelled`.
    #[derive(Clone, Object)]
    pub struct ActiveSession {
        session: Arc<link::Session>,
    }

    #[mlua_ir::object]
    impl ActiveSession {
        /// The session's id.
        #[get]
        pub fn session_id(&self) -> String {
            self.session.id.clone()
        }

        /// The session's modes, when the agent has modes.
        #[get]
        pub fn modes(&self) -> Option<SessionModeState> {
            self.session.modes.clone()
        }

        /// Send `text` as a prompt (`session/prompt`). What the agent does is read with `read_update`, up to the
        /// `prompt_response` that ends the turn.
        pub fn send_prompt(&self, text: String) -> Result<bool, AcpError> {
            self.session.send_prompt(vec![ContentBlock::Text { text }]).map(|()| true)
        }

        /// Send a prompt made of content blocks. Content other than text and resource links needs the matching
        /// entry of the agent's `prompt_capabilities`.
        pub fn send_prompt_blocks(&self, prompt: Vec<ContentBlock>) -> Result<bool, AcpError> {
            self.session.send_prompt(prompt).map(|()| true)
        }

        /// The next message the agent sent for the session, in order: `(message, nil)`, or `(nil, err)` when the
        /// agent answered a prompt with an error (the session goes on) or the connection closed.
        pub async fn read_update(&self) -> Result<SessionMessage, AcpError> {
            self.session.read_update().await
        }

        /// Answer the permission request `id` (a `request_permission` message's `id`) with `outcome`: one of the
        /// options the agent offered, or `cancelled`. A request is answered once; an `id` that is not waiting is
        /// `invalid_input`.
        pub fn respond_permission(&self, id: String, outcome: PermissionOutcome) -> Result<bool, AcpError> {
            self.session.respond(&id, &outcome).map(|()| true)
        }
    }
}

#[cfg(test)]
mod tests;

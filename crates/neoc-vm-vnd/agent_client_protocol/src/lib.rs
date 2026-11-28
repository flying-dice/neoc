//! The Agent Client Protocol, client side: `vnd:agent_client_protocol`, over the official ACP SDK
//! (`agent-client-protocol`, its protocol v1 client role).
//!
//! ACP (<https://agentclientprotocol.com>, version 1) is how a program in front of a coding agent talks to it: the
//! program opens a session, sends a prompt, and the agent streams what it does (`session/update`), asks before it
//! acts (`session/request_permission`), and answers the prompt when the turn is over. This module connects to an
//! agent over a WebSocket, one JSON-RPC message per text frame, as `acpd` serves it (`GET /acp`, `initialize` first,
//! one agent process per socket), and hands the socket to the SDK as its `Lines` transport: the SDK does the
//! JSON-RPC, the protocol guard and the routing of each session's messages. `connect_lines` hands the SDK the
//! script's own `read` and `write` functions as that transport instead, one message per call (an agent on a
//! container's standard streams, say), and everything after the transport is the same.
//!
//! ```lua
//! local acp = require("vnd:agent_client_protocol")
//! local conn = assert(acp.connect("ws://127.0.0.1:3000/acp"))
//! local session = assert(conn:new_session({ cwd = "/home/me/project", mcp_servers = {} }))
//! local turn = assert(conn:prompt({ session_id = session.session_id, prompt = { { type = "text", text = "hi" } } }))
//! while true do
//!     local event = assert(turn:next())     -- an error ends the turn
//!     if event.type == "done" then break end
//!     if event.type == "update" and event.update.type == "agent_message_chunk" then
//!         local content = event.update.content
//!         if content.type == "text" then io.write(content.text) end
//!     elseif event.type == "permission" then
//!         turn:respond_permission(event.id, { outcome = "selected", option_id = event.options[1].option_id })
//!     end
//! end
//! conn:close()
//! ```
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

/// A list in a tagged union a script gives: serde reads the union before it knows a field's type, so a script's empty
/// table (`args = {}`) comes as a map; that is an empty list.
fn list<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::Object(fields) if fields.is_empty() => Ok(Vec::new()),
        value => serde_json::from_value(value).map_err(serde::de::Error::custom),
    }
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
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use mlua::Lua;
    use mlua_ir::ir::{Field, Ty, TypeBody};
    use mlua_ir::{
        Callback, Definition, FromLuaArg, LuaError, Object, Options, Registry, Shape, name_params,
    };
    use serde::{Deserialize, Serialize};

    use super::{Data, lenient, lenient_items, list};
    use crate::link;
    use neoc_vm_core::lifecycle::Life;

    /// How long `connect` may take, unless `timeout` says otherwise.
    const DEFAULT_TIMEOUT_SECS: f64 = 30.0;

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
            #[serde(default, deserialize_with = "list")]
            headers: Vec<HttpHeader>,
        },
        /// A server at a URL, over server-sent events (when the agent's `mcp_capabilities.sse` says it can).
        Sse {
            /// The server's name, for the agent.
            name: String,
            /// Its URL.
            url: String,
            /// Headers to send it.
            #[serde(default, deserialize_with = "list")]
            headers: Vec<HttpHeader>,
        },
        /// A program the agent starts on its own machine, which every agent can do. The wire has no `type` for it.
        Stdio {
            /// The server's name, for the agent.
            name: String,
            /// The program.
            command: String,
            /// Its arguments.
            #[serde(default, deserialize_with = "list")]
            args: Vec<String>,
            /// Its environment, besides the agent's.
            #[serde(default, deserialize_with = "list")]
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

    /// What a turn gives, told apart by its `type`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum TurnEvent {
        /// The agent did or said something.
        Update {
            /// What.
            update: SessionUpdate,
        },
        /// The agent asks before it goes on: answer with `respond_permission(id, outcome)`.
        Permission {
            /// The request: what `respond_permission` names.
            id: String,
            /// The tool call it is about.
            tool_call: ToolCallUpdate,
            /// The answers the agent offers.
            options: Vec<PermissionOption>,
        },
        /// The agent answered the prompt: the turn is over.
        Done {
            /// Why it ended.
            stop_reason: StopReason,
        },
    }

    /// What `prompt` is given.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Shape)]
    pub struct PromptRequest {
        /// The session.
        pub session_id: String,
        /// What the person says.
        pub prompt: Vec<ContentBlock>,
    }

    // -----------------------------------------------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------------------------------------------

    /// The start of an error response's body, for the message: `: <text>`, or nothing.
    fn said(body: &str) -> String {
        if body.is_empty() { String::new() } else { format!(": {body}") }
    }

    /// Why a call failed. `reason` is for a person or a log.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum AcpError {
        /// An option, a header, the URL or an argument was refused before anything was sent.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// No connection could be made: the name did not resolve, the host refused or could not be reached, or the
        /// TLS handshake failed.
        #[error("{reason}")]
        Connect {
            /// What the transport said.
            reason: String,
        },
        /// The endpoint answered the WebSocket upgrade with an HTTP status instead (`403` for an `Origin` acpd
        /// refuses, `404` for an agent it does not have).
        #[error("the endpoint answered {code}{}", said(.body))]
        Status {
            /// The status code.
            code: u16,
            /// The start of the response's body, as text.
            body: String,
        },
        /// The endpoint is not an ACP agent: the WebSocket upgrade went wrong, or an answer is not what ACP v1 says.
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
        /// `connect` took longer than its `timeout`.
        #[error("{reason}")]
        TimedOut {
            /// The limit.
            reason: String,
        },
    }

    // -----------------------------------------------------------------------------------------------------------
    // The connection
    // -----------------------------------------------------------------------------------------------------------

    /// How `connect` connects.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    #[serde(default, deny_unknown_fields)]
    pub struct ConnectOptions {
        /// Headers to add to the WebSocket upgrade (`Authorization` for an endpoint that wants a token). No `Origin`
        /// is sent unless given here.
        pub headers: BTreeMap<String, String>,
        /// Seconds the connection and the agent's answer to `initialize` may take together.
        pub timeout: f64,
        /// Who the client says it is in `initialize`; `neoc` and its version when not given.
        pub client_info: Option<Implementation>,
    }

    impl Default for ConnectOptions {
        fn default() -> Self {
            Self { headers: BTreeMap::new(), timeout: DEFAULT_TIMEOUT_SECS, client_info: None }
        }
    }

    /// Connect to the ACP agent at `url` (`ws://` or `wss://`) and initialize it: the WebSocket upgrade, then
    /// `initialize` with protocol version 1 and no client capabilities (no file system, no terminal). Returns once
    /// the agent has answered with protocol version 1; the SDK refuses any other version (an `rpc` error, -32600).
    ///
    /// The connection belongs to the script: it closes with `close()`, when the connection and every turn of it
    /// are collected, and when the script stops. Closing it is how an `acpd` agent process is stopped.
    ///
    /// ```luau
    /// local conn, err = agent_client_protocol.connect("ws://127.0.0.1:1/acp", { timeout = 2 })
    /// if conn == nil then
    ///     assert(err ~= nil and (err.kind == "connect" or err.kind == "timed_out"))
    /// end
    /// ```
    pub async fn connect(lua: Lua, url: String, opts: ConnectOptions) -> Result<Connection, AcpError> {
        let limit = time_limit(opts.timeout)?;
        let client_info = client_info_or_ours(opts.client_info);
        let life = Life::of(&lua);
        let opened = link::connect(&url, opts.headers, &client_info, &life);
        let (link, agent, local_address) =
            tokio::time::timeout(limit, opened).await.map_err(|_| AcpError::TimedOut {
                reason: format!("{url} did not connect and initialize within {}s", opts.timeout),
            })??;
        Ok(Connection { link: Arc::new(link), agent: Arc::new(agent), local_address })
    }

    /// `timeout` as the limit it sets: a positive number of seconds.
    fn time_limit(timeout: f64) -> Result<Duration, AcpError> {
        Duration::try_from_secs_f64(timeout).ok().filter(|limit| !limit.is_zero()).ok_or_else(|| {
            AcpError::InvalidInput {
                reason: format!("timeout must be a positive number of seconds, got {timeout}"),
            }
        })
    }

    /// Who the client says it is: `client_info`, or `neoc` and its version.
    fn client_info_or_ours(client_info: Option<Implementation>) -> Implementation {
        client_info.unwrap_or_else(|| Implementation {
            name: "neoc".to_owned(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        })
    }

    /// What `connect_lines` speaks ACP over: two functions of the script's own, one JSON-RPC message per call.
    /// `read()` returns the agent's next message, or `nil` once the agent's side has closed; `write(line)` sends one
    /// message to the agent. Both may wait (read a container's output, write to its input), and each is called as a
    /// call of its own while the connection runs. An error either raises ends the connection.
    pub struct LineTransport {
        read: mlua::Function,
        write: mlua::Function,
    }

    impl Shape for LineTransport {
        fn shape(reg: &mut Registry) -> Ty {
            reg.named(
                "LineTransport",
                Some("vnd:agent_client_protocol"),
                "neoc_vm_vnd_agent_client_protocol::agent_client_protocol::LineTransport",
                |reg| {
                    let read = Callback::<fn() -> Option<String>>::shape(reg);
                    let mut write = Callback::<fn(String)>::shape(reg);
                    name_params(&mut write, &["line"]);
                    let field = |name: &str, doc: &str, ty: Ty| Field {
                        name: name.to_owned(),
                        doc: Some(doc.to_owned()),
                        ty,
                        default: None,
                        readonly: false,
                    };
                    Definition {
                        doc: Some(
                            "What `connect_lines` speaks ACP over: two functions of the script's own, one JSON-RPC \
                             message per call. Both may wait."
                                .to_owned(),
                        ),
                        body: TypeBody::Record {
                            fields: vec![
                                field(
                                    "read",
                                    "The agent's next message, or `nil` once the agent's side has closed.",
                                    read,
                                ),
                                field("write", "Send one message to the agent.", write),
                            ],
                        },
                    }
                },
            )
        }
    }

    impl FromLuaArg for LineTransport {
        fn from_lua_arg(value: mlua::Value, _: &Lua) -> Result<Self, String> {
            let mlua::Value::Table(table) = value else {
                return Err(mlua_ir::expected("a table of `read` and `write` functions", &value));
            };
            let (mut read, mut write) = (None, None);
            for pair in table.pairs::<mlua::Value, mlua::Value>() {
                let (key, value) = pair.map_err(|e| e.to_string())?;
                let name = match &key {
                    mlua::Value::String(name) => name.to_string_lossy(),
                    other => return Err(format!("a {} key, expected `read` or `write`", other.type_name())),
                };
                let slot = match name.as_str() {
                    "read" => &mut read,
                    "write" => &mut write,
                    other => return Err(format!("unknown field `{other}`, expected `read` or `write`")),
                };
                match value {
                    mlua::Value::Function(function) => *slot = Some(function),
                    other => return Err(format!("{name}: {}", mlua_ir::expected("a function", &other))),
                }
            }
            match (read, write) {
                (Some(read), Some(write)) => Ok(Self { read, write }),
                (None, _) => Err("missing field `read`".to_owned()),
                (_, None) => Err("missing field `write`".to_owned()),
            }
        }
    }

    /// How `connect_lines` connects: `connect`'s options without the headers, which there is no upgrade for.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    #[serde(default, deny_unknown_fields)]
    pub struct ConnectLinesOptions {
        /// Seconds the agent's answer to `initialize` may take.
        pub timeout: f64,
        /// Who the client says it is in `initialize`; `neoc` and its version when not given.
        pub client_info: Option<Implementation>,
    }

    impl Default for ConnectLinesOptions {
        fn default() -> Self {
            Self { timeout: DEFAULT_TIMEOUT_SECS, client_info: None }
        }
    }

    /// Speak ACP to an agent over the script's own lines (`transport`), and initialize it as `connect` does: the
    /// script carries each JSON-RPC message (an agent that runs as a container's program, attached with
    /// `vnd:bollard`, say). Returns the same connection as `connect`, whose `local_address` is `""`. The agent's side
    /// closing before it answers `initialize` (`read` returns `nil`) is `closed`.
    ///
    /// The connection belongs to the script: it closes with `close()`, when the connection and every turn of it
    /// are collected, and when the script stops. Closing it stops calling `read` and `write`; the script closes what
    /// they read and write itself.
    ///
    /// ```luau
    /// local sent = {}
    /// local conn, err = agent_client_protocol.connect_lines({
    ///     read = function(): string? return nil end, -- the agent's side is closed
    ///     write = function(line: string) table.insert(sent, line) end,
    /// })
    /// assert(conn == nil and err ~= nil and err.kind == "closed")
    /// ```
    pub async fn connect_lines(
        lua: Lua,
        transport: LineTransport,
        opts: ConnectLinesOptions,
    ) -> Result<Connection, AcpError> {
        let limit = time_limit(opts.timeout)?;
        let client_info = client_info_or_ours(opts.client_info);
        let life = Life::of(&lua);
        let lines = link::script_lines(&lua, transport.read, transport.write);
        let (link, agent) = tokio::time::timeout(limit, link::run(lines, &client_info, &life))
            .await
            .map_err(|_| AcpError::TimedOut {
                reason: format!("the agent did not initialize within {}s", opts.timeout),
            })??;
        Ok(Connection { link: Arc::new(link), agent: Arc::new(agent), local_address: String::new() })
    }

    /// A connection to an agent. Copies of the handle are the same connection.
    #[derive(Clone, Object)]
    pub struct Connection {
        link: Arc<link::Link>,
        agent: Arc<InitializeResponse>,
        local_address: String,
    }

    #[mlua_ir::object]
    impl Connection {
        /// The agent's answer to `initialize`: its version, what it can do, its name.
        #[get]
        pub fn agent(&self) -> InitializeResponse {
            (*self.agent).clone()
        }

        /// The IP address of this end of the connection: the address this machine has on the way to the agent
        /// (`127.0.0.1` for an agent on this machine); `""` for a connection over the script's lines.
        #[get]
        pub fn local_address(&self) -> String {
            self.local_address.clone()
        }

        /// Start a session (`session/new`) in `cwd` on the agent's machine, with the MCP servers it connects to.
        /// What the agent sends for the session from then on is read by its turns: an update sent between two
        /// turns (the commands a new session has, say) is given by the next one.
        pub async fn new_session(&self, params: NewSessionRequest) -> Result<NewSessionResponse, AcpError> {
            self.link.new_session(&params).await
        }

        /// Load a session the agent had before (`session/load`), when its `agent_capabilities.load_session` says
        /// it can. The agent replays the session's history before it answers; that history is dropped here.
        pub async fn load_session(
            &self,
            params: LoadSessionRequest,
        ) -> Result<LoadSessionResponse, AcpError> {
            self.link.load_session(&params).await
        }

        /// Send a prompt (`session/prompt`) to a session this connection started or loaded, and return the turn at
        /// once: its `next()` gives what the agent does until it answers. A session has one turn at a time: a
        /// prompt while one is going is `invalid_input`.
        pub fn prompt(&self, params: PromptRequest) -> Result<Turn, AcpError> {
            Ok(Turn { turn: Arc::new(self.link.prompt(&params)?) })
        }

        /// Ask the agent to stop the session's turn (`session/cancel`). The permission requests the turn is
        /// waiting on are answered `cancelled` first, as ACP asks; the turn then ends with a `done` event whose
        /// `stop_reason` is `cancelled`, when the agent has stopped.
        pub fn cancel(&self, session_id: String) -> Result<(), AcpError> {
            self.link.cancel(&session_id)
        }

        /// Close the connection. Calls in flight and turns end with a `closed` error. Closing again does nothing.
        pub fn close(&self) {
            self.link.close();
        }
    }

    /// A prompt's turn: what the agent does until it answers the prompt.
    #[derive(Clone, Object)]
    pub struct Turn {
        turn: Arc<link::TurnState>,
    }

    #[mlua_ir::object]
    impl Turn {
        /// The next event of the turn: `(event, nil)`; `(nil, nil)` once the `done` event has been given; or
        /// `(nil, err)` when the turn failed (the agent answered the prompt with an error, or the connection
        /// closed), after which nothing follows.
        pub async fn next(&self) -> Result<Option<TurnEvent>, AcpError> {
            self.turn.next().await
        }

        /// Answer the permission request `id` (a `permission` event's `id`) with `outcome`: one of the options the
        /// agent offered, or `cancelled`. A request is answered once; an `id` that is not waiting is
        /// `invalid_input`.
        pub fn respond_permission(&self, id: String, outcome: PermissionOutcome) -> Result<(), AcpError> {
            self.turn.respond(&id, &outcome)
        }
    }
}

#[cfg(test)]
mod tests;

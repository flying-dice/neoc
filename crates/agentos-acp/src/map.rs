//! ACP to domain: what a `session/update` or a `session/new` answer means for a thread. Pure functions.

use agent_client_protocol::v1::schema::{
    AvailableCommandInput, ContentBlock, McpServer, NewSessionResponse, PermissionOptionKind, Plan,
    PlanEntryPriority, PlanEntryStatus, RequestPermissionRequest, SessionConfigKind, SessionConfigOption,
    SessionConfigSelectOptions, SessionNotification, SessionUpdate, StopReason as AcpStop, ToolCallContent,
    ToolCallStatus,
};
use agentos_domain::thread::{
    AgentEvent, ConfigChoice, ConfigOption, ContextUsage, Cost, Mode, ModeState, PermissionKind,
    PermissionOption, PermissionRequest, PlanEntry, PlanStatus, SessionState, SlashCommand, StopReason,
    ToolKind, ToolStatus,
};
use serde::Serialize;
use serde_json::Value;

/// The `snake_case` name serde gives a unit-like ACP enum (`SwitchMode` is `switch_mode`).
fn name_of(value: &impl Serialize) -> Option<String> {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_owned))
}

fn tool_status(s: ToolCallStatus) -> ToolStatus {
    match s {
        ToolCallStatus::InProgress => ToolStatus::InProgress,
        ToolCallStatus::Completed => ToolStatus::Completed,
        ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Pending,
    }
}

pub fn stop_reason(s: AcpStop) -> StopReason {
    match s {
        AcpStop::MaxTokens => StopReason::MaxTokens,
        AcpStop::MaxTurnRequests => StopReason::MaxTurnRequests,
        AcpStop::Refusal => StopReason::Refusal,
        AcpStop::Cancelled => StopReason::Cancelled,
        _ => StopReason::EndTurn,
    }
}

fn plan(p: &Plan) -> Vec<PlanEntry> {
    p.entries
        .iter()
        .map(|e| PlanEntry {
            content: e.content.clone(),
            status: match e.status {
                PlanEntryStatus::InProgress => PlanStatus::InProgress,
                PlanEntryStatus::Completed => PlanStatus::Completed,
                _ => PlanStatus::Pending,
            },
            priority: match e.priority {
                PlanEntryPriority::High => "high",
                PlanEntryPriority::Low => "low",
                _ => "medium",
            }
            .to_owned(),
        })
        .collect()
}

/// The text a tool produced: its text content blocks, else its raw output as JSON.
fn tool_output(content: Option<&[ToolCallContent]>, raw: Option<&Value>) -> Option<String> {
    let text: Vec<&str> = content
        .unwrap_or_default()
        .iter()
        .filter_map(|c| match c {
            ToolCallContent::Content(c) => match &c.content {
                ContentBlock::Text(t) => Some(t.text.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if !text.is_empty() {
        return Some(text.join("\n"));
    }
    match raw? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

pub fn config_options(options: &[SessionConfigOption]) -> Vec<ConfigOption> {
    options
        .iter()
        .map(|o| {
            let category = o.category.as_ref().and_then(name_of);
            match &o.kind {
                SessionConfigKind::Select(select) => {
                    let choices = match &select.options {
                        SessionConfigSelectOptions::Ungrouped(list) => list.iter().collect::<Vec<_>>(),
                        SessionConfigSelectOptions::Grouped(groups) => {
                            groups.iter().flat_map(|g| g.options.iter()).collect()
                        }
                        _ => vec![],
                    };
                    ConfigOption::Select {
                        id: o.id.to_string(),
                        name: o.name.clone(),
                        description: o.description.clone(),
                        category,
                        current: select.current_value.to_string(),
                        options: choices
                            .into_iter()
                            .map(|c| ConfigChoice {
                                value: c.value.to_string(),
                                name: c.name.clone(),
                                description: c.description.clone(),
                            })
                            .collect(),
                    }
                }
                SessionConfigKind::Boolean(b) => ConfigOption::Boolean {
                    id: o.id.to_string(),
                    name: o.name.clone(),
                    description: o.description.clone(),
                    category,
                    current: b.current_value,
                },
                _ => ConfigOption::Boolean {
                    id: o.id.to_string(),
                    name: o.name.clone(),
                    description: o.description.clone(),
                    category,
                    current: false,
                },
            }
        })
        .collect()
}

/// What `session/new` answered, as the start of a thread's session state.
pub fn session_state(resp: &NewSessionResponse, tools_connected: bool) -> SessionState {
    SessionState {
        tools_connected,
        modes: resp.modes.as_ref().map(|m| ModeState {
            current: m.current_mode_id.to_string(),
            available: m
                .available_modes
                .iter()
                .map(|a| Mode {
                    id: a.id.to_string(),
                    name: a.name.clone(),
                    description: a.description.clone(),
                })
                .collect(),
        }),
        config: resp.config_options.as_deref().map(config_options).unwrap_or_default(),
        ..SessionState::default()
    }
}

/// A `session/update` as the event it is for a thread (`None`: nothing a thread shows).
pub fn event_of(n: &SessionNotification) -> Option<AgentEvent> {
    Some(match &n.update {
        SessionUpdate::AgentMessageChunk(c) => match &c.content {
            ContentBlock::Text(t) => AgentEvent::MessageChunk(t.text.clone()),
            _ => return None,
        },
        SessionUpdate::AgentThoughtChunk(c) => match &c.content {
            ContentBlock::Text(t) => AgentEvent::ThoughtChunk(t.text.clone()),
            _ => return None,
        },
        SessionUpdate::ToolCall(t) => AgentEvent::ToolCall {
            id: t.tool_call_id.to_string(),
            title: t.title.clone(),
            kind: name_of(&t.kind),
            status: tool_status(t.status),
            raw_input: t.raw_input.clone(),
        },
        SessionUpdate::ToolCallUpdate(u) => AgentEvent::ToolUpdate {
            id: u.tool_call_id.to_string(),
            title: u.fields.title.clone(),
            kind: u.fields.kind.as_ref().and_then(name_of),
            status: u.fields.status.map(tool_status),
            output: tool_output(u.fields.content.as_deref(), u.fields.raw_output.as_ref()),
        },
        SessionUpdate::Plan(p) => AgentEvent::Plan(plan(p)),
        SessionUpdate::CurrentModeUpdate(m) => AgentEvent::Mode(m.current_mode_id.to_string()),
        SessionUpdate::ConfigOptionUpdate(c) => AgentEvent::Config(config_options(&c.config_options)),
        SessionUpdate::AvailableCommandsUpdate(c) => AgentEvent::Commands(
            c.available_commands
                .iter()
                .map(|cmd| SlashCommand {
                    name: cmd.name.clone(),
                    description: cmd.description.clone(),
                    hint: match &cmd.input {
                        Some(AvailableCommandInput::Unstructured(u)) => Some(u.hint.clone()),
                        _ => None,
                    },
                })
                .collect(),
        ),
        SessionUpdate::SessionInfoUpdate(i) => match &i.title {
            agent_client_protocol::schema::MaybeUndefined::Value(t) => AgentEvent::Title(t.clone()),
            _ => return None,
        },
        SessionUpdate::UsageUpdate(u) => AgentEvent::Usage(ContextUsage {
            used: u.used,
            size: u.size,
            cost: u.cost.as_ref().map(|c| Cost { amount: c.amount, currency: c.currency.clone() }),
        }),
        _ => return None,
    })
}

fn permission_kind(k: PermissionOptionKind) -> PermissionKind {
    match k {
        PermissionOptionKind::AllowAlways => PermissionKind::AllowAlways,
        PermissionOptionKind::RejectOnce => PermissionKind::RejectOnce,
        PermissionOptionKind::RejectAlways => PermissionKind::RejectAlways,
        PermissionOptionKind::AllowOnce => PermissionKind::AllowOnce,
        _ => PermissionKind::Unknown,
    }
}

/// An ACP permission request as the domain's, under the host's id for it.
pub fn permission_request(request_id: String, req: &RequestPermissionRequest) -> PermissionRequest {
    PermissionRequest {
        request_id,
        tool_call_id: Some(req.tool_call.tool_call_id.to_string()),
        title: req.tool_call.fields.title.clone().unwrap_or_else(|| "Run a tool".to_owned()),
        kind: req.tool_call.fields.kind.as_ref().and_then(name_of).map(|k| ToolKind::parse(&k)),
        options: req
            .options
            .iter()
            .map(|o| PermissionOption {
                option_id: o.option_id.to_string(),
                name: o.name.clone(),
                kind: permission_kind(o.kind),
            })
            .collect(),
    }
}

/// The MCP server entry that hands an agent the host's tools under its own token.
pub fn mcp_server(url: &str, token: &str) -> McpServer {
    use agent_client_protocol::v1::schema::{HttpHeader, McpServerHttp};
    McpServer::Http(
        McpServerHttp::new("agentos", url)
            .headers(vec![HttpHeader::new("Authorization", format!("Bearer {token}"))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::v1::schema::{
        AvailableCommand, AvailableCommandsUpdate, ContentChunk, SessionConfigSelect,
        SessionConfigSelectGroup, SessionConfigSelectOption, TextContent, ToolCall, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind, UsageUpdate,
    };

    fn note(update: SessionUpdate) -> SessionNotification {
        SessionNotification::new("s1", update)
    }

    #[test]
    fn text_chunks_become_message_and_reasoning_events() {
        let text = |s: &str| ContentChunk::new(ContentBlock::Text(TextContent::new(s)));
        assert_eq!(
            event_of(&note(SessionUpdate::AgentMessageChunk(text("hi")))),
            Some(AgentEvent::MessageChunk("hi".into()))
        );
        assert_eq!(
            event_of(&note(SessionUpdate::AgentThoughtChunk(text("hm")))),
            Some(AgentEvent::ThoughtChunk("hm".into()))
        );
        assert_eq!(event_of(&note(SessionUpdate::UserMessageChunk(text("echo")))), None);
    }

    #[test]
    fn a_tool_call_and_its_update_keep_title_kind_status_and_text_output() {
        let call = ToolCall::new("tc1", "mcp__agentos__notes__search")
            .kind(ToolKind::Search)
            .status(ToolCallStatus::InProgress);
        let Some(AgentEvent::ToolCall { id, title, kind, status, .. }) =
            event_of(&note(SessionUpdate::ToolCall(call)))
        else {
            panic!()
        };
        assert_eq!(
            (id.as_str(), title.as_str(), kind.as_deref()),
            ("tc1", "mcp__agentos__notes__search", Some("search"))
        );
        assert_eq!(status, ToolStatus::InProgress);

        let fields = ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .content(vec![ToolCallContent::from("2 notes")]);
        let Some(AgentEvent::ToolUpdate { status, output, title, .. }) =
            event_of(&note(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("tc1", fields))))
        else {
            panic!()
        };
        assert_eq!((status, output.as_deref(), title), (Some(ToolStatus::Completed), Some("2 notes"), None));

        let raw = ToolCallUpdateFields::new()
            .status(ToolCallStatus::Failed)
            .raw_output(serde_json::json!({"error": "boom"}));
        let Some(AgentEvent::ToolUpdate { output, .. }) =
            event_of(&note(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("tc1", raw))))
        else {
            panic!()
        };
        assert_eq!(output.as_deref(), Some(r#"{"error":"boom"}"#), "no text: the raw output");
    }

    #[test]
    fn commands_usage_and_plans_map_across() {
        let cmds = AvailableCommandsUpdate::new(vec![
            AvailableCommand::new("compact", "Compact the context"),
            AvailableCommand::new("review", "Review").input(AvailableCommandInput::Unstructured(
                agent_client_protocol::v1::schema::UnstructuredCommandInput::new("a file"),
            )),
        ]);
        let Some(AgentEvent::Commands(c)) = event_of(&note(SessionUpdate::AvailableCommandsUpdate(cmds)))
        else {
            panic!()
        };
        assert_eq!((c[0].name.as_str(), c[0].hint.clone()), ("compact", None));
        assert_eq!(c[1].hint.as_deref(), Some("a file"));

        let Some(AgentEvent::Usage(u)) =
            event_of(&note(SessionUpdate::UsageUpdate(UsageUpdate::new(10, 200))))
        else {
            panic!()
        };
        assert_eq!((u.used, u.size), (10, 200));

        let plan = Plan::new(vec![agent_client_protocol::v1::schema::PlanEntry::new(
            "do it",
            PlanEntryPriority::High,
            PlanEntryStatus::InProgress,
        )]);
        let Some(AgentEvent::Plan(p)) = event_of(&note(SessionUpdate::Plan(plan))) else { panic!() };
        assert_eq!((p[0].status, p[0].priority.as_str()), (PlanStatus::InProgress, "high"));
    }

    #[test]
    fn config_options_flatten_groups_and_keep_switches() {
        let select = SessionConfigOption::select(
            "model",
            "Model",
            "opus",
            SessionConfigSelectOptions::Grouped(vec![SessionConfigSelectGroup::new(
                "g",
                "Group",
                vec![
                    SessionConfigSelectOption::new("opus", "Opus"),
                    SessionConfigSelectOption::new("sonnet", "Sonnet"),
                ],
            )]),
        );
        let _ = SessionConfigSelect::new("opus", vec![SessionConfigSelectOption::new("opus", "Opus")]);
        let boolean = SessionConfigOption::boolean("fast", "Fast", true);
        let out = config_options(&[select, boolean]);
        let ConfigOption::Select { current, options, .. } = &out[0] else { panic!() };
        assert_eq!((current.as_str(), options.len()), ("opus", 2));
        assert!(matches!(&out[1], ConfigOption::Boolean { current: true, .. }));
    }

    #[test]
    fn a_permission_request_keeps_its_typed_kind_and_option_kinds() {
        use agent_client_protocol::v1::schema::PermissionOption as AcpOption;
        let request = |kind| {
            let fields = ToolCallUpdateFields::new().title("mcp__agentos__notes__search").kind(kind);
            RequestPermissionRequest::new(
                "s1",
                ToolCallUpdate::new("tc", fields),
                vec![
                    AcpOption::new("a", "Allow", PermissionOptionKind::AllowOnce),
                    AcpOption::new("r", "Reject", PermissionOptionKind::RejectAlways),
                ],
            )
        };
        let execute = permission_request("p1".into(), &request(ToolKind::Execute));
        assert_eq!(execute.kind, Some(agentos_domain::thread::ToolKind::Execute));
        assert!(!execute.kind_allows_auto_approval());
        let other = permission_request("p2".into(), &request(ToolKind::Other));
        assert_eq!(other.kind, Some(agentos_domain::thread::ToolKind::Other));
        assert_eq!(
            other.options.iter().map(|o| o.kind).collect::<Vec<_>>(),
            [PermissionKind::AllowOnce, PermissionKind::RejectAlways]
        );
    }

    #[test]
    fn stop_reasons_map_one_to_one() {
        assert_eq!(stop_reason(AcpStop::EndTurn), StopReason::EndTurn);
        assert_eq!(stop_reason(AcpStop::Cancelled), StopReason::Cancelled);
        assert_eq!(stop_reason(AcpStop::Refusal), StopReason::Refusal);
    }
}

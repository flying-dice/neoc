//! A scripted ACP agent over stdio for tests. No model: the prompt text picks what it does.
//!
//! | prompt   | what the agent does                                                                          |
//! |----------|----------------------------------------------------------------------------------------------|
//! | `hello`  | streams "Hi " and "there", ends the turn                                                     |
//! | `think`  | a thought chunk, then a message                                                              |
//! | `tool`   | a tool call that completes with text output, then a message                                  |
//! | `fail`   | a tool call that fails, then the prompt itself fails                                         |
//! | `ask`    | asks permission for `mcp__agentos__notes__search`, then says what the answer was             |
//! | `ask-write` | the same for `mcp__agentos__notes__append`                                                |
//! | `state`  | commands, usage, a plan, a title, and a mode update                                          |
//! | `mcp`    | says which MCP servers the session got (name, url, Authorization header)                     |
//! | `slow`   | streams chunks until cancelled, then ends with `cancelled`                                   |
//! | `exit`   | the process exits (a crash)                                                                  |
//! | other    | echoes the text                                                                              |
use agent_client_protocol::v1::schema::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, CancelNotification, ContentBlock,
    ContentChunk, CurrentModeUpdate, Implementation, InitializeRequest, InitializeResponse, McpCapabilities,
    McpServer, NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, Plan,
    PlanEntry, PlanEntryPriority, PlanEntryStatus, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, SessionConfigOption, SessionConfigOptionValue, SessionConfigSelectOption,
    SessionInfoUpdate, SessionMode, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, StopReason, TextContent, ToolCall, ToolCallContent, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::{Agent, Result, Stdio};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct State {
    /// What each session was given as MCP servers, as text.
    mcp: HashMap<String, String>,
    cancels: HashMap<String, Arc<Notify>>,
    model: HashMap<String, String>,
    counter: u32,
}

fn text(s: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(s)))
}

fn say(cx: &ConnectionTo<agent_client_protocol::Client>, session: &str, s: &str) -> Result<()> {
    cx.send_notification(SessionNotification::new(
        session.to_owned(),
        SessionUpdate::AgentMessageChunk(text(s)),
    ))
}

fn update(cx: &ConnectionTo<agent_client_protocol::Client>, session: &str, u: SessionUpdate) -> Result<()> {
    cx.send_notification(SessionNotification::new(session.to_owned(), u))
}

fn model_options(current: &str) -> Vec<SessionConfigOption> {
    vec![SessionConfigOption::select(
        "model",
        "Model",
        current.to_owned(),
        vec![
            SessionConfigSelectOption::new("opus", "Opus"),
            SessionConfigSelectOption::new("sonnet", "Sonnet"),
        ],
    )]
}

fn modes(current: &str) -> SessionModeState {
    SessionModeState::new(
        current.to_owned(),
        vec![SessionMode::new("default", "Default"), SessionMode::new("plan", "Plan")],
    )
}

async fn ask(cx: &ConnectionTo<agent_client_protocol::Client>, session: &str, title: &str) -> Result<String> {
    let call = ToolCallUpdate::new(
        "tc-ask",
        ToolCallUpdateFields::new().title(title.to_owned()).kind(ToolKind::Other),
    );
    let options = vec![
        PermissionOption::new("allow", "Allow once", PermissionOptionKind::AllowOnce),
        PermissionOption::new("always", "Always allow", PermissionOptionKind::AllowAlways),
        PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
    ];
    let response = cx
        .send_request(RequestPermissionRequest::new(session.to_owned(), call, options))
        .block_task()
        .await?;
    Ok(match response.outcome {
        RequestPermissionOutcome::Selected(s) => format!("selected {}", s.option_id),
        _ => "cancelled".to_owned(),
    })
}

#[allow(clippy::too_many_lines)] // one arm per scripted behaviour: a table of what the fake does
async fn prompt(
    cx: &ConnectionTo<agent_client_protocol::Client>,
    state: &Arc<Mutex<State>>,
    session: &str,
    input: &str,
) -> Result<StopReason> {
    match input.trim() {
        "hello" => {
            say(cx, session, "Hi ")?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            say(cx, session, "there")?;
        }
        "think" => {
            update(cx, session, SessionUpdate::AgentThoughtChunk(text("considering")))?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            say(cx, session, "Done thinking")?;
        }
        "tool" => {
            update(
                cx,
                session,
                SessionUpdate::ToolCall(
                    ToolCall::new("tc-1", "mcp__agentos__notes__search")
                        .kind(ToolKind::Search)
                        .status(ToolCallStatus::InProgress)
                        .raw_input(serde_json::json!({"query": "milk"})),
                ),
            )?;
            update(
                cx,
                session,
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    "tc-1",
                    ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Completed)
                        .content(vec![ToolCallContent::from("2 notes")]),
                )),
            )?;
            say(cx, session, "Found two notes")?;
        }
        "fail" => {
            update(
                cx,
                session,
                SessionUpdate::ToolCall(ToolCall::new("tc-f", "Edit file").kind(ToolKind::Edit)),
            )?;
            update(
                cx,
                session,
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    "tc-f",
                    ToolCallUpdateFields::new().status(ToolCallStatus::Failed),
                )),
            )?;
            return Err(agent_client_protocol::Error::new(-32000, "the model is overloaded"));
        }
        "ask" | "ask-write" => {
            say(cx, session, "Asking. ")?;
            let tool = if input.trim() == "ask" {
                "mcp__agentos__notes__search"
            } else {
                "mcp__agentos__notes__append"
            };
            let answer = ask(cx, session, tool).await?;
            say(cx, session, &format!("Answer: {answer}"))?;
        }
        "state" => {
            update(
                cx,
                session,
                SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                    AvailableCommand::new("compact", "Compact the context"),
                ])),
            )?;
            update(cx, session, SessionUpdate::UsageUpdate(UsageUpdate::new(1200, 200_000)))?;
            update(
                cx,
                session,
                SessionUpdate::Plan(Plan::new(vec![PlanEntry::new(
                    "write the test",
                    PlanEntryPriority::High,
                    PlanEntryStatus::InProgress,
                )])),
            )?;
            update(
                cx,
                session,
                SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title("A titled chat".to_owned())),
            )?;
            update(cx, session, SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("plan")))?;
            say(cx, session, "State sent")?;
        }
        "mcp" => {
            let seen = state
                .lock()
                .map_err(|_| agent_client_protocol::Error::internal_error())?
                .mcp
                .get(session)
                .cloned();
            say(cx, session, &format!("mcp: {}", seen.unwrap_or_default()))?;
        }
        "slow" => {
            let cancel = state
                .lock()
                .map_err(|_| agent_client_protocol::Error::internal_error())?
                .cancels
                .entry(session.to_owned())
                .or_default()
                .clone();
            for n in 0..200 {
                say(cx, session, &format!("tick {n} "))?;
                tokio::select! {
                    () = cancel.notified() => return Ok(StopReason::Cancelled),
                    () = tokio::time::sleep(Duration::from_millis(25)) => {}
                }
            }
        }
        "exit" => std::process::exit(3),
        other => say(cx, session, &format!("echo: {other}"))?,
    }
    Ok(StopReason::EndTurn)
}

fn first_text(req: &PromptRequest) -> String {
    req.prompt
        .iter()
        .find_map(|b| if let ContentBlock::Text(t) = b { Some(t.text.clone()) } else { None })
        .unwrap_or_default()
}

fn describe(servers: &[McpServer]) -> String {
    servers
        .iter()
        .map(|s| match s {
            McpServer::Http(h) => {
                let auth = h.headers.iter().find(|x| x.name == "Authorization").map(|x| x.value.clone());
                format!("{} {} auth={}", h.name, h.url, auth.unwrap_or_default())
            }
            _ => "other".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[tokio::main]
async fn main() -> Result<()> {
    let state = Arc::new(Mutex::new(State::default()));
    let (new_state, prompt_state, cancel_state, config_state) =
        (state.clone(), state.clone(), state.clone(), state.clone());
    Agent
        .v1()
        .name("fake-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder, _cx| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(
                            AgentCapabilities::new().mcp_capabilities(McpCapabilities::new().http(true)),
                        )
                        .agent_info(Implementation::new("fake-agent", "0.0.1")),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, cx| {
                let id = {
                    let mut st =
                        new_state.lock().map_err(|_| agent_client_protocol::Error::internal_error())?;
                    st.counter += 1;
                    let id = format!("sess-{}", st.counter);
                    st.mcp.insert(id.clone(), describe(&req.mcp_servers));
                    st.model.insert(id.clone(), "opus".into());
                    id
                };
                responder.respond(
                    NewSessionResponse::new(id.clone())
                        .modes(modes("default"))
                        .config_options(model_options("opus")),
                )?;
                // an agent reports its commands right after the session exists
                update(
                    &cx,
                    &id,
                    SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                        AvailableCommand::new("init", "Initialise the project"),
                    ])),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let (state, cx2) = (prompt_state.clone(), cx.clone());
                cx.spawn(async move {
                    let session = req.session_id.to_string();
                    match prompt(&cx2, &state, &session, &first_text(&req)).await {
                        Ok(stop) => responder.respond(PromptResponse::new(stop)),
                        Err(e) => responder.respond_with_error(e),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |n: CancelNotification, _cx| {
                let notify = cancel_state
                    .lock()
                    .map_err(|_| agent_client_protocol::Error::internal_error())?
                    .cancels
                    .entry(n.session_id.to_string())
                    .or_default()
                    .clone();
                notify.notify_one();
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: SetSessionModeRequest, responder, cx| {
                update(
                    &cx,
                    &req.session_id.to_string(),
                    SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(req.mode_id.clone())),
                )?;
                responder.respond(SetSessionModeResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                let SessionConfigOptionValue::ValueId { value } = req.value else {
                    return responder.respond_with_error(agent_client_protocol::Error::invalid_params());
                };
                config_state
                    .lock()
                    .map_err(|_| agent_client_protocol::Error::internal_error())?
                    .model
                    .insert(req.session_id.to_string(), value.to_string());
                responder.respond(SetSessionConfigOptionResponse::new(model_options(&value.to_string())))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}

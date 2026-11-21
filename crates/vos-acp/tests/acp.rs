#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The ACP adapter against a scripted fake agent process (no model).

use vos_acp::{handshake, AcpConnector, AgentLaunch};
use vos_application::ports::{
    AgentConnector, AgentEventSink, McpAccess, PermissionDecision, SessionOpened,
};
use vos_application::AgentProcesses;
use vos_domain::{
    thread::{
        AgentEvent, ConfigOption, ConfigValue, PermissionAnswer, PermissionRequest, StopReason, ToolStatus,
    },
    Agent, AgentId, AgentRuntime, ApprovalPolicy, Grants,
};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::Duration};

fn fake_agent(command: &str) -> Agent {
    Agent {
        id: AgentId("fake".into()),
        name: "Fake".into(),
        avatar: None,
        role: String::new(),
        runtime: AgentRuntime { command: command.into(), ..AgentRuntime::default() },
        grants: Grants::default(),
        approval: ApprovalPolicy::default(),
    }
}

fn fake() -> Agent {
    fake_agent(env!("CARGO_BIN_EXE_fake_agent"))
}

#[derive(Default)]
struct Sink {
    events: Mutex<Vec<AgentEvent>>,
    permissions: Mutex<Vec<PermissionRequest>>,
    exits: Mutex<Vec<String>>,
    auto: Mutex<Option<String>>,
}

impl Sink {
    fn text(&self) -> String {
        self.events
            .lock()
            .iter()
            .filter_map(
                |e| if let AgentEvent::MessageChunk(t) = e { Some(t.as_str().to_owned()) } else { None },
            )
            .collect()
    }
    fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().clone()
    }
}

impl AgentEventSink for Sink {
    fn on_event(&self, event: AgentEvent) {
        self.events.lock().push(event);
    }
    fn on_permission(&self, request: &PermissionRequest) -> PermissionDecision {
        self.permissions.lock().push(request.clone());
        match self.auto.lock().clone() {
            Some(option) => PermissionDecision::Auto(option),
            None => PermissionDecision::Ask,
        }
    }
    fn on_exit(&self, reason: String) {
        self.exits.lock().push(reason);
    }
}

async fn within<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(15), f).await.expect("no hang")
}

async fn wait_for(what: &str, cond: impl Fn() -> bool) {
    within(async {
        while !cond() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(cond(), "{what}");
}

fn connector() -> AcpConnector {
    AcpConnector::new(std::env::current_dir().unwrap())
}

async fn open(c: &AcpConnector, sink: &Arc<Sink>) -> SessionOpened {
    within(c.open_session(&fake(), None, sink.clone())).await.unwrap()
}

#[tokio::test]
async fn handshake_with_fake_agent() {
    let launch = AgentLaunch::new(PathBuf::from(env!("CARGO_BIN_EXE_fake_agent")), vec![]);
    let hello = within(handshake(&launch)).await.unwrap();
    assert_eq!(hello.name.as_deref(), Some("fake-agent"));
}

#[tokio::test]
async fn a_session_opens_with_the_agents_modes_settings_and_identity() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let opened = open(&c, &sink).await;
    assert_eq!(opened.session_id, "sess-1");
    let s = &opened.session;
    assert_eq!(s.agent.as_ref().unwrap().name, "fake-agent");
    let modes = s.modes.as_ref().unwrap();
    assert_eq!((modes.current.as_str(), modes.available.len()), ("default", 2));
    let ConfigOption::Select { id, current, options, category, .. } = &s.config[0] else { panic!() };
    assert_eq!((id.as_str(), current.as_str(), options.len()), ("model", "opus", 2));
    assert_eq!(category, &None);
    assert!(!s.tools_connected, "no MCP access was offered");
    // what the agent reported right after answering session/new is not lost
    wait_for("the early commands update", || {
        sink.events().iter().any(|e| matches!(e, AgentEvent::Commands(c) if c[0].name == "init"))
    })
    .await;
    c.stop_all().await;
}

#[tokio::test]
async fn the_agent_gets_the_host_mcp_url_and_its_own_token() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let mcp = McpAccess { url: "http://127.0.0.1:7400/mcp".into(), token: "tok-claude".into() };
    let opened = within(c.open_session(&fake(), Some(mcp), sink.clone())).await.unwrap();
    assert!(opened.session.tools_connected);
    within(c.prompt(&fake().id, &opened.session_id, "mcp")).await.unwrap();
    assert_eq!(sink.text(), "mcp: agentos http://127.0.0.1:7400/mcp auth=Bearer tok-claude");
    c.stop_all().await;
}

#[tokio::test]
async fn a_prompt_streams_its_chunks_and_ends_with_a_stop_reason() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    let stop = within(c.prompt(&fake().id, &s.session_id, "hello")).await.unwrap();
    assert_eq!(stop, StopReason::EndTurn);
    assert_eq!(sink.text(), "Hi there");
    within(c.prompt(&fake().id, &s.session_id, "think")).await.unwrap();
    assert!(sink.events().iter().any(|e| matches!(e, AgentEvent::ThoughtChunk(t) if t == "considering")));
    c.stop_all().await;
}

#[tokio::test]
async fn tool_calls_carry_their_title_kind_status_and_output() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    within(c.prompt(&fake().id, &s.session_id, "tool")).await.unwrap();
    let events = sink.events();
    let call =
        events.iter().find_map(|e| if let AgentEvent::ToolCall { .. } = e { Some(e.clone()) } else { None });
    let Some(AgentEvent::ToolCall { id, title, kind, status, raw_input }) = call else {
        panic!("{events:?}")
    };
    assert_eq!(
        (id.as_str(), title.as_str(), kind.as_deref()),
        ("tc-1", "mcp__agentos__notes__search", Some("search"))
    );
    assert_eq!(status, ToolStatus::InProgress);
    assert_eq!(raw_input.unwrap()["query"], "milk");
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::ToolUpdate { status: Some(ToolStatus::Completed), output: Some(o), .. } if o == "2 notes")));
    c.stop_all().await;
}

#[tokio::test]
async fn state_updates_commands_usage_plan_title_and_mode_arrive_typed() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    within(c.prompt(&fake().id, &s.session_id, "state")).await.unwrap();
    let events = sink.events();
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Commands(c) if c[0].name == "compact")));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Usage(u) if u.used == 1200 && u.size == 200_000)));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Plan(p) if p[0].content == "write the test")));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Title(t) if t == "A titled chat")));
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Mode(m) if m == "plan")));
    c.stop_all().await;
}

#[tokio::test]
async fn a_permission_request_waits_for_the_person_and_the_answer_reaches_the_agent() {
    let c = Arc::new(connector());
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    let (c2, session) = (c.clone(), s.session_id.clone());
    let turn = tokio::spawn(async move { c2.prompt(&fake().id, &session, "ask").await });

    wait_for("the request", || !sink.permissions.lock().is_empty()).await;
    let request = sink.permissions.lock()[0].clone();
    assert_eq!(request.title, "mcp__agentos__notes__search");
    assert_eq!(request.options.len(), 3);
    assert!(!turn.is_finished(), "the turn waits for an answer");
    // a session update that arrives while the request is pending is still delivered (the loop keeps running)
    assert!(sink.text().contains("Asking."));

    c.answer_permission(&request.request_id, PermissionAnswer::Selected("allow".into())).await;
    let stop = within(turn).await.unwrap().unwrap();
    assert_eq!(stop, StopReason::EndTurn);
    assert!(sink.text().ends_with("Answer: selected allow"), "{}", sink.text());
    c.stop_all().await;
}

#[tokio::test]
async fn a_cancelled_or_auto_answered_request_resolves_without_a_prompt() {
    let c = Arc::new(connector());
    let sink = Arc::new(Sink::default());
    *sink.auto.lock() = Some("always".into());
    let s = open(&c, &sink).await;
    within(c.prompt(&fake().id, &s.session_id, "ask")).await.unwrap();
    assert!(
        sink.text().ends_with("Answer: selected always"),
        "the policy answered for the agent: {}",
        sink.text()
    );

    *sink.auto.lock() = None;
    let (c2, session) = (c.clone(), s.session_id.clone());
    let turn = tokio::spawn(async move { c2.prompt(&fake().id, &session, "ask-write").await });
    wait_for("the second request", || sink.permissions.lock().len() == 2).await;
    let id = sink.permissions.lock()[1].request_id.clone();
    c.answer_permission(&id, PermissionAnswer::Cancelled).await;
    within(turn).await.unwrap().unwrap();
    assert!(sink.text().ends_with("Answer: cancelled"), "{}", sink.text());
    // answering a request nobody waits for is a no-op
    c.answer_permission("perm-999", PermissionAnswer::Cancelled).await;
    c.stop_all().await;
}

#[tokio::test]
async fn cancel_ends_a_running_turn_as_cancelled() {
    let c = Arc::new(connector());
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    let (c2, session) = (c.clone(), s.session_id.clone());
    let turn = tokio::spawn(async move { c2.prompt(&fake().id, &session, "slow").await });
    wait_for("some output", || sink.text().contains("tick 1 ")).await;
    c.cancel(&fake().id, &s.session_id).await.unwrap();
    assert_eq!(within(turn).await.unwrap().unwrap(), StopReason::Cancelled);
    c.stop_all().await;
}

#[tokio::test]
async fn a_failing_prompt_is_an_error_not_a_hang() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    let err = within(c.prompt(&fake().id, &s.session_id, "fail")).await.unwrap_err().to_string();
    assert!(err.contains("overloaded"), "{err}");
    // the session is still usable
    assert_eq!(within(c.prompt(&fake().id, &s.session_id, "hello")).await.unwrap(), StopReason::EndTurn);
    c.stop_all().await;
}

#[tokio::test]
async fn settings_change_the_mode_and_the_model() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    within(c.set_mode(&fake().id, &s.session_id, "plan")).await.unwrap();
    wait_for("the mode update", || {
        sink.events().iter().any(|e| matches!(e, AgentEvent::Mode(m) if m == "plan"))
    })
    .await;
    let options =
        within(c.set_config(&fake().id, &s.session_id, "model", ConfigValue::Text("sonnet".into())))
            .await
            .unwrap();
    let ConfigOption::Select { current, .. } = &options[0] else { panic!() };
    assert_eq!(current, "sonnet");
    let bad = within(c.set_config(&fake().id, &s.session_id, "model", ConfigValue::Bool(true))).await;
    assert!(bad.is_err(), "the agent refused a value of the wrong kind");
    c.stop_all().await;
}

#[tokio::test]
async fn sessions_share_one_process_and_stay_apart() {
    let c = connector();
    let (a, b) = (Arc::new(Sink::default()), Arc::new(Sink::default()));
    let sa = open(&c, &a).await;
    let sb = open(&c, &b).await;
    assert_ne!(sa.session_id, sb.session_id);
    within(c.prompt(&fake().id, &sa.session_id, "hello")).await.unwrap();
    within(c.prompt(&fake().id, &sb.session_id, "other words")).await.unwrap();
    assert_eq!(a.text(), "Hi there");
    assert_eq!(b.text(), "echo: other words");
    c.close_session(&fake().id, &sa.session_id).await;
    c.stop_all().await;
}

#[tokio::test]
async fn a_crashing_agent_tells_its_sessions_and_the_next_session_starts_a_new_process() {
    let c = Arc::new(connector());
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    let err = within(c.prompt(&fake().id, &s.session_id, "exit")).await;
    assert!(err.is_err(), "the turn cannot complete: {err:?}");
    wait_for("the exit report", || !sink.exits.lock().is_empty()).await;
    assert!(sink.exits.lock()[0].contains("agent process"), "{:?}", sink.exits.lock());
    let again = Arc::new(Sink::default());
    let s2 = open(&c, &again).await;
    assert_eq!(s2.session_id, "sess-1", "a fresh process numbers its sessions from 1");
    within(c.prompt(&fake().id, &s2.session_id, "hello")).await.unwrap();
    c.stop_all().await;
}

#[tokio::test]
async fn a_program_that_does_not_exist_is_a_clear_error() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let err = within(c.open_session(&fake_agent("/definitely/not/an/agent"), None, sink)).await.unwrap_err();
    assert!(err.to_string().contains("could not start"), "{err}");
    assert!(c.prompt(&AgentId("fake".into()), "s", "x").await.is_err(), "nothing is running");
}

#[tokio::test]
async fn stop_all_refuses_new_sessions_and_is_idempotent() {
    let c = connector();
    c.stop_all().await;
    c.stop_all().await;
    let err = c.open_session(&fake(), None, Arc::new(Sink::default())).await.unwrap_err();
    assert!(err.to_string().contains("shutting down"), "{err}");
}

/// Real adapter: `node <repo>/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js`, initialize only.
/// Skipped when node or the package is not present.
#[tokio::test]
async fn handshake_with_real_claude_agent_acp_via_node() {
    let index = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js");
    if !index.exists() || std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: node or claude-agent-acp not installed");
        return;
    }
    let launch = AgentLaunch::new("node", vec![index.to_string_lossy().into_owned()]);
    let hello = tokio::time::timeout(Duration::from_secs(30), handshake(&launch))
        .await
        .expect("handshake in 30s")
        .unwrap();
    assert!(hello.name.is_some());
}

/// An agent that starts and then never answers `initialize`.
fn hung_agent() -> Agent {
    Agent {
        id: AgentId("hung".into()),
        runtime: AgentRuntime {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60".into()],
            ..AgentRuntime::default()
        },
        ..fake_agent("unused")
    }
}

#[tokio::test]
async fn an_agent_that_never_answers_initialize_does_not_block_another_agents_session() {
    let timeout = Duration::from_secs(4);
    let c = Arc::new(AcpConnector::new(std::env::current_dir().unwrap()).with_initialize_timeout(timeout));
    let (c2, hung) = (c.clone(), hung_agent());
    let stuck = tokio::spawn(async move { c2.open_session(&hung, None, Arc::new(Sink::default())).await });
    tokio::time::sleep(Duration::from_millis(300)).await; // the hung agent is now starting

    let started = std::time::Instant::now();
    let sink = Arc::new(Sink::default());
    let opened = within(c.open_session(&fake(), None, sink.clone())).await.unwrap();
    assert!(
        started.elapsed() < timeout / 2,
        "another agent waited for the hung one: {:?}",
        started.elapsed()
    );
    within(c.prompt(&fake().id, &opened.session_id, "hello")).await.unwrap();
    assert!(!stuck.is_finished(), "the hung agent is still being waited for");

    // and the hung one gives up with a clear error instead of waiting for ever
    let err = within(stuck).await.unwrap().unwrap_err().to_string();
    assert!(err.contains("did not answer `initialize`"), "{err}");
    assert!(c.prompt(&AgentId("hung".into()), "s", "x").await.is_err(), "nothing is left running for it");
    c.stop_all().await;
}

#[tokio::test]
async fn stopping_an_agent_ends_its_process_tells_its_sessions_and_the_next_session_starts_afresh() {
    let c = connector();
    let sink = Arc::new(Sink::default());
    let s = open(&c, &sink).await;
    within(c.prompt(&fake().id, &s.session_id, "hello")).await.unwrap();

    c.stop_agent(&fake().id).await;
    assert_eq!(sink.exits.lock().len(), 1, "the session is told: {:?}", sink.exits.lock());
    assert!(c.prompt(&fake().id, &s.session_id, "hello").await.is_err(), "the process is gone");
    c.stop_agent(&fake().id).await; // idempotent

    let again = Arc::new(Sink::default());
    let s2 = open(&c, &again).await;
    assert_eq!(s2.session_id, "sess-1", "a new process numbers its sessions from 1");
    // stopping one agent leaves the others alone
    c.stop_agent(&AgentId("somebody-else".into())).await;
    within(c.prompt(&fake().id, &s2.session_id, "hello")).await.unwrap();
    c.stop_all().await;
}

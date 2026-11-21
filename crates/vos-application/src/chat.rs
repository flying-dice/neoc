//! Agent chats: threads over long-lived agent sessions, as AG-UI runs.
//!
//! A **thread** is one conversation with one agent: an ACP session (opened on demand, with that agent's own MCP
//! token), a transcript the host keeps and persists, the shared state (modes, settings, commands, usage, plan)
//! and any open permission requests. A **run** is one POST to `/agui`: it streams the AG-UI events of a turn.
//!
//! A permission request ends the current run with an AG-UI interrupt while the agent's turn stays suspended;
//! the client resumes with a new run that carries the answer and streams the rest of the turn. The turn is a
//! background task owned by the thread, so it does not depend on any one HTTP request: only an explicit cancel,
//! or the client dropping the stream of a run that has nothing pending, stops it.

use crate::{
    agents::AgentService,
    error::PortError,
    ports::{
        AgentConnector, AgentEventSink, Background, Clock, McpAccess, PermissionDecision, RandomSource,
        StoredThread, TaskHandle, ThreadStore,
    },
    services::AppService,
    tokens::random_hex,
};
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use vos_domain::{
    agui::{AgUiEvent, ReasoningRole, Role, RunAgentInput, RunOutcome},
    thread::{
        json_of, AgentEvent, ConfigValue, ErrorMeta, Interrupt, PermissionAnswer, PermissionKind,
        PermissionRequest, PlanEntry, SettingsChange, StopReason, ThreadMessage, ThreadSnapshot, ThreadState,
        ToolCallRecord, ToolFunction, ToolMeta, ToolStatus,
    },
    time::iso8601_utc,
    AgentId, ApprovalPolicy,
};

/// How long a permission request waits for the person before it resolves as cancelled.
pub const PERMISSION_TTL_MS: u64 = 10 * 60 * 1000;

/// The events of one run, as the SSE endpoint streams them. The stream ends with the run.
pub type RunEvents = UnboundedReceiver<AgUiEvent>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    /// The request is malformed or asks for something this thread cannot do (HTTP 422).
    #[error("{0}")]
    BadInput(String),
    /// No such agent or thread (HTTP 404).
    #[error("{0}")]
    NotFound(String),
    /// A run is already in progress on the thread (HTTP 409).
    #[error("a run is already in progress on this thread")]
    Busy,
    /// The agent could not be started or did not answer (HTTP 502).
    #[error("{0}")]
    Agent(String),
}

/// Everything the chat use cases collaborate with.
pub struct ChatPorts {
    pub connector: Arc<dyn AgentConnector>,
    pub threads: Arc<dyn ThreadStore>,
    pub background: Arc<dyn Background>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub agents: Arc<AgentService>,
    pub apps: Arc<AppService>,
    /// The host's `/mcp` URL as agents reach it; `None` starts sessions without tools.
    pub mcp_url: Option<String>,
}

pub struct ChatService {
    p: ChatPorts,
    threads: Mutex<HashMap<String, Arc<Thread>>>,
    me: Weak<Self>,
}

struct Thread {
    id: String,
    agent_id: AgentId,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    session_id: Option<String>,
    state: ThreadState,
    messages: Vec<ThreadMessage>,
    /// The turn in flight, if any.
    run: Option<Run>,
    /// Permission requests the person has not answered (the run that raised them has ended).
    pending: Vec<Interrupt>,
    open_text: Option<usize>,
    open_reasoning: Option<usize>,
    tool_messages: HashMap<String, usize>,
    /// Keeps the turn task alive; dropped (stopping it) when the next turn replaces it.
    turn: Option<Box<dyn TaskHandle>>,
}

struct Run {
    run_id: String,
    /// The SSE stream of the run, until it ends (an interrupt ends the stream, not the turn).
    tx: Option<UnboundedSender<AgUiEvent>>,
}

/// Receives one session's reports and folds them into its thread.
struct ThreadSink {
    thread: Arc<Thread>,
    agents: Arc<AgentService>,
    apps: Arc<AppService>,
    random: Arc<dyn RandomSource>,
    clock: Arc<dyn Clock>,
}

fn hex_id(random: &dyn RandomSource, prefix: &str) -> String {
    format!("{prefix}{}", random_hex(random, 8))
}

fn replace(path: &str, value: &Value) -> Value {
    json!({ "op": "replace", "path": path, "value": value })
}

impl Inner {
    /// Send to the run's stream; a stream whose reader is gone is dropped.
    fn emit(&mut self, event: AgUiEvent) {
        if let Some(run) = &mut self.run {
            if let Some(tx) = &run.tx {
                if tx.unbounded_send(event).is_err() {
                    run.tx = None;
                }
            }
        }
    }

    fn end_stream(&mut self) {
        if let Some(run) = &mut self.run {
            run.tx = None;
        }
    }

    fn close_text(&mut self) {
        if let Some(index) = self.open_text.take() {
            if let Some(ThreadMessage::Assistant { id, .. }) = self.messages.get(index) {
                let id = id.clone();
                self.emit(AgUiEvent::TextMessageEnd { message_id: id });
            }
        }
    }

    fn close_reasoning(&mut self) {
        if let Some(index) = self.open_reasoning.take() {
            if let Some(ThreadMessage::Reasoning { id, .. }) = self.messages.get(index) {
                let id = id.clone();
                self.emit(AgUiEvent::ReasoningMessageEnd { message_id: id.clone() });
                self.emit(AgUiEvent::ReasoningEnd { message_id: id });
            }
        }
    }

    fn close_messages(&mut self) {
        self.close_text();
        self.close_reasoning();
    }

    fn delta(&mut self, ops: Vec<Value>) {
        self.emit(AgUiEvent::StateDelta { delta: ops });
    }

    fn snapshot_event(&self) -> AgUiEvent {
        AgUiEvent::StateSnapshot { snapshot: json_of(&self.state) }
    }

    fn apply(&mut self, random: &dyn RandomSource, event: AgentEvent) {
        match event {
            AgentEvent::MessageChunk(text) => self.message_chunk(random, &text),
            AgentEvent::ThoughtChunk(text) => self.thought_chunk(random, &text),
            AgentEvent::ToolCall { id, title, kind, status, raw_input } => {
                self.tool_call(&id, &title, kind, status, raw_input.as_ref());
            }
            AgentEvent::ToolUpdate { id, title, kind, status, output } => {
                self.tool_update(random, &id, title, kind, status, output);
            }
            AgentEvent::Plan(entries) => self.set_plan(entries),
            AgentEvent::Mode(mode) => {
                if let Some(modes) = &mut self.state.session.modes {
                    modes.current.clone_from(&mode);
                    self.delta(vec![replace("/session/modes/current", &json_of(&mode))]);
                }
            }
            AgentEvent::Config(options) => {
                self.state.session.config.clone_from(&options);
                self.delta(vec![replace("/session/config", &json_of(&options))]);
            }
            AgentEvent::Commands(commands) => {
                self.state.session.commands.clone_from(&commands);
                self.delta(vec![replace("/session/commands", &json_of(&commands))]);
            }
            AgentEvent::Usage(usage) => {
                self.delta(vec![replace("/session/usage", &json_of(&usage))]);
                self.state.session.usage = Some(usage);
            }
            AgentEvent::Title(title) => {
                self.delta(vec![replace("/session/title", &json_of(&title))]);
                self.state.session.title = Some(title);
            }
            AgentEvent::PermissionClosed { request_id } => self.pending.retain(|i| i.id != request_id),
        }
    }

    fn message_chunk(&mut self, random: &dyn RandomSource, text: &str) {
        if text.is_empty() {
            return;
        }
        self.close_reasoning();
        let index = if let Some(index) = self.open_text {
            index
        } else {
            let id = hex_id(random, "msg-");
            self.messages.push(ThreadMessage::Assistant {
                id: id.clone(),
                content: Some(String::new()),
                tool_calls: vec![],
                vos: None,
            });
            self.open_text = Some(self.messages.len() - 1);
            self.emit(AgUiEvent::TextMessageStart { message_id: id, role: Role::Assistant });
            self.messages.len() - 1
        };
        if let Some(ThreadMessage::Assistant { id, content, .. }) = self.messages.get_mut(index) {
            content.get_or_insert_with(String::new).push_str(text);
            let id = id.clone();
            self.emit(AgUiEvent::TextMessageContent { message_id: id, delta: text.to_owned() });
        }
    }

    fn thought_chunk(&mut self, random: &dyn RandomSource, text: &str) {
        if text.is_empty() {
            return;
        }
        self.close_text();
        let index = if let Some(index) = self.open_reasoning {
            index
        } else {
            let id = hex_id(random, "reason-");
            self.messages.push(ThreadMessage::Reasoning { id: id.clone(), content: String::new() });
            self.open_reasoning = Some(self.messages.len() - 1);
            self.emit(AgUiEvent::ReasoningStart { message_id: id.clone() });
            self.emit(AgUiEvent::ReasoningMessageStart { message_id: id, role: ReasoningRole::Reasoning });
            self.messages.len() - 1
        };
        if let Some(ThreadMessage::Reasoning { id, content }) = self.messages.get_mut(index) {
            content.push_str(text);
            let id = id.clone();
            self.emit(AgUiEvent::ReasoningMessageContent { message_id: id, delta: text.to_owned() });
        }
    }

    fn tool_event(id: &str, meta: &ToolMeta) -> AgUiEvent {
        AgUiEvent::Custom {
            name: "vos.tool".to_owned(),
            value: json!({ "toolCallId": id, "title": meta.title, "kind": meta.kind, "status": meta.status }),
        }
    }

    fn tool_call(
        &mut self,
        id: &str,
        title: &str,
        kind: Option<String>,
        status: ToolStatus,
        raw_input: Option<&Value>,
    ) {
        self.close_messages();
        let arguments = raw_input.map_or_default(Value::to_string);
        let meta = ToolMeta { title: title.to_owned(), kind, status };
        self.messages.push(ThreadMessage::Assistant {
            id: id.to_owned(),
            content: None,
            tool_calls: vec![ToolCallRecord {
                id: id.to_owned(),
                kind: "function".to_owned(),
                function: ToolFunction { name: title.to_owned(), arguments: arguments.clone() },
            }],
            vos: Some(meta.clone()),
        });
        self.tool_messages.insert(id.to_owned(), self.messages.len() - 1);
        self.emit(AgUiEvent::ToolCallStart {
            tool_call_id: id.to_owned(),
            tool_call_name: title.to_owned(),
            parent_message_id: None,
        });
        if !arguments.is_empty() {
            self.emit(AgUiEvent::ToolCallArgs { tool_call_id: id.to_owned(), delta: arguments });
        }
        self.emit(AgUiEvent::ToolCallEnd { tool_call_id: id.to_owned() });
        self.emit(Self::tool_event(id, &meta));
    }

    fn tool_update(
        &mut self,
        random: &dyn RandomSource,
        id: &str,
        title: Option<String>,
        kind: Option<String>,
        status: Option<ToolStatus>,
        output: Option<String>,
    ) {
        let Some(&index) = self.tool_messages.get(id) else { return };
        let mut changed = None;
        if let Some(ThreadMessage::Assistant { vos: Some(meta), .. }) = self.messages.get_mut(index) {
            if let Some(title) = title {
                meta.title = title;
            }
            if kind.is_some() {
                meta.kind = kind;
            }
            if let Some(status) = status {
                meta.status = status;
            }
            changed = Some(meta.clone());
        }
        if let Some(meta) = changed {
            self.emit(Self::tool_event(id, &meta));
        }
        let finished = matches!(status, Some(ToolStatus::Completed | ToolStatus::Failed));
        if let (true, Some(content)) = (finished, output) {
            if !content.is_empty() {
                let message_id = hex_id(random, "result-");
                self.messages.push(ThreadMessage::Tool {
                    id: message_id.clone(),
                    tool_call_id: id.to_owned(),
                    content: content.clone(),
                });
                self.emit(AgUiEvent::ToolCallResult { message_id, tool_call_id: id.to_owned(), content });
            }
        }
    }

    fn set_plan(&mut self, entries: Vec<PlanEntry>) {
        self.delta(vec![replace("/plan", &json_of(&entries))]);
        self.state.plan = Some(entries);
    }

    /// The turn is over, one way or another: nothing stays open.
    fn end_turn(&mut self) {
        self.close_messages();
        self.tool_messages.clear();
        self.run = None;
        self.pending.clear();
    }

    fn error_message(&mut self, random: &dyn RandomSource, text: &str) {
        self.messages.push(ThreadMessage::System {
            id: hex_id(random, "error-"),
            content: text.to_owned(),
            vos: Some(ErrorMeta { error: true }),
        });
    }
}

impl AgentEventSink for ThreadSink {
    fn on_event(&self, event: AgentEvent) {
        self.thread.inner.lock().apply(self.random.as_ref(), event);
    }

    fn on_permission(&self, request: &PermissionRequest) -> PermissionDecision {
        let agent = self.agents.get(&self.thread.agent_id);
        let approval = agent.as_ref().map_or(ApprovalPolicy::Ask, |a| a.approval);
        let allow_once =
            request.options.iter().find(|o| o.kind == PermissionKind::AllowOnce).map(|o| o.option_id.clone());
        let auto = match approval {
            // every request, a shell command or a file edit included (see `ApprovalPolicy::AllowEverything`): once
            // if the agent offers it, else the standing allow
            ApprovalPolicy::AllowEverything => allow_once.or_else(|| {
                request
                    .options
                    .iter()
                    .find(|o| o.kind == PermissionKind::AllowAlways)
                    .map(|o| o.option_id.clone())
            }),
            ApprovalPolicy::Ask => None,
            // the title and kind are the agent's text: only an exact `mcp__vos__<app>__<tool>` that exists, is
            // granted to this agent, was declared readOnly (the runtime refuses writes through `app.*` and trusts
            // the app for the rest: apps are the person's own code) and comes with a read-only kind skips the
            // prompt, and then only as allow-once, never as a standing allow
            ApprovalPolicy::AutoReadOnly => request
                .vos_tool()
                .filter(|_| request.kind_allows_auto_approval())
                .filter(|tool| agent.as_ref().is_some_and(|a| a.grants.allows(tool.app())))
                .filter(|tool| self.apps.tool_is_read_only(tool) == Some(true))
                .and(allow_once),
        };
        if let Some(option) = auto {
            return PermissionDecision::Auto(option);
        }
        let expires = iso8601_utc(self.clock.now_unix_ms() + PERMISSION_TTL_MS);
        let interrupt = Interrupt::permission(request, expires);
        let mut inner = self.thread.inner.lock();
        inner.close_messages();
        inner.pending.push(interrupt.clone());
        // the run that raised it ends here; the turn is suspended until a resume answers
        if let Some(run) = &inner.run {
            let (thread_id, run_id) = (self.thread.id.clone(), run.run_id.clone());
            inner.emit(AgUiEvent::RunFinished {
                thread_id,
                run_id,
                outcome: Some(RunOutcome::Interrupt { interrupts: vec![interrupt] }),
                result: None,
            });
            inner.end_stream();
        }
        PermissionDecision::Ask
    }

    fn on_exit(&self, reason: String) {
        let mut inner = self.thread.inner.lock();
        inner.session_id = None;
        let had_run = inner.run.is_some();
        inner.close_messages();
        if had_run {
            inner.error_message(self.random.as_ref(), &reason);
            inner.emit(AgUiEvent::RunError { message: reason, code: Some("agent_exited".into()) });
        }
        inner.end_turn();
        self.agents.revoke_launch_tokens(&self.thread.id);
    }
}

impl ChatService {
    #[must_use]
    pub fn new(ports: ChatPorts) -> Arc<Self> {
        Arc::new_cyclic(|me| Self { p: ports, threads: Mutex::default(), me: me.clone() })
    }

    fn this(&self) -> Result<Arc<Self>, ChatError> {
        self.me.upgrade().ok_or_else(|| ChatError::Agent("the host is shutting down".into()))
    }

    // ---- threads -------------------------------------------------------------------------------------

    /// The thread, from memory or from the stored transcript. `None` for a thread nobody created.
    async fn thread(&self, id: &str) -> Result<Option<Arc<Thread>>, ChatError> {
        if let Some(t) = self.threads.lock().get(id) {
            return Ok(Some(t.clone()));
        }
        let stored = self.p.threads.load(id).await?;
        Ok(stored.map(|s| self.adopt(id, s.agent_id, s.messages)))
    }

    fn adopt(&self, id: &str, agent_id: AgentId, messages: Vec<ThreadMessage>) -> Arc<Thread> {
        let mut threads = self.threads.lock();
        threads
            .entry(id.to_owned())
            .or_insert_with(|| {
                Arc::new(Thread {
                    id: id.to_owned(),
                    agent_id,
                    inner: Mutex::new(Inner { messages, ..Inner::default() }),
                })
            })
            .clone()
    }

    fn snapshot_of(thread: &Thread) -> ThreadSnapshot {
        let inner = thread.inner.lock();
        ThreadSnapshot {
            thread_id: thread.id.clone(),
            agent_id: thread.agent_id.0.clone(),
            running: inner.run.is_some(),
            state: inner.state.clone(),
            messages: inner.messages.clone(),
            interrupts: inner.pending.clone(),
        }
    }

    /// Start (or find) the thread and make sure its agent session is open. Idempotent for the same ids.
    pub async fn ensure(&self, thread_id: &str, agent_id: &str) -> Result<ThreadSnapshot, ChatError> {
        let thread = self.thread_for(thread_id, agent_id).await?;
        self.open_session(&thread).await?;
        Ok(Self::snapshot_of(&thread))
    }

    /// The thread's current snapshot; the agent is not started.
    pub async fn snapshot(&self, thread_id: &str) -> Result<ThreadSnapshot, ChatError> {
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| ChatError::NotFound(format!("no thread `{thread_id}`")))?;
        Ok(Self::snapshot_of(&thread))
    }

    async fn thread_for(&self, thread_id: &str, agent_id: &str) -> Result<Arc<Thread>, ChatError> {
        let agent = AgentId::parse(agent_id).map_err(|e| ChatError::BadInput(e.to_string()))?;
        if self.p.agents.get(&agent).is_none() {
            return Err(ChatError::NotFound(format!("no agent `{agent_id}`")));
        }
        if thread_id.is_empty() || thread_id.len() > 128 {
            return Err(ChatError::BadInput("threadId must be 1-128 characters".into()));
        }
        match self.thread(thread_id).await? {
            Some(t) if t.agent_id == agent => Ok(t),
            Some(t) => Err(ChatError::BadInput(format!(
                "thread `{thread_id}` belongs to agent `{}`, not `{agent_id}`",
                t.agent_id.0
            ))),
            None => Ok(self.adopt(thread_id, agent, vec![])),
        }
    }

    /// Open the thread's agent session if it has none.
    async fn open_session(&self, thread: &Arc<Thread>) -> Result<(), ChatError> {
        if thread.inner.lock().session_id.is_some() {
            return Ok(());
        }
        let agent = self
            .p
            .agents
            .get(&thread.agent_id)
            .ok_or_else(|| ChatError::NotFound(format!("no agent `{}`", thread.agent_id.0)))?;
        if !agent.runtime.is_launchable() {
            return Err(ChatError::BadInput(format!("agent `{}` has no launch command", agent.id.0)));
        }
        let mcp = match &self.p.mcp_url {
            Some(url) => Some(McpAccess {
                url: url.clone(),
                token: self
                    .p
                    .agents
                    .launch_token(&agent.id, &thread.id)
                    .map_err(|e| ChatError::Agent(e.to_string()))?,
            }),
            None => None,
        };
        let sink = Arc::new(ThreadSink {
            thread: thread.clone(),
            agents: self.p.agents.clone(),
            apps: self.p.apps.clone(),
            random: self.p.random.clone(),
            clock: self.p.clock.clone(),
        });
        let opened = match self.p.connector.open_session(&agent, mcp, sink).await {
            Ok(o) => o,
            Err(e) => {
                self.p.agents.revoke_launch_tokens(&thread.id);
                return Err(ChatError::Agent(e.to_string()));
            }
        };
        let raced = {
            let mut inner = thread.inner.lock();
            if inner.session_id.is_some() {
                true
            } else {
                inner.session_id = Some(opened.session_id.clone());
                inner.state.session = opened.session;
                false
            }
        };
        if raced {
            // a concurrent ensure won the race: drop the extra session
            self.p.connector.close_session(&agent.id, &opened.session_id).await;
        }
        Ok(())
    }

    // ---- runs ----------------------------------------------------------------------------------------

    /// One `POST /agui`: either a new prompt, or a resume that answers the thread's open interrupts.
    pub async fn run(&self, input: RunAgentInput) -> Result<RunEvents, ChatError> {
        let this = self.this()?;
        let known = self.thread(&input.thread_id).await?;
        let thread = match (&known, input.agent_id()) {
            (Some(t), _) => t.clone(),
            (None, Some(agent)) => self.thread_for(&input.thread_id, agent).await?,
            (None, None) => {
                return Err(ChatError::NotFound(format!(
                    "no thread `{}`: pass forwardedProps.agentId or POST /agui/threads first",
                    input.thread_id
                )))
            }
        };
        if !input.resume.is_empty() {
            return Ok(this.resume(&thread, &input).await);
        }
        let pending = !thread.inner.lock().pending.is_empty();
        if pending {
            return Ok(refused(
                &input,
                "this thread is waiting for an answer to a permission request: resume it",
                "interrupts_pending",
            ));
        }
        let Some(prompt) = input.prompt() else {
            return Err(ChatError::BadInput("the input has no user message to send".into()));
        };
        if thread.inner.lock().run.is_some() {
            return Err(ChatError::Busy);
        }
        self.open_session(&thread).await?;
        let (tx, rx) = unbounded();
        let session_id = {
            let mut inner = thread.inner.lock();
            if inner.run.is_some() {
                return Err(ChatError::Busy);
            }
            let Some(session_id) = inner.session_id.clone() else {
                return Err(ChatError::Agent("the agent session closed before the run started".into()));
            };
            let message_id = input
                .messages
                .iter()
                .rev()
                .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
                .and_then(|m| m.get("id").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_else(|| hex_id(self.p.random.as_ref(), "user-"));
            inner.messages.push(ThreadMessage::User { id: message_id, content: prompt.clone() });
            inner.run = Some(Run { run_id: input.run_id.clone(), tx: Some(tx) });
            Self::start_events(&mut inner, &thread.id, &input.run_id);
            session_id
        };
        let handle = self
            .p
            .background
            .spawn("agent-turn", Box::pin(turn(this.clone(), thread.clone(), session_id, prompt)));
        thread.inner.lock().turn = Some(handle);
        Ok(rx)
    }

    fn start_events(inner: &mut Inner, thread_id: &str, run_id: &str) {
        inner.emit(AgUiEvent::RunStarted {
            thread_id: thread_id.to_owned(),
            run_id: run_id.to_owned(),
            parent_run_id: None,
        });
        let snapshot = inner.snapshot_event();
        inner.emit(snapshot);
    }

    /// Answer the open interrupts and stream the rest of the suspended turn.
    async fn resume(&self, thread: &Arc<Thread>, input: &RunAgentInput) -> RunEvents {
        // the thread is locked only in the synchronous part: a guard must not be held across an await
        let rx = match Self::take_resume(thread, input) {
            Ok(rx) => rx,
            Err(refusal) => return refusal,
        };
        // the suspended turn task picks the rest up once the connector delivers the answers
        for entry in &input.resume {
            self.p.connector.answer_permission(&entry.interrupt_id, entry.answer()).await;
        }
        rx
    }

    /// Validate a resume against the open interrupts and switch the turn over to a new stream. `Err` is the
    /// stream of a run that only reports why it was refused.
    fn take_resume(thread: &Arc<Thread>, input: &RunAgentInput) -> Result<RunEvents, RunEvents> {
        let mut inner = thread.inner.lock();
        let open: Vec<String> = inner.pending.iter().map(|i| i.id.clone()).collect();
        let answered: Vec<&str> = input.resume.iter().map(|r| r.interrupt_id.as_str()).collect();
        if open.is_empty() || inner.run.is_none() {
            return Err(refused(input, "there is nothing to resume on this thread", "nothing_to_resume"));
        }
        if let Some(unknown) = answered.iter().find(|a| !open.iter().any(|o| o == *a)) {
            return Err(refused(input, &format!("unknown interrupt `{unknown}`"), "unknown_interrupt"));
        }
        if let Some(missing) = open.iter().find(|o| !answered.contains(&o.as_str())) {
            return Err(refused(
                input,
                &format!("the resume does not answer interrupt `{missing}`"),
                "partial_resume",
            ));
        }
        let (tx, rx) = unbounded();
        if let Some(run) = &mut inner.run {
            run.run_id.clone_from(&input.run_id);
            run.tx = Some(tx);
        }
        inner.pending.clear();
        Self::start_events(&mut inner, &thread.id, &input.run_id);
        Ok(rx)
    }

    /// Cancel the thread's turn: `session/cancel`, and every open permission request resolves as cancelled.
    pub async fn cancel(&self, thread_id: &str) -> Result<(), ChatError> {
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| ChatError::NotFound(format!("no thread `{thread_id}`")))?;
        self.cancel_thread(&thread).await;
        Ok(())
    }

    async fn cancel_thread(&self, thread: &Arc<Thread>) {
        let (session, pending) = {
            let mut inner = thread.inner.lock();
            (inner.session_id.clone(), std::mem::take(&mut inner.pending))
        };
        for interrupt in pending {
            self.p.connector.answer_permission(&interrupt.id, PermissionAnswer::Cancelled).await;
        }
        if let Some(session) = session {
            if let Err(e) = self.p.connector.cancel(&thread.agent_id, &session).await {
                tracing::warn!(thread = %thread.id, error = %e, "cancel failed");
            }
        }
    }

    /// The client of run `run_id` went away. A run with nothing pending is cancelled; one that ended in an
    /// interrupt is left suspended for a resume.
    pub async fn client_gone(&self, thread_id: &str, run_id: &str) {
        let Ok(Some(thread)) = self.thread(thread_id).await else { return };
        let unanswered_run = {
            let inner = thread.inner.lock();
            inner.pending.is_empty() && inner.run.as_ref().is_some_and(|r| r.run_id == run_id)
        };
        if unanswered_run {
            self.cancel_thread(&thread).await;
        }
    }

    // ---- settings --------------------------------------------------------------------------------------

    /// Change the session's mode or one of its settings; answers the new state.
    pub async fn settings(&self, thread_id: &str, change: SettingsChange) -> Result<ThreadState, ChatError> {
        let thread = self
            .thread(thread_id)
            .await?
            .ok_or_else(|| ChatError::NotFound(format!("no thread `{thread_id}`")))?;
        self.open_session(&thread).await?;
        let session = thread
            .inner
            .lock()
            .session_id
            .clone()
            .ok_or_else(|| ChatError::Agent("the agent session closed".into()))?;
        match change {
            SettingsChange::Mode(mode) => {
                let known = thread
                    .inner
                    .lock()
                    .state
                    .session
                    .modes
                    .as_ref()
                    .is_some_and(|m| m.available.iter().any(|a| a.id == mode));
                if !known {
                    return Err(ChatError::BadInput(format!("the agent has no mode `{mode}`")));
                }
                self.p.connector.set_mode(&thread.agent_id, &session, &mode).await?;
                thread.inner.lock().apply(self.p.random.as_ref(), AgentEvent::Mode(mode));
            }
            SettingsChange::Config { id, value } => {
                let valid = thread.inner.lock().state.session.config.iter().any(|o| o.id() == id);
                if !valid {
                    return Err(ChatError::BadInput(format!("the agent has no setting `{id}`")));
                }
                if let ConfigValue::Text(t) = &value {
                    if t.is_empty() {
                        return Err(ChatError::BadInput("a setting's value cannot be empty".into()));
                    }
                }
                let options = self.p.connector.set_config(&thread.agent_id, &session, &id, value).await?;
                thread.inner.lock().apply(self.p.random.as_ref(), AgentEvent::Config(options));
            }
        }
        let state = thread.inner.lock().state.clone();
        Ok(state)
    }

    /// Host shutdown: stop every agent process.
    pub async fn shutdown(&self) {
        self.p.connector.stop_all().await;
    }
}

impl From<PortError> for ChatError {
    fn from(e: PortError) -> Self {
        Self::Agent(e.to_string())
    }
}

/// A run that only reports why it cannot proceed (AG-UI: a refused resume is a `RUN_ERROR`).
fn refused(input: &RunAgentInput, message: &str, code: &str) -> RunEvents {
    let (tx, rx) = unbounded();
    let _ = tx.unbounded_send(AgUiEvent::RunStarted {
        thread_id: input.thread_id.clone(),
        run_id: input.run_id.clone(),
        parent_run_id: None,
    });
    let _ =
        tx.unbounded_send(AgUiEvent::RunError { message: message.to_owned(), code: Some(code.to_owned()) });
    rx
}

/// The background task of one turn: prompt, wait, and end the run.
async fn turn(chat: Arc<ChatService>, thread: Arc<Thread>, session_id: String, prompt: String) {
    let result = chat.p.connector.prompt(&thread.agent_id, &session_id, &prompt).await;
    let (messages, agent_id) = {
        let mut inner = thread.inner.lock();
        // a turn that ended after the agent went away already reported it (`on_exit`)
        if inner.run.is_some() {
            inner.close_messages();
            let (thread_id, run_id) =
                (thread.id.clone(), inner.run.as_ref().map_or_default(|r| r.run_id.clone()));
            match result {
                Ok(StopReason::Refusal) => {
                    inner.error_message(chat.p.random.as_ref(), "The agent declined this request.");
                    inner.emit(AgUiEvent::RunError {
                        message: "The agent declined this request.".into(),
                        code: Some("refusal".into()),
                    });
                }
                Ok(stop) => inner.emit(AgUiEvent::RunFinished {
                    thread_id,
                    run_id,
                    outcome: Some(RunOutcome::Success),
                    result: Some(json!({ "stopReason": stop.as_str() })),
                }),
                Err(e) => {
                    let message = e.to_string();
                    inner.error_message(chat.p.random.as_ref(), &message);
                    inner.emit(AgUiEvent::RunError { message, code: Some("prompt_failed".into()) });
                }
            }
            inner.end_turn();
        }
        (inner.messages.clone(), thread.agent_id.clone())
    };
    let stored = StoredThread { agent_id, messages };
    if let Err(e) = chat.p.threads.save(&thread.id, &stored, chat.p.clock.now_unix_ms()).await {
        tracing::warn!(thread = %thread.id, error = %e, "saving the transcript failed");
    }
}

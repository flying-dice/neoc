//! One long-lived agent process: its ACP connection, the sessions opened on it and the permission requests
//! waiting for a person.
//!
//! The connection (and with it the child process) lives inside one tokio task: `connect_with` serves ACP
//! until its main function returns, and that function is a loop over [`Command`]s. Dropping an
//! [`AgentProcess`] aborts the task, which drops the connection future, which terminates the child (the ACP
//! crate's guard kills its process group).

use crate::{launch::AgentLaunch, map};
use agent_client_protocol::{
    schema::ProtocolVersion,
    v1::{
        schema::{
            CancelNotification, ContentBlock, InitializeRequest, McpServer, NewSessionRequest,
            NewSessionResponse, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
            RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification,
            SetSessionConfigOptionRequest, SetSessionModeRequest, TextContent,
        },
        ConnectionTo,
    },
    Agent, Client,
};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use vos_application::{
    ports::{AgentEventSink, PermissionDecision},
    PERMISSION_TTL_MS,
};
use vos_domain::thread::{AgentEvent, AgentInfo, ConfigOption, ConfigValue, PermissionAnswer, StopReason};

/// What `session/new` answered, with what the process told at `initialize`.
pub(crate) struct NewSession {
    pub id: String,
    pub response: NewSessionResponse,
}

type Reply<T> = oneshot::Sender<Result<T, String>>;

enum Command {
    NewSession { cwd: PathBuf, mcp: Vec<McpServer>, reply: Reply<NewSession> },
    Prompt { session: String, text: String, reply: Reply<StopReason> },
    Cancel { session: String },
    SetMode { session: String, mode: String, reply: Reply<()> },
    SetConfig { session: String, id: String, value: ConfigValue, reply: Reply<Vec<ConfigOption>> },
}

/// What the process said about itself at `initialize`.
#[derive(Clone, Debug)]
pub(crate) struct ProcessInfo {
    pub agent: Option<AgentInfo>,
    /// The agent can connect to an HTTP MCP server (how it reaches the host's tools).
    pub http_mcp: bool,
}

/// Pending permission requests by id; shared by every process of a connector.
pub(crate) type Permissions = Arc<Mutex<HashMap<String, oneshot::Sender<PermissionAnswer>>>>;

#[derive(Default)]
struct SessionsInner {
    sinks: HashMap<String, Arc<dyn AgentEventSink>>,
    /// Updates that arrived before their session was registered (an agent may report commands right after
    /// answering `session/new`): kept, bounded, and delivered on registration.
    early: HashMap<String, Vec<AgentEvent>>,
}

/// The event sinks of a process's sessions.
#[derive(Default)]
pub(crate) struct Sessions(Mutex<SessionsInner>);

const MAX_EARLY_EVENTS: usize = 64;
const MAX_EARLY_SESSIONS: usize = 16;

impl Sessions {
    pub fn register(&self, id: &str, sink: &Arc<dyn AgentEventSink>) {
        let early = {
            let mut inner = self.0.lock();
            inner.sinks.insert(id.to_owned(), Arc::clone(sink));
            inner.early.remove(id)
        };
        for event in early.unwrap_or_default() {
            sink.on_event(event);
        }
    }

    pub fn remove(&self, id: &str) {
        let mut inner = self.0.lock();
        inner.sinks.remove(id);
        inner.early.remove(id);
    }

    fn sink(&self, id: &str) -> Option<Arc<dyn AgentEventSink>> {
        self.0.lock().sinks.get(id).cloned()
    }

    fn deliver(&self, id: &str, event: AgentEvent) {
        let sink = {
            let mut inner = self.0.lock();
            if let Some(sink) = inner.sinks.get(id) {
                Arc::clone(sink)
            } else {
                if inner.early.len() < MAX_EARLY_SESSIONS || inner.early.contains_key(id) {
                    let list = inner.early.entry(id.to_owned()).or_default();
                    if list.len() < MAX_EARLY_EVENTS {
                        list.push(event);
                    }
                }
                return;
            }
        };
        sink.on_event(event);
    }

    /// The process ended: every session is told, then forgotten.
    fn exit_all(&self, reason: &str) {
        let sinks: Vec<_> = {
            let mut inner = self.0.lock();
            inner.early.clear();
            inner.sinks.drain().map(|(_, s)| s).collect()
        };
        for sink in sinks {
            sink.on_exit(reason.to_owned());
        }
    }
}

pub(crate) struct AgentProcess {
    commands: mpsc::UnboundedSender<Command>,
    pub info: ProcessInfo,
    pub sessions: Arc<Sessions>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn gone<T>() -> Result<T, String> {
    Err("the agent process is not running".to_owned())
}

impl AgentProcess {
    pub fn is_alive(&self) -> bool {
        !self.task.is_finished() && !self.commands.is_closed()
    }

    /// End the process now (its child with it) and tell every session why.
    pub fn stop(&self, reason: &str) {
        self.task.abort();
        self.sessions.exit_all(reason);
    }

    /// Start the process and run `initialize`, giving the agent `timeout` to answer.
    pub async fn start(
        launch: AgentLaunch,
        permissions: Permissions,
        next_request: Arc<AtomicU64>,
        timeout: Duration,
    ) -> Result<Arc<Self>, String> {
        let (commands, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel::<Result<ProcessInfo, String>>();
        let sessions = Arc::new(Sessions::default());
        let task = tokio::spawn(serve(launch, rx, ready_tx, sessions.clone(), permissions, next_request));
        let info = match tokio::time::timeout(timeout, ready_rx).await {
            Ok(Ok(Ok(info))) => info,
            Ok(Ok(Err(e))) => return Err(e),
            Ok(Err(_)) => return Err("the agent process ended before it answered `initialize`".to_owned()),
            Err(_) => {
                task.abort(); // ends the child too
                return Err(format!("the agent did not answer `initialize` within {timeout:?}"));
            }
        };
        Ok(Arc::new(Self { commands, info, sessions, task }))
    }

    async fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (tx, rx) = oneshot::channel();
        if self.commands.send(make(tx)).is_err() {
            return gone();
        }
        rx.await.unwrap_or_else(|_| gone())
    }

    pub async fn new_session(&self, cwd: PathBuf, mcp: Vec<McpServer>) -> Result<NewSession, String> {
        self.ask(|reply| Command::NewSession { cwd, mcp, reply }).await
    }

    pub async fn prompt(&self, session: &str, text: &str) -> Result<StopReason, String> {
        self.ask(|reply| Command::Prompt { session: session.to_owned(), text: text.to_owned(), reply }).await
    }

    pub fn cancel(&self, session: &str) {
        let _ = self.commands.send(Command::Cancel { session: session.to_owned() });
    }

    pub async fn set_mode(&self, session: &str, mode: &str) -> Result<(), String> {
        self.ask(|reply| Command::SetMode { session: session.to_owned(), mode: mode.to_owned(), reply }).await
    }

    pub async fn set_config(
        &self,
        session: &str,
        id: &str,
        value: ConfigValue,
    ) -> Result<Vec<ConfigOption>, String> {
        self.ask(|reply| Command::SetConfig { session: session.to_owned(), id: id.to_owned(), value, reply })
            .await
    }
}

fn err(e: &agent_client_protocol::Error) -> String {
    e.to_string()
}

/// Run the connection until the command channel closes or the process ends, then tell the sessions.
async fn serve(
    launch: AgentLaunch,
    mut rx: mpsc::UnboundedReceiver<Command>,
    ready: oneshot::Sender<Result<ProcessInfo, String>>,
    sessions: Arc<Sessions>,
    permissions: Permissions,
    next_request: Arc<AtomicU64>,
) {
    let notify_sessions = sessions.clone();
    let request_sessions = sessions.clone();
    let connection = Client
        .v1()
        .name("vos")
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                if let Some(event) = map::event_of(&n) {
                    notify_sessions.deliver(&n.session_id.to_string(), event);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, cx| {
                let session = req.session_id.to_string();
                let Some(sink) = request_sessions.sink(&session) else {
                    return responder
                        .respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled));
                };
                let id = format!("perm-{}", next_request.fetch_add(1, Ordering::SeqCst));
                let request = map::permission_request(id.clone(), &req);
                if let PermissionDecision::Auto(option) = sink.on_permission(&request) {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
                    ));
                }
                // the person is asked: wait for the answer off the dispatch loop, which must keep running
                let (tx, answer) = oneshot::channel();
                permissions.lock().insert(id.clone(), tx);
                let permissions = permissions.clone();
                cx.spawn(async move {
                    let outcome =
                        match tokio::time::timeout(Duration::from_millis(PERMISSION_TTL_MS), answer).await {
                            Ok(Ok(PermissionAnswer::Selected(option))) => {
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option))
                            }
                            Ok(_) => RequestPermissionOutcome::Cancelled,
                            Err(_) => {
                                // nobody answered in time
                                sink.on_event(AgentEvent::PermissionClosed { request_id: id.clone() });
                                RequestPermissionOutcome::Cancelled
                            }
                        };
                    permissions.lock().remove(&id);
                    responder.respond(RequestPermissionResponse::new(outcome))
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(launch.acp_agent(), async move |cx: ConnectionTo<Agent>| {
            let init = match cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await {
                Ok(init) => init,
                Err(e) => {
                    let _ = ready.send(Err(format!("initialize failed: {}", err(&e))));
                    return Err(e);
                }
            };
            let info = ProcessInfo {
                agent: init
                    .agent_info
                    .as_ref()
                    .map(|i| AgentInfo { name: i.name.clone(), version: i.version.clone() }),
                http_mcp: init.agent_capabilities.mcp_capabilities.http,
            };
            let _ = ready.send(Ok(info));
            while let Some(command) = rx.recv().await {
                run_command(&cx, command)?;
            }
            Ok(())
        });
    // a panic in the connection (in a handler, or in what it calls on the agent's data) ends it like the process
    // ending: every session is told, so its chat starts a fresh one, and the connector starts the agent again
    let reason = match vos_application::panics::caught(connection).await {
        Ok(Ok(())) => "the agent process stopped".to_owned(),
        Ok(Err(e)) => format!("the agent process ended: {}", err(&e)),
        Err(message) => {
            tracing::error!(%message, "the agent connection panicked");
            format!("the agent connection failed: the host panicked: {message}")
        }
    };
    sessions.exit_all(&reason);
}

/// Start one command without waiting for it: prompts take as long as the turn, and a cancel must not queue
/// behind them.
fn run_command(cx: &ConnectionTo<Agent>, command: Command) -> Result<(), agent_client_protocol::Error> {
    let cx2 = cx.clone();
    match command {
        Command::Cancel { session } => cx.send_notification(CancelNotification::new(session)),
        Command::NewSession { cwd, mcp, reply } => cx.spawn(async move {
            let r = cx2
                .send_request(NewSessionRequest::new(cwd).mcp_servers(mcp))
                .block_task()
                .await
                .map(|response| NewSession { id: response.session_id.to_string(), response })
                .map_err(|e| err(&e));
            let _ = reply.send(r);
            Ok(())
        }),
        Command::Prompt { session, text, reply } => cx.spawn(async move {
            let r = cx2
                .send_request(PromptRequest::new(session, vec![ContentBlock::Text(TextContent::new(text))]))
                .block_task()
                .await
                .map(|p| map::stop_reason(p.stop_reason))
                .map_err(|e| err(&e));
            let _ = reply.send(r);
            Ok(())
        }),
        Command::SetMode { session, mode, reply } => cx.spawn(async move {
            let r = cx2
                .send_request(SetSessionModeRequest::new(session, mode))
                .block_task()
                .await
                .map(|_| ())
                .map_err(|e| err(&e));
            let _ = reply.send(r);
            Ok(())
        }),
        Command::SetConfig { session, id, value, reply } => cx.spawn(async move {
            use agent_client_protocol::v1::schema::SessionConfigOptionValue;
            let value = match value {
                ConfigValue::Bool(value) => SessionConfigOptionValue::Boolean { value },
                ConfigValue::Text(v) => SessionConfigOptionValue::value_id(v),
            };
            let r = cx2
                .send_request(SetSessionConfigOptionRequest::new(session, id, value))
                .block_task()
                .await
                .map(|r| map::config_options(&r.config_options))
                .map_err(|e| err(&e));
            let _ = reply.send(r);
            Ok(())
        }),
    }
}

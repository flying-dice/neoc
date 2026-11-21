//! Driven adapter: ACP client. Starts agent processes, keeps them alive and speaks ACP v1 to them over stdio.
//! Implements the `AgentConnector` port.
//!
//! One process per agent contact serves all of that agent's sessions (one per chat thread). What a session
//! reports (`session/update`, permission requests) goes to the sink the application registered for it; a
//! permission request the policy does not answer waits, off the connection's dispatch loop, for
//! [`AgentConnector::answer_permission`] (or its time limit).

mod launch;
mod map;
mod process;

pub use launch::{handshake, AgentHello, AgentLaunch};

use vos_application::{
    ports::{AgentConnector, AgentEventSink, McpAccess, PortResult, SessionOpened},
    AgentProcesses, PortError,
};
use vos_domain::{
    thread::{ConfigOption, ConfigValue, PermissionAnswer, StopReason},
    Agent, AgentId,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use process::{AgentProcess, Permissions};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

/// How long an agent gets to answer `initialize` before the host gives up on it.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// One agent's process, behind a lock of its own: starting it (which can take as long as the program likes)
/// holds up only that agent.
type Slot = Arc<AsyncMutex<Option<Arc<AgentProcess>>>>;

pub struct AcpConnector {
    /// The working directory handed to every session.
    cwd: PathBuf,
    /// The slots by agent. The map's lock is held only to find or add a slot, never across a start.
    processes: Mutex<HashMap<AgentId, Slot>>,
    permissions: Permissions,
    next_request: Arc<AtomicU64>,
    initialize_timeout: Duration,
    stopped: Mutex<bool>,
}

impl AcpConnector {
    #[must_use]
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            processes: Mutex::default(),
            permissions: Arc::default(),
            next_request: Arc::default(),
            initialize_timeout: INITIALIZE_TIMEOUT,
            stopped: Mutex::new(false),
        }
    }

    /// Give an agent this long to answer `initialize` (default [`INITIALIZE_TIMEOUT`]).
    #[must_use]
    pub fn with_initialize_timeout(mut self, timeout: Duration) -> Self {
        self.initialize_timeout = timeout;
        self
    }

    fn slot(&self, agent: &AgentId) -> Slot {
        self.processes.lock().entry(agent.clone()).or_default().clone()
    }

    fn check_running(&self) -> PortResult<()> {
        if *self.stopped.lock() {
            return Err(PortError::Rejected("the host is shutting down".into()));
        }
        Ok(())
    }

    /// The agent's running process, started (and initialized) if there is none or it died.
    async fn process(&self, agent: &Agent) -> PortResult<Arc<AgentProcess>> {
        self.check_running()?;
        let slot = self.slot(&agent.id);
        let mut held = slot.lock().await;
        if let Some(p) = held.as_ref().filter(|p| p.is_alive()) {
            return Ok(p.clone());
        }
        *held = None;
        let started = AgentProcess::start(
            AgentLaunch::of(agent),
            self.permissions.clone(),
            self.next_request.clone(),
            self.initialize_timeout,
        )
        .await
        .map_err(|e| PortError::Rejected(format!("could not start `{}`: {e}", agent.runtime.command)))?;
        // a shutdown that came while it started: do not leave a process behind
        self.check_running()?;
        *held = Some(started.clone());
        Ok(started)
    }

    async fn running(&self, agent: &AgentId) -> PortResult<Arc<AgentProcess>> {
        let slot = self.processes.lock().get(agent).cloned();
        if let Some(slot) = slot {
            if let Some(p) = slot.lock().await.as_ref().filter(|p| p.is_alive()) {
                return Ok(p.clone());
            }
        }
        Err(PortError::Rejected("the agent process is not running".into()))
    }
}

#[async_trait]
impl AgentProcesses for AcpConnector {
    async fn stop_agent(&self, agent: &AgentId) {
        let slot = self.processes.lock().remove(agent);
        if let Some(slot) = slot {
            if let Some(process) = slot.lock().await.take() {
                process.stop("the agent was stopped by the host");
            }
        }
    }
}

fn rejected(e: String) -> PortError {
    PortError::Rejected(e)
}

#[async_trait]
impl AgentConnector for AcpConnector {
    async fn open_session(
        &self,
        agent: &Agent,
        mcp: Option<McpAccess>,
        sink: Arc<dyn AgentEventSink>,
    ) -> PortResult<SessionOpened> {
        let process = self.process(agent).await?;
        // tools only when the agent can reach an HTTP MCP server
        let tools = mcp.filter(|_| process.info.http_mcp);
        let servers = tools.iter().map(|a| map::mcp_server(&a.url, &a.token)).collect();
        let session = process.new_session(self.cwd.clone(), servers).await.map_err(rejected)?;
        process.sessions.register(&session.id, &sink);
        let mut state = map::session_state(&session.response, tools.is_some());
        state.agent.clone_from(&process.info.agent);
        Ok(SessionOpened { session_id: session.id, session: state })
    }

    async fn prompt(&self, agent: &AgentId, session_id: &str, text: &str) -> PortResult<StopReason> {
        self.running(agent).await?.prompt(session_id, text).await.map_err(rejected)
    }

    async fn cancel(&self, agent: &AgentId, session_id: &str) -> PortResult<()> {
        self.running(agent).await?.cancel(session_id);
        Ok(())
    }

    async fn answer_permission(&self, request_id: &str, answer: PermissionAnswer) {
        let waiting = self.permissions.lock().remove(request_id);
        if let Some(tx) = waiting {
            let _ = tx.send(answer);
        }
    }

    async fn set_mode(&self, agent: &AgentId, session_id: &str, mode: &str) -> PortResult<()> {
        self.running(agent).await?.set_mode(session_id, mode).await.map_err(rejected)
    }

    async fn set_config(
        &self,
        agent: &AgentId,
        session_id: &str,
        id: &str,
        value: ConfigValue,
    ) -> PortResult<Vec<ConfigOption>> {
        self.running(agent).await?.set_config(session_id, id, value).await.map_err(rejected)
    }

    async fn close_session(&self, agent: &AgentId, session_id: &str) {
        if let Ok(process) = self.running(agent).await {
            process.sessions.remove(session_id);
        }
    }

    async fn stop_all(&self) {
        *self.stopped.lock() = true;
        let slots: Vec<Slot> = self.processes.lock().drain().map(|(_, slot)| slot).collect();
        for slot in slots {
            if let Some(process) = slot.lock().await.take() {
                process.stop("the host is shutting down");
            }
        }
    }
}

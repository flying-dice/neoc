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

use agentos_application::{
    ports::{AgentConnector, AgentEventSink, McpAccess, PortResult, SessionOpened},
    PortError,
};
use agentos_domain::{
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
};
use tokio::sync::Mutex as AsyncMutex;

pub struct AcpConnector {
    /// The working directory handed to every session.
    cwd: PathBuf,
    processes: AsyncMutex<HashMap<AgentId, Arc<AgentProcess>>>,
    permissions: Permissions,
    next_request: Arc<AtomicU64>,
    stopped: Mutex<bool>,
}

impl AcpConnector {
    #[must_use]
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            processes: AsyncMutex::default(),
            permissions: Arc::default(),
            next_request: Arc::default(),
            stopped: Mutex::new(false),
        }
    }

    /// The agent's running process, started (and initialized) if there is none or it died.
    async fn process(&self, agent: &Agent) -> PortResult<Arc<AgentProcess>> {
        if *self.stopped.lock() {
            return Err(PortError::Rejected("the host is shutting down".into()));
        }
        let mut processes = self.processes.lock().await;
        if let Some(p) = processes.get(&agent.id) {
            if p.is_alive() {
                return Ok(p.clone());
            }
            processes.remove(&agent.id);
        }
        let started =
            AgentProcess::start(AgentLaunch::of(agent), self.permissions.clone(), self.next_request.clone())
                .await
                .map_err(|e| {
                    PortError::Rejected(format!("could not start `{}`: {e}", agent.runtime.command))
                })?;
        processes.insert(agent.id.clone(), started.clone());
        Ok(started)
    }

    async fn running(&self, agent: &AgentId) -> PortResult<Arc<AgentProcess>> {
        match self.processes.lock().await.get(agent) {
            Some(p) if p.is_alive() => Ok(p.clone()),
            _ => Err(PortError::Rejected("the agent process is not running".into())),
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
        // dropping a process aborts its task, which ends the child
        self.processes.lock().await.clear();
    }
}

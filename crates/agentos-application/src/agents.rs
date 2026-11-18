//! Agent contacts: the roster, per-agent grants and bearer tokens.
//!
//! An agent has an identity (`ctx.actor` for its tool calls), grants (deny by default) and MCP bearer
//! tokens. Two kinds of token resolve to the same identity:
//!
//! - a **persistent token**, shown once when it is minted (`create`, `rotate_token`) and kept only as a
//!   SHA-256 hash: for agents the host does not launch (a person's own MCP client);
//! - a **launch token**, minted for each session the host opens for an agent and kept only as a hash in
//!   memory, scoped to that session so it dies with it.
//!
//! The roster is mirrored in memory so [`AgentDirectory::authenticate`] (called on every MCP request) is
//! synchronous. Grants are read at call time: editing them takes effect on the next `tools/call`, and the
//! agent's sessions are told the tool list changed.

use crate::{
    error::PortError,
    ports::{AgentCard, AgentDirectory, AgentRoster, AgentStore, RandomSource, ToolRegistry},
    tokens::random_hex,
};
use agentos_domain::{
    hash_token, same_hash, Agent, AgentId, AgentIdentity, AgentRuntime, ApprovalPolicy, DomainError, Grants,
};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error("no agent `{0}`")]
    NotFound(String),
    #[error("an agent `{0}` already exists")]
    Exists(String),
}

/// What `agents.create` takes. `id` defaults to a slug of the name (made unique with a number).
#[derive(Clone, Debug, Default)]
pub struct NewAgent {
    pub id: Option<String>,
    pub name: String,
    pub avatar: Option<String>,
    pub role: String,
    pub runtime: AgentRuntime,
    pub grants: Grants,
    pub approval: ApprovalPolicy,
}

/// What `agents.update` may change (`None` leaves a field alone). Grants have their own call.
#[derive(Clone, Debug, Default)]
pub struct AgentPatch {
    pub name: Option<String>,
    /// `Some(None)` clears the avatar.
    pub avatar: Option<Option<String>>,
    pub role: Option<String>,
    pub runtime: Option<AgentRuntime>,
    pub approval: Option<ApprovalPolicy>,
}

struct LaunchToken {
    hash: String,
    agent: AgentId,
    scope: String,
}

#[derive(Default)]
struct Roster {
    agents: BTreeMap<AgentId, Agent>,
    persistent: Vec<(AgentId, String)>,
    launch: Vec<LaunchToken>,
}

/// The processes the host runs for agents. When an agent is deleted, or the way it is launched changes, the
/// process it runs in must not outlive that: a deleted agent keeps no process, and an edited command or
/// environment is never ignored because an old process is still serving.
///
/// (Kept beside the roster that drives it; it belongs with the other ports in `ports.rs`.)
#[async_trait]
pub trait AgentProcesses: Send + Sync {
    /// Stop this agent's process and tell its sessions it ended. A no-op when none is running; the next
    /// session starts a new one from the agent's current settings.
    async fn stop_agent(&self, agent: &AgentId);
}

pub struct AgentService {
    store: Arc<dyn AgentStore>,
    tools: Arc<dyn ToolRegistry>,
    random: Arc<dyn RandomSource>,
    roster: RwLock<Roster>,
    processes: Option<Arc<dyn AgentProcesses>>,
}

/// 192 bits of randomness as hex, prefixed so a leaked token is recognisable.
fn mint(random: &dyn RandomSource) -> String {
    format!("agentos_{}", random_hex(random, 24))
}

impl AgentService {
    #[must_use]
    pub fn new(
        store: Arc<dyn AgentStore>,
        tools: Arc<dyn ToolRegistry>,
        random: Arc<dyn RandomSource>,
    ) -> Self {
        Self { store, tools, random, roster: RwLock::default(), processes: None }
    }

    /// Stop an agent's process when the agent is deleted or its runtime (command, arguments, environment) changes.
    #[must_use]
    pub fn with_processes(mut self, processes: Arc<dyn AgentProcesses>) -> Self {
        self.processes = Some(processes);
        self
    }

    async fn stop_process(&self, id: &AgentId) {
        if let Some(processes) = &self.processes {
            processes.stop_agent(id).await;
        }
    }

    /// Read the roster from the store (at start, and in tests that share a store).
    pub async fn load(&self) -> Result<(), AgentError> {
        let agents = self.store.list().await?;
        let persistent = self.store.token_hashes().await?;
        let mut roster = self.roster.write();
        roster.agents = agents.into_iter().map(|a| (a.id.clone(), a)).collect();
        roster.persistent = persistent;
        Ok(())
    }

    #[must_use]
    pub fn list(&self) -> Vec<Agent> {
        self.roster.read().agents.values().cloned().collect()
    }

    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<Agent> {
        self.roster.read().agents.get(id).cloned()
    }

    fn require(&self, id: &AgentId) -> Result<Agent, AgentError> {
        self.get(id).ok_or_else(|| AgentError::NotFound(id.0.clone()))
    }

    /// Add an agent with no token. Returns it with the id it was given.
    pub async fn create(&self, new: NewAgent) -> Result<Agent, AgentError> {
        let id = match new.id {
            Some(id) => AgentId::parse(&id)?,
            None => self.free_id(&AgentId::slug(&new.name)),
        };
        let mut grants = new.grants;
        grants.normalize();
        let agent = Agent {
            id,
            name: new.name.trim().to_owned(),
            avatar: new.avatar,
            role: new.role,
            runtime: new.runtime,
            grants,
            approval: new.approval,
        };
        self.insert(agent, None).await
    }

    /// Add an agent from configuration or a test if it does not exist yet; an existing one is left exactly
    /// as the person edited it. `token` (a test's fixed bearer token) is stored hashed.
    pub async fn ensure(&self, agent: Agent, token: Option<&str>) -> Result<Agent, AgentError> {
        if let Some(existing) = self.get(&agent.id) {
            if let Some(token) = token {
                self.set_persistent_hash(&existing.id, &hash_token(token)).await?;
            }
            return Ok(existing);
        }
        let hash = token.map(hash_token);
        self.insert(agent, hash).await
    }

    async fn insert(&self, agent: Agent, token_hash: Option<String>) -> Result<Agent, AgentError> {
        agent.validate()?;
        if !self.store.insert(&agent, token_hash.as_deref()).await? {
            return Err(AgentError::Exists(agent.id.0));
        }
        let mut roster = self.roster.write();
        roster.agents.insert(agent.id.clone(), agent.clone());
        if let Some(hash) = token_hash {
            roster.persistent.push((agent.id.clone(), hash));
        }
        Ok(agent)
    }

    fn free_id(&self, base: &str) -> AgentId {
        let roster = self.roster.read();
        let taken = |s: &str| roster.agents.keys().any(|k| k.0 == s);
        let mut candidate = base.to_owned();
        let mut n = 2;
        while taken(&candidate) {
            let suffix = format!("-{n}");
            let keep = 40usize.saturating_sub(suffix.len());
            candidate = format!("{}{suffix}", base.chars().take(keep).collect::<String>());
            n += 1;
        }
        AgentId(candidate)
    }

    pub async fn update(&self, id: &AgentId, patch: AgentPatch) -> Result<Agent, AgentError> {
        let mut agent = self.require(id)?;
        let launch_before = agent.runtime.clone();
        if let Some(name) = patch.name {
            name.trim().clone_into(&mut agent.name);
        }
        if let Some(avatar) = patch.avatar {
            agent.avatar = avatar;
        }
        if let Some(role) = patch.role {
            agent.role = role;
        }
        if let Some(runtime) = patch.runtime {
            agent.runtime = runtime;
        }
        if let Some(approval) = patch.approval {
            agent.approval = approval;
        }
        agent.validate()?;
        self.store_update(&agent).await?;
        if agent.runtime != launch_before {
            self.stop_process(id).await;
        }
        Ok(agent)
    }

    /// Replace what the agent may call. Its sessions are told the tool list changed.
    pub async fn set_grants(&self, id: &AgentId, mut grants: Grants) -> Result<Agent, AgentError> {
        let mut agent = self.require(id)?;
        grants.normalize();
        if agent.grants == grants {
            return Ok(agent);
        }
        agent.grants = grants;
        self.store_update(&agent).await?;
        if let Err(e) = self.tools.visibility_changed(id).await {
            tracing::warn!(agent = %id.0, error = %e, "could not tell the agent its tool list changed");
        }
        Ok(agent)
    }

    async fn store_update(&self, agent: &Agent) -> Result<(), AgentError> {
        if !self.store.update(agent).await? {
            return Err(AgentError::NotFound(agent.id.0.clone()));
        }
        self.roster.write().agents.insert(agent.id.clone(), agent.clone());
        Ok(())
    }

    /// Remove the agent, its tokens and its sessions' access.
    pub async fn delete(&self, id: &AgentId) -> Result<(), AgentError> {
        if !self.store.delete(id).await? {
            return Err(AgentError::NotFound(id.0.clone()));
        }
        {
            let mut roster = self.roster.write();
            roster.agents.remove(id);
            roster.persistent.retain(|(a, _)| a != id);
            roster.launch.retain(|t| &t.agent != id);
        }
        self.stop_process(id).await;
        if let Err(e) = self.tools.visibility_changed(id).await {
            tracing::warn!(agent = %id.0, error = %e, "could not tell the agent its tool list changed");
        }
        Ok(())
    }

    /// Mint a new persistent token, replacing the old one. The plaintext is returned once and never stored.
    pub async fn rotate_token(&self, id: &AgentId) -> Result<String, AgentError> {
        self.require(id)?;
        let token = mint(self.random.as_ref());
        self.set_persistent_hash(id, &hash_token(&token)).await?;
        Ok(token)
    }

    async fn set_persistent_hash(&self, id: &AgentId, hash: &str) -> Result<(), AgentError> {
        if !self.store.set_token_hash(id, hash).await? {
            return Err(AgentError::NotFound(id.0.clone()));
        }
        let mut roster = self.roster.write();
        roster.persistent.retain(|(a, _)| a != id);
        roster.persistent.push((id.clone(), hash.to_owned()));
        Ok(())
    }

    /// A fresh bearer token for a session the host opens for `agent`. It is valid until
    /// [`AgentService::revoke_launch_tokens`] is called with the same `scope` (or the agent is deleted).
    pub fn launch_token(&self, agent: &AgentId, scope: &str) -> Result<String, AgentError> {
        self.require(agent)?;
        let token = mint(self.random.as_ref());
        self.roster.write().launch.push(LaunchToken {
            hash: hash_token(&token),
            agent: agent.clone(),
            scope: scope.to_owned(),
        });
        Ok(token)
    }

    pub fn revoke_launch_tokens(&self, scope: &str) {
        self.roster.write().launch.retain(|t| t.scope != scope);
    }
}

impl AgentDirectory for AgentService {
    fn authenticate(&self, bearer_token: &str) -> Option<AgentIdentity> {
        let hash = hash_token(bearer_token);
        let roster = self.roster.read();
        // compare every entry: no early exit on the first match
        let mut found: Option<&AgentId> = None;
        for (agent, stored) in &roster.persistent {
            if same_hash(stored, &hash) {
                found = Some(agent);
            }
        }
        for token in &roster.launch {
            if same_hash(&token.hash, &hash) {
                found = Some(&token.agent);
            }
        }
        found.and_then(|id| roster.agents.get(id)).map(Agent::identity)
    }
}

impl AgentRoster for AgentService {
    fn cards(&self) -> Vec<AgentCard> {
        self.list()
            .into_iter()
            .map(|a| AgentCard { id: a.id, name: a.name, avatar: a.avatar, role: a.role })
            .collect()
    }
}

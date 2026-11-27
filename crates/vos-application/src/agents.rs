//! Agent contacts: the roster, per-agent grants and bearer tokens.
//!
//! An agent has an identity (`ctx.actor` for its tool calls), grants (deny by default) and an MCP bearer
//! token. The token is shown once when it is minted (`rotate_token`) and kept only as a SHA-256 hash.
//!
//! The roster is mirrored in memory so [`AgentDirectory::authenticate`] (called on every MCP request) is
//! synchronous. Grants are read at call time: editing them takes effect on the next `tools/call`, and the
//! agent's sessions are told the tool list changed.
//!
//! An app may also lease a token for an agent of its own ([`ToolLeases`]): kept in memory beside the roster, never
//! stored and never listed, it authenticates as the agent `app.tools.call` names and goes when the app releases it
//! or its backend stops.

use crate::{
    error::PortError,
    ports::{
        AgentCard, AgentDirectory, AgentRoster, AgentStore, Lease, RandomSource, ToolLeases, ToolRegistry,
    },
    tokens::random_hex,
};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::Arc,
};
use vos_domain::{
    hash_token, same_hash, Actor, Agent, AgentId, AgentIdentity, AppId, DomainError, Grants, ToolGrant,
};

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
    pub grants: Grants,
}

/// What `agents.update` may change (`None` leaves a field alone). Grants have their own call.
#[derive(Clone, Debug, Default)]
pub struct AgentPatch {
    pub name: Option<String>,
    /// `Some(None)` clears the avatar.
    pub avatar: Option<Option<String>>,
    pub role: Option<String>,
}

#[derive(Default)]
struct Roster {
    agents: BTreeMap<AgentId, Agent>,
    persistent: Vec<(AgentId, String)>,
    /// The tokens apps leased for agents of their own: in memory only.
    leases: Vec<Leased>,
}

/// A token an app leased: the hash of the token, and who it authenticates as.
struct Leased {
    app: AppId,
    hash: String,
    identity: AgentIdentity,
}

/// Where the host serves `/mcp`: what a lease's URL names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpEndpoint {
    /// `http`, or `https` when the host serves TLS.
    pub scheme: String,
    /// The host's own address, for a lease that names none.
    pub host: String,
    pub port: u16,
}

impl McpEndpoint {
    /// `<scheme>://<host>:<port>/mcp` for `host` (a name or an IP address), or the host's own address; an IPv6
    /// address is bracketed.
    pub fn url(&self, host: Option<&str>) -> Result<String, PortError> {
        let host = host.unwrap_or(&self.host);
        let host = url_host(host)
            .ok_or_else(|| PortError::Rejected(format!("`{host}` is not a host name or an IP address")))?;
        Ok(format!("{}://{host}:{}/mcp", self.scheme, self.port))
    }
}

/// `host` as a URL names it: an IP address (an IPv6 one in brackets, given with or without them), or a DNS name of
/// dot-separated labels of letters, digits and `-`. Anything else is `None`.
fn url_host(host: &str) -> Option<String> {
    let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => return Some(v4.to_string()),
        Ok(IpAddr::V6(v6)) => return Some(format!("[{v6}]")),
        Err(_) => {}
    }
    let label = |l: &str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    (host.len() <= 253 && host.split('.').all(label)).then(|| host.to_owned())
}

pub struct AgentService {
    store: Arc<dyn AgentStore>,
    tools: Arc<dyn ToolRegistry>,
    random: Arc<dyn RandomSource>,
    roster: RwLock<Roster>,
    /// Where `/mcp` is, for a lease's URL (none: the host leases nothing).
    mcp: Option<McpEndpoint>,
}

/// A bearer token: 192 bits of randomness as hex, prefixed so a leaked token is recognisable. Every agent token
/// is minted here, the host's own local MCP client's included.
#[must_use]
pub fn mint_agent_token(random: &dyn RandomSource) -> String {
    format!("vos_{}", random_hex(random, 24))
}

impl AgentService {
    #[must_use]
    pub fn new(
        store: Arc<dyn AgentStore>,
        tools: Arc<dyn ToolRegistry>,
        random: Arc<dyn RandomSource>,
    ) -> Self {
        Self { store, tools, random, roster: RwLock::default(), mcp: None }
    }

    /// Where the host serves `/mcp`: what the URL of a token an app leases names.
    #[must_use]
    pub fn with_mcp(mut self, endpoint: McpEndpoint) -> Self {
        self.mcp = Some(endpoint);
        self
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
        let agent =
            Agent { id, name: new.name.trim().to_owned(), avatar: new.avatar, role: new.role, grants };
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
        if let Some(name) = patch.name {
            name.trim().clone_into(&mut agent.name);
        }
        if let Some(avatar) = patch.avatar {
            agent.avatar = avatar;
        }
        if let Some(role) = patch.role {
            agent.role = role;
        }
        agent.validate()?;
        self.store_update(&agent).await?;
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
        self.tell(id).await;
        Ok(agent)
    }

    /// Tell the sessions of `id` that what it may call changed.
    async fn tell(&self, id: &AgentId) {
        if let Err(e) = self.tools.visibility_changed(id).await {
            tracing::warn!(agent = %id.0, error = %e, "could not tell the agent its tool list changed");
        }
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
        }
        self.tell(id).await;
        Ok(())
    }

    /// Mint a new persistent token, replacing the old one. The plaintext is returned once and never stored.
    pub async fn rotate_token(&self, id: &AgentId) -> Result<String, AgentError> {
        self.require(id)?;
        let token = mint_agent_token(self.random.as_ref());
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
        let mut leased: Option<&AgentIdentity> = None;
        for lease in &roster.leases {
            if same_hash(&lease.hash, &hash) {
                leased = Some(&lease.identity);
            }
        }
        found.and_then(|id| roster.agents.get(id)).map(Agent::identity).or_else(|| leased.cloned())
    }
}

#[async_trait]
impl ToolLeases for AgentService {
    fn lease(&self, app: &AppId, agent: &Actor, host: Option<&str>) -> Result<Lease, PortError> {
        let mcp = self.mcp.as_ref().ok_or_else(|| PortError::NotFound("this host serves no /mcp".into()))?;
        let url = mcp.url(host)?;
        let token = mint_agent_token(self.random.as_ref());
        // the agent `app.tools.call` names, offered what an app offers its agents: every app's tools but its own
        let identity = AgentIdentity {
            id: AgentId(agent.id.clone()),
            name: agent.name.clone(),
            grants: vec![ToolGrant::CallAnyToolBut(app.clone())],
        };
        self.roster.write().leases.push(Leased { app: app.clone(), hash: hash_token(&token), identity });
        Ok(Lease { token, url })
    }

    async fn release(&self, app: &AppId, token: &str) -> bool {
        let hash = hash_token(token);
        let released = {
            let mut roster = self.roster.write();
            let at = roster.leases.iter().position(|l| l.app == *app && same_hash(&l.hash, &hash));
            at.map(|at| roster.leases.remove(at))
        };
        match released {
            Some(lease) => {
                self.tell(&lease.identity.id).await;
                true
            }
            None => false,
        }
    }

    async fn release_all(&self, app: &AppId) {
        let released: BTreeSet<AgentId> = {
            let mut roster = self.roster.write();
            let (gone, kept): (Vec<Leased>, Vec<Leased>) =
                std::mem::take(&mut roster.leases).into_iter().partition(|l| l.app == *app);
            roster.leases = kept;
            gone.into_iter().map(|l| l.identity.id).collect()
        };
        for id in &released {
            self.tell(id).await;
        }
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

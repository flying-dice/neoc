//! The system database's agent contacts (`agents`) and chat transcripts (`threads`), behind the `AgentStore`
//! and `ThreadStore` ports. A bearer token is stored only as its SHA-256 hash.

use crate::devices::{sql_ms, SqliteSystemDb};
use agentos_application::{
    ports::{AgentStore, PortResult, StoredThread, ThreadStore},
    PortError,
};
use agentos_domain::{thread::ThreadMessage, Agent, AgentId, AgentRuntime, ApprovalPolicy, Grants};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Contacts. `args`, `env` and `grants` are JSON text; `token_hash` is NULL until a token is minted.
pub(crate) const AGENTS: &str = "
CREATE TABLE agents(
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    avatar     TEXT,
    role       TEXT NOT NULL DEFAULT '',
    command    TEXT NOT NULL DEFAULT '',
    args       TEXT NOT NULL DEFAULT '[]',
    env        TEXT NOT NULL DEFAULT '{}',
    grants     TEXT NOT NULL DEFAULT '{\"all\":false,\"apps\":[]}',
    approval   TEXT NOT NULL DEFAULT 'auto_read_only',
    token_hash TEXT,
    created_at INTEGER NOT NULL DEFAULT (CAST(strftime('%s','now') AS INTEGER) * 1000)
);";

/// A chat transcript per thread: the whole AG-UI message list as JSON, rewritten when a turn ends.
pub(crate) const THREADS: &str = "
CREATE TABLE threads(
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    messages   TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);";

const COLUMNS: &str = "id, name, avatar, role, command, args, env, grants, approval";

fn to_agent(row: &Value) -> Option<Agent> {
    Some(Agent {
        id: AgentId(row["id"].as_str()?.to_owned()),
        name: row["name"].as_str()?.to_owned(),
        avatar: row["avatar"].as_str().map(str::to_owned),
        role: row["role"].as_str()?.to_owned(),
        runtime: AgentRuntime {
            command: row["command"].as_str()?.to_owned(),
            args: serde_json::from_str(row["args"].as_str()?).ok()?,
            env: serde_json::from_str(row["env"].as_str()?).ok()?,
        },
        grants: serde_json::from_str::<Grants>(row["grants"].as_str()?).ok()?,
        approval: ApprovalPolicy::parse(row["approval"].as_str()?)?,
    })
}

fn json_text(value: &impl serde::Serialize) -> PortResult<Value> {
    serde_json::to_string(value).map(Value::String).map_err(|e| PortError::Internal(e.to_string()))
}

fn fields(agent: &Agent) -> PortResult<Vec<Value>> {
    Ok(vec![
        json!(agent.id.0),
        json!(agent.name),
        agent.avatar.as_ref().map_or(Value::Null, |a| json!(a)),
        json!(agent.role),
        json!(agent.runtime.command),
        json_text(&agent.runtime.args)?,
        json_text(&agent.runtime.env)?,
        json_text(&agent.grants)?,
        json!(agent.approval.as_str()),
    ])
}

#[async_trait]
impl AgentStore for SqliteSystemDb {
    async fn list(&self) -> PortResult<Vec<Agent>> {
        let rows = self.db.query(format!("SELECT {COLUMNS} FROM agents ORDER BY rowid"), vec![]).await?;
        // a row that no longer parses is skipped, not fatal (the rules may have tightened since)
        Ok(rows.iter().filter_map(to_agent).collect())
    }

    async fn insert(&self, agent: &Agent, token_hash: Option<&str>) -> PortResult<bool> {
        let mut params = fields(agent)?;
        params.push(token_hash.map_or(Value::Null, |h| json!(h)));
        let done = self
            .db
            .exec(
                format!(
                    "INSERT OR IGNORE INTO agents({COLUMNS}, token_hash) \
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
                ),
                params,
            )
            .await?;
        Ok(done.changes == 1)
    }

    async fn update(&self, agent: &Agent) -> PortResult<bool> {
        let done = self
            .db
            .exec(
                "UPDATE agents SET name = ?2, avatar = ?3, role = ?4, command = ?5, args = ?6, env = ?7, \
                 grants = ?8, approval = ?9 WHERE id = ?1"
                    .into(),
                fields(agent)?,
            )
            .await?;
        Ok(done.changes == 1)
    }

    async fn delete(&self, id: &AgentId) -> PortResult<bool> {
        let done = self.db.exec("DELETE FROM agents WHERE id = ?1".into(), vec![json!(id.0)]).await?;
        Ok(done.changes == 1)
    }

    async fn set_token_hash(&self, id: &AgentId, token_hash: &str) -> PortResult<bool> {
        let done = self
            .db
            .exec(
                "UPDATE agents SET token_hash = ?2 WHERE id = ?1".into(),
                vec![json!(id.0), json!(token_hash)],
            )
            .await?;
        Ok(done.changes == 1)
    }

    async fn token_hashes(&self) -> PortResult<Vec<(AgentId, String)>> {
        let rows = self
            .db
            .query("SELECT id, token_hash FROM agents WHERE token_hash IS NOT NULL".into(), vec![])
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some((AgentId(r["id"].as_str()?.to_owned()), r["token_hash"].as_str()?.to_owned()))
            })
            .collect())
    }
}

#[async_trait]
impl ThreadStore for SqliteSystemDb {
    async fn load(&self, thread_id: &str) -> PortResult<Option<StoredThread>> {
        let rows = self
            .db
            .query("SELECT agent_id, messages FROM threads WHERE id = ?1".into(), vec![json!(thread_id)])
            .await?;
        let Some(row) = rows.first() else { return Ok(None) };
        let (Some(agent), Some(text)) = (row["agent_id"].as_str(), row["messages"].as_str()) else {
            return Ok(None);
        };
        // an unreadable transcript is dropped rather than blocking the thread
        let messages: Vec<ThreadMessage> = serde_json::from_str(text).unwrap_or_default();
        Ok(Some(StoredThread { agent_id: AgentId(agent.to_owned()), messages }))
    }

    async fn save(&self, thread_id: &str, thread: &StoredThread, now_ms: u64) -> PortResult<()> {
        self.db
            .exec(
                "INSERT INTO threads(id, agent_id, messages, updated_at) VALUES(?1, ?2, ?3, ?4) \
                 ON CONFLICT(id) DO UPDATE SET messages = excluded.messages, updated_at = excluded.updated_at"
                    .into(),
                vec![json!(thread_id), json!(thread.agent_id.0), json_text(&thread.messages)?, sql_ms(now_ms)],
            )
            .await?;
        Ok(())
    }

    async fn delete(&self, thread_id: &str) -> PortResult<()> {
        self.db.exec("DELETE FROM threads WHERE id = ?1".into(), vec![json!(thread_id)]).await?;
        Ok(())
    }
}

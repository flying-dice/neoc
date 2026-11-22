//! The system database's agent contacts (`agents`), behind the `AgentStore` port. A bearer token is stored only
//! as its SHA-256 hash.

use crate::devices::SqliteSystemDb;
use async_trait::async_trait;
use serde_json::{json, Value};
use vos_application::{
    ports::{AgentStore, PortResult},
    PortError,
};
use vos_domain::{Agent, AgentId, Grants};

/// Contacts. `grants` is JSON text; `token_hash` is NULL until a token is minted. `command`, `args`, `env` and
/// `approval` are from when the host launched agents itself: nothing reads or writes them any more (an insert
/// leaves them at their defaults, an update leaves them as they are). They stay because migrations are numbered
/// and checksummed, so this statement cannot change.
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

/// The chat transcripts the host kept when it ran chat itself. Nothing reads or writes the table any more: the
/// migration stays in the list so that the numbering of the system database's migrations does not change, and a
/// database that already has the table is left as it is.
pub(crate) const THREADS: &str = "
CREATE TABLE threads(
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    messages   TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);";

const COLUMNS: &str = "id, name, avatar, role, grants";

fn to_agent(row: &Value) -> Option<Agent> {
    Some(Agent {
        id: AgentId(row["id"].as_str()?.to_owned()),
        name: row["name"].as_str()?.to_owned(),
        avatar: row["avatar"].as_str().map(str::to_owned),
        role: row["role"].as_str()?.to_owned(),
        grants: serde_json::from_str::<Grants>(row["grants"].as_str()?).ok()?,
    })
}

fn fields(agent: &Agent) -> PortResult<Vec<Value>> {
    Ok(vec![
        json!(agent.id.0),
        json!(agent.name),
        agent.avatar.as_ref().map_or(Value::Null, |a| json!(a)),
        json!(agent.role),
        serde_json::to_string(&agent.grants)
            .map(Value::String)
            .map_err(|e| PortError::Internal(e.to_string()))?,
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
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6)"
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
                "UPDATE agents SET name = ?2, avatar = ?3, role = ?4, grants = ?5 WHERE id = ?1".into(),
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

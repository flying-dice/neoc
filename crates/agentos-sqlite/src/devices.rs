//! The host's **system database** (`<data>/system.db`): paired devices and pairing codes, behind the
//! `DeviceRegistry` port.
//!
//! It reuses the app-database machinery (WAL, busy timeout, recorded migrations, work on the blocking
//! pool). The file is shared between the running host and the `agentos-host pair` CLI, so every
//! operation here is a single atomic statement.

use crate::{AppDb, DbError, Migration};
use agentos_application::{
    ports::{DeviceRegistry, InstalledApp, InstalledAppStore, PortResult},
    PortError,
};
use agentos_domain::{AppId, Device, DeviceId};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE pairing_codes(
    code_hash  TEXT PRIMARY KEY,
    expires_at INTEGER NOT NULL
);
CREATE TABLE sessions(
    id         TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen  INTEGER NOT NULL
);";

/// Which apps are installed, where from, and whether they are enabled: restored at every start.
const INSTALLED_APPS: &str = "
CREATE TABLE installed_apps(
    id           TEXT PRIMARY KEY,
    source_url   TEXT NOT NULL,
    version      TEXT NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    installed_at INTEGER NOT NULL
);";

/// What was approved at install (permissions as JSON) and the digest of the app ref that was loaded. Rows from
/// before get an empty digest, which the host reads as "adopt what the manifest asks for on the next load".
const INSTALLED_APPS_APPROVALS: &str = "
ALTER TABLE installed_apps ADD COLUMN permissions TEXT NOT NULL DEFAULT '[]';
ALTER TABLE installed_apps ADD COLUMN ref_sha256 TEXT NOT NULL DEFAULT '';";

/// The system database behind both `DeviceRegistry` and `InstalledAppStore`. The host and the CLI open the
/// same file, so they must agree on the migration list: it lives here.
pub struct SqliteSystemDb {
    pub(crate) db: AppDb,
}

/// The old name, from when the system database held only devices.
pub type SqliteDeviceRegistry = SqliteSystemDb;

impl SqliteSystemDb {
    /// Open (or create) the system database. `path = None` is in memory (tests).
    pub fn open(path: Option<&Path>) -> Result<Self, DbError> {
        let db = AppDb::open_system(
            path,
            &[
                Migration::new(1, "devices", SCHEMA),
                Migration::new(2, "installed_apps", INSTALLED_APPS),
                Migration::new(3, "agents", crate::agents::AGENTS),
                Migration::new(4, "threads", crate::agents::THREADS),
                Migration::new(5, "installed_apps_approvals", INSTALLED_APPS_APPROVALS),
            ],
        )?;
        Ok(Self { db })
    }
}

fn to_installed(row: &Value) -> Option<InstalledApp> {
    Some(InstalledApp {
        id: AppId::parse(row["id"].as_str()?).ok()?,
        source_url: row["source_url"].as_str()?.to_owned(),
        version: row["version"].as_str()?.to_owned(),
        enabled: row["enabled"].as_i64()? != 0,
        installed_at_ms: row["installed_at"].as_u64()?,
        permissions: serde_json::from_str(row["permissions"].as_str()?).ok()?,
        ref_sha256: row["ref_sha256"].as_str()?.to_owned(),
    })
}

#[async_trait]
impl InstalledAppStore for SqliteSystemDb {
    async fn save(&self, app: &InstalledApp) -> PortResult<()> {
        self.db
            .exec(
                "INSERT OR REPLACE INTO installed_apps(id, source_url, version, enabled, installed_at, \
                 permissions, ref_sha256) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                    .into(),
                vec![
                    json!(app.id.as_str()),
                    json!(app.source_url),
                    json!(app.version),
                    json!(i64::from(app.enabled)),
                    sql_ms(app.installed_at_ms),
                    json!(serde_json::to_string(&app.permissions)
                        .map_err(|e| PortError::Internal(e.to_string()))?),
                    json!(app.ref_sha256),
                ],
            )
            .await?;
        Ok(())
    }

    async fn remove(&self, id: &AppId) -> PortResult<()> {
        self.db.exec("DELETE FROM installed_apps WHERE id = ?1".into(), vec![json!(id.as_str())]).await?;
        Ok(())
    }

    async fn set_enabled(&self, id: &AppId, enabled: bool) -> PortResult<()> {
        self.db
            .exec(
                "UPDATE installed_apps SET enabled = ?2 WHERE id = ?1".into(),
                vec![json!(id.as_str()), json!(i64::from(enabled))],
            )
            .await?;
        Ok(())
    }

    async fn list(&self) -> PortResult<Vec<InstalledApp>> {
        let rows = self
            .db
            .query(
                "SELECT id, source_url, version, enabled, installed_at, permissions, ref_sha256 \
                 FROM installed_apps ORDER BY installed_at, id"
                    .into(),
                vec![],
            )
            .await?;
        // a row that no longer parses (an id the rules since forbade) is skipped, not fatal
        Ok(rows.iter().filter_map(to_installed).collect())
    }
}

fn to_device(row: &Value) -> Option<Device> {
    Some(Device {
        id: DeviceId(row["id"].as_str()?.to_owned()),
        name: row["name"].as_str()?.to_owned(),
        created_at: row["created_at"].as_u64()?,
        last_seen_at: row["last_seen"].as_u64()?,
    })
}

/// Unix milliseconds fit an i64 for the next 292 million years; saturate rather than wrap.
pub(crate) fn sql_ms(ms: u64) -> Value {
    json!(i64::try_from(ms).unwrap_or(i64::MAX))
}

#[async_trait]
impl DeviceRegistry for SqliteSystemDb {
    async fn add_pairing_code(&self, code_hash: &str, expires_at_ms: u64) -> PortResult<()> {
        self.db
            .exec(
                "INSERT OR REPLACE INTO pairing_codes(code_hash, expires_at) VALUES(?1, ?2)".into(),
                vec![json!(code_hash), sql_ms(expires_at_ms)],
            )
            .await?;
        Ok(())
    }

    async fn take_pairing_code(&self, code_hash: &str, now_ms: u64) -> PortResult<bool> {
        // expired codes are dead weight: sweep them on every attempt
        self.db.exec("DELETE FROM pairing_codes WHERE expires_at <= ?1".into(), vec![sql_ms(now_ms)]).await?;
        let r = self
            .db
            .exec(
                "DELETE FROM pairing_codes WHERE code_hash = ?1 AND expires_at > ?2".into(),
                vec![json!(code_hash), sql_ms(now_ms)],
            )
            .await?;
        Ok(r.changes == 1)
    }

    async fn clear_pairing_codes(&self) -> PortResult<()> {
        self.db.exec("DELETE FROM pairing_codes".into(), vec![]).await?;
        Ok(())
    }

    async fn add_session(&self, device: &Device, token_hash: &str) -> PortResult<()> {
        self.db
            .exec(
                "INSERT INTO sessions(id, token_hash, name, created_at, last_seen) VALUES(?1, ?2, ?3, ?4, ?5)"
                    .into(),
                vec![
                    json!(device.id.0),
                    json!(token_hash),
                    json!(device.name),
                    sql_ms(device.created_at),
                    sql_ms(device.last_seen_at),
                ],
            )
            .await?;
        Ok(())
    }

    async fn find_session(&self, token_hash: &str) -> PortResult<Option<Device>> {
        let rows = self
            .db
            .query(
                "SELECT id, name, created_at, last_seen FROM sessions WHERE token_hash = ?1".into(),
                vec![json!(token_hash)],
            )
            .await?;
        Ok(rows.first().and_then(to_device))
    }

    async fn touch_session(&self, id: &DeviceId, now_ms: u64) -> PortResult<()> {
        self.db
            .exec(
                "UPDATE sessions SET last_seen = ?2 WHERE id = ?1".into(),
                vec![json!(id.0), sql_ms(now_ms)],
            )
            .await?;
        Ok(())
    }

    async fn list_devices(&self) -> PortResult<Vec<Device>> {
        let rows = self
            .db
            .query(
                "SELECT id, name, created_at, last_seen FROM sessions ORDER BY created_at, id".into(),
                vec![],
            )
            .await?;
        rows.iter()
            .map(|r| to_device(r).ok_or_else(|| PortError::Internal("malformed session row".into())))
            .collect()
    }

    async fn revoke_device(&self, id: &DeviceId) -> PortResult<bool> {
        let r = self.db.exec("DELETE FROM sessions WHERE id = ?1".into(), vec![json!(id.0)]).await?;
        Ok(r.changes == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, created: u64) -> Device {
        Device {
            id: DeviceId(id.into()),
            name: format!("device {id}"),
            created_at: created,
            last_seen_at: created,
        }
    }

    #[tokio::test]
    async fn an_install_record_from_before_approvals_were_kept_reads_with_none() {
        let dir = std::env::temp_dir().join(format!("system-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("system.db");
        let old = AppDb::open_system(
            Some(&path),
            &[
                Migration::new(1, "devices", SCHEMA),
                Migration::new(2, "installed_apps", INSTALLED_APPS),
                Migration::new(3, "agents", crate::agents::AGENTS),
                Migration::new(4, "threads", crate::agents::THREADS),
            ],
        )
        .unwrap();
        old.exec(
            "INSERT INTO installed_apps(id, source_url, version, enabled, installed_at) \
             VALUES('notes', 'file:///x/app-ref.json', '1.0.0', 1, 7)"
                .into(),
            vec![],
        )
        .await
        .unwrap();
        drop(old);
        let listed = SqliteSystemDb::open(Some(&path)).unwrap().list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].permissions.clone(), listed[0].ref_sha256.as_str()), (vec![], ""));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_pairing_code_is_consumed_once_and_only_before_it_expires() {
        let reg = SqliteDeviceRegistry::open(None).unwrap();
        reg.add_pairing_code("h1", 5_000).await.unwrap();
        reg.add_pairing_code("h2", 5_000).await.unwrap();
        assert!(reg.take_pairing_code("h1", 4_999).await.unwrap());
        assert!(!reg.take_pairing_code("h1", 4_999).await.unwrap(), "single use");
        assert!(!reg.take_pairing_code("h2", 5_000).await.unwrap(), "expired exactly at its expiry");
        assert!(!reg.take_pairing_code("never-issued", 0).await.unwrap());
        reg.add_pairing_code("h3", u64::MAX).await.unwrap();
        reg.clear_pairing_codes().await.unwrap();
        assert!(!reg.take_pairing_code("h3", 0).await.unwrap());
    }

    #[tokio::test]
    async fn sessions_are_found_by_token_hash_listed_touched_and_revoked() {
        let reg = SqliteDeviceRegistry::open(None).unwrap();
        reg.add_session(&device("b", 2), "tok-b").await.unwrap();
        reg.add_session(&device("a", 1), "tok-a").await.unwrap();
        assert_eq!(reg.find_session("tok-a").await.unwrap(), Some(device("a", 1)));
        assert_eq!(reg.find_session("nope").await.unwrap(), None);
        reg.touch_session(&DeviceId("a".into()), 99).await.unwrap();
        let listed = reg.list_devices().await.unwrap();
        assert_eq!(listed.iter().map(|d| d.id.0.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(listed[0].last_seen_at, 99);
        assert!(reg.revoke_device(&DeviceId("a".into())).await.unwrap());
        assert!(!reg.revoke_device(&DeviceId("a".into())).await.unwrap());
        assert_eq!(reg.find_session("tok-a").await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_cli_and_the_host_share_one_file() {
        let dir = std::env::temp_dir().join(format!("system-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("system.db");
        let host = SqliteDeviceRegistry::open(Some(&path)).unwrap();
        let cli = SqliteDeviceRegistry::open(Some(&path)).unwrap(); // a second process, as far as SQLite knows
        cli.add_pairing_code("from-cli", u64::MAX).await.unwrap();
        assert!(host.take_pairing_code("from-cli", 1).await.unwrap());
        host.add_session(&device("d", 1), "t").await.unwrap();
        assert_eq!(cli.list_devices().await.unwrap().len(), 1);
        drop((host, cli));
        let _ = std::fs::remove_dir_all(dir);
    }
}

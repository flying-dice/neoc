#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the Lua adapter's integration tests: apps run against the REAL SQLite adapter.

use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc, time::Duration};
use vos_application::data::{ExecOutcome, Params, PortResult, QueryInfo, SqlMigration};
use vos_application::ports::{AppData, AppPackage, CallContext};
use vos_domain::{Actor, AppId};
use vos_lua::{AppVm, VmConfig};
use vos_sqlite::AppDb;

pub fn db() -> Arc<AppDb> {
    Arc::new(AppDb::open(None, &[]).unwrap())
}

pub fn id(name: &str) -> AppId {
    AppId::parse(name).unwrap()
}

/// A package whose entry is `app.lua` plus extra modules.
pub fn package(entry: &str, modules: &[(&str, &str)]) -> AppPackage {
    let mut files = BTreeMap::new();
    files.insert("app.lua".to_owned(), entry.as_bytes().to_vec());
    for (path, src) in modules {
        files.insert((*path).to_owned(), src.as_bytes().to_vec());
    }
    AppPackage { entry: "app.lua".into(), files }
}

pub fn config() -> VmConfig {
    VmConfig::default()
}

pub async fn try_load_with(name: &str, src: &str, db: Arc<dyn AppData>) -> vos_lua::Result<AppVm> {
    AppVm::load(&id(name), package(src, &[]), db, config()).await
}

pub async fn try_load(name: &str, src: &str) -> vos_lua::Result<AppVm> {
    try_load_with(name, src, db()).await
}

pub async fn load(src: &str) -> AppVm {
    try_load("probe", src).await.unwrap()
}

pub fn agent_ctx(now_ms: u64) -> CallContext {
    CallContext { actor: Actor::agent("claude", "Claude"), now_ms }
}

pub fn user_ctx(now_ms: u64) -> CallContext {
    CallContext { actor: Actor::user("You"), now_ms }
}

/// Run the tool `name` (no arguments) as an agent, giving up after `secs`. The error names what went wrong: the
/// call failed (the script raised), never finished (the app is wedged), or the host panicked (a panic in a call is a
/// failed join).
pub async fn timed_tool(vm: &Arc<AppVm>, name: &str, secs: u64) -> Result<Value, String> {
    let name = name.to_owned();
    let vm = vm.clone();
    let call = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(secs), vm.call_tool(&name, json!({}), &agent_ctx(0), false))
            .await
    });
    match call.await {
        Err(e) => Err(format!("(f) the call panicked or was aborted: {e}")),
        Ok(Err(_)) => Err("(e) the call never finished: the app is wedged".into()),
        Ok(Ok(Ok(v))) => Ok(v),
        Ok(Ok(Err(e))) => Err(format!("the call failed: {e}")),
    }
}

/// The bundled Lua 5.1 text of an app entry file, built with the product's own pipeline (`scripts/lua51.ts`: darklua
/// with `.darklua.json`), so these tests run what `bun run build:apps` ships. Needs bun and darklua.
pub fn build_lua(entry: &Path) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("vos-lua-test-{}-{}.lua", std::process::id(), nonce()));
    let run = Command::new("bun")
        .arg("scripts/lua51-build.ts")
        .arg(entry)
        .arg(&out)
        .current_dir(&root)
        .output()
        .expect("bun is needed to build the apps these tests run (https://bun.sh)");
    assert!(
        run.status.success(),
        "building {} failed (is darklua 0.18.0 installed? see scripts/lua51.ts):\n{}{}",
        entry.display(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let bytes = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_file(out);
    bytes
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// An app package of the one built file: `entry` is a Lua file (`apps/calculator/app.lua`, a codegen
/// fixture), bundled.
pub fn built_package(entry: &Path) -> AppPackage {
    let files = BTreeMap::from([("app.lua".to_owned(), build_lua(entry))]);
    AppPackage { entry: "app.lua".into(), files }
}

/// The shipped app `apps/<id>` (or, when the repo has none, the codegen example of that name), built.
pub fn built_app(app: &str) -> AppPackage {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let shipped = manifest.join("../../apps").join(app);
    let dir = if shipped.join("app.json").exists() {
        shipped
    } else {
        manifest.join("../vos-codegen/tests/fixtures").join(app)
    };
    built_package(&dir.join("app.lua"))
}

/// The real database, except that a statement whose SQL contains `stall` never finishes (so a test can hold a
/// call at a host call while it holds the VM's turn, and then give up on it).
pub struct Stalling(Arc<dyn AppData>);

impl Stalling {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(db()))
    }
}

async fn maybe_stall(sql: &str) {
    if sql.contains("stall") {
        std::future::pending::<()>().await;
    }
}

struct StallingTx(Box<dyn vos_application::data::DataTx>);

#[async_trait::async_trait]
impl vos_application::data::DataTx for StallingTx {
    async fn query(&mut self, sql: String, params: Params) -> PortResult<Vec<serde_json::Value>> {
        maybe_stall(&sql).await;
        self.0.query(sql, params).await
    }
    async fn exec(&mut self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        maybe_stall(&sql).await;
        self.0.exec(sql, params).await
    }
    async fn commit(self: Box<Self>) -> PortResult<()> {
        self.0.commit().await
    }
    async fn rollback(self: Box<Self>) -> PortResult<()> {
        self.0.rollback().await
    }
}

#[async_trait::async_trait]
impl AppData for Stalling {
    async fn migrate(&self, migrations: Vec<SqlMigration>) -> PortResult<()> {
        self.0.migrate(migrations).await
    }
    async fn query(&self, sql: String, params: Params) -> PortResult<Vec<serde_json::Value>> {
        maybe_stall(&sql).await;
        self.0.query(sql, params).await
    }
    async fn exec(&self, sql: String, params: Params) -> PortResult<ExecOutcome> {
        maybe_stall(&sql).await;
        self.0.exec(sql, params).await
    }
    async fn analyze(&self, sql: String) -> PortResult<QueryInfo> {
        self.0.analyze(sql).await
    }
    async fn begin(&self) -> PortResult<Box<dyn vos_application::data::DataTx>> {
        Ok(Box::new(StallingTx(self.0.begin().await?)))
    }
    fn changes(&self) -> Box<dyn vos_application::data::ChangeFeed> {
        self.0.changes()
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        self.0.read_only()
    }
    async fn ui_state(&self, window: &str) -> PortResult<serde_json::Map<String, serde_json::Value>> {
        self.0.ui_state(window).await
    }
    async fn set_ui_state(
        &self,
        window: &str,
        entries: serde_json::Map<String, serde_json::Value>,
    ) -> PortResult<()> {
        self.0.set_ui_state(window, entries).await
    }
    async fn clear_ui_state(&self) -> PortResult<()> {
        self.0.clear_ui_state().await
    }
    async fn record_revision(&self, revision: vos_domain::Revision) -> PortResult<i64> {
        self.0.record_revision(revision).await
    }
    async fn revision(&self, id: i64) -> PortResult<Option<vos_domain::StoredRevision>> {
        self.0.revision(id).await
    }
    async fn revisions(
        &self,
        source: &str,
        key: &serde_json::Value,
        field: &str,
    ) -> PortResult<Vec<vos_domain::StoredRevision>> {
        self.0.revisions(source, key, field).await
    }
    async fn purge_revisions(&self, cutoff_ms: u64) -> PortResult<u64> {
        self.0.purge_revisions(cutoff_ms).await
    }
}

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the Lua adapter's integration tests: apps run against the REAL SQLite adapter.

use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc, time::Duration};
use vos_application::data::{ExecOutcome, Params, PortResult, SqlMigration};
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

/// A package whose entry is `app.lua` plus extra modules. The entry is taken as it is: it must return the app's
/// handlers itself (see [`backend`]).
pub fn package(entry: &str, modules: &[(&str, &str)]) -> AppPackage {
    let mut files = BTreeMap::new();
    files.insert("app.lua".to_owned(), entry.as_bytes().to_vec());
    for (path, src) in modules {
        files.insert((*path).to_owned(), src.as_bytes().to_vec());
    }
    AppPackage { entry: "app.lua".into(), files }
}

/// `src` as a whole backend: what it declares (tools, hooks, tasks), then the handlers an entry must return, here
/// none. The return is on a line of its own after the source, so the line numbers of `src` are its own. For a
/// source that says nothing about handlers; one that returns its own is loaded with [`try_load_entry`].
pub fn backend(src: &str) -> String {
    format!("{src}\nreturn {{}}\n")
}

/// The config of these tests: the host's, plus the `hook(name, fn)` global `AppVm::emit` calls.
pub fn config() -> VmConfig {
    VmConfig { test_hooks: true, ..VmConfig::default() }
}

/// Load `src` followed by `return {}` ([`backend`]): an app with tools or hooks and no handlers.
pub async fn try_load_with(name: &str, src: &str, db: Arc<dyn AppData>) -> vos_lua::Result<AppVm> {
    AppVm::load(&id(name), package(&backend(src), &[]), db, config()).await
}

/// Load `src` exactly as it is: an entry that returns its handlers (or shows what happens when it does not).
pub async fn try_load_entry(name: &str, src: &str) -> vos_lua::Result<AppVm> {
    AppVm::load(&id(name), package(src, &[]), db(), config()).await
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

/// The bundled Luau text of an app entry file, built with the product's own pipeline (`scripts/luau.ts`: darklua
/// with `.darklua.json`), so these tests run what `bun run build:apps` ships. Needs bun and darklua.
pub fn build_luau(entry: &Path) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("vos-lua-test-{}-{}.luau", std::process::id(), nonce()));
    let run = Command::new("bun")
        .arg("scripts/luau-build.ts")
        .arg(entry)
        .arg(&out)
        .current_dir(&root)
        .output()
        .expect("bun is needed to build the apps these tests run (https://bun.sh)");
    assert!(
        run.status.success(),
        "building {} failed (is darklua 0.18.0 installed? see scripts/luau.ts):\n{}{}",
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

/// An app package of the one built file: `entry` is a Luau file (an app's generated `.app/entry.luau`), bundled.
pub fn built_package(entry: &Path) -> AppPackage {
    let files = BTreeMap::from([("app.luau".to_owned(), build_luau(entry))]);
    AppPackage { entry: "app.luau".into(), files }
}

/// The backend of the app project in `dir` (relative to the repository root: `apps/calculator`,
/// `e2e/apps/tasks-svelte`), built. Its generated files are not committed: they are generated first, once per
/// project, as the project's own scripts do. A backend written in TypeScript (`server/main.ts`) is compiled by the
/// plugin, as the app's own build compiles it.
pub fn built_project(dir: &str) -> AppPackage {
    use std::sync::Mutex;
    static GENERATED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    if project.join("server/main.ts").exists() {
        return built_typescript(&project);
    }
    {
        let mut generated = GENERATED.lock().unwrap();
        if !generated.iter().any(|done| done == dir) {
            let run = Command::new("bun")
                .args(["run", "generate"])
                .current_dir(&project)
                .output()
                .expect("bun is needed to generate the app's router (https://bun.sh)");
            assert!(
                run.status.success(),
                "bun run generate failed in {dir}:\n{}",
                String::from_utf8_lossy(&run.stderr)
            );
            generated.push(dir.to_owned());
        }
    }
    built_package(&project.join(".app/entry.luau"))
}

/// The backend of an app project written in TypeScript, as its build bundles it: `vos bundle` of the plugin
/// (`packages/vite-plugin/bin/vos.ts`) generates the project's `.app/` and compiles the backend to one Luau chunk.
/// Needs bun; no darklua.
fn built_typescript(project: &Path) -> AppPackage {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = tempfile::Builder::new().prefix("vos-lua-test-").suffix(".luau").tempfile().unwrap();
    let run = Command::new("bun")
        .arg(root.join("packages/vite-plugin/bin/vos.ts"))
        .arg("bundle")
        .arg(out.path())
        .current_dir(project)
        .output()
        .expect("bun is needed to build the apps these tests run (https://bun.sh)");
    assert!(
        run.status.success(),
        "compiling the TypeScript backend of {} failed:\n{}{}",
        project.display(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let files = BTreeMap::from([("app.luau".to_owned(), std::fs::read(out.path()).unwrap())]);
    AppPackage { entry: "app.luau".into(), files }
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
    async fn begin(&self) -> PortResult<Box<dyn vos_application::data::DataTx>> {
        Ok(Box::new(StallingTx(self.0.begin().await?)))
    }
    fn read_only(&self) -> Arc<dyn AppData> {
        self.0.read_only()
    }
}

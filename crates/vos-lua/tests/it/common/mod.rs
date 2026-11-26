#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the Lua adapter's integration tests: apps run against the REAL SQLite adapter.

use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc, time::Duration};
use vos_application::contract::AppContract;
use vos_application::data::{ExecOutcome, Params, PortResult, SqlMigration};
use vos_application::ports::{AppData, AppPackage, AppRuntime, CallContext};
use vos_application::PortError;
use vos_domain::{Actor, ActorKind, AppId};
use vos_lua::{AppVm, LuaRuntime, VmConfig};
use vos_sqlite::AppDb;

pub fn db() -> Arc<AppDb> {
    Arc::new(AppDb::open(None, &[]).unwrap())
}

pub fn id(name: &str) -> AppId {
    AppId::parse(name).unwrap()
}

/// A package whose entry is `app.lua` plus extra modules. The entry is taken as it is.
pub fn package(entry: &str, modules: &[(&str, &str)]) -> AppPackage {
    let mut files = BTreeMap::new();
    files.insert("app.lua".to_owned(), entry.as_bytes().to_vec());
    for (path, src) in modules {
        files.insert((*path).to_owned(), src.as_bytes().to_vec());
    }
    AppPackage { entry: "app.lua".into(), files }
}

/// The config of these tests: the host's, plus the `hook(name, fn)` global `AppVm::emit` calls.
pub fn config() -> VmConfig {
    VmConfig { test_hooks: true, ..VmConfig::default() }
}

/// Load `src`: an app with handlers or hooks.
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

/// Call the handler `name` on the bus as the app service does: the call is read-only only when `require_read_only`
/// says so (the caller decides; the VM no longer knows what a handler changes).
pub async fn serve_with(
    vm: &AppVm,
    name: &str,
    args: Value,
    ctx: &CallContext,
    require_read_only: bool,
) -> vos_lua::Result<Value> {
    vm.call_handler_with(name, args, ctx, require_read_only).await
}

/// [`serve_with`], with nothing required: the call may write.
pub async fn serve(vm: &AppVm, name: &str, args: Value, ctx: &CallContext) -> vos_lua::Result<Value> {
    serve_with(vm, name, args, ctx, false).await
}

/// [`serve_with`], read-only: every write the call tries is refused.
pub async fn serve_ro(vm: &AppVm, name: &str, args: Value, ctx: &CallContext) -> vos_lua::Result<Value> {
    serve_with(vm, name, args, ctx, true).await
}

/// Run the handler `name` (no arguments) as an agent, giving up after `secs`. The error names what went wrong: the
/// call failed (the script raised), never finished (the app is wedged), or the host panicked (a panic in a call is a
/// failed join).
pub async fn timed_tool(vm: &Arc<AppVm>, name: &str, secs: u64) -> Result<Value, String> {
    timed_tool_with(vm, name, secs, false).await
}

/// [`timed_tool`], read-only when `require_read_only` says so.
pub async fn timed_tool_with(
    vm: &Arc<AppVm>,
    name: &str,
    secs: u64,
    require_read_only: bool,
) -> Result<Value, String> {
    let name = name.to_owned();
    let vm = vm.clone();
    let call = tokio::spawn(async move {
        tokio::time::timeout(
            Duration::from_secs(secs),
            serve_with(&vm, &name, json!({}), &agent_ctx(0), require_read_only),
        )
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

/// An app package of the one built file: `entry` is a Luau file, bundled.
pub fn built_package(entry: &Path) -> AppPackage {
    let files = BTreeMap::from([("app.luau".to_owned(), build_luau(entry))]);
    AppPackage { entry: "app.luau".into(), files }
}

/// The backend of the app project in `dir` (relative to the repository root: `apps/calculator`,
/// `e2e/apps/tasks-svelte`), built, as an app package of the one file the host ships.
///
/// A project is built by `vite build` with `@vos/vite-plugin`, once per process, into a directory of this process's
/// own (`--outDir`): every test of `cargo nextest` is a process of its own, and two of them building the same
/// project into its `dist/` would write over each other. The built `app.luau` is the package: Luau or TypeScript,
/// bundled by the plugin.
pub fn built_project(dir: &str) -> AppPackage {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static BUILT: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    // the lock is held while building: a second test of the same project waits for the first one's build
    let mut built = BUILT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let bundle = built.get_or_insert_with(HashMap::new).entry(dir.to_owned()).or_insert_with(|| {
        let out = std::env::temp_dir().join(format!(
            "vos-lua-test-{}-{}",
            std::process::id(),
            dir.replace(['/', '\\'], "-")
        ));
        let run = Command::new("bunx")
            .args(["vite", "build", "--outDir"])
            .arg(&out)
            .current_dir(&project)
            .output()
            .expect("bun is needed to build the app (https://bun.sh)");
        assert!(
            run.status.success(),
            "vite build failed in {dir}:
{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        let bundle = std::fs::read(out.join("app.luau"))
            .unwrap_or_else(|e| panic!("{dir} built no app.luau in {}: {e}", out.display()));
        let _ = std::fs::remove_dir_all(&out);
        bundle
    });
    AppPackage { entry: "app.luau".into(), files: BTreeMap::from([("app.luau".to_owned(), bundle.clone())]) }
}

/// The contract (`openrpc.json`) of the app project in `dir`, as the RPC bus holds it.
pub fn contract(dir: &str) -> AppContract {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir).join("openrpc.json");
    AppContract::parse(&std::fs::read(&path).unwrap()).unwrap()
}

/// An app's backend served the way the app service serves it on the bus (`AppService::request_with`): a request the
/// contract does not declare reaches no handler (-32601), params it refuses reach none either (-32602), an agent's
/// call of a method the contract marks `x-readOnly` runs with every write refused, and the answer is held to the
/// contract (-32603). The backend and its contract are the app's own.
pub struct Served {
    pub rt: LuaRuntime,
    pub app: AppId,
    pub contract: AppContract,
}

impl Served {
    /// `dir`'s app, loaded as `app` on `data`.
    pub async fn load(rt: LuaRuntime, app: &str, dir: &str, data: Arc<dyn AppData>) -> PortResult<Self> {
        rt.load(&id(app), built_project(dir), data).await?;
        Ok(Self { rt, app: id(app), contract: contract(dir) })
    }

    /// The page's call: the person's own, so it may write.
    pub async fn page(&self, method: &str, params: Value) -> PortResult<Value> {
        self.request(user_ctx(0), method, params).await
    }

    /// An agent's call of a public method.
    pub async fn agent(&self, method: &str, params: Value) -> PortResult<Value> {
        self.request(agent_ctx(0), method, params).await
    }

    pub async fn request(&self, ctx: CallContext, method: &str, params: Value) -> PortResult<Value> {
        let Some(declared) = self.contract.request(method) else {
            return Err(PortError::Rpc {
                code: -32601,
                message: format!("Method not found: {method}"),
                data: None,
            });
        };
        if let Err(problem) = self.contract.check_params(method, &params) {
            return Err(PortError::Rpc {
                code: -32602,
                message: format!("Invalid params: {problem}"),
                data: None,
            });
        }
        // as `AppService::call_tool` has it: an agent's call of a read-only method is a read-only call
        let answer = if declared.read_only && ctx.actor.kind == ActorKind::Agent {
            self.rt.call_handler_read_only(&self.app, method, params, ctx).await?
        } else {
            self.rt.call_handler(&self.app, method, params, ctx).await?
        };
        if let Err(problem) = self.contract.check_result(method, &answer) {
            return Err(PortError::Rpc {
                code: -32603,
                message: format!(
                    "Internal error: the answer to {method} does not match the contract ({problem})"
                ),
                data: None,
            });
        }
        Ok(answer)
    }

    /// The methods the backend has on the bus, in the order it registered them.
    pub fn handlers(&self) -> Vec<String> {
        self.rt.handlers(&self.app)
    }
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

/// [`serve_with`] for the runtime (`AppRuntime`'s calls, as the app service makes them): a call that must be
/// read-only is made with `call_handler_read_only`, any other with `call_handler`.
pub async fn serve_rt_with(
    rt: &LuaRuntime,
    app: &AppId,
    method: &str,
    args: Value,
    ctx: CallContext,
    require_read_only: bool,
) -> PortResult<Value> {
    if require_read_only {
        rt.call_handler_read_only(app, method, args, ctx).await
    } else {
        rt.call_handler(app, method, args, ctx).await
    }
}

/// [`serve_rt_with`], with nothing required: the call may write.
pub async fn serve_rt(
    rt: &LuaRuntime,
    app: &AppId,
    method: &str,
    args: Value,
    ctx: CallContext,
) -> PortResult<Value> {
    serve_rt_with(rt, app, method, args, ctx, false).await
}

/// What the app asked the host to do since the last call, except that its handlers changed: every handler an app
/// registers says so (the host re-lists its agent tools), so each of these apps has said it once, at load.
pub fn take_effects(vm: &AppVm) -> Vec<vos_lua::Effect> {
    vm.take_effects().into_iter().filter(|e| !matches!(e, vos_lua::Effect::HandlersChanged)).collect()
}

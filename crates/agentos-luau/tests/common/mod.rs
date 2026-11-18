#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the Luau adapter's integration tests: apps run against the REAL SQLite adapter.

use agentos_application::ports::{AppData, AppPackage, CallContext};
use agentos_domain::{Actor, AppId, Capabilities};
use agentos_luau::{AppVm, Limits, VmConfig};
use agentos_sqlite::AppDb;
use std::{collections::BTreeMap, path::Path, process::Command, sync::Arc};

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

pub fn config(limits: Limits) -> VmConfig {
    VmConfig { capabilities: Capabilities::all(), limits, ..VmConfig::default() }
}

pub async fn try_load_with(
    name: &str,
    src: &str,
    db: Arc<dyn AppData>,
    limits: Limits,
) -> agentos_luau::Result<AppVm> {
    AppVm::load(&id(name), package(src, &[]), db, config(limits)).await
}

pub async fn try_load(name: &str, src: &str) -> agentos_luau::Result<AppVm> {
    try_load_with(name, src, db(), Limits::default()).await
}

pub async fn load(src: &str) -> AppVm {
    try_load("probe", src).await.unwrap()
}

pub fn quick(cpu_ticks: u64) -> Limits {
    Limits { cpu_ticks, ..Limits::default() }
}

pub fn agent_ctx(now_ms: u64) -> CallContext {
    CallContext { actor: Actor::agent("claude", "Claude"), now_ms }
}

pub fn user_ctx(now_ms: u64) -> CallContext {
    CallContext { actor: Actor::user("You"), now_ms }
}

/// The Lua 5.1 text of a Luau entry file, built with the product's own pipeline (`scripts/lua51.ts`: darklua
/// with `.darklua.json`), so these tests run what `bun run build:apps` ships. Needs bun and darklua.
pub fn build_lua(entry: &Path) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("agentos-luau-test-{}-{}.lua", std::process::id(), nonce()));
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

/// An app package of the one built file: `entry` is a Luau file (`apps/calculator/app.luau`, a codegen
/// fixture), built and bundled.
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
        manifest.join("../agentos-codegen/tests/fixtures").join(app)
    };
    built_package(&dir.join("app.lua"))
}

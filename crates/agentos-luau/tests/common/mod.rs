#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the Luau adapter's integration tests: apps run against the REAL SQLite adapter.

use agentos_application::ports::{AppData, AppPackage, CallContext};
use agentos_domain::{Actor, AppId, Capabilities};
use agentos_luau::{AppVm, Limits, VmConfig};
use agentos_sqlite::AppDb;
use std::{collections::BTreeMap, sync::Arc};

pub fn db() -> Arc<AppDb> {
    Arc::new(AppDb::open(None, &[]).unwrap())
}

pub fn id(name: &str) -> AppId {
    AppId::parse(name).unwrap()
}

/// A package whose entry is `app.luau` plus extra modules.
pub fn package(entry: &str, modules: &[(&str, &str)]) -> AppPackage {
    let mut files = BTreeMap::new();
    files.insert("app.luau".to_owned(), entry.as_bytes().to_vec());
    for (path, src) in modules {
        files.insert((*path).to_owned(), src.as_bytes().to_vec());
    }
    AppPackage { entry: "app.luau".into(), files }
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

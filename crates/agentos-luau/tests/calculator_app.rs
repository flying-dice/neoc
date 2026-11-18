#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The shipped Calculator (`apps/calculator`, not a fixture) under the real runtime: its arithmetic, its
//! number formatting through the tools, and what its tools declare to agents.

mod common;

use agentos_application::ports::{AppData, AppPackage, AppRuntime, CallContext, Notifier, PortResult};
use agentos_domain::Capabilities;
use agentos_luau::{Limits, LuauRuntime};
use common::{agent_ctx, db, id};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, sync::Arc};

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: agentos_domain::Notification) -> PortResult<()> {
        Ok(())
    }
}

async fn calculator() -> LuauRuntime {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/calculator");
    let mut package = AppPackage { entry: "app.luau".into(), files: BTreeMap::new() };
    for name in ["app.luau", "calc.luau"] {
        package.files.insert(name.to_owned(), std::fs::read(dir.join(name)).unwrap());
    }
    let runtime = LuauRuntime::new(
        Arc::new(Quiet),
        Arc::new(agentos_application::testing::RecordingShell::default()),
        Limits::default(),
    );
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("calculator"), package, data, Capabilities::all()).await.unwrap();
    runtime
}

async fn tool(rt: &LuauRuntime, name: &str, args: Value, ctx: CallContext) -> PortResult<Value> {
    rt.call_tool(&id("calculator"), name, args, ctx).await
}

#[tokio::test]
async fn calculate_evaluates_exactly_and_rejects_what_is_not_arithmetic() {
    let rt = calculator().await;
    for (expression, result) in [
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("2 ^ 3 ^ 2", 512.0),
        ("-4 + 10", 6.0),
        ("-(2 + 3)", -5.0),
        ("10 % 4", 2.0),
        ("7 ÷ 2", 3.5),
        ("3 × −2", -6.0),
        ("0.1 + 0.2", 0.3),
    ] {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(out["result"].as_f64(), Some(result), "{expression}");
    }
    for bad in ["", "1 +", "(1 + 2", "1 / 0", "2 $ 3", "1..2"] {
        assert!(tool(&rt, "calculate", json!({ "expression": bad }), agent_ctx(0)).await.is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn results_are_formatted_for_the_display() {
    let rt = calculator().await;
    for (expression, shown) in [
        ("1234567", "1,234,567"),
        ("7 / 2", "3.5"),
        ("1000000 * 1000000", "1e12"),
        ("987654321 * 999", "9.86666667e11"),
        ("0.00000012", "1.2e-7"),
    ] {
        let out = tool(&rt, "calculate", json!({ "expression": expression }), agent_ctx(0)).await.unwrap();
        assert_eq!(out["display"], shown, "{expression}");
    }
}

#[tokio::test]
async fn calculate_writes_history_so_it_is_not_read_only() {
    let rt = calculator().await;
    tool(&rt, "calculate", json!({ "expression": "2 + 2" }), agent_ctx(0)).await.unwrap();
    let history = tool(&rt, "history", json!({ "limit": 5 }), agent_ctx(0)).await.unwrap();
    assert_eq!(history["entries"][0], json!({ "expression": "2 + 2", "result": 4 }));
    let loaded = rt.windows(&id("calculator")).await.unwrap();
    assert_eq!(loaded, ["main"]);
}

#[tokio::test]
async fn only_the_tools_that_change_nothing_are_marked_read_only() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/calculator");
    let source = std::fs::read_to_string(dir.join("app.luau")).unwrap();
    // `calculate` and `show_on_calculator` write app.kv / app.state; `history` only reads
    let marked: Vec<&str> = source
        .split("app.tool {")
        .skip(1)
        .filter(|block| block.contains("readOnly = true"))
        .filter_map(|block| block.split('"').nth(1))
        .collect();
    assert_eq!(marked, ["history"]);
}

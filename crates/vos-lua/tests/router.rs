#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! The router the Vite plugin generates from an app's contract (`e2e/apps/tasks-svelte`: `openrpc.json`, and the `.app/`
//! folder `vos generate` makes from it), bundled with the product's own pipeline and run under the real runtime: every call
//! is held to the contract before it reaches the developer's handler, and so is what the handler returns.

mod common;

use common::{built_package, db, id, user_ctx};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    sync::{Arc, Once},
};
use vos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

/// The fixture's generated files are not committed: generate them once, as the project's own scripts do.
fn generate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/apps/tasks-svelte");
        let run = Command::new("bun")
            .args(["run", "generate"])
            .current_dir(&project)
            .output()
            .expect("bun is needed to generate the fixture's router (https://bun.sh)");
        assert!(run.status.success(), "bun run generate failed:\n{}", String::from_utf8_lossy(&run.stderr));
    });
}

async fn tasks() -> LuaRuntime {
    generate();
    let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/apps/tasks-svelte/.app/entry.luau");
    let package = vos_application::ports::AppPackage { has_client: true, ..built_package(&entry) };
    let runtime =
        LuaRuntime::new(Arc::new(Quiet), Arc::new(vos_application::testing::RecordingShell::default()));
    let data: Arc<dyn AppData> = db();
    runtime.load(&id("tasks-svelte"), package, data).await.unwrap();
    runtime
}

async fn call(rt: &LuaRuntime, method: &str, args: Value) -> PortResult<Value> {
    rt.call_handler(&id("tasks-svelte"), method, args, user_ctx(1_000)).await
}

fn invalid(message: &str) -> PortError {
    PortError::Rpc { code: -32602, message: format!("Invalid params: {message}"), data: None }
}

#[tokio::test]
async fn a_call_the_definition_allows_reaches_the_handler_and_its_result_comes_back() {
    let rt = tasks().await;
    assert_eq!(call(&rt, "list", Value::Null).await.unwrap(), json!([]));
    // surrounding blanks are the handler's business; the router only holds the call to the definition
    let added = call(&rt, "add", json!({ "title": "  Buy milk " })).await.unwrap();
    assert_eq!(added, json!([{ "id": 1, "title": "Buy milk", "done": false }]));
    let toggled = call(&rt, "toggle", json!({ "id": 1 })).await.unwrap();
    assert_eq!(toggled[0]["done"], true);
    let who = call(&rt, "whoami", json!({})).await.unwrap();
    assert_eq!(who, json!({ "kind": "user", "app": "tasks-svelte" }));
}

#[tokio::test]
async fn a_call_the_definition_does_not_allow_is_refused_before_the_handler_runs() {
    let rt = tasks().await;
    for (method, args, message) in [
        ("add", json!({}), "params.title is required"),
        ("add", Value::Null, "params.title is required"),
        ("add", json!({ "title": 5 }), "params.title must be a string, not a number"),
        ("add", json!({ "title": "x".repeat(201) }), "params.title must have at most 200 characters"),
        ("add", json!({ "title": "ok", "extra": true }), "params.extra is not a property of the definition"),
        ("add", json!(["ok"]), "params must be an object, not an array"),
        ("toggle", json!({ "id": 0 }), "params.id must be at least 1"),
        ("toggle", json!({ "id": 1.5 }), "params.id must be a whole number"),
        ("toggle", json!({ "id": "1" }), "params.id must be a number, not a string"),
        ("list", json!({ "unexpected": 1 }), "params.unexpected is not a property of the definition"),
    ] {
        assert_eq!(call(&rt, method, args.clone()).await.unwrap_err(), invalid(message), "{method} {args}");
    }
    // nothing the refused calls asked for happened
    assert_eq!(call(&rt, "list", Value::Null).await.unwrap(), json!([]));
}

#[tokio::test]
async fn a_failure_the_handler_chooses_is_a_message_and_an_unknown_method_is_method_not_found() {
    let rt = tasks().await;
    assert_eq!(
        call(&rt, "add", json!({ "title": "   " })).await.unwrap_err(),
        PortError::Rejected("A task needs a title.".into()),
        "the definition allows a blank title: the handler says no, as an application error"
    );
    assert_eq!(
        call(&rt, "delete", json!({ "id": 1 })).await.unwrap_err(),
        PortError::Rpc { code: -32601, message: "Method not found: delete".into(), data: None }
    );
}

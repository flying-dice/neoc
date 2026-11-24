#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! A backend written in TypeScript under the real runtime. The fixture (`fixtures/typescript`, an app project with an
//! `openrpc.json` and `server/main.ts`) is built by the product's own pipeline: `@vos/vite-plugin` generates its types
//! and router and compiles it with `TypeScriptToLua`. What is held here is that the compiler's output runs in an app's
//! environment, which is Luau without `debug`, `os` and `print`: its runtime library, the bitwise operators, an
//! `Error`, and the host's modules and `app` API called the way the generated declarations say.

use crate::common;

use common::{built_project, db, id, user_ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

const APP: &str = "typescript";

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

async fn probe() -> LuaRuntime {
    let runtime = LuaRuntime::new(Arc::new(Quiet), Arc::new(RecordingShell::default()));
    runtime.load(&id(APP), built_project("crates/vos-lua/tests/fixtures/typescript"), db()).await.unwrap();
    runtime
}

async fn call(rt: &LuaRuntime, method: &str, args: Value) -> PortResult<Value> {
    rt.call_handler(&id(APP), method, args, user_ctx(0)).await
}

#[tokio::test]
async fn what_the_compiler_writes_runs_in_an_apps_environment() {
    let vm = probe().await;
    let lines = call(&vm, "probe", Value::Null).await.unwrap();
    assert_eq!(
        lines,
        json!([
            "class=6",
            "getter=12",
            "map=ada,alan",
            "set=3",
            "generator=1234",
            "array=4+6",
            "string=..ABC",
            "number=3 3.14 42 true",
            "optional=none 1",
            "spread=1 bc",
            "caught=not this one",
            "shown=Error: not this one",
            "bits=2 7 5 16 64 16 15 4294967292 2147483644",
            "module=aGk=",
            "values=hi true",
            "method=true",
        ])
    );
}

#[tokio::test]
async fn a_handler_fails_a_call_as_a_luau_one_does() {
    let vm = probe().await;
    assert_eq!(
        call(&vm, "raise", json!({ "how": "fail" })).await.unwrap_err(),
        PortError::Rejected("Not like that.".into()),
        "ctx.fail is a plain message"
    );
    assert_eq!(
        call(&vm, "raise", json!({ "how": "coded" })).await.unwrap_err(),
        PortError::Rpc { code: -32001, message: "not today".into(), data: None },
        "a thrown string that starts with RPC and a code is that JSON-RPC error"
    );
    // an Error that nothing catches is a rejected call that says what it was, never a table's address
    for (how, said) in [("error", "a plain error"), ("subclass", "a refusal")] {
        let PortError::Rejected(message) = call(&vm, "raise", json!({ "how": how })).await.unwrap_err()
        else {
            panic!("{how}: a thrown Error is a rejected call");
        };
        assert!(message.contains(said), "{how}: {message}");
        assert!(!message.contains("0x"), "{how}: {message}");
    }
    // the router holds a TypeScript backend to the definition too
    let refused = call(&vm, "raise", json!({ "how": "politely" })).await.unwrap_err();
    assert!(matches!(refused, PortError::Rpc { code: -32602, .. }), "{refused:?}");
    // and the app goes on
    assert_eq!(call(&vm, "remember", json!({ "word": "one" })).await.unwrap(), json!(["one"]));
    assert_eq!(call(&vm, "remember", json!({ "word": "two" })).await.unwrap(), json!(["one", "two"]));
}

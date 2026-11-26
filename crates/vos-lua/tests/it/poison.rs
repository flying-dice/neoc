#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! An app must not be able to take the host down with the value it raises or hands over.
//!
//! The probe is an error value whose `__tostring` makes a protected
//! call and then fails (`mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end`). On LuaJIT the
//! engine's message handler recursed on it until the stack overflowed, which aborted the process (the PUC Lua 5.1 VM
//! this host first ran on turned the same recursion into a catchable "C stack overflow" error). On Luau the engine
//! answers `error in error handling` once, so the value reaches the caller as an ordinary error, whichever path it
//! takes: the entry chunk, a tool, a handler of the app's page, an event or request handler, a task, a transaction
//! body. A table the app hands over (given to `RPC.on_request` or `app.notify`, or returned from a handler) is read raw,
//! so its `__index` never runs.
//!
//! Each case runs in a child process (this test binary again, `poison::poison_case_child`) so that an abort would kill the
//! child and not the harness; the case passes when the child exits normally after handling the value as an ordinary
//! error and the app (or the next app) still answers. The child runs like the host does: a runtime built by
//! the host's recipe (workers with `VM_THREAD_STACK`, the case run on a worker), driven from a plain thread. A case that read
//! nothing would pass at any engine, so every case here asserts the outcome the raise produces: the engine's `error in
//! error handling` in the error, the 500 the server answers, the task that ran (it sets a key first), or, for the
//! handler that returns a poisoned table, the plain value the host read without running the app's `__index`.

use crate::common;

use common::{agent_ctx, db, id, load, package, try_load};
use serde_json::{json, Value};
use std::{process::Command, sync::Arc, time::Duration};
use vos_application::ports::{AppRuntime, CallContext, Notifier};
use vos_lua::LuaRuntime;

const CASE: &str = "POISON_CASE";

/// Lua that defines `poison`, an error value that makes the engine's handler recurse.
const POISON: &str = r#"
local mt = {}
mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
local poison = setmetatable({}, mt)
-- a table whose every field read raises the poison
local booby = setmetatable({}, { __index = function() error(poison) end })
"#;

fn app(body: &str) -> String {
    format!("{POISON}\nRPC.on_request('ping', function() return 'pong' end)\n{body}")
}

/// The text of an error the engine produced for the poison: `error in error handling`, once. A case that failed for
/// some other reason (a typo in the probe, a missing global) would not carry it.
fn poisoned(e: &dyn std::fmt::Display) -> String {
    let text = e.to_string();
    assert!(text.contains("error in error handling"), "not the engine's answer to the poison: {text}");
    text
}

/// Load an app with `src` and call its tool `tool`, which must fail with the engine's answer to the poison; the app
/// must still answer afterwards.
async fn failing_tool(src: &str, tool: &str) -> String {
    let vm = load(&app(src)).await;
    let e = common::serve(&vm, tool, json!({}), &agent_ctx(0)).await.unwrap_err();
    ping(&vm).await;
    poisoned(&e)
}

async fn ping(vm: &vos_lua::AppVm) {
    let out = common::serve(vm, "ping", json!({}), &agent_ctx(0)).await.unwrap();
    assert_eq!(out, json!("pong"), "the app no longer answers");
}

async fn get(addr: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(format!("GET / HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out).await;
    out
}

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: vos_domain::Notification) -> vos_application::ports::PortResult<()> {
        Ok(())
    }
}

/// The app with these handlers (the `RPC.on_request` calls that serve them), through the runtime the page's calls go
/// through.
async fn runtime_with(handlers: &str) -> LuaRuntime {
    let src = app(handlers);
    let shell = Arc::new(vos_application::testing::RecordingShell::default());
    let runtime = LuaRuntime::new(Arc::new(Quiet), shell);
    runtime.load(&id("probe"), package(&src, &[]), db()).await.unwrap();
    runtime
}

/// The person's page calling the handler `name`.
async fn call_handler(runtime: &LuaRuntime, name: &str) -> vos_application::ports::PortResult<Value> {
    let ctx = CallContext { actor: vos_domain::Actor::user("You"), now_ms: 0 };
    runtime.call_handler(&id("probe"), name, json!(null), ctx).await
}

/// Runs one case and says what it observed. A panic (an assertion) fails the child; an abort kills it.
async fn run_case(name: &str) -> String {
    match name {
        // the entry chunk raises the value
        "entry_chunk" => match try_load("probe", &app("error(poison)")).await {
            Ok(_) => panic!("the app loaded"),
            Err(e) => poisoned(&e),
        },
        // a table handed to `RPC.on_request` as what the handler says of itself would raise the value if the host
        // ran its `__index`: it is read raw, so the app's code does not run and the table says what it holds (nothing)
        "tool_spec_index" => {
            let vm = try_load("probe", &app("RPC.on_request('x', function() end, booby)")).await.unwrap();
            let said = vm.handlers().into_iter().find(|h| h.method == "x").unwrap();
            assert!(said.title.is_none() && said.description.is_none() && !said.read_only, "{said:?}");
            ping(&vm).await;
            "it holds nothing".into()
        }
        // a tool body raises the value
        "tool_body" => failing_tool("RPC.on_request('boom', function() error(poison) end)", "boom").await,
        // an event handler raises the value
        "event_handler" => {
            let vm = load(&app("hook('e', function() error(poison) end)")).await;
            let e = vm.emit("e", json!({})).await.unwrap_err();
            ping(&vm).await;
            poisoned(&e)
        }
        // a handler of the app's own HTTP server raises the value
        "request_handler" => {
            let vm = load(&app(
                r#"local net, hyper = require("std:net"), require("vnd:hyper")
                   local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function() error(poison) end)
                   RPC.on_request('addr', function() return server:addr() end)"#,
            ))
            .await;
            let addr = common::serve(&vm, "addr", json!({}), &agent_ctx(0)).await.unwrap();
            let answer = get(addr.as_str().unwrap()).await;
            ping(&vm).await;
            let status = answer.lines().next().unwrap_or_default().to_owned();
            // the handler did raise the value: the server answered with an error instead of the app's reply
            assert!(status.contains("500"), "{answer}");
            status
        }
        // a background task raises the value
        "task_body" => {
            let vm = load(&app(r#"local task = require("std:task")
                   RPC.on_request('spawn', function()
                     task.spawn(function() app.kv:set('ran', 'yes') error(poison) end) return 'ok'
                   end)
                   RPC.on_request('ran', function() return app.kv:get('ran') end)"#))
            .await;
            common::serve(&vm, "spawn", json!({}), &agent_ctx(0)).await.unwrap();
            // wait for the task to have run (it sets a key, then raises)
            for _ in 0..100 {
                if common::serve(&vm, "ran", json!({}), &agent_ctx(0)).await.unwrap() == json!("yes") {
                    ping(&vm).await;
                    return "the task ran and failed".into();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("the task never ran")
        }
        // a handler the app's page calls raises the value
        "handler_body" => {
            let runtime = runtime_with(
                "RPC.on_request('boom', function() error(poison) end)
                     RPC.on_request('ok', function() return 'pong' end)",
            )
            .await;
            let e = call_handler(&runtime, "boom").await.unwrap_err();
            assert_eq!(
                call_handler(&runtime, "ok").await.unwrap(),
                json!("pong"),
                "the app no longer answers"
            );
            poisoned(&e)
        }
        // a handler returns a table whose reads raise the value
        "handler_return_value" => {
            let runtime = runtime_with(
                "RPC.on_request('boom', function() return booby end)
                     RPC.on_request('ok', function() return 'pong' end)",
            )
            .await;
            // the host reads what a handler returned raw (no `__index`, no `__pairs`): the booby trap is an empty
            // table to it, so the call answers with one and the poison is never raised
            let out = call_handler(&runtime, "boom").await.unwrap();
            assert!(out == json!([]) || out == json!({}), "{out}");
            assert_eq!(
                call_handler(&runtime, "ok").await.unwrap(),
                json!("pong"),
                "the app no longer answers"
            );
            out.to_string()
        }
        // a table handed to `app.notify` is read raw too: refused for the title it does not hold
        "notify_spec" => {
            let vm = load(&app("RPC.on_request('n', function() app.notify(booby) return 'ok' end)")).await;
            let e = common::serve(&vm, "n", json!({}), &agent_ctx(0)).await.unwrap_err().to_string();
            ping(&vm).await;
            assert!(e.contains("missing field `title`"), "{e}");
            e
        }
        // the body of a transaction raises the value
        "tx_body" => {
            failing_tool("RPC.on_request('tx', function() app.db:tx(function() error(poison) end) end)", "tx")
                .await
        }
        other => panic!("no case {other}"),
    }
}

/// The child: only does anything when `POISON_CASE` names a case, so a plain `cargo test` passes over it.
#[test]
fn poison_case_child() {
    let Ok(name) = std::env::var(CASE) else { return };
    // the host's recipe (`vos_host::runtime`, which this crate cannot depend on): workers with `VM_THREAD_STACK`, and
    // the case run on a worker, not on the thread that drives the runtime (a plain thread here, with the default stack)
    let driver = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_stack_size(vos_lua::VM_THREAD_STACK)
            .enable_all()
            .build()
            .unwrap();
        let case = name.clone();
        let saw: Value = json!(
            rt.block_on(async move { tokio::spawn(async move { run_case(&case).await }).await.unwrap() })
        );
        println!("SURVIVED {name}: {saw}");
    });
    driver.join().unwrap();
}

/// Runs the case in a child process and asserts that it ended normally, having seen an ordinary outcome.
fn survives(case: &str) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "poison::poison_case_child", "--nocapture", "--test-threads=1"])
        .env(CASE, case)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let tail: String =
        stderr.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    assert!(
        out.status.success() && stdout.contains(&format!("SURVIVED {case}")),
        "the host did not survive `{case}`: {:?}\n{tail}",
        out.status
    );
}

#[test]
fn entry_chunk_raising_the_poison_fails_the_load() {
    survives("entry_chunk");
}

#[test]
fn a_table_given_to_rpc_on_request_that_would_raise_the_poison_on_a_field_read_is_read_raw() {
    survives("tool_spec_index");
}

#[test]
fn a_tool_raising_the_poison_fails_the_call() {
    survives("tool_body");
}

#[test]
fn an_event_handler_raising_the_poison_fails_the_event() {
    survives("event_handler");
}

#[test]
fn a_request_handler_raising_the_poison_answers_an_error() {
    survives("request_handler");
}

#[test]
fn a_task_raising_the_poison_fails_the_task() {
    survives("task_body");
}

#[test]
fn a_handler_raising_the_poison_fails_the_call() {
    survives("handler_body");
}

#[test]
fn a_handler_returning_a_table_that_raises_the_poison_on_a_read() {
    survives("handler_return_value");
}

#[test]
fn a_table_given_to_app_notify_that_would_raise_the_poison_on_a_field_read_is_read_raw_and_refused() {
    survives("notify_spec");
}

#[test]
fn a_transaction_body_raising_the_poison_fails_the_call() {
    survives("tx_body");
}

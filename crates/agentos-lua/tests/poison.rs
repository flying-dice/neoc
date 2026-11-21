#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! An app must not be able to take the host down with the value it raises or hands over.
//!
//! The probe is the one neoc-vm's own `abort_regressions.rs` uses: an error value whose `__tostring` makes a protected
//! call and then fails (`mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end`). On LuaJIT the
//! engine's message handler recursed on it until the stack overflowed, which aborted the process (the PUC Lua 5.1 VM
//! this host first ran on turned the same recursion into a catchable "C stack overflow" error). On Luau the engine
//! answers `error in error handling` once, so the value reaches the caller as an ordinary error, whichever path it
//! takes: the entry chunk, a tool, an event or request handler, a task, a render, a write-back, a transaction body, or a
//! field read of a table the app handed over (given to `app.tool`, `app.notify`, `app.window`, or returned from a
//! render).
//!
//! Each case runs in a child process (this test binary again, `poison_case_child`) so that an abort would kill the
//! child and not the harness; the case passes when the child exits normally after handling the value as an ordinary
//! error and the app (or the next app) still answers. The child runs like the host does: a runtime built by
//! the host's recipe (workers with `VM_THREAD_STACK`, the case run on a worker), driven from a plain thread. A case that read
//! nothing would pass at any engine, so every case here asserts the error the raise produces (or the 500 the server
//! answers).

mod common;

use agentos_application::ports::{AppRuntime, Notifier};
use agentos_lua::LuaRuntime;
use common::{agent_ctx, db, id, load, package, try_load};
use serde_json::{json, Map, Value};
use std::{process::Command, sync::Arc, time::Duration};

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
    format!("{POISON}\napp.tool {{ name = 'ping', run = function() return 'pong' end }}\n{body}")
}

async fn ping(vm: &agentos_lua::AppVm) {
    let out = vm.call_tool("ping", json!({}), &agent_ctx(0), false).await.unwrap();
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
    async fn notify(&self, _: agentos_domain::Notification) -> agentos_application::ports::PortResult<()> {
        Ok(())
    }
}

/// The app, through the runtime that views go through (it needs a manifest).
async fn runtime_with(src: &str) -> LuaRuntime {
    let src = format!("app.manifest {{ id = 'probe', name = 'Probe', icon = 'gear' }}\n{src}");
    let src = src.as_str();
    let shell = Arc::new(agentos_application::testing::RecordingShell::default());
    let runtime = LuaRuntime::new(Arc::new(Quiet), shell);
    runtime.load(&id("probe"), package(src, &[]), db()).await.unwrap();
    runtime
}

/// Runs one case and says what it observed. A panic (an assertion) fails the child; an abort kills it.
async fn run_case(name: &str) -> String {
    match name {
        // the entry chunk raises the value
        "entry_chunk" => match try_load("probe", &app("error(poison)")).await {
            Ok(_) => panic!("the app loaded"),
            Err(e) => e.to_string(),
        },
        // a table handed to `app.tool` raises the value when the host reads a field of it, at install
        "tool_spec_index" => match try_load("probe", &app("app.tool(booby)")).await {
            Ok(_) => panic!("the app loaded"),
            Err(e) => e.to_string(),
        },
        // a tool body raises the value
        "tool_body" => {
            let vm = load(&app("app.tool { name = 'boom', run = function() error(poison) end }")).await;
            let e = vm.call_tool("boom", json!({}), &agent_ctx(0), false).await.unwrap_err();
            ping(&vm).await;
            e.to_string()
        }
        // an event handler raises the value
        "event_handler" => {
            let vm = load(&app("app.on('e', function() error(poison) end)")).await;
            let e = vm.emit("e", json!({})).await.unwrap_err();
            ping(&vm).await;
            e.to_string()
        }
        // a handler of the app's own HTTP server raises the value
        "request_handler" => {
            let vm = load(&app(
                r#"local net, hyper = require("std:net"), require("vnd:hyper")
                   local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function() error(poison) end)
                   app.tool { name = 'addr', run = function() return server:addr() end }"#,
            ))
            .await;
            let addr = vm.call_tool("addr", json!({}), &agent_ctx(0), false).await.unwrap();
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
                   app.tool { name = 'spawn', run = function()
                     task.spawn(function() app.kv:set('ran', 'yes') error(poison) end) return 'ok'
                   end }
                   app.tool { name = 'ran', run = function() return app.kv:get('ran') end }"#))
            .await;
            vm.call_tool("spawn", json!({}), &agent_ctx(0), false).await.unwrap();
            // wait for the task to have run (it sets a key, then raises)
            for _ in 0..100 {
                if vm.call_tool("ran", json!({}), &agent_ctx(0), false).await.unwrap() == json!("yes") {
                    ping(&vm).await;
                    return "the task ran and failed".into();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("the task never ran")
        }
        // a window's render raises the value
        "render_error" => {
            let runtime =
                runtime_with(&app("app.window('main', { render = function() error(poison) end })")).await;
            let e = runtime.open_view(&id("probe"), "v1", "main", Map::new(), Map::new()).await.unwrap_err();
            e.to_string()
        }
        // a window's render returns a table whose reads raise the value
        "render_return_value" => {
            let runtime =
                runtime_with(&app("app.window('main', { render = function() return booby end })")).await;
            // the host reads the returned table raw (no `__index` runs), finds no node in it and refuses the render
            let e = runtime.open_view(&id("probe"), "v1", "main", Map::new(), Map::new()).await.unwrap_err();
            e.to_string()
        }
        // a table handed to `app.notify` raises the value when the host reads a field of it
        "notify_spec" => {
            let vm =
                load(&app("app.tool { name = 'n', run = function() app.notify(booby) return 'ok' end }"))
                    .await;
            let e = vm.call_tool("n", json!({}), &agent_ctx(0), false).await.unwrap_err();
            ping(&vm).await;
            e.to_string()
        }
        // a table handed to `app.window` raises the value when the host reads a field of it, at install
        "window_spec" => match try_load("probe", &app("app.window('main', booby)")).await {
            Ok(_) => panic!("the app loaded"),
            Err(e) => e.to_string(),
        },
        // a row source's write-back callback raises the value
        "write_back" => {
            let vm = load(&app(
                "app.db:row('select 1 as id', { key = 'id', write = function() error(poison) end })",
            ))
            .await;
            let source = agentos_application::live::SourceId("s1".into());
            let e = vm.write_row(&source, &json!(1), "body", &json!("x"), &agent_ctx(0)).await.unwrap_err();
            ping(&vm).await;
            e.to_string()
        }
        // the body of a transaction raises the value
        "tx_body" => {
            let vm = load(&app(
                "app.tool { name = 'tx', run = function() app.db:tx(function() error(poison) end) end }",
            ))
            .await;
            let e = vm.call_tool("tx", json!({}), &agent_ctx(0), false).await.unwrap_err();
            ping(&vm).await;
            e.to_string()
        }
        other => panic!("no case {other}"),
    }
}

/// The child: only does anything when `POISON_CASE` names a case, so a plain `cargo test` passes over it.
#[test]
fn poison_case_child() {
    let Ok(name) = std::env::var(CASE) else { return };
    // the host's recipe (`agentos_host::runtime`, which this crate cannot depend on): workers with `VM_THREAD_STACK`, and
    // the case run on a worker, not on the thread that drives the runtime (a plain thread here, with the default stack)
    let driver = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_stack_size(agentos_lua::VM_THREAD_STACK)
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
        .args(["--exact", "poison_case_child", "--nocapture", "--test-threads=1"])
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
fn a_table_given_to_app_tool_that_raises_the_poison_on_a_field_read_fails_the_load() {
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
fn a_render_raising_the_poison_fails_the_render() {
    survives("render_error");
}

#[test]
fn a_render_returning_a_table_that_raises_the_poison_on_a_read() {
    survives("render_return_value");
}

#[test]
fn a_table_given_to_app_notify_that_raises_the_poison_on_a_field_read_fails_the_call() {
    survives("notify_spec");
}

#[test]
fn a_table_given_to_app_window_that_raises_the_poison_on_a_field_read_fails_the_load() {
    survives("window_spec");
}

#[test]
fn a_write_back_raising_the_poison_fails_the_write() {
    survives("write_back");
}

#[test]
fn a_transaction_body_raising_the_poison_fails_the_call() {
    survives("tx_body");
}

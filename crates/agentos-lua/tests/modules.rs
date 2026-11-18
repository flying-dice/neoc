#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What apps can do now that nothing is budgeted and every library module is theirs: `require("std:..")`,
//! `("lib:..")` and `("vnd:..")`; background tasks, servers and workers; and that all of it is tied to the
//! app, which means it stops when the app is unloaded or replaced and runs one call at a time with the rest.

mod common;

use agentos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
};
use agentos_domain::Notification;
use agentos_lua::LuaRuntime;
use agentos_luavm::lua::modules;
use common::{agent_ctx, db, id, load, package};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear" }"#;

/// Wait (up to `secs`) for `check` to hold.
async fn eventually(secs: u64, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A bare HTTP/1.1 GET: the whole response (head and body).
async fn http_get(addr: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).await.unwrap();
    out
}

fn body_of(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_, body)| body)
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Notification>>);

#[async_trait::async_trait]
impl Notifier for Recorder {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        self.0.lock().push(n);
        Ok(())
    }
}

struct Rig {
    runtime: Arc<LuaRuntime>,
    notes: Arc<Recorder>,
    shell: Arc<RecordingShell>,
}

fn rig() -> Rig {
    let notes = Arc::new(Recorder::default());
    let shell = Arc::new(RecordingShell::default());
    let runtime = Arc::new(LuaRuntime::new(notes.clone(), shell.clone()));
    Rig { runtime, notes, shell }
}

impl Rig {
    async fn load(&self, name: &str, src: &str) {
        let data: Arc<dyn AppData> = db();
        self.runtime.load(&id(name), package(src, &[]), data).await.unwrap();
    }
    async fn tool(&self, name: &str, tool: &str) -> Value {
        self.runtime.call_tool(&id(name), tool, json!({}), agent_ctx(0), false).await.unwrap()
    }
}

// ---- the modules ------------------------------------------------------------------------------------------

#[tokio::test]
async fn every_library_module_can_be_required_and_is_one_table_per_app() {
    let names: Vec<&str> = modules::names().collect();
    assert!(names.len() >= 46, "{names:?}");
    let list = names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let src = format!(
        r#"app.on("probe", function()
             local out = {{}}
             for _, name in ipairs({{ {list} }}) do
               local ok, m = pcall(require, name)
               out[name] = ok and type(m) or tostring(m)
             end
             out.same = require("lib:json") == require("lib:json")
             return out
           end)"#
    );
    let out = load(&src).await.emit("probe", json!(null)).await.unwrap();
    for name in &names {
        assert_eq!(out[*name], "table", "{name}: {out}");
    }
    assert_eq!(out["same"], true, "one handle per module, as with Lua's own require");

    // two apps never share a module table: a change one makes is invisible to the other
    let writer = load(r#"require("lib:json").mine = 1 app.on("probe", function() return 1 end)"#).await;
    drop(writer);
    let reader = load(r#"app.on("probe", function() return require("lib:json").mine == nil end)"#).await;
    assert_eq!(reader.emit("probe", json!(null)).await.unwrap(), json!(true));
}

#[tokio::test]
async fn the_modules_work_inside_an_app_data_formats_time_hashes_and_the_filesystem() {
    let dir = std::env::temp_dir().join(format!("modules-fs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = format!(
        r#"
        local json = require("lib:json")
        local base64 = require("lib:base64")
        local hash = require("lib:hash")
        local hex = require("lib:hex")
        local yaml = require("vnd:serde_yaml")
        local fs = require("std:fs")
        local collections = require("std:collections")
        app.tool {{ name = "go", run = function()
          local path = {path:?}
          fs.write(path, "from the app")
          local text = fs.read(path)
          local counter = collections.Counter.new()
          counter:increment()
          counter:increment()
          return {{
            b64 = base64.encode("hi"),
            sha = hex.encode((hash.hash("sha256", "abc"))),
            file = text,
            counter = counter:get(),
            yaml = type(yaml),
            json = type(json),
          }}
        end }}
        "#,
        path = dir.join("note.txt").display().to_string()
    );
    let out = load(&src).await.call_tool("go", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out["b64"], "aGk=", "{out}");
    assert_eq!(out["sha"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "{out}");
    assert_eq!(out["file"], "from the app", "{out}");
    assert_eq!(out["counter"], 2, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_worker_gets_the_same_modules_and_lib_log_goes_to_tracing_not_stdout() {
    let src = r#"
        local thread = require("std:thread")
        app.tool { name = "work", run = function()
          local worker = thread.spawn {
            source = [[
              local json = require("lib:json")
              local hash, hex = require("lib:hash"), require("lib:hex")
              local hyper = require("vnd:hyper")
              require("lib:log").info("from a worker")
              return { sha = hex.encode((hash.hash("sha256", "abc"))), hyper = type(hyper.serve), json = type(json) }
            ]],
          }
          return worker:await()
        end }
        app.tool { name = "log", run = function()
          require("lib:log").info("hello from the app", { k = 1 })
          return true
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("work", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out["hyper"], "function", "{out}");
    assert_eq!(out["sha"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "{out}");
    assert_eq!(vm.call_tool("log", json!({}), &agent_ctx(0), false).await.unwrap(), json!(true));
    // the worker's OS thread is released with the app, not left to the end of the process
    vm.shutdown();
}

// ---- tasks and servers take their turn ---------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_background_loop_does_not_stop_the_app_and_tools_can_wait_too() {
    let src = r#"
        local task = require("std:task")
        local ticks = 0
        task.spawn(function() while true do task.wait(0.01) ticks = ticks + 1 end end)
        app.tool { name = "ticks", run = function() return ticks end }
        app.tool { name = "slow", run = function() local before = ticks task.wait(0.15) return ticks - before end }
    "#;
    let vm = load(src).await;
    let first = vm.call_tool("ticks", json!({}), &agent_ctx(0), false).await.unwrap().as_i64().unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let second = vm.call_tool("ticks", json!({}), &agent_ctx(0), false).await.unwrap().as_i64().unwrap();
    assert!(second > first, "the task ran between calls: {first} -> {second}");
    // a tool that waits hands the VM back while it waits: the loop kept ticking meanwhile
    let during = vm.call_tool("slow", json!({}), &agent_ctx(0), false).await.unwrap().as_i64().unwrap();
    assert!(during >= 3, "{during}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_to_the_apps_server_run_one_at_a_time_in_the_vm() {
    // each handler reads the counter, makes a host call (the database), then writes: if two handlers could
    // overlap, updates would be lost and two responses would carry the same number
    let src = r#"
        local net, hyper = require("std:net"), require("vnd:hyper")
        local count = 0
        local listener = net.TcpListener.bind("127.0.0.1:0")
        local server = hyper.serve(listener, function(req)
          local seen = count
          app.db:all("select 1")
          count = seen + 1
          return { status = 200, body = tostring(count) }
        end)
        app.tool { name = "addr", run = function() return server:addr() end }
        app.tool { name = "count", run = function() return count end }
    "#;
    let vm = Arc::new(load(src).await);
    let addr = vm.call_tool("addr", json!({}), &agent_ctx(0), false).await.unwrap();
    let addr = addr.as_str().unwrap().to_owned();
    let mut requests = vec![];
    for _ in 0..24 {
        let addr = addr.clone();
        requests.push(tokio::spawn(async move { http_get(&addr, "/").await }));
    }
    let mut seen = vec![];
    for request in requests {
        let response = request.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        seen.push(body_of(&response).parse::<u32>().unwrap());
    }
    seen.sort_unstable();
    assert_eq!(seen, (1..=24).collect::<Vec<_>>(), "every request saw its own turn");
    // and the app's own tools queue with them
    assert_eq!(vm.call_tool("count", json!({}), &agent_ctx(0), false).await.unwrap(), json!(24));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_waiting_in_a_read_only_tool_keeps_its_read_only_guarantee_while_others_run() {
    // the readOnly tool waits; while it does, a task of the app writes state (allowed: it is not the tool);
    // when the tool goes on, its own write is refused again
    let src = r#"
        local task = require("std:task")
        local state = app.state { v = 0 }
        local from_task = "unset"
        task.spawn(function()
          task.wait(0.05)
          from_task = tostring(pcall(function() state.v = 1 end))
        end)
        app.tool { name = "ro", readOnly = true, run = function()
          task.wait(0.25)
          local wrote, why = pcall(function() state.v = 2 end)
          return { tool_write = wrote, why = tostring(why), task_write = from_task, v = state.v }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("ro", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out["task_write"], "true", "another call may write while the read-only one waits: {out}");
    assert_eq!(out["tool_write"], false, "the read-only tool still cannot: {out}");
    assert!(out["why"].as_str().unwrap().contains("readOnly"), "{out}");
    assert_eq!(out["v"], 1, "{out}");
}

#[tokio::test]
async fn a_call_cannot_wait_inside_a_transaction() {
    let src = r#"
        local task = require("std:task")
        app.db:migrate { [1] = "create table t(a)" }
        app.tool { name = "tx", run = function()
          local ok, why = pcall(function() app.db:tx(function() app.db:exec("insert into t values (1)") task.wait(0.01) end) end)
          local n = app.db:one("select count(*) as n from t").n
          task.wait(0.01) -- outside a transaction waiting is fine
          return { ok = ok, why = tostring(why), n = n }
        end }
    "#;
    let out = load(src).await.call_tool("tx", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["why"].as_str().unwrap().contains("cannot wait inside db:tx"), "{out}");
    assert_eq!(out["n"], 0, "the transaction was rolled back: {out}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_a_request_handler_asks_of_the_host_is_forwarded() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local net, hyper = require("std:net"), require("vnd:hyper")
        local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req)
          app.notify {{ title = "webhook " .. req.path }}
          app.shell.badge(7)
          return "ok"
        end)
        app.tool {{ name = "addr", run = function() return server:addr() end }}"#
    );
    r.load("probe", &src).await;
    let addr = r.tool("probe", "addr").await;
    let response = http_get(addr.as_str().unwrap(), "/hook").await;
    assert_eq!(body_of(&response), "ok");
    eventually(5, "the notification", || !r.notes.0.lock().is_empty()).await;
    assert_eq!(r.notes.0.lock()[0].title, "webhook /hook");
    eventually(5, "the badge", || !r.shell.badges.lock().unwrap().is_empty()).await;
}

// ---- unload and replace stop what the app started -----------------------------------------------------------

async fn port_is_free(addr: &str) -> bool {
    TcpListener::bind(addr).await.is_ok()
}

const SERVER_APP: &str = r#"
    local net, hyper = require("std:net"), require("vnd:hyper")
    local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req) return "up" end)
    app.tool { name = "addr", run = function() return server:addr() end }
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unloading_an_app_closes_its_listener_and_stops_its_tasks_and_workers() {
    let r = rig();
    let dir = std::env::temp_dir().join(format!("modules-life-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (task_file, worker_file) = (dir.join("task.txt"), dir.join("worker.txt"));
    let src = format!(
        r#"{MANIFEST}
        {SERVER_APP}
        local task, thread, fs = require("std:task"), require("std:thread"), require("std:fs")
        task.spawn(function()
          local n = 0
          while true do n = n + 1 fs.write({task:?}, tostring(n)) task.wait(0.01) end
        end)
        local worker = thread.spawn {{ source = [[
          local fs, task = require("std:fs"), require("std:task")
          local n = 0
          while true do n = n + 1 fs.write({worker:?}, tostring(n)) task.wait(0.01) end
        ]] }}
        "#,
        task = task_file.display().to_string(),
        worker = worker_file.display().to_string(),
    );
    r.load("probe", &src).await;
    let addr = r.tool("probe", "addr").await.as_str().unwrap().to_owned();
    assert_eq!(body_of(&http_get(&addr, "/").await), "up");
    assert!(!port_is_free(&addr).await, "the app holds its port");
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok().and_then(|s| s.parse::<u64>().ok());
    eventually(5, "the task and the worker to run", || {
        read(&task_file).is_some_and(|n| n > 3) && read(&worker_file).is_some_and(|n| n > 3)
    })
    .await;

    r.runtime.unload(&id("probe")).await.unwrap();

    eventually(5, "the port to be released", || std::net::TcpListener::bind(&addr).is_ok()).await;
    assert!(port_is_free(&addr).await, "the listener of an unloaded app is closed");
    tokio::time::sleep(Duration::from_millis(150)).await; // anything in flight settles
    let (task_then, worker_then) = (read(&task_file).unwrap(), read(&worker_file).unwrap());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(read(&task_file).unwrap(), task_then, "the app's task is stopped");
    assert_eq!(read(&worker_file).unwrap(), worker_then, "the app's worker is stopped");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_an_app_frees_the_port_of_the_version_it_replaces_for_the_new_one() {
    let r = rig();
    let src = format!("{MANIFEST}\n{SERVER_APP}");
    r.load("probe", &src).await;
    let first = r.tool("probe", "addr").await.as_str().unwrap().to_owned();
    assert_eq!(body_of(&http_get(&first, "/").await), "up");
    // the new version listens on the same port: the old one must have let go
    let port = first.rsplit(':').next().unwrap();
    let second_src = src.replace("127.0.0.1:0", &format!("127.0.0.1:{port}"));
    let mut tries = 0;
    // the old version is shut down before the new one starts, but its listener is dropped by its aborted task
    // an instant later: the new version's bind may have to be tried again
    loop {
        let data: Arc<dyn AppData> = db();
        match r.runtime.load(&id("probe"), package(&second_src, &[]), data).await {
            Ok(_) => break,
            Err(e) => {
                tries += 1;
                assert!(tries < 100, "the old version never released its port: {e}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    assert_eq!(r.tool("probe", "addr").await.as_str().unwrap(), first);
    assert_eq!(body_of(&http_get(&first, "/").await), "up");
    r.runtime.unload(&id("probe")).await.unwrap();
}

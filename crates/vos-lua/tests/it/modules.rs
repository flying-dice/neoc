#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What apps can do now that nothing is budgeted and every library module is theirs: `require("std:..")`,
//! `("lib:..")` and `("vnd:..")`; background tasks, servers and workers; and that all of it is tied to the
//! app, which means it stops when the app is unloaded or replaced and runs one call at a time with the rest.

use crate::common;

use common::{agent_ctx, built_package, db, id, load, package};
use neoc_vm::modules;
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
use vos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

/// The tests' own fixture apps, which the product's build pipeline (darklua) bundles.
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name).join("app.luau")
}

/// The library modules an app is not offered, spelled out here on purpose: the adapter's own `NOT_FOR_APPS` must
/// say exactly this (a test that read the list from the constant would still pass with the constant emptied).
const REFUSED: [&str; 3] = ["lib:test", "lib:compile", "std:workers"];

/// The first line of every source here: the line numbers the tests name are the body's plus one.
const MANIFEST: &str = "-- probe";

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
    /// Load `src` as a backend with no handlers (`common::backend`).
    async fn load(&self, name: &str, src: &str) {
        let data: Arc<dyn AppData> = db();
        self.runtime.load(&id(name), package(&common::backend(src), &[]), data).await.unwrap();
    }
    /// The names of the events the app has emitted to its pages so far (`app.emit`).
    fn events(&self) -> Vec<String> {
        self.shell.events.lock().unwrap().iter().map(|(_, name, _)| name.clone()).collect()
    }
    async fn tool(&self, name: &str, tool: &str) -> Value {
        self.runtime.call_tool(&id(name), tool, json!({}), agent_ctx(0), false).await.unwrap()
    }
}

// ---- the modules ------------------------------------------------------------------------------------------

/// The modules of the VM library: every module linked into the program but the adapter's own (`vos:app`, the `app`
/// API, which is an mlua-ir module like them and so is in the same registry).
fn library() -> impl Iterator<Item = &'static str> {
    modules::names().filter(|name| *name != "vos:app")
}

/// The `(module, type)` of every `require` overload of the generated app declarations, in order.
fn declared_overloads() -> Vec<(String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../types/generated/vos.d.luau");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text.find("\ndeclare require:").expect("the one declare require");
    text[start..]
        .lines()
        .filter_map(|line| {
            let rest = line.trim().trim_start_matches('&').trim().strip_prefix("((path: \"")?;
            let (module, rest) = rest.split_once("\") -> ")?;
            Some((module.to_owned(), rest.strip_suffix(')')?.to_owned()))
        })
        .collect()
}

/// What luau-lsp checks an app against (`types/generated/vos.d.luau`) and what the host gives it are one list: every
/// module the VM library has, less `NOT_FOR_APPS`, is typed by its own type, `vos:app` and `vos:docker` are the
/// host's own, and every refused module is typed as unusable; nothing else is declared.
#[test]
fn the_declared_require_offers_exactly_what_the_host_offers() {
    let declared = declared_overloads();
    let typed: Vec<&str> = declared
        .iter()
        .filter(|(_, ty)| !ty.starts_with("vos_NotForApps_") && ty.as_str() != "any")
        .map(|(module, _)| module.as_str())
        .collect();
    let mut offered: Vec<&str> = library().filter(|name| !REFUSED.contains(name)).collect();
    offered.push("vos:app");
    offered.push("vos:docker");
    let mut expected = offered.clone();
    expected.sort_unstable();
    let mut got = typed.clone();
    got.sort_unstable();
    assert_eq!(got, expected, "the declarations (bun run codegen) and the host's offered modules disagree");

    let unusable: Vec<&str> = declared
        .iter()
        .filter(|(_, ty)| ty.starts_with("vos_NotForApps_"))
        .map(|(module, _)| module.as_str())
        .collect();
    assert_eq!(unusable, REFUSED, "the refused modules are typed unusable");

    // and the host really does refuse them, and really does give the rest
    for module in REFUSED {
        assert!(library().any(|name| name == module), "{module} exists in the VM library, and is refused");
    }
}

/// The globals the declarations type as unusable are the ones the app's environment lacks.
#[tokio::test]
async fn the_globals_the_declarations_refuse_are_absent_from_the_environment() {
    let names =
        vos_domain::GLOBALS_NOT_FOR_APPS.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let src = format!(
        r#"hook("present", function()
             local found = {{}}
             for _, name in ipairs({{ {names} }}) do
               if _G[name] ~= nil or rawget(_G, name) ~= nil then found[#found + 1] = name end
             end
             return found
           end)"#
    );
    let out = load(&src).await.emit("present", json!(null)).await.unwrap();
    assert_eq!(
        out,
        json!([]),
        "these exist in the app's environment, so the declarations must not refuse them"
    );
}

/// The module names `neoc-vm` declares in its source: each `#[mlua_ir::module("<name>")]` that starts a line (a doc
/// comment that shows one is not a module).
fn modules_in_the_source(dir: &std::path::Path, names: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            modules_in_the_source(&path, names);
        } else if path.extension().is_some_and(|e| e == "rs") {
            for line in std::fs::read_to_string(&path).unwrap().lines() {
                if let Some(rest) = line.strip_prefix("#[mlua_ir::module(\"") {
                    names.push(rest.split('"').next().unwrap().to_owned());
                }
            }
        }
    }
}

/// A module registers itself when its object code is linked into the program, so one the linker left out would be
/// missing from `require` with nothing to say so. What a program that links this adapter gets is every module the
/// library's source declares (the test below requires each of them from an app).
#[test]
fn every_module_the_library_declares_is_linked_into_the_adapter() {
    let mut declared = Vec::new();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../neoc-vm/src/modules");
    modules_in_the_source(&source, &mut declared);
    declared.sort();
    assert!(declared.len() >= 46, "{declared:?}");
    let mut linked: Vec<&str> = library().collect();
    linked.sort_unstable();
    assert_eq!(linked, declared);
}

#[tokio::test]
async fn every_library_module_can_be_required_once_per_app_and_cannot_be_patched() {
    // the library offers every module; an app is not offered the ones in `REFUSED`
    assert_eq!(vos_lua::NOT_FOR_APPS, REFUSED);
    let names: Vec<&str> = library().filter(|n| !REFUSED.contains(n)).collect();
    assert!(names.len() >= 46, "{names:?}");
    let list = names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let src = format!(
        r#"hook("probe", function()
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

    // a module table is read-only: an app cannot patch a module, for itself or for anything else
    let patcher = load(
        r#"hook("probe", function()
             local json = require("lib:json")
             local ok, e = pcall(function() json.mine = 1 end)
             return { ok = ok, e = tostring(e), mine = json.mine == nil }
           end)"#,
    )
    .await;
    let out = patcher.emit("probe", json!(null)).await.unwrap();
    assert_eq!((&out["ok"], &out["mine"]), (&json!(false), &json!(true)), "{out}");
    assert!(out["e"].as_str().unwrap().contains("readonly"), "{out}");
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

/// A module's failure is a typed error: the app branches on its `kind` and fields. One the app raises as it is
/// (`error(err)`) ends the call with the error's text, as a message does, not with "a value that is not a message".
#[tokio::test]
async fn a_modules_typed_error_is_read_by_the_app_and_reaches_the_caller_as_its_text() {
    let src = r#"
        local fs = require("std:fs")
        local missing = "no-such-dir-of-the-modules-test/file.txt"
        app.tool { name = "read", run = function()
          local text, err = fs.read(missing)
          return { text = text == nil, kind = err.kind, op = err.op, said = tostring(err) }
        end }
        app.tool { name = "raise", run = function()
          local text, err = fs.read(missing)
          if not text then error(err) end
          return text
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("read", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(
        (&out["text"], &out["kind"], &out["op"]),
        (&json!(true), &json!("not_found"), &json!("std:fs.read"))
    );
    let said = out["said"].as_str().unwrap();
    assert!(said.starts_with("std:fs.read: not_found: "), "{said}");
    let raised = vm.call_tool("raise", json!({}), &agent_ctx(0), false).await.unwrap_err().to_string();
    assert!(raised.starts_with("probe/app.lua:10: std:fs.read: not_found: "), "{raised}");
}

#[tokio::test]
async fn a_worker_gets_the_same_modules_and_lib_log_goes_to_tracing_not_stdout() {
    let src = r#"
        local thread = require("std:thread")
        app.tool { name = "work", run = function()
          local worker = assert(thread.spawn([[
            local json = require("lib:json")
            local hash, hex = require("lib:hash"), require("lib:hex")
            local hyper = require("vnd:hyper")
            require("lib:log").info("from a worker")
            return { sha = hex.encode((hash.hash("sha256", "abc"))), hyper = type(hyper.serve), json = type(json) }
          ]]))
          return assert(worker:await())
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

/// A worker is offered what its app was offered: `std:workers` exposes the process-wide shutdown signal that every
/// app's `server:wait()` selects on, so an app that is refused it must not get it through `std:thread`.
#[tokio::test]
async fn a_worker_is_refused_what_its_app_is_refused() {
    // what is asked for, once: every module the library has. The refused ones must fail and every other one must
    // load, for the app and for its worker alike
    let asked: Vec<&str> = library().collect();
    assert!(
        REFUSED.iter().all(|r| asked.contains(r)),
        "the library no longer has a refused module: {asked:?}"
    );
    let list = asked.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let probe = format!(
        "local out = {{}} for _, n in ipairs {{ {list} }} do out[n] = (pcall(require, n)) end return out"
    );
    let src = format!(
        r#"
        local thread = require("std:thread")
        app.tool {{ name = "probe", run = function()
          local app_side = (function() {probe} end)()
          local worker = assert(thread.spawn([[ {probe} ]]))
          return {{ app = app_side, worker = assert(worker:await()) }}
        end }}
    "#
    );
    let vm = load(&src).await;
    let out = vm.call_tool("probe", json!({}), &agent_ctx(0), false).await.unwrap();
    let expected: Value = asked
        .iter()
        .map(|n| ((*n).to_owned(), json!(!REFUSED.contains(n))))
        .collect::<serde_json::Map<_, _>>()
        .into();
    assert_eq!(out["app"], expected, "{out}");
    assert_eq!(out["worker"], expected, "{out}");
    vm.shutdown();

    // the `app` API is the app's own: its worker, which runs in another engine, is not offered it
    let src = r#"
        local thread = require("std:thread")
        app.tool { name = "probe", run = function()
          local worker = assert(thread.spawn([[ return { offered = (pcall(require, "vos:app")) } ]]))
          return { app = (pcall(require, "vos:app")), worker = assert(worker:await()).offered }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("probe", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({"app": true, "worker": false}));
    vm.shutdown();
}

/// `neoc run` scripts have Luau's `buffer` (NEOC-206); an app does not yet, in its environment, in a worker it
/// starts or in that worker's own worker (a worker runs on the engine's own globals, where the engine opened it).
/// NEOC-44 offers it to all of them.
#[tokio::test]
async fn an_app_and_its_workers_are_not_offered_buffer_yet() {
    let src = r#"
        local thread = require("std:thread")
        app.tool { name = "probe", run = function()
          local worker = assert(thread.spawn([[
            local inner = assert(require("std:thread").spawn("return type(buffer)"))
            return { type(buffer), (assert(inner:await())) }
          ]]))
          local out = assert(worker:await())
          return { app = type(buffer), worker = out[1], nested = out[2] }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("probe", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({ "app": "nil", "worker": "nil", "nested": "nil" }));
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

// ---- what background work asks of the host is sent at once ----------------------------------------------------------------

#[tokio::test]
async fn long_running_work_started_with_task_spawn_does_not_hold_up_the_load() {
    let r = rig();
    // the entry chunk returns: the loop, the delayed job and the server all live on after it
    let src = format!(
        r#"{MANIFEST}
        {SERVER_APP}
        local task = require("std:task")
        local ticks = 0
        task.spawn(function() while true do task.wait(3600) end end)
        task.delay(3600, function() ticks = -1 end)
        task.spawn(function() while true do ticks = ticks + 1 task.wait(0.01) end end)
        app.tool {{ name = "ticks", run = function() return ticks end }}"#
    );
    let data: Arc<dyn AppData> = db();
    let app = id("probe");
    let load = r.runtime.load(&app, package(&common::backend(&src), &[]), data);
    tokio::time::timeout(Duration::from_secs(5), load).await.expect("the load returned").unwrap();
    let ticks = || async { r.tool("probe", "ticks").await.as_i64().unwrap() };
    let deadline = Instant::now() + Duration::from_secs(5);
    while ticks().await < 3 {
        assert!(Instant::now() < deadline, "the background loop never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_loop_that_emits_is_heard_at_every_pause_not_only_when_it_ends() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        local n = 0
        task.spawn(function() while true do n = n + 1 app.emit("tick", {{ n = n }}) task.wait(0.01) end end)"#
    );
    r.load("probe", &src).await;
    // the loop never ends, so only a pause can send its events: they keep coming while it runs
    eventually(5, "the loop's events to reach the shell", || r.events().len() >= 3).await;
    let events = r.shell.events.lock().unwrap().clone();
    assert!(events.iter().all(|(app, name, _)| app == "probe" && name == "tick"), "{events:?}");
    assert_eq!(events[0].2, json!({"n": 1}), "in the order they were emitted");
}

#[tokio::test]
async fn a_background_call_that_ends_after_emitting_is_heard() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        -- emits and ends without ever waiting
        task.spawn(function() app.emit("done") end)"#
    );
    r.load("probe", &src).await;
    eventually(5, "the ended task's event", || r.events() == ["done"]).await;
}

#[tokio::test]
async fn a_pause_that_asked_nothing_of_the_host_sends_nothing() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        local polls = 0
        -- works on its own: nothing for a page or the person to hear
        task.spawn(function() while true do polls = polls + 1 task.wait(0.01) end end)
        app.tool {{ name = "polls", run = function() return polls end }}"#
    );
    r.load("probe", &src).await;
    let polls = || async { r.tool("probe", "polls").await.as_i64().unwrap() };
    let deadline = Instant::now() + Duration::from_secs(5);
    while polls().await < 3 {
        assert!(Instant::now() < deadline, "the loop never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(r.events().is_empty(), "nothing was emitted, so nothing was sent");
    assert!(r.notes.0.lock().is_empty());
}

#[tokio::test]
async fn nothing_is_sent_for_an_app_after_it_is_unloaded() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        task.spawn(function() while true do app.emit("tick") task.wait(0.01) end end)"#
    );
    r.load("probe", &src).await;
    eventually(5, "the loop's first event", || !r.events().is_empty()).await;

    r.runtime.unload(&id("probe")).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await; // anything in flight settles
    let then = r.events().len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(r.events().len(), then, "an unloaded app's loop is stopped and sends nothing more");
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
    // the readOnly tool waits; while it does, a task of the app writes (allowed: it is not the tool);
    // when the tool goes on, its own write is refused again
    let src = r#"
        local task = require("std:task")
        app.kv:set("v", 0)
        local from_task = "unset"
        task.spawn(function()
          task.wait(0.05)
          from_task = tostring(pcall(function() app.kv:set("v", 1) end))
        end)
        app.tool { name = "ro", readOnly = true, run = function()
          task.wait(0.25)
          local wrote, why = pcall(function() app.kv:set("v", 2) end)
          return { tool_write = wrote, why = tostring(why), task_write = from_task, v = app.kv:get("v") }
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
          app.emit("hook.called", req.path)
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
    eventually(5, "the event", || !r.events().is_empty()).await;
    assert_eq!(r.events(), ["hook.called"]);
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
        local worker = assert(thread.spawn([[
          local fs, task = require("std:fs"), require("std:task")
          local n = 0
          while true do n = n + 1 fs.write({worker:?}, tostring(n)) task.wait(0.01) end
        ]]))
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
        match r.runtime.load(&id("probe"), package(&common::backend(&second_src), &[]), data).await {
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

// ---- a readOnly call and everything it starts change nothing ------------------------------------------------

const READ_ONLY_REACH: &str = r#"
    local task, net, hyper = require("std:task"), require("std:net"), require("vnd:hyper")
    app.db:migrate { [1] = "create table log(id integer primary key, msg text)" }
    local outcome = {}
    local function insert(who)
      local ok, err = pcall(function() app.db:exec("insert into log(msg) values (?)", { who }) end)
      outcome[who] = ok and "wrote" or tostring(err)
    end
    -- the app's own listener: whatever reaches it runs the handler
    local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req)
      insert("handler")
      return { status = 200, body = outcome.handler }
    end)
    local function poke() return hyper.get("http://" .. server:addr() .. "/").body end
    -- route 1: a readOnly tool asks the app's own server to write while the tool waits for the answer
    app.tool { name = "poke_ro", readOnly = true, run = poke }
    app.tool { name = "poke", run = poke }
    -- route 2: a readOnly tool leaves a task behind that writes after the tool has returned
    app.tool { name = "spawn_ro", readOnly = true, run = function()
      task.spawn(function() task.wait(0.05) insert("spawned") end)
      return "returned"
    end }
    app.tool { name = "defer_ro", readOnly = true, run = function() task.defer(function() insert("deferred") end) return "returned" end }
    app.tool { name = "delay_ro", readOnly = true, run = function() task.delay(0.05, function() insert("delayed") end) return "returned" end }
    app.tool { name = "yield_ro", readOnly = true, run = function()
      task.spawn(function() coroutine.yield() insert("yielded") end)
      return "returned"
    end }
    -- a task spawned by a readOnly tool spawns another from inside: both stay read-only
    app.tool { name = "nested_ro", readOnly = true, run = function()
      task.spawn(function() task.wait(0.01) task.spawn(function() task.wait(0.01) insert("nested") end) end)
      return "returned"
    end }
    app.tool { name = "spawn", run = function() task.spawn(function() task.wait(0.05) insert("writable") end) return "returned" end }
    app.tool { name = "report", readOnly = true, run = function()
      return { outcome = outcome, n = app.db:one("select count(*) as n from log").n }
    end }
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_only_tool_cannot_write_through_a_request_to_its_own_server() {
    let r = rig();
    r.load("probe", &format!("{MANIFEST}\n{READ_ONLY_REACH}")).await;
    let body = r.tool("probe", "poke_ro").await;
    assert!(
        body.as_str().unwrap().contains("readOnly") || body.as_str().unwrap().contains("read-only"),
        "{body}"
    );
    assert_ne!(body, "wrote");
    assert_eq!(r.tool("probe", "report").await["n"], 0, "nothing was written");
    // the same request outside a readOnly call writes, as the app's own code may
    assert_eq!(r.tool("probe", "poke").await, "wrote");
    assert_eq!(r.tool("probe", "report").await["n"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_a_read_only_tool_starts_stays_read_only_after_the_tool_returns() {
    let r = rig();
    r.load("probe", &format!("{MANIFEST}\n{READ_ONLY_REACH}")).await;
    for tool in ["spawn_ro", "defer_ro", "delay_ro", "yield_ro", "nested_ro"] {
        assert_eq!(r.tool("probe", tool).await, "returned", "{tool}");
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let report = r.tool("probe", "report").await;
    assert_eq!(report["n"], 0, "no task a readOnly tool left behind wrote: {report}");
    for who in ["spawned", "deferred", "delayed", "yielded", "nested"] {
        let said = report["outcome"][who].as_str().unwrap_or_else(|| panic!("{who} never ran: {report}"));
        assert!(
            said.contains("readOnly") || said.contains("read-only") || said.contains("read only"),
            "{who}: {said}"
        );
    }
    // a task of a writable tool is not held back by them
    assert_eq!(r.tool("probe", "spawn").await, "returned");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(r.tool("probe", "report").await["outcome"]["writable"], "wrote");
}

#[tokio::test]
async fn a_read_only_tool_cannot_register_or_change_the_apps_wiring() {
    let r = rig();
    let src = r#"
        local function try(f) local ok, err = pcall(f) return { ok = ok, err = (not ok) and tostring(err) or nil } end
        app.tool { name = "register", readOnly = true, run = function()
          return try(function() app.tool { name = "late", run = function() end } end) end }
        app.tool { name = "reads", readOnly = true, run = function()
          return { one = app.db:one("select 1 as n").n } end }
        app.tool { name = "wire", run = function()
          app.tool { name = "late", run = function() return "late" end }
          return "wired"
        end }
    "#;
    r.load("probe", &format!("{MANIFEST}\n{src}")).await;
    let out = r.tool("probe", "register").await;
    assert_eq!(out["ok"], false, "register must be refused: {out}");
    assert!(out["err"].as_str().unwrap().contains("readOnly"), "{out}");
    // reads stay allowed, and a writable tool may still wire the app
    let reads = r.tool("probe", "reads").await;
    assert_eq!(reads["one"], 1, "{reads}");
    assert_eq!(r.tool("probe", "wire").await, "wired");
    assert_eq!(r.tool("probe", "late").await, "late", "the tool it registered is there");
}

// ---- a task that stopped at a plain yield takes its turn before it runs -------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_continuation_does_not_interleave_with_the_call_that_spawned_it() {
    let src = r#"
        local task = require("std:task")
        local log = {}
        app.tool { name = "a", run = function()
          log = { "a-start" }
          task.spawn(function() coroutine.yield() log[#log + 1] = "continuation" end)
          for _ = 1, 40 do app.kv:get("x") end
          log[#log + 1] = "a-end"
          local during = table.concat(log, ",")
          return during
        end }
        app.tool { name = "log", run = function() return table.concat(log, ",") end }
    "#;
    let vm = Arc::new(load(src).await);
    for _ in 0..20 {
        let during = vm.call_tool("a", json!({}), &agent_ctx(0), false).await.unwrap();
        assert_eq!(during, "a-start,a-end", "the continuation ran in the middle of the call");
        // it runs once the call is over (the next call queues behind it or after it, never inside)
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let log = vm.call_tool("log", json!({}), &agent_ctx(0), false).await.unwrap();
            if log == json!("a-start,a-end,continuation") {
                break;
            }
            assert_eq!(log, json!("a-start,a-end"), "the continuation never ran, or ran out of order");
            assert!(Instant::now() < deadline, "the continuation never ran");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

// ---- what an unloaded app queued is dropped -----------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn effects_queued_after_the_app_was_unloaded_are_dropped_not_flushed() {
    let r = rig();
    let src = format!(
        r#"{MANIFEST}
        local task = require("std:task")
        app.tool {{ name = "late", run = function()
          task.wait(0.3) -- the call is still in flight when the app is unloaded
          app.notify {{ title = "from a ghost" }}
          app.emit("ghost")
          return "done"
        end }}
        app.tool {{ name = "now", run = function() app.notify {{ title = "from a live app" }} return "done" end }}"#
    );
    r.load("probe", &src).await;
    // control: a live app's effects are delivered
    assert_eq!(r.tool("probe", "now").await, "done");
    assert_eq!(r.notes.0.lock().len(), 1);

    let runtime = r.runtime.clone();
    let call =
        tokio::spawn(
            async move { runtime.call_tool(&id("probe"), "late", json!({}), agent_ctx(0), false).await },
        );
    tokio::time::sleep(Duration::from_millis(100)).await;
    r.runtime.unload(&id("probe")).await.unwrap();
    let _ = call.await.unwrap(); // the call ends (done or abandoned), either way
    tokio::time::sleep(Duration::from_millis(200)).await;
    let titles: Vec<String> = r.notes.0.lock().iter().map(|n| n.title.clone()).collect();
    assert_eq!(titles, ["from a live app"], "nothing queued after the unload was delivered");
    assert!(r.events().is_empty(), "no event from an app that is gone");
}

/// `bun run build:apps` bundles an app's relative requires and leaves `vos:*` and the neoc modules
/// (`std:`, `lib:`, `vnd:`) as runtime requires. darklua used to fail on `std:fs` ("unable to require
/// resource"), so no built app could use a module. The fixture is built through the real pipeline and then
/// loaded and exercised in the VM.
#[tokio::test]
async fn an_app_built_by_the_real_pipeline_requires_the_neoc_modules() {
    let package = built_package(&fixture("neoc_modules"));
    let bundle = String::from_utf8(package.files["app.luau"].clone()).unwrap();
    for required in ["vos:app", "std:fs", "lib:json", "vnd:serde_yaml"] {
        assert!(bundle.contains(&format!("require(\"{required}\")")), "{required} stays a runtime require");
    }
    assert!(!bundle.contains("./helpers"), "the relative module is bundled in");
    let vm = vos_lua::AppVm::load(&id("neoc-modules"), package, db(), common::config()).await.unwrap();
    let out = vm.call_tool("probe", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({"encoded": "aGk=", "same_module": true, "n": 6, "has_fs": true, "yaml": true}));
}

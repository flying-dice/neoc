#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What an app cannot do to the host. Every test here failed before its fix.
//!
//! - an unload or a reload ends what the app was doing at its next host call;
//! - bounded conversion of what the app hands back (tool results, handler results): a cyclic or exponentially
//!   shared table is an error, not a stack overflow or an allocation;
//! - a `read-only` call changes nothing: no data, no window, no notifications, no events;
//! - the SQL sandbox holds against `VACUUM INTO` and `ATTACH` through `db:all`.

use crate::common;

use common::{agent_ctx, db, id, load, package, try_load_with, Stalling};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use vos_application::{
    data::Params,
    ports::{AppData, AppRuntime, CallContext, Notifier, PortResult},
    testing::RecordingShell,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;

/// "Ended, not still running": far above what any of these takes.
const PROMPTLY: Duration = Duration::from_secs(8);

// ---- a kill on unload (nothing budgets a call; unloading or replacing the app ends what it was doing) ------------

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

struct Rig {
    runtime: Arc<LuaRuntime>,
    shell: Arc<RecordingShell>,
    db: Arc<vos_sqlite::AppDb>,
}

fn rig() -> Rig {
    let shell = Arc::new(RecordingShell::default());
    let runtime = Arc::new(LuaRuntime::new(Arc::new(Quiet), shell.clone()));
    Rig { runtime, shell, db: db() }
}

impl Rig {
    /// Load `src` as a backend (`common::backend`).
    async fn load(&self, app: &str, src: &str) -> PortResult<()> {
        let data: Arc<dyn AppData> = self.db.clone();
        self.runtime.load(&id(app), package(src, &[]), data).await
    }
    async fn tool(&self, app: &str, tool: &str) -> PortResult<Value> {
        common::serve_rt(&self.runtime, &id(app), tool, json!({}), agent_ctx(0)).await
    }
    /// [`Rig::tool`], read-only: every write the call tries is refused.
    async fn tool_ro(&self, app: &str, tool: &str) -> PortResult<Value> {
        common::serve_rt_with(&self.runtime, &id(app), tool, json!({}), agent_ctx(0), true).await
    }
}

/// The first line of every source here: the line numbers the tests name are the body's plus one.
const MANIFEST: &str = "-- probe";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unloading_stops_a_runaway_call_and_the_vm_is_freed() {
    // nothing budgets the call: only the unload can end it
    let rig = rig();
    let src =
        format!("{MANIFEST}\nRPC.on_request(\"spin\", function() while true do app.kv:get(\"k\") end end)");
    rig.load("probe", &src).await.unwrap();
    let (runtime, started) = (rig.runtime.clone(), std::time::Instant::now());
    let call = tokio::spawn(async move {
        common::serve_rt(&runtime, &id("probe"), "spin", json!({}), agent_ctx(0)).await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!call.is_finished(), "the call is running");
    assert!(Arc::strong_count(&rig.db) > 1, "and the VM holds the database");

    rig.runtime.unload(&id("probe")).await.unwrap();
    let ended = tokio::time::timeout(PROMPTLY, call).await.expect("an unloaded app's call stops").unwrap();
    assert!(ended.unwrap_err().to_string().contains("unloaded"));
    assert!(started.elapsed() < PROMPTLY);
    assert_eq!(
        Arc::strong_count(&rig.db),
        1,
        "no VM, closure or handle of the app is left holding the database"
    );
    assert!(rig.tool("probe", "spin").await.is_err(), "and the app is gone");
}

// ---- blocker 3: bounded output, bounded effects, the one window only ------------------------------------------

#[tokio::test]
async fn a_tool_result_that_expands_to_gigabytes_is_an_error_not_an_allocation() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        -- 2^30 paths to the leaf, a handful of table allocations
        RPC.on_request("dag", function()
          local t = {{ 1 }}
          for i = 1, 30 do t = {{ t, t }} end
          return t
        end)
        RPC.on_request("text", function()
          local s = string.rep("x", 1000000)
          local t = {{}}
          for i = 1, 40 do t[i] = s end
          return t
        end)
        RPC.on_request("cycle", function()
          local t = {{}}
          t.me = t
          return t
        end)"#
    );
    rig.load("probe", &src).await.unwrap();
    for (tool, why) in
        [("dag", "more than 100000 values"), ("text", "bytes of text"), ("cycle", "contains itself")]
    {
        let started = std::time::Instant::now();
        let err = rig.tool("probe", tool).await.unwrap_err().to_string();
        assert!(err.contains(why), "{tool}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{tool} took {:?}", started.elapsed());
    }
}

/// What a handler hands back to the app's page is converted under the same bounds as any other result.
#[tokio::test]
async fn a_handler_result_that_contains_itself_or_expands_to_gigabytes_is_an_error_not_an_allocation() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        RPC.on_request("cycle", function() local n = {{ type = "Stack", key = "n" }} n.children = {{ n }} return n end)
        RPC.on_request("dag", function()
          local t = {{ 1 }}
          for i = 1, 25 do t = {{ t, t }} end
          return {{ type = "Stack", key = "n", props = {{ data = t }} }}
        end)
        RPC.on_request("deep", function()
          local n = {{ key = "leaf" }}
          for i = 1, 5000 do n = {{ key = "k" .. i, children = {{ n }} }} end
          return n
        end)"#
    );
    rig.load("probe", &src).await.unwrap();
    let user = CallContext { actor: vos_domain::Actor::user("You"), now_ms: 0 };
    for (handler, why) in
        [("cycle", "contains itself"), ("dag", "more than 100000 values"), ("deep", "nested too deeply")]
    {
        let started = std::time::Instant::now();
        let err = rig
            .runtime
            .call_handler(&id("probe"), handler, json!(null), user.clone())
            .await
            .expect_err("the result must be refused")
            .to_string();
        assert!(err.contains(why), "{handler}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{handler} took {:?}", started.elapsed());
    }
}

#[tokio::test]
async fn open_window_asks_for_the_apps_one_window_and_refuses_any_other_name() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        local function try(f) local ok, err = pcall(f); return {{ ok = ok, err = (not ok) and tostring(err) or nil }} end
        RPC.on_request("main", function() app.open_window({{ a = 1 }}) return true end)
        RPC.on_request("other", function() return try(function() app.open_window("evil") end) end)"#
    );
    rig.load("probe", &src).await.unwrap();
    rig.tool("probe", "main").await.unwrap();
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1);
    let out = rig.tool("probe", "other").await.unwrap();
    assert_eq!(out["ok"], false);
    let why = out["err"].as_str().unwrap();
    assert!(why.contains("an app has one window: it takes no name"), "{why}");
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1, "nothing was asked of the shell");
}

// ---- blocker 4 and major 15: a read-only call changes nothing -------------------------------------------------

const READ_ONLY_APP: &str = r#"
    app.db:migrate { [1] = "create table log(id integer primary key, msg text)" }
    local n = 0
    local function try(f) local ok, err = pcall(f); return { ok = ok, err = (not ok) and tostring(err) or nil } end
    RPC.on_request("open", function() return try(function() app.open_window() end) end)
    RPC.on_request("notify", function() return try(function() app.notify { title = "hi" } end) end)
    RPC.on_request("emit", function() return try(function() RPC.send_notification("changed") end) end)
    -- not read-only: allowed to do all of it (but the caller may still require a read-only run)
    RPC.on_request("everything", function()
      app.open_window()
      app.notify { title = "hi" }
      RPC.send_notification("changed")
      app.db:exec("insert into log(msg) values ('did everything')")
      n = n + 1
      return n
    end)
    RPC.on_request("count", function() return app.db:one("select count(*) as n from log").n end)
"#;

#[tokio::test]
async fn a_read_only_call_cannot_open_the_window_notify_or_send_a_notification() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{READ_ONLY_APP}")).await.unwrap();
    for tool in ["open", "notify", "emit"] {
        let out = rig.tool_ro("probe", tool).await.unwrap();
        assert_eq!(out["ok"], false, "{tool} must be refused: {out}");
        assert!(out["err"].as_str().unwrap().contains("read-only"), "{tool}: {out}");
    }
    assert!(rig.shell.windows.lock().unwrap().is_empty(), "the shell was not asked for the window");
    assert!(rig.shell.events.lock().unwrap().is_empty(), "no notification reached the app's pages");
    assert_eq!(rig.tool_ro("probe", "count").await.unwrap(), json!(0), "the table is unchanged");

    // and a call that is not read-only still can
    assert_eq!(rig.tool("probe", "everything").await.unwrap(), json!(1));
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1);
    assert_eq!(rig.shell.events.lock().unwrap().len(), 1);
    assert_eq!(rig.tool_ro("probe", "count").await.unwrap(), json!(1));
}

#[tokio::test]
async fn a_caller_can_require_a_read_only_run_of_a_handler_that_writes() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{READ_ONLY_APP}")).await.unwrap();
    let ctx = agent_ctx(0);
    let err = common::serve_rt_with(&rig.runtime, &id("probe"), "everything", json!({}), ctx.clone(), true)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("read-only"), "the port's requirement is enforced by the adapter: {err}");
    assert!(rig.shell.windows.lock().unwrap().is_empty());
    // the requirement lasts one call
    let ok = common::serve_rt(&rig.runtime, &id("probe"), "everything", json!({}), ctx).await.unwrap();
    assert_eq!(ok, json!(1));
}

// ---- minors ---------------------------------------------------------------------------------------------------

#[tokio::test]
async fn loadstring_getfenv_and_setfenv_are_not_in_the_sandbox() {
    let src = r#"
        hook("probe", function()
          return { loadstring = loadstring == nil, getfenv = getfenv == nil, setfenv = setfenv == nil }
        end)
    "#;
    let vm = load(src).await;
    let out = vm.emit("probe", json!(null)).await.unwrap();
    for name in ["loadstring", "getfenv", "setfenv"] {
        assert_eq!(out[name], true, "{name} must be gone: {out}");
    }
}

/// SQLite's authorizer has no action for `VACUUM`, so `VACUUM INTO 'file'` is a file write the sandbox must
/// refuse by other means; `ATTACH` is denied by the authorizer. Through Lua `db:all` and `db:exec`.
#[tokio::test]
async fn vacuum_into_and_attach_are_refused_through_db_all_and_exec() {
    let dir = std::env::temp_dir().join(format!("vos-vacuum-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (vacuumed, attached) = (dir.join("vacuumed.db"), dir.join("attached.db"));
    let src = format!(
        r#"
        local function try(f) local ok, err = pcall(f); return {{ ok = ok, err = (not ok) and tostring(err) or nil }} end
        hook("probe", function()
          return {{
            vacuum_all = try(function() return app.db:all("VACUUM INTO '{vacuumed}'") end),
            vacuum_exec = try(function() return app.db:exec("vacuum into '{vacuumed}'") end),
            attach_all = try(function() return app.db:all("ATTACH DATABASE '{attached}' AS evil") end),
            attach_exec = try(function() return app.db:exec("ATTACH '{attached}' AS evil") end),
          }}
        end)"#,
        vacuumed = vacuumed.display(),
        attached = attached.display(),
    );
    let vm = load(&src).await;
    let out = vm.emit("probe", json!(null)).await.unwrap();
    for route in ["vacuum_all", "vacuum_exec", "attach_all", "attach_exec"] {
        assert_eq!(out[route]["ok"], false, "{route} must be refused: {out}");
        let why = out[route]["err"].as_str().unwrap();
        assert!(why.contains("not authorized") || why.contains("authorization denied"), "{route}: {why}");
    }
    assert!(!vacuumed.exists(), "VACUUM INTO wrote a file outside the app's database");
    assert!(!attached.exists(), "ATTACH created a file outside the app's database");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- wave D1: the second review's sandbox holes ----------------------------------------------------------------

// N1: a host call inside a coroutine (it once switched a CPU budget off; there is no budget now).

/// On Lua 5.1 a host call inside the app's own coroutine is not refused (the resume wrappers pass the
/// executor's wait outward, see `async_coroutines.lua`), so nothing is ever parked: the call finishes before
/// the coroutine's `resume` returns, and what the app sees is what it would see on the main thread.
#[tokio::test]
async fn a_host_call_inside_a_coroutine_finishes_before_resume_returns_and_leaves_nothing_parked() {
    let src = r#"
      hook("probe", function()
        local co = coroutine.create(function() return app.kv:get("k") end)
        local ok, value = coroutine.resume(co)
        local wrapped = coroutine.wrap(function() return app.db:all("select 1 as x") end)
        local ok2, rows = pcall(wrapped)
        -- coroutines still do what they are for, and the main thread still reaches the host afterwards
        local gen = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end)
        return { ok = ok, value = value, status = coroutine.status(co), ok2 = ok2, rows = rows,
                 gen = gen() + gen() + gen(), kv = app.kv:get("k") }
      end)
    "#;
    let out = load(src).await.emit("probe", json!(null)).await.unwrap();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["status"], "dead", "the coroutine ran to its end, it is not parked: {out}");
    assert_eq!(out["ok2"], true, "{out}");
    assert_eq!(out["rows"], json!([{"x": 1}]), "{out}");
    assert_eq!(out["gen"], 6, "{out}");
    assert_eq!(out["kv"], Value::Null);
}

/// The same parked call started inside `db:tx` held the database's write gate after the call had returned.
#[tokio::test]
async fn a_coroutine_cannot_leave_the_write_gate_held_after_the_call_returns() {
    let src = r#"
      hook("leak", function()
        local co = coroutine.create(function()
          app.db:tx(function()
            app.db:exec("create table if not exists t(a)")
            app.db:exec("insert into t values (1)")
          end)
        end)
        coroutine.resume(co)
        KEEP = co
        return coroutine.status(co)
      end)
    "#;
    let data = db();
    let vm = try_load_with("leak", src, data.clone()).await.unwrap();
    vm.emit("leak", json!(null)).await.unwrap();
    let write = tokio::time::timeout(
        Duration::from_secs(3),
        data.exec("create table if not exists y(a)".into(), vec![]),
    )
    .await
    .expect("the database's write gate is free once the call has returned");
    write.unwrap();
}

/// A call cancelled between two statements of a `db:tx` (its caller gave up) must not keep the transaction,
/// and with it the write gate, until some later call happens to start.
#[tokio::test]
async fn a_cancelled_call_gives_up_its_transaction_at_once() {
    // a call held at a host call (a statement that never finishes) with the transaction open in the VM
    let src = r#"
      hook("tx", function()
        app.db:tx(function()
          app.db:exec("create table if not exists t(a)")
          app.db:all("select 'stall'")
        end)
      end)
    "#;
    let data = Stalling::new();
    let vm = try_load_with("txcancel", src, data.clone()).await.unwrap();
    let cancelled = tokio::time::timeout(Duration::from_millis(300), vm.emit("tx", json!(null))).await;
    assert!(cancelled.is_err(), "the call is held at the statement and its caller gives up");
    let write = tokio::time::timeout(
        Duration::from_secs(3),
        data.exec("create table if not exists y(a)".into(), Params::None),
    )
    .await
    .expect("the cancelled call's transaction was rolled back and the gate released");
    write.unwrap();
}

/// `db:migrate` takes the database's write gate, which an open transaction holds: inside `db:tx` it would wait
/// for itself for ever, so it is an error that says what to do.
#[tokio::test]
async fn db_migrate_inside_a_transaction_is_an_error_not_a_self_deadlock() {
    let src = r#"
      RPC.on_request("m", function()
        local ok, why = pcall(function()
          app.db:tx(function()
            app.db:exec("create table if not exists t(a)")
            app.db:migrate { [1] = "create table m(a)" }
          end)
        end)
        -- the transaction is gone and the database is free: a migration works now
        app.db:migrate { [1] = "create table m(a)" }
        return { ok = ok, why = tostring(why) }
      end)
    "#;
    let vm = load(src).await;
    let out = tokio::time::timeout(PROMPTLY, common::serve(&vm, "m", json!({}), &agent_ctx(0)))
        .await
        .expect("no self-deadlock")
        .unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["why"].as_str().unwrap().contains("db:migrate cannot run inside db:tx"), "{out}");
}

// ---- wave D1: the second review's sandbox holes ----------------------------------------------------------------

// N5: a hot reload swaps the VM in; the one it replaces must stop, not carry on writing.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hot_reload_retires_the_vm_it_replaces() {
    let rig = rig();
    let v1 =
        format!("{MANIFEST}\nRPC.on_request(\"spin\", function() while true do app.kv:get(\"k\") end end)");
    rig.load("probe", &v1).await.unwrap();
    let (runtime, started) = (rig.runtime.clone(), Instant::now());
    let call = tokio::spawn(async move {
        common::serve_rt(&runtime, &id("probe"), "spin", json!({}), agent_ctx(0)).await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!call.is_finished(), "the call is running on the old VM");

    let v2 = format!("{MANIFEST}\nRPC.on_request(\"spin\", function() return 1 end)");
    rig.load("probe", &v2).await.unwrap();
    let ended = tokio::time::timeout(PROMPTLY, call).await.expect("the replaced VM's call stops").unwrap();
    assert!(ended.unwrap_err().to_string().contains("unloaded"));
    assert!(started.elapsed() < PROMPTLY);
    assert_eq!(rig.tool("probe", "spin").await.unwrap(), json!(1), "the new version answers");
}

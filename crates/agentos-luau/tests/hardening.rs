#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Wave C1: what an untrusted app cannot do to the host. Every test here failed before its fix.
//!
//! - one budget per top-level call, which host calls do not refill, and a kill on unload;
//! - bounded conversion of what the app hands back (tool results, window trees, effects);
//! - a `readOnly` call changes nothing: no data, no windows, no notifications, no shell, no state;
//! - the SQL sandbox holds against `VACUUM INTO` and `ATTACH` through `db:all`.

mod common;

use agentos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
};
use agentos_domain::{Capabilities, Notification};
use agentos_luau::{Limits, LuauRuntime, VmError};
use common::{agent_ctx, db, id, load, package, try_load_with};
use serde_json::{json, Map, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// "Killed by its budget, not still running": far below any wall-clock backstop.
const PROMPTLY: Duration = Duration::from_secs(8);

fn script_error(e: &VmError) -> String {
    e.to_string()
}

// ---- blocker 2: one budget per call, and a kill on unload ----------------------------------------------------

/// A loop that only ever calls the host. Each host call refilled the slice, so no count ran out, and the loop
/// held the VM's gate for ever.
const HOST_LOOP: &str = r#"
    app.on("spin", function() while true do app.kv:get("k") end end)
    app.on("ok", function() return 42 end)
"#;

#[tokio::test]
async fn a_loop_of_host_calls_is_stopped_by_the_calls_tick_budget() {
    // a host call is charged 1,000 ticks: this is about a hundred of them
    let limits = Limits { call_ticks: 100_000, ..Limits::default() };
    let vm = try_load_with("spin", HOST_LOOP, db(), limits).await.unwrap();
    let out = tokio::time::timeout(PROMPTLY, vm.emit("spin", json!(null)))
        .await
        .expect("the call must be killed by its budget, not run for ever");
    assert!(script_error(&out.unwrap_err()).contains("cpu budget exceeded"));
    // the next call has a fresh budget
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(42));
}

#[tokio::test]
async fn a_loop_of_host_calls_is_stopped_by_the_calls_wall_clock_deadline() {
    let limits = Limits { call_ticks: u64::MAX, call_wall: Duration::from_millis(300), ..Limits::default() };
    let vm = try_load_with("spin", HOST_LOOP, db(), limits).await.unwrap();
    let started = std::time::Instant::now();
    let out = tokio::time::timeout(PROMPTLY, vm.emit("spin", json!(null)))
        .await
        .expect("the call must hit its deadline");
    assert!(script_error(&out.unwrap_err()).contains("cpu budget exceeded"));
    assert!(started.elapsed() < PROMPTLY);
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!(42));
}

#[tokio::test]
async fn a_runaway_inside_a_transaction_ends_and_frees_the_write_gate() {
    let src = r#"
        app.on("tx", function() app.db:tx(function() while true do app.kv:get("k") end end) end)
        app.on("write", function() app.db:exec("create table if not exists z(a)") return true end)
    "#;
    let limits = Limits { call_ticks: 100_000, ..Limits::default() };
    let data = db();
    let vm = try_load_with("txloop", src, data.clone(), limits).await.unwrap();
    let out = tokio::time::timeout(PROMPTLY, vm.emit("tx", json!(null))).await.expect("ends");
    assert!(out.is_err());
    // the transaction was rolled back and dropped: a writer is not kept waiting behind it
    let write =
        tokio::time::timeout(PROMPTLY, vm.emit("write", json!(null))).await.expect("write gate is free");
    assert_eq!(write.unwrap(), json!(true));
    let direct = tokio::time::timeout(PROMPTLY, data.exec("create table if not exists y(a)".into(), vec![]))
        .await
        .expect("and so is the database itself");
    direct.unwrap();
}

/// Records what reached the notification port.
#[derive(Default)]
struct Notes(std::sync::Mutex<Vec<Notification>>);
#[async_trait::async_trait]
impl Notifier for Notes {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        self.0.lock().unwrap().push(n);
        Ok(())
    }
}

struct Rig {
    runtime: Arc<LuauRuntime>,
    shell: Arc<RecordingShell>,
    notes: Arc<Notes>,
    db: Arc<agentos_sqlite::AppDb>,
}

fn rig() -> Rig {
    rig_with(Limits::default())
}

fn rig_with(limits: Limits) -> Rig {
    let shell = Arc::new(RecordingShell::default());
    let notes = Arc::new(Notes::default());
    let runtime = Arc::new(LuauRuntime::new(notes.clone(), shell.clone(), limits));
    Rig { runtime, shell, notes, db: db() }
}

impl Rig {
    async fn load(&self, app: &str, src: &str) -> PortResult<agentos_application::ports::LoadedApp> {
        let data: Arc<dyn AppData> = self.db.clone();
        self.runtime.load(&id(app), package(src, &[]), data, Capabilities::all()).await
    }
    async fn tool(&self, app: &str, tool: &str) -> PortResult<Value> {
        self.runtime.call_tool(&id(app), tool, json!({}), agent_ctx(0), false).await
    }
}

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear", permissions = { "data", "notify", "shell" } }"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unloading_stops_a_runaway_call_and_the_vm_is_freed() {
    // a call budget that would never run out in this test: only the unload can end the call
    let rig =
        rig_with(Limits { call_ticks: u64::MAX, call_wall: Duration::from_secs(3600), ..Limits::default() });
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
async fn a_call_may_queue_only_so_many_effects() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        app.tool {{ name = "flood", run = function()
          for i = 1, 100 do app.notify {{ title = "n" .. i }} end
        end }}
        app.tool {{ name = "few", run = function()
          for i = 1, 32 do app.notify {{ title = "n" .. i }} end
          for i = 1, 10000 do app.shell.badge(i) end   -- shell declarations replace each other: no growth
        end }}"#
    );
    rig.load("probe", &src).await.unwrap();
    let err = rig.tool("probe", "flood").await.unwrap_err().to_string();
    assert!(err.contains("too many effects in one call (at most 32"), "{err}");
    // the cap is per call: the next call starts at zero
    rig.tool("probe", "few").await.unwrap();
    assert_eq!(rig.shell.badges.lock().unwrap().len(), 1, "one badge reached the shell, the last");
}

#[tokio::test]
async fn window_commands_are_capped_per_call_too() {
    let rig = rig();
    let src = format!(
        r#"{MANIFEST}
        app.window("main", {{
          onOpen = function(w) for i = 1, 40 do w:announce("x") end end,
          render = function(w) return {{ type = "Stack", key = "n" }} end,
        }})"#
    );
    rig.load("probe", &src).await.unwrap();
    let err = rig.runtime.open_view(&id("probe"), "v1", "main", Map::new(), Map::new()).await.unwrap_err();
    assert!(err.to_string().contains("too many effects"), "{err}");
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
/// refuse by other means; `ATTACH` is denied by the authorizer. Through Luau `db:all` and `db:exec`.
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

// N1: a coroutine parked in a host call switched the CPU budget off for the whole VM.

/// A host call from inside a coroutine yielded to the app's own `coroutine.resume`, which left the "waiting on
/// the host" flag set for the code that went on running: nothing counted and nothing stopped it.
#[tokio::test]
async fn a_coroutine_cannot_park_a_host_call_and_switch_off_the_cpu_budget() {
    let src = r#"
      app.on("spin", function()
        local co = coroutine.create(function() app.kv:get("k") end)
        local ok, why = coroutine.resume(co)
        local n = 0
        while n < 30000000 do n = n + 1 end
        return { ok = ok, why = tostring(why), n = n }
      end)
    "#;
    let limits = Limits { cpu_ticks: 100_000, call_ticks: 200_000, ..Limits::default() };
    let vm = try_load_with("spin", src, db(), limits).await.unwrap();
    let started = Instant::now();
    let err = vm.emit("spin", json!(null)).await.expect_err("the loop must run into the budget");
    assert!(script_error(&err).contains("cpu budget exceeded"), "{err}");
    assert!(started.elapsed() < PROMPTLY);
}

#[tokio::test]
async fn a_host_call_inside_a_coroutine_is_refused_with_an_error_that_says_why() {
    let src = r#"
      app.on("probe", function()
        local co = coroutine.create(function() return app.kv:get("k") end)
        local ok, why = coroutine.resume(co)
        local wrapped = coroutine.wrap(function() return app.db:all("select 1") end)
        local ok2, why2 = pcall(wrapped)
        -- coroutines still do what they are for, and the main thread still reaches the host afterwards
        local gen = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end)
        return { ok = ok, why = tostring(why), ok2 = ok2, why2 = tostring(why2),
                 gen = gen() + gen() + gen(), kv = app.kv:get("k") }
      end)
    "#;
    let out = load(src).await.emit("probe", json!(null)).await.unwrap();
    assert_eq!(out["ok"], false, "{out}");
    assert_eq!(out["ok2"], false, "{out}");
    for why in [&out["why"], &out["why2"]] {
        assert!(why.as_str().unwrap().contains("coroutine"), "{out}");
    }
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
    let vm = try_load_with("leak", src, data.clone(), Limits::default()).await.unwrap();
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
    // `db:migrate` inside the transaction waits for the very gate the transaction holds: a call parked
    // between two statements with the transaction open in the VM
    let src = r#"
      app.on("tx", function()
        app.db:tx(function()
          app.db:exec("create table if not exists t(a)")
          app.db:migrate { [1] = "create table m(a)" }
        end)
      end)
    "#;
    let data = db();
    let vm = try_load_with("txcancel", src, data.clone(), Limits::default()).await.unwrap();
    let cancelled = tokio::time::timeout(Duration::from_millis(300), vm.emit("tx", json!(null))).await;
    assert!(cancelled.is_err(), "the call is parked on the gate and its caller gives up");
    let write = tokio::time::timeout(
        Duration::from_secs(3),
        data.exec("create table if not exists y(a)".into(), vec![]),
    )
    .await
    .expect("the cancelled call's transaction was rolled back and the gate released");
    write.unwrap();
}

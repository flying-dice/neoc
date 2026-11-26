#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `app` API from Lua: require, db, kv, tools, notify, events, the window.

use crate::common;

use common::{agent_ctx, db, id, load, package, try_load};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_domain::Notification;
use vos_lua::{AppVm, Effect, VmConfig, VmError, MAX_QUEUED_NOTIFICATIONS};

/// The first line of every source here, so the line numbers the tests name are the body's plus one.
const HEADER: &str = "-- probe";

fn app(body: &str) -> String {
    format!("{HEADER}\n{body}")
}

async fn call(vm: &AppVm, tool: &str, args: Value) -> Result<Value, VmError> {
    vm.call_tool(tool, args, &agent_ctx(1_763_457_120_000), false).await
}

// ---- require ----------------------------------------------------------------------------------------------

#[tokio::test]
async fn require_gives_the_app_table_and_one_table_per_library_module() {
    let src = r#"
        local app2 = require("vos:app")
        local rpc2 = require("vos:rpc")
        local json, again = require("lib:json"), require("lib:json")
        hook("t", function()
          return { app = app2 == app, rpc = type(rpc2.send_request) == "function" and type(RPC.on_request) == "function",
            same = json == again, t = type(json) }
        end)
        return {}
    "#;
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), common::config()).await.unwrap();
    assert_eq!(
        vm.emit("t", json!(null)).await.unwrap(),
        json!({"app": true, "rpc": true, "same": true, "t": "table"})
    );
}

/// `vos:app`, `vos:rpc` and `vos:docker` are the only `vos:` names an app can require: there is no way to register another.
#[tokio::test]
async fn the_host_offers_no_vos_module_but_its_own() {
    let e = try_load("probe", "local ui = require('vos:ui')").await.err().unwrap().to_string();
    assert!(e.contains("unknown host module \"vos:ui\""), "{e}");
    assert!(
        e.contains("only \"vos:app\", \"vos:rpc\", \"vos:docker\" and the std:, lib: and vnd: modules"),
        "{e}"
    );
}

#[tokio::test]
async fn require_refuses_everything_but_the_host_and_library_modules_with_the_requiring_line() {
    let load_with =
        |src: &'static str| async move { try_load("probe", src).await.err().unwrap().to_string() };
    // an app is bundled into one file: its relative requires are resolved by the build, never at run time
    for spec in ["./calc", "../../etc/passwd", "calc", "@other/app"] {
        let src: &'static str = Box::leak(format!("local x = require({spec:?})").into_boxed_str());
        let e = load_with(src).await;
        assert!(e.contains("can be required at run time"), "{spec}: {e}");
    }
    assert!(load_with("require('vos:nope')").await.contains("unknown host module"));
    assert!(load_with(
        "local lines = 1
require('./nope')"
    )
    .await
    .contains("probe/app.lua:2"));
    // a library module that does not exist says what was meant
    let e = load_with("require('lib:jsn')").await;
    assert!(e.contains("unknown module") && e.contains("did you mean \"lib:json\""), "{e}");
    // the ones left out on purpose are not there
    for spec in ["lib:test", "lib:compile", "std:workers"] {
        let src: &'static str = Box::leak(format!("require({spec:?})").into_boxed_str());
        assert!(load_with(src).await.contains("unknown module"), "{spec}");
    }
}

// ---- app.db -----------------------------------------------------------------------------------------------

#[tokio::test]
async fn migrate_exec_all_one_with_named_and_positional_params() {
    let src = app(r#"
        local db = app.db
        db:migrate { [1] = "create table notes(id integer primary key, body text not null, rev integer not null default 0)" }
        hook("go", function()
            local a = db:exec("insert into notes(body) values (:b)", { b = "milk" })
            local b = db:exec("insert into notes(body) values (?)", { "eggs" })
            local all = db:all("select id, body from notes where id > :n order by id", { n = 0 })
            local one = db:one("select body from notes where id = :id", { id = 2 })
            local none = db:one("select body from notes where id = :id", { id = 99 })
            local upd = db:exec("update notes set rev = rev + 1")
            return { a = a, b = b, n = #all, first = all[1].body, one = one.body, none = none == nil, changes = upd.changes }
        end)
    "#);
    let vm = load(&src).await;
    let out = vm.emit("go", json!(null)).await.unwrap();
    assert_eq!(
        out,
        json!({"a": {"changes": 1, "last_insert_id": 1}, "b": {"changes": 1, "last_insert_id": 2}, "n": 2,
               "first": "milk", "one": "eggs", "none": true, "changes": 2})
    );
}

#[tokio::test]
async fn migrations_are_recorded_ordered_and_a_changed_one_fails_the_load() {
    let shared = db();
    let v1 = app(r#"app.db:migrate { [1] = "create table t(a)" }"#);
    let v2 = app(r#"app.db:migrate { [1] = "create table t(a)", [2] = "alter table t add column b" }"#);
    vos_lua_load(&v1, shared.clone()).await.unwrap();
    vos_lua_load(&v2, shared.clone()).await.unwrap(); // an app upgrade: only [2] runs
    vos_lua_load(&v2, shared.clone()).await.unwrap(); // idempotent
    let e = vos_lua_load(&app(r#"app.db:migrate { [1] = "create table t(a, c)" }"#), shared.clone())
        .await
        .err()
        .unwrap();
    assert!(e.to_string().contains("edited after being applied"), "{e}");
    let e =
        vos_lua_load(&app(r#"app.db:migrate { [1] = "x", [3] = "y" }"#), shared.clone()).await.err().unwrap();
    assert!(e.to_string().contains("a list has no holes; item 2 is missing"), "{e}");
    let e = vos_lua_load(&app(r"app.db:migrate { [1] = 5 }"), shared).await.err().unwrap();
    assert!(e.to_string().contains("item 1: expected a string, got 5"), "{e}");
}

async fn vos_lua_load(src: &str, db: Arc<vos_sqlite::AppDb>) -> Result<AppVm, VmError> {
    common::try_load_with("probe", src, db).await
}

#[tokio::test]
async fn tx_commits_together_rolls_back_on_error_and_cannot_nest() {
    let src = app(r#"
        local db = app.db
        db:migrate { [1] = "create table t(a integer unique)" }
        hook("both", function()
            db:tx(function() db:exec("insert into t values (1)") db:exec("insert into t values (2)") end)
            return db:one("select count(*) as n from t").n
        end)
        hook("failing", function()
            local ok, err = pcall(function() db:tx(function()
                db:exec("insert into t values (10)")
                db:exec("insert into t values (1)")        -- unique violation
            end) end)
            return { ok = ok, n = db:one("select count(*) as n from t").n }
        end)
        hook("raise", function()
            pcall(function() db:tx(function() db:exec("insert into t values (20)") error("changed my mind") end) end)
            return db:one("select count(*) as n from t where a = 20").n
        end)
        hook("nested", function() return pcall(function() db:tx(function() db:tx(function() end) end) end) end)
        hook("value", function() return db:tx(function() return "from tx" end) end)
    "#);
    let vm = load(&src).await;
    assert_eq!(vm.emit("both", json!(null)).await.unwrap(), json!(2));
    assert_eq!(
        vm.emit("failing", json!(null)).await.unwrap(),
        json!({"ok": false, "n": 2}),
        "10 was rolled back"
    );
    assert_eq!(vm.emit("raise", json!(null)).await.unwrap(), json!(0));
    assert_eq!(vm.emit("nested", json!(null)).await.unwrap(), json!(false));
    assert_eq!(vm.emit("value", json!(null)).await.unwrap(), json!("from tx"));
}

#[tokio::test]
async fn kv_stores_json_values() {
    let src = app(r#"
        hook("go", function()
            local before = app.kv:get("history")
            app.kv:set("history", { { expression = "1+1", result = 2 } })
            app.kv:set("n", 7)
            local got = app.kv:get("history")
            app.kv:set("n", nil)
            local gone = app.kv:get("n")
            app.kv:set("flag", true) app.kv:delete("flag")
            return { before = before == nil, entry = got[1].expression, gone = gone == nil, flag = app.kv:get("flag") == nil }
        end)
    "#);
    let vm = load(&src).await;
    assert_eq!(
        vm.emit("go", json!(null)).await.unwrap(),
        json!({"before": true, "entry": "1+1", "gone": true, "flag": true})
    );
}

#[tokio::test]
async fn params_with_whole_numbers_bind_as_integers() {
    let src = app(r#"
        hook("go", function()
            return app.db:one("select typeof(:id) as t, :id as v, typeof(:name) as s", { id = 7, name = "x" })
        end)"#);
    let vm = load(&src).await;
    assert_eq!(vm.emit("go", json!(null)).await.unwrap(), json!({"t": "integer", "v": 7, "s": "text"}));
}

// ---- tools ------------------------------------------------------------------------------------------------

const TOOLS: &str = r#"
    local t = app.t
    app.tool {
      name = "search", title = "Search Notes", readOnly = true,
      description = "Find notes",
      input = { query = t.string { description = "text" }, limit = t.integer { default = 10, min = 1, max = 50 }, folder = t.string { optional = true } },
      run = function(input, ctx) return { q = input.query, n = input.limit, folder = input.folder, who = ctx.actor.name, kind = ctx.actor.kind, id = ctx.actor.id, now = ctx.now } end,
    }
    app.tool {
      name = "fails", description = "x", input = {},
      run = function(_, ctx) return ctx:fail("No note with id 9") end,
    }
    app.tool { name = "bug", run = function() local x = nil; return x.y end }
    app.tool { name = "nothing", run = function() end }
    app.tool { name = "plain", input = { n = t.integer {} }, run = function(i) return i.n end }
"#;

#[tokio::test]
async fn tools_are_registered_with_a_compiled_schema_and_run_as_the_calling_actor() {
    let vm = load(&app(TOOLS)).await;
    let tools = vm.tools();
    let search = tools.iter().find(|t| t.name == "search").unwrap();
    assert_eq!(search.title.as_deref(), Some("Search Notes"));
    assert!(search.read_only);
    assert_eq!(search.input_schema["required"], json!(["query"]));
    assert_eq!(search.input_schema["properties"]["limit"]["maximum"], 50);
    assert!(!tools.iter().find(|t| t.name == "fails").unwrap().read_only);

    let out = call(&vm, "search", json!({"query": "milk"})).await.unwrap();
    assert_eq!(
        out,
        json!({"q": "milk", "n": 10, "who": "Claude", "kind": "agent", "id": "claude", "now": "2025-11-18T09:12:00.000Z"}),
        "defaults filled, optional folder absent"
    );
    assert_eq!(call(&vm, "nothing", json!({})).await.unwrap(), Value::Null);
    assert_eq!(call(&vm, "plain", json!({"n": 3})).await.unwrap(), json!(3));
}

#[tokio::test]
async fn bad_tool_input_is_rejected_before_any_app_code_runs() {
    let vm = load(&app(TOOLS)).await;
    for (args, needle) in [
        (json!({}), "input.query is required"),
        (json!({"query": 5}), "must be a string"),
        (json!({"query": "x", "limit": 99}), "between 1 and 50"),
        (json!({"query": "x", "bogus": 1}), "unknown field `bogus`"),
    ] {
        let e = call(&vm, "search", args).await.unwrap_err();
        assert!(matches!(&e, VmError::InvalidInput(m) if m.contains(needle)), "{e:?}");
    }
    assert!(matches!(call(&vm, "ghost", json!({})).await, Err(VmError::NoSuchTool(_))));
}

#[tokio::test]
async fn ctx_fail_is_a_tool_failure_and_a_bug_is_a_script_error_with_its_line() {
    let vm = load(&app(TOOLS)).await;
    let e = call(&vm, "fails", json!({})).await.unwrap_err();
    assert!(matches!(&e, VmError::ToolFailed(m) if m == "No note with id 9"), "{e:?}");
    let e = call(&vm, "bug", json!({})).await.unwrap_err();
    assert!(matches!(&e, VmError::Script(s) if s.summary.starts_with("probe/app.lua:")), "{e:?}");
}

#[tokio::test]
async fn bad_tool_declarations_fail_the_load() {
    let fails = |tool: &'static str, needle: &'static str| async move {
        let e = try_load("probe", &app(tool)).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
    };
    fails("app.tool { run = function() end }", "argument #1 'def': name: expected a string, got nil").await;
    fails("app.tool { name = 'bad name', run = function() end }", "invalid tool name").await;
    fails("app.tool { name = 'x' }", "run: expected a function, got nil").await;
    fails("app.tool { name = 'x', run = function() end, input = { a = 5 } }", "app.t").await;
    fails("app.tool { name = 'x', run = function() end, readOnly = 'yes' }", "readOnly").await;
    fails(
        "app.tool { name = 'x', run = function() end } app.tool { name = 'x', run = function() end }",
        "already registered",
    )
    .await;
    // a key a tool does not have is refused, not dropped: a raw JSON schema is not a way to declare the input
    fails("app.tool { name = 'x', run = function() end, input_schema = {} }", "unknown key `input_schema`")
        .await;
    fails("app.tool('x')", "expected a tool definition (a table)").await;
}

#[tokio::test]
async fn a_write_made_by_a_tool_is_visible_to_the_next_read() {
    let src = app(r#"
        app.db:migrate { [1] = "create table notes(id integer primary key, body text)" }
        app.tool { name = "add", input = { body = app.t.string {} }, run = function(i, ctx)
            return app.db:exec("insert into notes(body) values (:b)", { b = i.body .. " by " .. ctx.actor.name })
        end }
        app.tool { name = "list", readOnly = true, run = function() return app.db:all("select body from notes") end }
    "#);
    let vm = load(&src).await;
    call(&vm, "add", json!({"body": "milk"})).await.unwrap();
    assert_eq!(call(&vm, "list", json!({})).await.unwrap(), json!([{"body": "milk by Claude"}]));
}

// ---- a call from anywhere may write ---------------------------------------------------------------------------

#[tokio::test]
async fn a_call_that_is_not_read_only_may_write_data_notify_and_read() {
    let src = app(r#"
        app.db:migrate { [1] = "create table t(a)" }
        hook("write", function() app.db:exec("insert into t values (1)") end)
        hook("notify", function() app.notify { title = "x" } end)
        hook("read", function() return app.db:all("select * from t") end)
    "#);
    let vm = load(&src).await;
    vm.emit("write", json!(null)).await.unwrap();
    vm.emit("notify", json!(null)).await.unwrap();
    assert_eq!(vm.emit("read", json!(null)).await.unwrap(), json!([{"a": 1}]));
}

// ---- notify, emit and the window ---------------------------------------------------------------------------------------

#[tokio::test]
async fn notify_emit_and_open_window_become_effects_for_the_host() {
    let src = r#"
        app.notify { title = "Reminder", body = "Call mum" }
        app.notify { title = "No body" }
        app.emit("notes.changed", { id = 12 })
        app.openWindow({ noteId = 12 })
        return {}
    "#;
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), VmConfig::default()).await.unwrap();
    let n = |title: &str, body: Option<&str>| {
        Effect::Notify(Notification { app: id("probe"), title: title.into(), body: body.map(Into::into) })
    };
    assert_eq!(
        vm.take_effects(),
        vec![
            n("Reminder", Some("Call mum")),
            n("No body", None),
            Effect::Emit { name: "notes.changed".into(), data: json!({"id": 12}) },
            Effect::OpenWindow { name: "main".into(), params: json!({"noteId": 12}) },
        ]
    );
    assert!(vm.take_effects().is_empty());
}

#[tokio::test]
async fn notify_and_emit_are_validated() {
    for (call, needle) in [
        (
            "app.notify { body = 'no title' }",
            "vos:app.notify: argument #1 'notification': missing field `title`",
        ),
        ("app.notify { title = '' }", "app.notify: `title` must be 1 to 200 characters, not blank"),
        ("app.notify { title = '   ' }", "not blank"),
        ("app.notify { title = string.rep('t', 201) }", "1 to 200"),
        ("app.notify { title = 5 }", "title: expected a string, got 5"),
        ("app.notify { title = 't', body = false }", "body: expected a string"),
        (
            "app.notify { title = 't', body = string.rep('b', 2001) }",
            "app.notify: `body` must be at most 2000 characters",
        ),
        ("app.emit('')", "an event name is 1 to 64"),
    ] {
        let src = app(call);
        let e = try_load("probe", &src).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
    }
}

/// A notification has a title and a body and nothing else: scheduling (`at`) and buttons (`actions`) are not
/// part of it, and a key it does not have raises rather than being dropped.
#[tokio::test]
async fn notify_refuses_a_key_it_does_not_have() {
    for (call, needle) in [
        ("app.notify { title = 't', at = '2026-01-01T09:00:00Z' }", "unknown field `at`"),
        ("app.notify { title = 't', actions = { 'Open' } }", "unknown field `actions`"),
        ("app.notify { titel = 't' }", "unknown field `titel`"),
    ] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
        assert!(e.contains("expected one of title, body"), "names what it takes: {e}");
    }
    // a value written without a name is a positional entry, the likeliest slip; a key that is not a name
    for call in [
        "app.notify { 'Backup done' }",
        "app.notify { 'positional', title = 't' }",
        "app.notify { title = 't', [true] = 1 }",
    ] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains("vos:app.notify: argument #1 'notification': unknown field"), "{call}: {e}");
    }
    // the longest title and body, and an empty body, are fine
    let vm = load(&app("app.notify { title = string.rep('t', 200), body = string.rep('b', 2000) }\napp.notify { title = 't', body = '' }")).await;
    assert_eq!(vm.take_effects().len(), 2);
    // it is a function of the module, not a method: a colon call passes `app` first, which is one argument too many
    for (call, needle) in [
        ("app.notify()", "argument #1 'notification': expected"),
        ("app.notify(app)", "argument #1 'notification': "),
        ("app:notify { title = 't' }", "argument #1 'notification': unknown field"),
    ] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains(needle), "{call}: {e}");
    }
}

/// A call can queue only so many notifications before the host takes them: the next one raises, so a loop
/// cannot grow the queue without bound. What was queued before the refusal is still delivered, and the next
/// call starts from an empty queue.
#[tokio::test]
async fn one_call_queues_at_most_64_notifications() {
    assert_eq!(MAX_QUEUED_NOTIFICATIONS, 64);
    let vm =
        load(&app(r#"hook("burst", function(n) for i = 1, n do app.notify { title = "n" .. i } end end)"#))
            .await;
    vm.emit("burst", json!(64)).await.unwrap();
    assert_eq!(vm.take_effects().len(), 64);
    let e = vm.emit("burst", json!(65)).await.unwrap_err().to_string();
    assert!(e.contains("app.notify: this call already has 64 notifications waiting to be delivered; combine them into one notification"), "{e}");
    assert_eq!(vm.take_effects().len(), 64);
    vm.emit("burst", json!(1)).await.unwrap();
    assert_eq!(vm.take_effects().len(), 1);
}

/// The cap is each call's own: what another call queued while this one was parked at a wait does not count
/// against it. Here, at the VM level, nothing takes the queue (the runtime would, when `b` ends), so this also
/// shows that a call's own count survives its wait while its notifications are still waiting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_parked_call_does_not_use_up_another_calls_notifications() {
    let vm = Arc::new(
        load(&app(r#"
            local task = require("std:task")
            local parked, go = false, false
            local function burst(n) for i = 1, n do app.notify { title = "n" .. i } end end
            hook("a", function()
              burst(40)
              parked = true
              while not go do task.wait(0.01) end -- parked until the test says go
              burst(25)
            end)
            hook("b", function() burst(64) end)
            hook("parked", function() return parked end)
            hook("go", function() go = true end)
        "#))
        .await,
    );
    let a = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.emit("a", json!(null)).await })
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while vm.emit("parked", json!(null)).await.unwrap() != json!(true) {
        assert!(tokio::time::Instant::now() < deadline, "`a` never reached its wait");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    // `a` has 40 waiting and is parked: `b` still gets its own 64
    vm.emit("b", json!(null)).await.unwrap();
    vm.emit("go", json!(null)).await.unwrap();
    // nothing took the queue, so `a`'s own 40 are still waiting: its 25th after the wait is its 65th, and raises
    let e = a.await.unwrap().unwrap_err().to_string();
    assert!(e.contains("app.notify: this call already has 64 notifications waiting to be delivered; combine them into one notification"), "{e}");
    assert_eq!(vm.take_effects().len(), 64 + 64);
}

/// A tool that notifies and then works for a while does not hold the notification back until it returns: the
/// VM wakes the runtime at the wait (`effects_signal`), which takes what is queued. A call that waits and has
/// notified nothing says nothing: a tool is over when it returns.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_that_notifies_and_then_waits_has_its_notification_taken_at_the_wait() {
    let vm = Arc::new(
        load(&app(r#"
            local task = require("std:task")
            local go = false
            hook("quiet", function() task.wait(0.05) end)
            hook("slow", function()
              app.notify { title = "half way", body = "still working" }
              while not go do task.wait(0.01) end
              app.notify { title = "done" }
            end)
            hook("go", function() go = true end)
        "#))
        .await,
    );
    let signal = vm.effects_signal();
    let quiet = std::time::Duration::from_millis(300);
    // what loading left behind
    let _ = tokio::time::timeout(quiet, signal.notified()).await;
    let _ = vm.take_effects();

    // waiting without having notified is silent
    vm.emit("quiet", json!(null)).await.unwrap();
    assert!(
        tokio::time::timeout(quiet, signal.notified()).await.is_err(),
        "a wait with nothing to say woke the host"
    );

    // notifying and then waiting is heard while the call is still running
    let slow = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.emit("slow", json!(null)).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), signal.notified())
        .await
        .expect("the host was never woken at the wait");
    assert!(!slow.is_finished(), "the call is still waiting");
    let taken = vm.take_effects();
    assert_eq!(taken.len(), 1);
    assert!(
        matches!(&taken[0], Effect::Notify(n) if n.title == "half way" && n.body.as_deref() == Some("still working"))
    );

    // the rest goes when the call ends, as before
    vm.emit("go", json!(null)).await.unwrap();
    slow.await.unwrap().unwrap();
    let rest = vm.take_effects();
    assert!(matches!(rest.as_slice(), [Effect::Notify(n)] if n.title == "done"), "{rest:?}");
}

// ---- readOnly tools cannot write ----------------------------------------------------------------------------

const READ_ONLY: &str = r#"
    local db = app.db
    db:migrate { [1] = "create table log(id integer primary key, msg text)" }
    db:exec("insert into log(msg) values ('seed')")
    local function try(f) local ok, err = pcall(f); return { ok = ok, err = (not ok) and tostring(err) or nil } end
    app.tool { name = "count", readOnly = true, run = function() return { n = db:one("select count(*) as n from log").n } end }
    app.tool { name = "write_exec", readOnly = true, run = function() return try(function() db:exec("insert into log(msg) values ('x')") end) end }
    app.tool { name = "write_returning", readOnly = true, run = function() return try(function() db:all("insert into log(msg) values ('y') returning id") end) end }
    app.tool { name = "write_tx", readOnly = true, run = function() return try(function() db:tx(function() db:exec("delete from log") end) end) end }
    app.tool { name = "write_ddl", readOnly = true, run = function() return try(function() db:all("drop table log") end) end }
    app.tool { name = "lift_it", readOnly = true, run = function() return try(function() db:all("pragma query_only = 0") end) end }
    app.tool { name = "write_kv", readOnly = true, run = function() return try(function() app.kv:set("k", 1) end) end }
    app.tool { name = "delete_kv", readOnly = true, run = function() return try(function() app.kv:delete("k") end) end }
    app.tool { name = "write_migrate", readOnly = true, run = function() return try(function() db:migrate { [1] = "create table log(id integer primary key, msg text)", [2] = "create table z(a)" } end) end }
    app.tool { name = "add", run = function(_, ctx) db:exec("insert into log(msg) values (:m)", { m = ctx.actor.name }); app.kv:set("k", 1); return { n = db:one("select count(*) as n from log").n } end }
"#;

#[tokio::test]
async fn a_read_only_tool_cannot_write_by_any_route_and_a_normal_tool_still_can() {
    let vm = load(&app(READ_ONLY)).await;
    assert_eq!(call(&vm, "count", json!({})).await.unwrap(), json!({"n": 1}), "reads work");

    for tool in [
        "write_exec",
        "write_returning",
        "write_tx",
        "write_ddl",
        "lift_it",
        "write_kv",
        "delete_kv",
        "write_migrate",
    ] {
        let out = call(&vm, tool, json!({})).await.unwrap();
        assert_eq!(out["ok"], false, "{tool} must be refused: {out}");
        let why = out["err"].as_str().unwrap().to_lowercase();
        assert!(
            why.contains("readonly") || why.contains("read-only") || why.contains("not authorized"),
            "{tool}: {why}"
        );
    }
    assert_eq!(call(&vm, "count", json!({})).await.unwrap(), json!({"n": 1}), "nothing was written");

    // the VM is writable again for the next, non-readOnly call (even after the failures above)
    assert_eq!(call(&vm, "add", json!({})).await.unwrap(), json!({"n": 2}));
    assert_eq!(call(&vm, "count", json!({})).await.unwrap(), json!({"n": 2}));
}

/// `vos:docker` is Luau the host carries, run in the app's own VM: one table per app, which keeps the app's choice of
/// Docker in the app's own settings. A Docker that is not there is an answer, and nothing an app passes is an error.
#[tokio::test]
async fn the_container_framework_is_required_like_the_app_table_and_answers_without_a_docker() {
    let src = r#"
        local docker, again = require("vos:docker"), require("vos:docker")
        hook("t", function()
            local server = docker.container("vos-probe")
            local out = { same = docker == again, first = docker.host() }
            out.not_an_address = docker.use("nas.local:2375")
            out.too_long = docker.use("tcp://" .. string.rep("a", 400))
            out.not_a_string = docker.use(7)
            out.taken = docker.use("  tcp://127.0.0.1:1 ") == nil
            out.host, out.kept = docker.host(), app.kv:get("host")
            local status = docker.status()
            out.reachable, out.problem = status.reachable, status.problem
            out.absent = server:status({ image = "nginx" }).state
            out.applied = server:apply({ image = "nginx" })
            out.undeclared = server:status(nil).problem
            local logs, why = docker.logs("vos-probe", 0 / 0)
            out.logs, out.why = logs == nil, why
            out.filter = server.filters.label[1]
            out.back = docker.use("") == nil and docker.host() == "" and app.kv:get("host") == nil
            return out
        end)
        return {}
    "#;
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), common::config()).await.unwrap();
    let out = vm.emit("t", json!(null)).await.unwrap();
    // an address every platform takes (a Unix socket is none on Windows), where nothing listens
    let away = "Docker cannot be reached at tcp://127.0.0.1:1.";
    assert_eq!(out["same"], json!(true), "{out}");
    assert_eq!(out["first"], json!(""), "{out}");
    assert!(out["not_an_address"].as_str().unwrap().starts_with("The address must start with"), "{out}");
    assert_eq!(out["too_long"], json!("That is not an address of a Docker."), "{out}");
    assert_eq!(out["not_a_string"], json!("That is not an address of a Docker."), "{out}");
    assert_eq!(out["taken"], json!(true), "{out}");
    assert_eq!(out["host"], json!("tcp://127.0.0.1:1"), "{out}");
    assert_eq!(out["kept"], out["host"], "{out}");
    assert_eq!(out["reachable"], json!(false), "{out}");
    assert_eq!(out["problem"], json!(away), "{out}");
    assert_eq!(out["absent"], json!("absent"), "{out}");
    assert_eq!(out["applied"], json!(away), "{out}");
    assert_eq!(out["undeclared"], json!(away), "what Docker refused is kept for the status: {out}");
    assert_eq!(out["logs"], json!(true), "{out}");
    assert_eq!(out["why"], json!(away), "{out}");
    assert_eq!(out["filter"], json!("vos.app=probe"), "{out}");
    assert_eq!(out["back"], json!(true), "{out}");
}

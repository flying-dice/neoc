#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `app` API from Lua: manifest, require, db, kv, sources, tools, state, notify, shell.

mod common;

use common::{agent_ctx, db, id, load, package, try_load, user_ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::live::SourceKind;
use vos_domain::Notification;
use vos_lua::{AppVm, Effect, ShellEffect, VmConfig, VmError, MAX_QUEUED_NOTIFICATIONS};

const MANIFEST: &str = r#"app.manifest { id = "probe", name = "Probe", icon = "gear" }"#;

fn app(body: &str) -> String {
    format!("{MANIFEST}\n{body}")
}

async fn call(vm: &AppVm, tool: &str, args: Value) -> Result<Value, VmError> {
    vm.call_tool(tool, args, &agent_ctx(1_763_457_120_000), false).await
}

// ---- app.manifest ---------------------------------------------------------------------------------------

#[tokio::test]
async fn the_manifest_is_parsed_validated_and_checked_against_the_package() {
    let vm = load(&app("")).await;
    let decl = vm.decl().unwrap();
    assert_eq!((decl.id.as_str(), decl.name.as_str(), decl.icon.as_str()), ("probe", "Probe", "gear"));

    let full = r#"app.manifest { id = "probe", name = "P", icon = "gear", tint = "orange", kit = ">=1.0",
        window = { w = 340, h = 560, minW = 320, minH = 480 } }"#;
    let decl = load(full).await.decl().unwrap();
    assert_eq!(decl.window.unwrap().min_h, 480);

    let fails = |src: &'static str, needle: &'static str| async move {
        let e = try_load("probe", src).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
        assert!(e.contains("probe/app.lua:1"), "with the line: {e}");
    };
    fails(r#"app.manifest { id = "other", name = "P", icon = "gear" }"#, "does not match the package id")
        .await;
    fails(r#"app.manifest { id = "probe", name = "P", icon = "gear", colour = "red" }"#, "unknown field")
        .await;
    fails(r#"app.manifest { id = "probe", name = "P", icon = "gear", window = { w = 100, h = 100, minW = 100, minH = 100 } }"#, "at least 320x240").await;
    fails(r#"app.manifest { id = "probe", name = "P", icon = "gear", kit = "1.0" }"#, "kit").await;
    fails("app.manifest()", "takes a table").await;

    // apps have no permissions: a `permissions` key from before is accepted and ignored
    let legacy =
        r#"app.manifest { id = "probe", name = "P", icon = "gear", permissions = { "data", "root" } }"#;
    assert!(try_load("probe", legacy).await.is_ok());

    let e = try_load("probe", &app(MANIFEST)).await.err().unwrap().to_string();
    assert!(e.contains("already called"), "{e}");
}

// ---- require ----------------------------------------------------------------------------------------------

#[tokio::test]
async fn require_loads_the_two_host_modules_and_caches_them() {
    let src = r#"
        local app2 = require("vos:app")
        local ui = require("vos:ui")
        local again = require("vos:ui")
        hook("t", function() return { app = app2 == app, same = ui == again, kit = type(ui.VStack), t = type(ui.each) } end)
    "#;
    let host_modules =
        std::collections::HashMap::from([("ui".to_owned(), vos_lua::UI_MODULE.as_bytes().to_vec())]);
    let config = VmConfig { host_modules: Arc::new(host_modules), ..common::config() };
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), config).await.unwrap();
    assert_eq!(
        vm.emit("t", json!(null)).await.unwrap(),
        json!({"app": true, "same": true, "kit": "function", "t": "function"})
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

#[tokio::test]
async fn a_host_module_that_requires_itself_is_a_cycle_not_a_stack_overflow() {
    let mut modules = std::collections::HashMap::new();
    modules.insert("loop".to_owned(), b"return require('vos:loop')".to_vec());
    let config = VmConfig { host_modules: Arc::new(modules), ..VmConfig::default() };
    let e = AppVm::load(&id("probe"), package("require('vos:loop')", &[]), db(), config)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("cyclic require"), "{e}");
}

#[tokio::test]
async fn host_modules_are_registered_by_the_host_and_load_once() {
    let mut modules = std::collections::HashMap::new();
    modules.insert("ui".to_owned(), b"local n = (n or 0) return { version = 1 }".to_vec());
    let cfg = VmConfig { host_modules: Arc::new(modules), ..common::config() };
    let src = "local ui = require('vos:ui')\nhook('t', function() return ui.version end)";
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), cfg).await.unwrap();
    assert_eq!(vm.emit("t", json!(null)).await.unwrap(), json!(1));
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
    assert!(e.to_string().contains("without gaps"), "{e}");
    let e = vos_lua_load(&app(r"app.db:migrate { [1] = 5 }"), shared).await.err().unwrap();
    assert!(e.to_string().contains("[1] must be a SQL string"), "{e}");
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

// ---- sources ----------------------------------------------------------------------------------------------

const NOTES_DB: &str = r"
    local db = app.db
    db:migrate { [1] = [[
      create table folders(id integer primary key, name text not null);
      create table notes(id integer primary key, folder_id integer not null, body text not null default '', rev integer not null default 0);
      insert into folders(name) values ('Notes');
    ]] }
";

#[tokio::test]
async fn sources_are_prepared_validated_and_listed_with_their_dependencies() {
    let src = app(&format!(
        "{NOTES_DB}
        local folders = db:query([[select f.id, f.name, (select count(*) from notes n where n.folder_id = f.id) as count from folders f order by f.id]], {{ key = 'id' }})
        local list = db:query([[select id, body from notes where (:folder = 0 or folder_id = :folder) and body like '%' || :q || '%' order by id desc]], {{ key = 'id', page = 60 }})
        hook('peek', function() return db:one('select body from notes where id = 1').body end)
        local note = db:row('select id, body, rev from notes where id = :id', {{ key = 'id', rev = 'rev', write = function() end }})
        local n = db:scalar('select count(*) from notes')
        hook('handles', function()
            local h = list {{ folder = 1, q = '' }}
            return {{ folders = folders {{}}['$source'], list = h['$source'], note = note {{ id = 3 }}['$source'], n = n()['$source'], field = note {{ id = 3 }}.body['$ref'], bound = note {{ id = 3 }}:bind('body')['$ref'], dollar = h['$nothing'] }}
        end)"
    ));
    let vm = load(&src).await;
    let defs = vm.sources();
    assert_eq!(defs.iter().map(|d| d.id.0.as_str()).collect::<Vec<_>>(), ["s1", "s2", "s3", "s4"]);
    assert_eq!(defs[0].info.tables, ["folders".to_owned(), "notes".to_owned()].into(), "a sub-select counts");
    assert_eq!(defs[1].info.named_params, ["folder", "q"]);
    assert!(matches!(&defs[1].kind, SourceKind::Rows { key, page: Some(60), .. } if key == "id"));
    assert!(matches!(&defs[2].kind, SourceKind::Row { rev: Some(r), has_write: true, .. } if r == "rev"));
    assert!(matches!(defs[3].kind, SourceKind::Scalar));
    assert_eq!(
        vm.emit("handles", json!(null)).await.unwrap(),
        json!({"folders": "s1", "list": "s2", "note": "s3", "n": "s4", "field": "src.s3.body", "bound": "src.s3.body"})
    );
}

#[tokio::test]
async fn bad_sources_fail_at_load_with_the_declaring_line() {
    let fails = |sources: &'static str, needle: &'static str| async move {
        let src = app(&format!("{NOTES_DB}\n{sources}"));
        let e = try_load("probe", &src).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
    };
    fails("db:query('select * from nope', { key = 'id' })", "no such table").await;
    fails("db:query('select id from notes order by body', { key = 'id', page = 10 })", "total order").await;
    fails("db:query('select body from notes', { key = 'id' })", "key column `id`").await;
    fails("db:query('select id from notes', {})", "needs { key").await;
    fails("db:query('select id from notes', { key = 'id', pge = 3 })", "unknown option `pge`").await;
    fails("db:query('delete from notes', { key = 'id' })", "read-only").await;
    fails("db:row('select id from notes where id = :id', { key = 'id', rev = 'rev' })", "rev column").await;
    fails("db:scalar('select id, body from notes')", "exactly one column").await;
    fails("db:query('select id from notes order by id', { key = 'id', total = 'nope' })", "total must be")
        .await;
    fails("db:row('select id from notes', { key = 'id', write = 5 })", "write must be a function").await;
}

#[tokio::test]
async fn source_handles_check_their_params() {
    let src = app(&format!(
        "{NOTES_DB}
        local note = db:row('select id, body from notes where id = :id', {{ key = 'id' }})
        hook('ok', function() return note {{ id = 1 }}['$source'] end)
        hook('missing', function() return note {{}} end)
        hook('extra', function() return note {{ id = 1, other = 2 }} end)"
    ));
    let vm = load(&src).await;
    assert_eq!(vm.emit("ok", json!(null)).await.unwrap(), json!("s1"));
    assert!(vm.emit("missing", json!(null)).await.unwrap_err().to_string().contains("needs param `id`"));
    let e = vm.emit("extra", json!(null)).await.unwrap_err().to_string();
    assert!(e.contains("has no param `other`") && e.contains("probe/app.lua:"), "{e}");
}

#[tokio::test]
async fn the_write_callback_runs_with_key_field_value_and_a_context() {
    let src = app(&format!(
        "{NOTES_DB}
        hook('peek', function() return db:one('select body from notes where id = 1').body end)
        local note = db:row('select id, body, rev from notes where id = :id', {{ key = 'id', rev = 'rev',
          write = function(id, field, value, ctx)
            assert(field == 'body', 'only body is writable')
            db:exec('update notes set body = :v, rev = rev + 1 where id = :id', {{ v = value .. ' (' .. ctx.actor.name .. ' at ' .. ctx.now .. ')', id = id }})
          end }})
        db:exec('insert into notes(folder_id, body) values (1, \\'x\\')')"
    ));
    let vm = load(&src).await;
    let source = vos_application::live::SourceId("s1".into());
    vm.write_row(&source, &json!(1), "body", &json!("hello"), &user_ctx(1_763_457_120_000)).await.unwrap();
    let row = db_row(&vm).await;
    assert_eq!(row, json!("hello (You at 2025-11-18T09:12:00.000Z)"));
    let e =
        vm.write_row(&source, &json!(1), "title", &json!("x"), &user_ctx(0)).await.unwrap_err().to_string();
    assert!(e.contains("only body is writable") && e.contains("probe/app.lua:"), "{e}");
    let e = vm
        .write_row(
            &vos_application::live::SourceId("s9".into()),
            &json!(1),
            "body",
            &json!("x"),
            &user_ctx(0),
        )
        .await;
    assert!(matches!(e, Err(VmError::NoSuchWriter(_))));
}

async fn db_row(vm: &AppVm) -> Value {
    // read through a tool-less path: a temporary hook is not available, so reuse the VM's own database
    vm.emit("peek", json!(null)).await.unwrap()
}

// ---- tools ------------------------------------------------------------------------------------------------

const TOOLS: &str = r#"
    local t = app.t
    app.tools.register {
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
    app.tool { name = "raw", input_schema = { type = "object", properties = { n = { type = "integer" } }, required = { "n" } }, run = function(i) return i.n end }
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
    assert_eq!(call(&vm, "raw", json!({"n": 3})).await.unwrap(), json!(3));
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
    fails("app.tool { run = function() end }", "needs a `name`").await;
    fails("app.tool { name = 'bad name', run = function() end }", "invalid tool name").await;
    fails("app.tool { name = 'x' }", "needs a `run").await;
    fails("app.tool { name = 'x', run = function() end, input = { a = 5 } }", "app.t").await;
    fails("app.tool { name = 'x', run = function() end, readOnly = 'yes' }", "readOnly").await;
    fails(
        "app.tool { name = 'x', run = function() end } app.tool { name = 'x', run = function() end }",
        "already registered",
    )
    .await;
    fails("app.tool { name = 'x', run = function() end, input = {}, input_schema = {} }", "not both").await;
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

// ---- app.state --------------------------------------------------------------------------------------------

#[tokio::test]
async fn state_reads_are_tracked_and_assignments_mark_fields_dirty() {
    let src = app(r#"
        local s = app.state { entry = "0", acc = 0, caption = "" }
        hook("render", function() return s.entry .. "|" .. s.caption end)
        hook("press", function(d) s.entry = d.digit return s.entry end)
        hook("noop", function() s.acc = 0 end)
        hook("render_write", function() return s.entry end)
        hook("shared", function() return app.state { x = 1 } ~= s end)
    "#);
    let vm = load(&src).await;
    let (out, deps) = vm.emit_tracked("render", json!(null)).await.unwrap();
    assert_eq!(out, json!("0|"));
    let keys: Vec<String> = deps.iter().map(|d| d.key.clone()).collect();
    assert_eq!(keys, ["caption", "entry"], "exactly the fields read");
    assert!(vm.take_dirty_state().is_empty());

    vm.emit("press", json!({"digit": "7"})).await.unwrap();
    let dirty = vm.take_dirty_state();
    assert_eq!(dirty.iter().map(|d| d.key.as_str()).collect::<Vec<_>>(), ["entry"]);
    assert!(deps.is_superset(&dirty), "a view that read `entry` must re-render");
    assert!(vm.take_dirty_state().is_empty(), "taken once");

    vm.emit("noop", json!(null)).await.unwrap();
    assert!(vm.take_dirty_state().is_empty(), "assigning the same value is not a change");
    assert_eq!(vm.emit("render_write", json!(null)).await.unwrap(), json!("7"));
    assert_eq!(vm.emit("shared", json!(null)).await.unwrap(), json!(true));
}

#[tokio::test]
async fn render_is_pure_state_db_and_effects_raise_while_tracking_a_render() {
    // `emit_tracked(.., pure)` is the host's render scope; here the handler stands in for render
    let src = app(r#"
        local s = app.state { n = 0 }
        app.db:migrate { [1] = "create table t(a)" }
        hook("bad_state", function() s.n = 1 end)
        hook("bad_db", function() app.db:exec("insert into t values (1)") end)
        hook("bad_notify", function() app.notify { title = "x" } end)
        hook("ok_read", function() return app.db:all("select * from t") end)
    "#);
    let vm = load(&src).await;
    // outside a render all of these are fine
    vm.emit("bad_state", json!(null)).await.unwrap();
    vm.emit("bad_db", json!(null)).await.unwrap();
    vm.emit("bad_notify", json!(null)).await.unwrap();
    assert_eq!(vm.emit("ok_read", json!(null)).await.unwrap(), json!([{"a": 1}]));
}

// ---- notify and shell ---------------------------------------------------------------------------------------

#[tokio::test]
async fn notify_and_shell_calls_become_effects_for_the_host() {
    let src = r#"
        app.manifest { id = "probe", name = "Probe", icon = "gear" }
        app.notify { title = "Reminder", body = "Call mum" }
        app:notify { title = "Colon call" }
        app.shell.deepLink("note/:old", function(params) end)
        app.shell.deepLink("note/:id", function(params) end)
        app.window("main", { render = function(w) end })
        app:openWindow("main", { noteId = 12 })
    "#;
    let vm = AppVm::load(&id("probe"), package(src, &[]), db(), VmConfig::default()).await.unwrap();
    let n = |title: &str, body: Option<&str>| {
        Effect::Notify(Notification { app: id("probe"), title: title.into(), body: body.map(Into::into) })
    };
    assert_eq!(
        vm.take_effects(),
        vec![
            n("Reminder", Some("Call mum")),
            n("Colon call", None),
            // two links: the shell only ever needs the last list, so the first declaration's is replaced
            Effect::Shell(ShellEffect::DeepLinks(vec!["note/:old".into(), "note/:id".into()])),
            Effect::OpenWindow { name: "main".into(), params: json!({"noteId": 12}) },
        ]
    );
    assert!(vm.take_effects().is_empty());
}

#[tokio::test]
async fn notify_and_deep_links_are_validated() {
    for (call, needle) in [
        ("app.notify { body = 'no title' }", "app.notify needs `title`"),
        ("app.notify { title = '' }", "app.notify: `title` must be 1 to 200 characters, not blank"),
        ("app.notify { title = '   ' }", "not blank"),
        ("app.notify { title = string.rep('t', 201) }", "1 to 200"),
        ("app.notify { title = 5 }", "app.notify: `title` must be a string, got number"),
        ("app.notify { title = 't', body = false }", "`body` must be a string, got boolean"),
        (
            "app.notify { title = 't', body = string.rep('b', 2001) }",
            "app.notify: `body` must be at most 2000 characters",
        ),
        ("app.shell.deepLink('', function() end)", "1 to 128"),
    ] {
        let src = format!("app.manifest {{ id = 'probe', name = 'P', icon = 'gear' }}\n{call}");
        let e = try_load("probe", &src).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
    }
}

/// A notification has a title and a body and nothing else: scheduling (`at`) and buttons (`actions`) are not
/// part of it, and a key it does not have raises rather than being dropped.
#[tokio::test]
async fn notify_refuses_a_key_it_does_not_have() {
    for (call, needle) in [
        ("app.notify { title = 't', at = '2026-01-01T09:00:00Z' }", "unknown key `at`"),
        ("app.notify { title = 't', actions = { 'Open' } }", "unknown key `actions`"),
        ("app.notify { titel = 't' }", "unknown key `titel`"),
        ("app.notify { 'positional', title = 't' }", "unknown key of type number"),
    ] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
        assert!(e.contains("(it takes `title`, `body`)"), "names what it takes: {e}");
    }
    // the longest title and body, and an empty body, are fine
    let vm = load(&app("app.notify { title = string.rep('t', 200), body = string.rep('b', 2000) }\napp.notify { title = 't', body = '' }")).await;
    assert_eq!(vm.take_effects().len(), 2);
    // a colon call with no table is told it needs one; `app`'s own keys are not read as a notification's
    for call in ["app:notify()", "app.notify(app)", "app.notify()"] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains("app.notify takes a table"), "{call}: {e}");
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
    assert!(e.contains("app.notify: at most 64 notifications per call"), "{e}");
    assert_eq!(vm.take_effects().len(), 64);
    vm.emit("burst", json!(1)).await.unwrap();
    assert_eq!(vm.take_effects().len(), 1);
}

/// The cap is each call's own: what another call queued while this one was parked at a wait does not count
/// against it, so one call cannot make another hit the cap early.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_parked_call_does_not_use_up_another_calls_notifications() {
    let vm = Arc::new(
        load(&app(r#"
            local task = require("std:task")
            local function burst(n) for i = 1, n do app.notify { title = "n" .. i } end end
            hook("a", function() burst(40) task.wait(0.3) burst(24) end)
            hook("b", function() burst(64) end)
        "#))
        .await,
    );
    let a = {
        let vm = vm.clone();
        tokio::spawn(async move { vm.emit("a", json!(null)).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // `a` has 40 waiting and is parked: `b` still gets its own 64
    vm.emit("b", json!(null)).await.unwrap();
    // and `a` its remaining 24, though 104 are queued in all
    a.await.unwrap().unwrap();
    assert_eq!(vm.take_effects().len(), 128);
}

// ---- windows ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn windows_are_declared_by_name() {
    let src = app("app.window('main', { title = 'Notes', render = function(w) end })\napp.window('settings', { render = function() end })");
    let vm = load(&src).await;
    assert_eq!(vm.window_names(), ["main", "settings"]);
    for (call, needle) in [
        ("app.window('main', { title = 'x' })", "needs `render"),
        ("app.window('bad name', { render = function() end })", "letters, digits"),
        (
            "app.window('a', { render = function() end }) app.window('a', { render = function() end })",
            "already declared",
        ),
    ] {
        let e = try_load("probe", &app(call)).await.err().unwrap().to_string();
        assert!(e.contains(needle), "wanted `{needle}` in `{e}`");
    }
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

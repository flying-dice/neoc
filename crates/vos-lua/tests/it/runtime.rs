#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `AppRuntime` port over the real Lua adapter and the real SQLite adapter: data written by a tool and read by
//! the page's handlers, effects forwarded to ports, a failing app isolated.

use crate::common;

use common::{agent_ctx, db, id, package, user_ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    data::Params,
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;
use vos_sqlite::AppDb;

const NOTES: &str = r#"
-- notes
local t = app.t
local db = app.db

db:migrate { [1] = [[
  create table folders(id integer primary key, name text not null);
  create table notes(id integer primary key, folder_id integer not null, body text not null default '',
                     rev integer not null default 0, author text);
  insert into folders(name) values ('Notes');
  insert into notes(folder_id, body) values (1, 'first'), (1, 'second');
]] }

app.tool { name = "append", input = { id = t.integer {}, text = t.string {} }, run = function(input, ctx)
  local r = db:exec("update notes set body = body || char(10) || :text, rev = rev + 1, author = :who where id = :id",
    { text = input.text, id = input.id, who = ctx.actor.name })
  if r.changes == 0 then return ctx:fail("No note with id " .. input.id) end
  app.notify { title = "Note changed", body = ctx.actor.name }
  app.emit("notes.changed", { changes = r.changes })
  return { id = input.id }
end }
app.tool { name = "add_two", run = function(_, ctx)
  db:tx(function()
    db:exec("insert into notes(folder_id, body) values (1, 'a')")
    db:exec("insert into notes(folder_id, body) values (1, 'b')")
  end)
  return "ok"
end }

return {
  -- the page's list: what it shows after an agent's tool (or anything else) changed the notes
  list = function(ctx)
    return db:all("select id, body, rev, author from notes order by id")
  end,
  -- the person edits a note from the page
  edit = function(ctx, args)
    assert(args.field == "body", "only the body can be edited")
    db:exec("update notes set body = :v, rev = rev + 1, author = :who where id = :id", { v = args.value, id = args.id, who = ctx.actor.name })
    return db:one("select body, rev from notes where id = :id", { id = args.id })
  end,
}
"#;

#[derive(Default)]
struct RecordingNotifier(Mutex<Vec<Notification>>);
#[async_trait::async_trait]
impl Notifier for RecordingNotifier {
    async fn notify(&self, n: Notification) -> PortResult<()> {
        self.0.lock().push(n);
        Ok(())
    }
}

struct Rig {
    runtime: LuaRuntime,
    notifier: Arc<RecordingNotifier>,
    shell: Arc<RecordingShell>,
    db: Arc<AppDb>,
}

fn rig() -> Rig {
    let notifier = Arc::new(RecordingNotifier::default());
    let shell = Arc::new(RecordingShell::default());
    let runtime = LuaRuntime::new(notifier.clone(), shell.clone());
    Rig { runtime, notifier, shell, db: db() }
}

async fn load(rig: &Rig, src: &str) -> PortResult<vos_application::ports::LoadedApp> {
    let data: Arc<dyn AppData> = rig.db.clone();
    rig.runtime.load(&id("notes"), package(src, &[]), data).await
}

#[tokio::test]
async fn loading_reports_what_the_app_declared() {
    let rig = rig();
    let loaded = load(&rig, NOTES).await.unwrap();
    assert_eq!(loaded.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["append", "add_two"]);
}

#[tokio::test]
async fn an_entry_that_returns_no_handlers_does_not_load() {
    let rig = rig();
    let e = load(&rig, "local x = 1").await.unwrap_err();
    assert!(
        matches!(&e, PortError::Rejected(m) if m.contains("the entry must return the app's handlers")),
        "{e:?}"
    );
}

#[tokio::test]
async fn an_agent_tool_writes_data_the_page_reads_it_and_the_tools_effects_reach_their_ports() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    let list =
        || async { rig.runtime.call_handler(&id("notes"), "list", json!(null), user_ctx(2_000)).await };
    let before = list().await.unwrap();
    assert_eq!(before[0], json!({"id": 1, "body": "first", "rev": 0}), "a null column is no key");

    // the agent appends to note 1 over the tool: no UI code anywhere
    let out = rig
        .runtime
        .call_tool(
            &id("notes"),
            "append",
            json!({"id": 1, "text": "- buy oat milk"}),
            agent_ctx(1_000),
            false,
        )
        .await
        .unwrap();
    assert_eq!(out, json!({"id": 1}));

    // the page's next call sees the tool's write, and only that row changed
    let after = list().await.unwrap();
    assert_eq!(after[0], json!({"id": 1, "body": "first\n- buy oat milk", "rev": 1, "author": "Claude"}));
    assert_eq!(after[1], before[1]);

    // the tool's app.notify and app.emit reached their ports
    let notes = rig.notifier.0.lock().clone();
    assert_eq!((notes[0].title.as_str(), notes[0].body.as_deref()), ("Note changed", Some("Claude")));
    let events = rig.shell.events.lock().unwrap().clone();
    assert_eq!(events, [("notes".to_owned(), "notes.changed".to_owned(), json!({"changes": 1}))]);
}

#[tokio::test]
async fn a_transaction_in_a_tool_commits_its_statements_together() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    rig.runtime.call_tool(&id("notes"), "add_two", json!({}), agent_ctx(0), false).await.unwrap();
    let rows = rig.runtime.call_handler(&id("notes"), "list", json!(null), user_ctx(0)).await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 4, "both inserts, seen together");
}

#[tokio::test]
async fn the_person_edits_a_note_through_a_handler() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    let runtime = &rig.runtime;
    let edit = |field: &'static str| async move {
        let args = json!({"id": 2, "field": field, "value": "edited"});
        runtime.call_handler(&id("notes"), "edit", args, user_ctx(5)).await
    };
    assert_eq!(edit("body").await.unwrap(), json!({"body": "edited", "rev": 1}));
    let rows = rig.db.query_rows("select body, rev, author from notes where id = 2").await;
    assert_eq!(rows[0], json!({"body": "edited", "rev": 1, "author": "You"}));
    let e = edit("title").await.unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("notes/app.lua:")), "{e:?}");
}

#[tokio::test]
async fn tool_failures_and_bugs_are_errors_for_the_caller_not_for_the_host() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    let e = rig
        .runtime
        .call_tool(&id("notes"), "append", json!({"id": 99, "text": "x"}), agent_ctx(0), false)
        .await
        .unwrap_err();
    assert_eq!(e, PortError::Rejected("No note with id 99".into()));
    let e = rig
        .runtime
        .call_tool(&id("notes"), "append", json!({"id": "x"}), agent_ctx(0), false)
        .await
        .unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("invalid input")), "{e:?}");
    assert!(matches!(
        rig.runtime.call_tool(&id("notes"), "nope", json!({}), agent_ctx(0), false).await,
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        rig.runtime.call_tool(&id("ghost"), "x", json!({}), agent_ctx(0), false).await,
        Err(PortError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_failing_app_is_isolated_and_a_failed_reload_keeps_the_old_version_running() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    // a second app that dies while loading does not disturb the first
    let other = LuaRuntime::new(rig.notifier.clone(), rig.shell.clone());
    let data: Arc<dyn AppData> = db();
    let e = other.load(&id("crashy"), package("local t = nil\nreturn t.x", &[]), data).await.unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("crashy/app.lua:2")), "{e:?}");

    // reload: new code that does not load leaves the running VM in place
    let e = load(&rig, "-- the next version\nerror('typo in the new version')").await;
    assert!(matches!(&e, Err(PortError::Rejected(m)) if m.contains("typo in the new version")));
    assert!(rig.runtime.call_tool(&id("notes"), "add_two", json!({}), agent_ctx(0), false).await.is_ok());

    // and a good reload swaps the code (the database is the caller's, so the data stays)
    let v2 = NOTES.replace("return \"ok\"", "return \"v2\"");
    load(&rig, &v2).await.unwrap();
    assert_eq!(
        rig.runtime.call_tool(&id("notes"), "add_two", json!({}), agent_ctx(0), false).await.unwrap(),
        json!("v2")
    );
    let count = rig.db.query_rows("select count(*) as n from notes").await;
    assert_eq!(count[0]["n"], 6, "2 seeded + 2 + 2: no re-seeding on reload");
}

#[tokio::test]
async fn unload_removes_the_vm() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    rig.runtime.unload(&id("notes")).await.unwrap();
    assert!(matches!(
        rig.runtime.call_tool(&id("notes"), "add_two", json!({}), agent_ctx(0), false).await,
        Err(PortError::NotFound(_))
    ));
    rig.runtime.unload(&id("notes")).await.unwrap(); // idempotent
}

/// Read rows straight from the database (the tests' own back door).
trait QueryRows {
    async fn query_rows(&self, sql: &str) -> Vec<Value>;
}
impl QueryRows for AppDb {
    async fn query_rows(&self, sql: &str) -> Vec<Value> {
        AppData::query(self, sql.to_owned(), Params::None).await.unwrap()
    }
}

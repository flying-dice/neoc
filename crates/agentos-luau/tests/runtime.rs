#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The `AppRuntime` port over the real Luau adapter and the real SQLite adapter: live sources fed by a tool,
//! effects forwarded to ports, a failing app isolated.

mod common;

use agentos_application::{
    data::{ChangeFeed, Params},
    live::{pump_changes, LiveSources, SourceId, SourceUpdate, Window},
    ports::{AppData, AppRuntime, Notifier, PortResult, Shell},
    testing::{CollectingSink, RecordingShell},
    PortError,
};
use agentos_domain::{Capabilities, Notification};
use agentos_luau::{Limits, LuauRuntime};
use agentos_sqlite::{AppDb, SqliteFeed};
use common::{agent_ctx, db, id, package, user_ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const NOTES: &str = r#"
app.manifest { id = "notes", name = "Notes", icon = "note.text", permissions = { "data", "notify", "shell" } }
local t = app.t
local db = app.db

db:migrate { [1] = [[
  create table folders(id integer primary key, name text not null);
  create table notes(id integer primary key, folder_id integer not null, body text not null default '',
                     rev integer not null default 0, author text);
  insert into folders(name) values ('Notes');
  insert into notes(folder_id, body) values (1, 'first'), (1, 'second');
]] }

local folders = db:query("select f.id, f.name, (select count(*) from notes n where n.folder_id = f.id) as count from folders f order by f.id", { key = "id" })
local list = db:query("select id, body from notes order by id", { key = "id", page = 2 })
local note = db:row("select id, body, rev from notes where id = :id", { key = "id", rev = "rev",
  write = function(id, field, value, ctx)
    assert(field == "body")
    db:exec("update notes set body = :v, rev = rev + 1, author = :who where id = :id", { v = value, id = id, who = ctx.actor.name })
  end })

app.tool { name = "notes.append", input = { id = t.integer {}, text = t.string {} }, run = function(input, ctx)
  local r = db:exec("update notes set body = body || char(10) || :text, rev = rev + 1, author = :who where id = :id",
    { text = input.text, id = input.id, who = ctx.actor.name })
  if r.changes == 0 then return ctx:fail("No note with id " .. input.id) end
  app.notify { title = "Note changed", body = ctx.actor.name }
  app.shell.badge(r.changes)
  return { id = input.id }
end }
app.tool { name = "notes.add_two", run = function(_, ctx)
  db:tx(function()
    db:exec("insert into notes(folder_id, body) values (1, 'a')")
    db:exec("insert into notes(folder_id, body) values (1, 'b')")
  end)
  return "ok"
end }
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
    runtime: LuauRuntime,
    notifier: Arc<RecordingNotifier>,
    shell: Arc<RecordingShell>,
    db: Arc<AppDb>,
}

fn rig() -> Rig {
    let notifier = Arc::new(RecordingNotifier::default());
    let shell = Arc::new(RecordingShell::default());
    let runtime = LuauRuntime::new(notifier.clone(), shell.clone(), Limits::default());
    Rig { runtime, notifier, shell, db: db() }
}

async fn load(rig: &Rig, src: &str) -> PortResult<agentos_application::ports::LoadedApp> {
    let data: Arc<dyn AppData> = rig.db.clone();
    rig.runtime.load(&id("notes"), package(src, &[]), data, Capabilities::all()).await
}

async fn wait_for(
    sink: &CollectingSink,
    n: usize,
) -> Vec<(agentos_application::SubscriptionId, SourceUpdate)> {
    for _ in 0..200 {
        if sink.peek().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sink.take()
}

#[tokio::test]
async fn loading_reports_what_the_app_declared() {
    let rig = rig();
    let loaded = load(&rig, NOTES).await.unwrap();
    assert_eq!(loaded.decl.unwrap().name, "Notes");
    assert_eq!(
        loaded.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["notes.append", "notes.add_two"]
    );
    assert_eq!(loaded.sources.iter().map(|s| s.id.0.as_str()).collect::<Vec<_>>(), ["s1", "s2", "s3"]);
}

#[tokio::test]
async fn an_app_without_a_manifest_does_not_load() {
    let rig = rig();
    let e = load(&rig, "local x = 1").await.unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("app.manifest")), "{e:?}");
}

#[tokio::test]
async fn an_agent_tool_writes_data_and_every_live_source_gets_a_delta() {
    let rig = rig();
    let loaded = load(&rig, NOTES).await.unwrap();
    let live = Arc::new(LiveSources::new(rig.db.clone(), loaded.sources));
    let sink = Arc::new(CollectingSink::default());

    // an open view: the folder list, a window of the note list, and note 1 (the open note)
    let (_, folders) =
        live.subscribe(&SourceId("s1".into()), Params::None, None, sink.clone()).await.unwrap();
    let (_, list) = live.subscribe(&SourceId("s2".into()), Params::None, None, sink.clone()).await.unwrap();
    let (_, open) = live
        .subscribe(&SourceId("s3".into()), Params::named([("id", json!(1))]), None, sink.clone())
        .await
        .unwrap();
    let SourceUpdate::Rows(f) = folders else { panic!("rows") };
    assert_eq!(f.changed[0].1["count"], 2);
    let SourceUpdate::Rows(l) = list else { panic!("rows") };
    assert_eq!((l.keys.len(), l.total), (2, Some(2)));
    let SourceUpdate::Row(o) = open else { panic!("row") };
    assert_eq!(o.rev, Some(json!(0)));

    // the host pumps commits into refreshes
    let pump =
        tokio::spawn(pump_changes(Box::new(SqliteFeed::new(&rig.db)) as Box<dyn ChangeFeed>, live.clone()));

    // the agent appends to note 1 over the tool: no UI code anywhere
    let out = rig
        .runtime
        .call_tool(&id("notes"), "notes.append", json!({"id": 1, "text": "- buy oat milk"}), agent_ctx(1_000))
        .await
        .unwrap();
    assert_eq!(out, json!({"id": 1}));

    let updates = wait_for(&sink, 2).await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    let updates = [updates, sink.take()].concat();
    assert_eq!(
        updates.len(),
        2,
        "the note list row and the open note; the folder count did not change: {updates:?}"
    );
    let by_kind = |f: fn(&SourceUpdate) -> bool| updates.iter().filter(|(_, u)| f(u)).count();
    assert_eq!(by_kind(|u| matches!(u, SourceUpdate::Rows(_))), 1);
    let row =
        updates.iter().find_map(|(_, u)| if let SourceUpdate::Row(r) = u { Some(r) } else { None }).unwrap();
    assert_eq!(row.rev, Some(json!(1)));
    assert_eq!(row.row.as_ref().unwrap()["body"], "first\n- buy oat milk");
    let list_delta = updates
        .iter()
        .find_map(|(_, u)| match u {
            SourceUpdate::Rows(d) if d.keys == [json!(1), json!(2)] && d.changed.len() == 1 => Some(d),
            _ => None,
        })
        .expect("the note list sends only the changed row");
    assert_eq!(list_delta.changed[0].0, json!(1));

    // the tool's app.notify and app.shell.badge reached their ports
    let notes = rig.notifier.0.lock().clone();
    assert_eq!((notes[0].title.as_str(), notes[0].body.as_deref()), ("Note changed", Some("Claude")));
    assert_eq!(rig.shell.badges.lock().unwrap().last().unwrap().1.as_deref(), Some("1"));
    pump.abort();
}

#[tokio::test]
async fn a_transaction_in_a_tool_is_one_commit_and_one_delta() {
    let rig = rig();
    let loaded = load(&rig, NOTES).await.unwrap();
    let live = Arc::new(LiveSources::new(rig.db.clone(), loaded.sources));
    let sink = Arc::new(CollectingSink::default());
    live.subscribe(&SourceId("s2".into()), Params::None, Some(Window { from: 0, to: 10 }), sink.clone())
        .await
        .unwrap();
    let pump = tokio::spawn(pump_changes(Box::new(SqliteFeed::new(&rig.db)), live));
    rig.runtime.call_tool(&id("notes"), "notes.add_two", json!({}), agent_ctx(0)).await.unwrap();
    let updates = wait_for(&sink, 1).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(updates.len(), 1);
    let SourceUpdate::Rows(d) = &updates[0].1 else { panic!("rows") };
    assert_eq!((d.keys.len(), d.total), (4, Some(4)), "both inserts, seen together");
    assert!(sink.take().is_empty(), "and no second delta");
    pump.abort();
}

#[tokio::test]
async fn the_human_edits_a_bound_field_through_the_write_callback() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    rig.runtime
        .write_row(&id("notes"), &SourceId("s3".into()), json!(2), "body", json!("edited"), user_ctx(5))
        .await
        .unwrap();
    let rows = rig.db.query_rows("select body, rev, author from notes where id = 2").await;
    assert_eq!(rows[0], json!({"body": "edited", "rev": 1, "author": "You"}));
    let e = rig
        .runtime
        .write_row(&id("notes"), &SourceId("s3".into()), json!(2), "title", json!("x"), user_ctx(5))
        .await
        .unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("notes/app.luau:")), "{e:?}");
}

#[tokio::test]
async fn tool_failures_and_bugs_are_errors_for_the_caller_not_for_the_host() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    let e = rig
        .runtime
        .call_tool(&id("notes"), "notes.append", json!({"id": 99, "text": "x"}), agent_ctx(0))
        .await
        .unwrap_err();
    assert_eq!(e, PortError::Rejected("No note with id 99".into()));
    let e = rig
        .runtime
        .call_tool(&id("notes"), "notes.append", json!({"id": "x"}), agent_ctx(0))
        .await
        .unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("invalid input")), "{e:?}");
    assert!(matches!(
        rig.runtime.call_tool(&id("notes"), "nope", json!({}), agent_ctx(0)).await,
        Err(PortError::NotFound(_))
    ));
    assert!(matches!(
        rig.runtime.call_tool(&id("ghost"), "x", json!({}), agent_ctx(0)).await,
        Err(PortError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_failing_app_is_isolated_and_a_failed_reload_keeps_the_old_version_running() {
    let rig = rig();
    load(&rig, NOTES).await.unwrap();
    // a second app that dies while loading does not disturb the first
    let other = LuauRuntime::new(rig.notifier.clone(), rig.shell.clone(), Limits::default());
    let data: Arc<dyn AppData> = db();
    let e = other
        .load(&id("crashy"), package("local t = nil\nreturn t.x", &[]), data, Capabilities::all())
        .await
        .unwrap_err();
    assert!(matches!(&e, PortError::Rejected(m) if m.contains("crashy/app.luau:2")), "{e:?}");

    // reload: new code that does not load leaves the running VM in place
    let e =
        load(&rig, "app.manifest { id = 'notes', name = 'N', icon = 'x' }\nerror('typo in the new version')")
            .await;
    assert!(matches!(&e, Err(PortError::Rejected(m)) if m.contains("typo in the new version")));
    assert!(rig.runtime.call_tool(&id("notes"), "notes.add_two", json!({}), agent_ctx(0)).await.is_ok());

    // and a good reload swaps the code (the database is the caller's, so the data stays)
    let v2 = NOTES.replace("return \"ok\"", "return \"v2\"");
    load(&rig, &v2).await.unwrap();
    assert_eq!(
        rig.runtime.call_tool(&id("notes"), "notes.add_two", json!({}), agent_ctx(0)).await.unwrap(),
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
        rig.runtime.call_tool(&id("notes"), "notes.add_two", json!({}), agent_ctx(0)).await,
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

#[allow(dead_code)]
fn _shell_is_a_port(_: &dyn Shell) {}

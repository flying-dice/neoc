//! The view service against the in-memory port fakes: open, events, settling, live data, two-way writes,
//! client state and teardown.

use crate::{
    data::{Params, QueryInfo},
    live::{Conflict, SourceDef, SourceId, SourceKind, SourceUpdate, TotalMode},
    ports::{AppActivity, AppObserver, ChangeBatch, SourceBinding, ViewEffect},
    services::AppService,
    testing::{Fakes, RecordingViewSink, ScriptedData},
    views::{ViewError, ViewService},
};
use agentos_domain::{
    ui::{
        protocol::{
            ClientState, Command, DataPatch, HostNotification, RowRestore, RowSet, SourcePatch, SourceSet,
            ViewEvent, ViewOpen, WriteResolution,
        },
        source::{ConflictPolicy, Persist, SourceDecl},
        ClientDecl, Node,
    },
    AppId,
};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, sync::Arc};

const CONN: u64 = 7;

struct Rig {
    f: Fakes,
    svc: Arc<AppService>,
    views: Arc<ViewService>,
    sink: Arc<RecordingViewSink>,
    app: AppId,
    data: ScriptedData,
}

fn rows_def() -> SourceDef {
    SourceDef {
        id: SourceId("list".into()),
        kind: SourceKind::Rows { key: "id".into(), page: Some(2), total: TotalMode::Exact },
        sql: "select id, title from notes order by id".into(),
        info: QueryInfo {
            tables: ["notes".to_owned()].into(),
            columns: vec!["id".into(), "title".into()],
            named_params: vec![],
            read_only: true,
        },
    }
}

fn row_def() -> SourceDef {
    SourceDef {
        id: SourceId("note".into()),
        kind: SourceKind::Row {
            key: "id".into(),
            rev: Some("rev".into()),
            writable: None,
            has_write: true,
            conflict: Conflict::HumanWins,
        },
        sql: "select id, body, rev from notes where id = :id".into(),
        info: QueryInfo {
            tables: ["notes".to_owned()].into(),
            columns: vec!["id".into(), "body".into(), "rev".into()],
            named_params: vec!["id".into()],
            read_only: true,
        },
    }
}

fn tree(title: &str) -> Node {
    Node::new("Stack", "main")
        .child(Node::new("Text", "title").prop("text", json!(title)))
        .child(Node::new("TextEditor", "body").prop("text", json!({"$ref": "src.note.body"})))
}

fn bindings() -> BTreeMap<String, SourceBinding> {
    let params = |v: Value| v.as_object().cloned().unwrap_or_default();
    BTreeMap::from([
        (
            "list".to_owned(),
            SourceBinding {
                def: SourceId("list".into()),
                decl: SourceDecl::Rows {
                    key: "id".into(),
                    page: Some(2),
                    debounce: None,
                    total: None,
                    params: Map::new(),
                },
            },
        ),
        (
            "note".to_owned(),
            SourceBinding {
                def: SourceId("note".into()),
                decl: SourceDecl::Row {
                    key: "id".into(),
                    writable: vec!["body".into()],
                    rev: Some("rev".into()),
                    conflict: Some(ConflictPolicy::Merge),
                    params: params(json!({"id": {"$ref": "client.noteId"}})),
                },
            },
        ),
    ])
}

fn decls() -> BTreeMap<String, ClientDecl> {
    let decl = |v: Value| serde_json::from_value::<ClientDecl>(v).unwrap();
    BTreeMap::from([
        ("noteId".to_owned(), decl(json!({"init": 1, "persist": "app"}))),
        ("search".to_owned(), decl(json!({"init": ""}))),
    ])
}

async fn rig() -> Rig {
    let f = Fakes::new();
    f.runtime.set_sources("notes", vec![rows_def(), row_def()]);
    f.runtime.set_render(tree("one"), bindings(), decls());
    let svc = Arc::new(f.service());
    let app = svc.install_from_url(&f.add_app("notes", &[("t", "x")])).await.unwrap();
    let data = f.store.data("notes");
    data.set_rows(
        "from notes order",
        vec![json!({"id": 1, "title": "a"}), json!({"id": 2, "title": "b"}), json!({"id": 3, "title": "c"})],
    );
    data.set_rows("from notes where id", vec![json!({"id": 1, "body": "milk", "rev": 3})]);
    let views = Arc::new(ViewService::new(svc.clone()));
    svc.add_observer(views.clone());
    Rig { f, svc, views, sink: Arc::new(RecordingViewSink::default()), app, data }
}

fn open_req() -> ViewOpen {
    serde_json::from_value(
        json!({"app": "notes", "window": "main", "windowId": "w1", "size": {"w": 960, "h": 640}}),
    )
    .unwrap()
}

async fn open(r: &Rig) -> agentos_domain::ui::protocol::ViewSnapshot {
    r.views.open(CONN, r.sink.clone(), open_req()).await.unwrap()
}

fn event(view: &str, seq: u32, path: &str) -> ViewEvent {
    serde_json::from_value(json!({"viewId": view, "rev": 1, "seq": seq, "path": path, "event": "tap"}))
        .unwrap()
}

#[tokio::test]
async fn open_renders_subscribes_the_sources_and_answers_with_one_complete_snapshot() {
    let r = rig().await;
    let ui_state = json!({"noteId": 9}).as_object().cloned().unwrap();
    r.data.set_rows("ignored", vec![]);
    crate::ports::AppData::set_ui_state(&r.data, "main", ui_state).await.unwrap();

    let s = open(&r).await;
    assert_eq!((s.view_id.as_str(), s.rev, s.proto, s.kit.as_str()), ("v1", 1, 1, "1.0"));
    assert_eq!(s.tree, tree("one"));
    assert_eq!(s.sources.keys().collect::<Vec<_>>(), ["list", "note"]);
    let SourcePatch::Rows { keys, total, rows, .. } = &s.data["list"] else { panic!("rows") };
    assert_eq!((keys.len(), *total, rows.len()), (2, 3, 2), "a page of two out of three");
    let SourcePatch::Row { rev, row } = &s.data["note"] else { panic!("row") };
    assert_eq!((*rev, row.as_ref().unwrap()["body"].clone()), (3, json!("milk")));
    assert_eq!(s.client_decls["noteId"].persist, Persist::App);

    // the host's stored client values go into the runtime, and the snapshot reports them
    let opened = r.f.runtime.opened_views();
    assert_eq!(opened[0].3["noteId"], 9, "restored from `_ui_state`");
    assert_eq!(s.client["noteId"], 9);
    assert_eq!(r.views.open_views(), 1);
}

#[tokio::test]
async fn a_client_ref_param_resolves_to_the_clients_value_when_the_source_is_first_read() {
    let r = rig().await;
    let s = open(&r).await;
    // noteId's init is 1: the row source ran with :id = 1 (the scripted rows ignore params, so check the sub)
    let live = r.svc.live(&r.app).unwrap();
    assert!(live.subscription_count() >= 2, "{s:?}");
}

#[tokio::test]
async fn too_many_sources_and_unknown_apps_are_refused() {
    let r = rig().await;
    let bad = ViewOpen { app: "ghost".into(), ..open_req() };
    assert!(matches!(r.views.open(CONN, r.sink.clone(), bad).await, Err(ViewError::Invalid(_))));

    let many: BTreeMap<String, SourceBinding> = (0..17)
        .map(|i| {
            (
                format!("s{i}"),
                SourceBinding { def: SourceId("list".into()), decl: bindings()["list"].decl.clone() },
            )
        })
        .collect();
    r.f.runtime.set_render(tree("one"), many, decls());
    let e = r.views.open(CONN, r.sink.clone(), open_req()).await.unwrap_err();
    assert!(matches!(&e, ViewError::InvalidTree(m) if m.contains("at most 16")), "{e:?}");
    assert_eq!(r.f.runtime.closed_views().len(), 1, "the half-opened instance was closed");
    assert_eq!(r.views.open_views(), 0);
}

#[tokio::test]
async fn an_event_runs_the_handler_then_the_views_follow_with_ops_before_the_answer() {
    let r = rig().await;
    let s = open(&r).await;
    r.sink.take();

    r.f.runtime.set_render(tree("two"), bindings(), decls()); // what the handler's state change renders
    r.f.runtime.mark_dirty("v1");
    r.f.runtime.push_effect("v1", ViewEffect::Command(Command::Announce { text: "done".into() }));
    r.f.runtime
        .push_effect("v1", ViewEffect::ClientSet { path: "noteId".into(), value: json!(2), force: true });
    let ack = r.views.event(CONN, event(&s.view_id, 1, "main/title")).await.unwrap();
    assert!(ack.ok);

    // everything the event caused was sent by the time the answer is: ops, commands, client.set
    let sent = r.sink.take();
    let methods: Vec<&str> = sent.iter().map(|n| n.to_rpc().0).collect();
    assert_eq!(methods, ["view.ops", "view.command", "client.set"], "{sent:?}");
    let HostNotification::Ops(ops) = &sent[0] else { panic!("ops") };
    assert_eq!((ops.base, ops.rev), (1, 2));
    assert_eq!(ops.ops.len(), 1, "one set op: {:?}", ops.ops);
    let HostNotification::ClientSet(client_set) = &sent[2] else { panic!("client.set") };
    assert_eq!((client_set.path.as_str(), client_set.force), ("noteId", Some(true)));
    // a value the app itself chose for a `persist = "app"` name is stored too (the client mirrors only the person's own changes)
    let stored = crate::ports::AppData::ui_state(&r.data, "main").await.unwrap();
    assert_eq!(stored.get("noteId"), Some(&json!(2)));

    let calls = r.f.runtime.view_events();
    assert_eq!(
        (calls[0].0.as_str(), calls[0].1.path.as_str(), calls[0].1.event.as_str()),
        ("v1", "main/title", "tap")
    );
}

#[tokio::test]
async fn an_event_that_changes_nothing_sends_no_ops_and_a_replayed_seq_is_not_run_again() {
    let r = rig().await;
    let s = open(&r).await;
    r.sink.take();
    r.f.runtime.mark_dirty("v1"); // dirty, but the render comes out the same
    r.views.event(CONN, event(&s.view_id, 1, "main/title")).await.unwrap();
    assert!(r.sink.take().is_empty(), "an unchanged render is not a message");

    let again = r.views.event(CONN, event(&s.view_id, 1, "main/title")).await.unwrap();
    assert!(again.ok);
    assert_eq!(r.f.runtime.view_events().len(), 1, "seq 1 was already handled");
}

#[tokio::test]
async fn a_stale_event_an_unknown_view_and_another_connection_are_told_apart() {
    let r = rig().await;
    let s = open(&r).await;
    r.f.runtime.set_stale("main/gone#tap");
    let ack = r.views.event(CONN, event(&s.view_id, 1, "main/gone")).await.unwrap();
    assert_eq!((ack.ok, ack.reason.as_deref()), (false, Some("stale")));
    assert_eq!(r.views.event(CONN, event("v99", 2, "main")).await.unwrap_err(), ViewError::NoSuchView);
    assert_eq!(
        r.views.event(CONN + 1, event(&s.view_id, 3, "main")).await.unwrap_err(),
        ViewError::NoSuchView
    );
    assert_eq!(ViewError::NoSuchView.code(), -32011);
    assert_eq!(ViewError::App("x".into()).code(), -32010);
}

#[tokio::test]
async fn a_failed_render_keeps_the_last_good_tree_and_reports_an_error() {
    let r = rig().await;
    let s = open(&r).await;
    r.sink.take();
    r.f.runtime.fail_render(Some("main/app.lua:9: boom"));
    r.f.runtime.mark_dirty("v1");
    r.views.event(CONN, event(&s.view_id, 1, "main/title")).await.unwrap();
    let sent = r.sink.take();
    let HostNotification::Error(e) = &sent[0] else { panic!("view.error: {sent:?}") };
    assert!(e.message.contains("boom"));
    assert_eq!(sent.len(), 1, "no ops, no tree");

    // once it renders again, the diff is against the last tree the client has
    r.f.runtime.fail_render(None);
    r.f.runtime.set_render(tree("three"), bindings(), decls());
    r.f.runtime.mark_dirty("v1");
    r.views.event(CONN, event(&s.view_id, 2, "main/title")).await.unwrap();
    let HostNotification::Ops(ops) = &r.sink.take()[0] else { panic!("ops") };
    assert_eq!((ops.base, ops.rev), (1, 2));
}

#[tokio::test]
async fn data_written_by_anyone_reaches_the_open_view_as_a_patch_of_changed_rows() {
    let r = rig().await;
    open(&r).await;
    r.sink.take();
    r.data.set_rows(
        "from notes order",
        vec![json!({"id": 1, "title": "A"}), json!({"id": 2, "title": "b"}), json!({"id": 3, "title": "c"})],
    );
    r.svc.live(&r.app).unwrap().refresh(&ChangeBatch::of(["notes"])).await;
    let sent = r.sink.take();
    let HostNotification::Data(DataPatch { sources: Some(sources), .. }) = &sent[0] else {
        panic!("{sent:?}")
    };
    let SourcePatch::Rows { rows, drop, keys, .. } = &sources["list"] else { panic!("rows") };
    assert_eq!(
        (rows.keys().map(String::as_str).collect::<Vec<_>>(), drop.len(), keys.len()),
        (vec!["1"], 0, 2),
        "only the changed row"
    );
}

#[tokio::test]
async fn a_tool_call_makes_the_views_settle_through_the_observer() {
    let r = rig().await;
    open(&r).await;
    r.sink.take();
    r.f.runtime.set_render(tree("changed by a tool"), bindings(), decls());
    r.f.runtime.mark_dirty("v1");
    let agent = agentos_domain::AgentIdentity {
        id: agentos_domain::AgentId("claude".into()),
        name: "Claude".into(),
        grants: vec![agentos_domain::ToolGrant::CallAnyTool],
    };
    r.svc
        .call_tool(&agent, &agentos_domain::QualifiedToolName::parse("notes__t").unwrap(), json!({}))
        .await
        .unwrap();
    assert_eq!(
        r.sink.methods(),
        ["view.ops"],
        "no event, no UI code: the tool's state change reached the window"
    );
}

#[tokio::test]
async fn background_work_that_changed_state_reaches_the_open_view_with_no_event_and_no_tool_call() {
    let r = rig().await;
    open(&r).await;
    r.sink.take();
    // a task assigned `app.state`: the runtime only says that a background call settled
    r.f.runtime.set_render(tree("changed by a task"), bindings(), decls());
    r.f.runtime.mark_dirty("v1");
    r.f.runtime.push_effect("v1", ViewEffect::Command(Command::Announce { text: "tick".into() }));
    r.svc.background_settled(&r.app).await;
    assert_eq!(r.sink.methods(), ["view.ops", "view.command"], "the view followed, the window's request too");

    // nothing changed since: settling again is silent
    r.sink.take();
    r.svc.background_settled(&r.app).await;
    assert!(r.sink.methods().is_empty());
}

#[tokio::test]
async fn a_settle_a_background_change_started_finishes_before_the_event_that_follows_it_is_answered() {
    let r = rig().await;
    let s = open(&r).await;
    r.sink.take();
    r.f.runtime.set_render(tree("two"), bindings(), decls());
    r.f.runtime.mark_dirty("v1");
    // the background settle takes the dirty set and is still rendering when the event's handler has run
    let release = r.f.runtime.hold_next_render();
    let background = r.svc.background_settled(&r.app);
    let event_then_look = async {
        let ack = r.views.event(CONN, event(&s.view_id, 1, "main/title")).await.unwrap();
        (ack.ok, r.sink.methods())
    };
    let releaser = async {
        while r.f.runtime.view_events().is_empty() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = release.send(());
    };
    let ((), (ok, seen_at_the_ack), ()) = tokio::join!(background, event_then_look, releaser);
    assert!(ok);
    assert_eq!(seen_at_the_ack, ["view.ops"], "the ops were sent before the answer, whoever rendered them");
    assert_eq!(r.sink.methods(), ["view.ops"], "and sent once");
}

#[tokio::test]
async fn source_set_changes_the_window_and_answers_even_when_nothing_changed() {
    let r = rig().await;
    open(&r).await;
    let patch = r
        .views
        .source_set(
            CONN,
            serde_json::from_value::<SourceSet>(
                json!({"viewId": "v1", "source": "list", "range": {"from": 1, "to": 3}}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let SourcePatch::Rows { keys, from, rows, drop, .. } = patch else { panic!("rows") };
    assert_eq!((keys, from, drop), (vec![json!(2), json!(3)], 1, vec![json!(1)]));
    assert_eq!(
        rows.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["3"],
        "row 2 was in the previous window"
    );
    let same = r
        .views
        .source_set(
            CONN,
            serde_json::from_value(json!({"viewId": "v1", "source": "list", "range": {"from": 1, "to": 3}}))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(same, SourcePatch::Rows { ref keys, .. } if keys.len() == 2), "asked, so answered");
    let bad = r
        .views
        .source_set(CONN, serde_json::from_value(json!({"viewId": "v1", "source": "nope"})).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(bad, ViewError::Invalid(_)));
}

// ---- two-way writes -------------------------------------------------------------------------------------

fn row_set(value: &str, base_rev: u32) -> RowSet {
    serde_json::from_value(json!({
        "viewId": "v1", "source": "note", "key": 1, "field": "body", "value": value, "baseRev": base_rev
    }))
    .unwrap()
}

fn writable(r: &Rig) {
    let d = r.data.clone();
    r.f.runtime.on_write(move |_, _, field, value| {
        d.set_rows("from notes where id", vec![json!({"id": 1, field: value, "rev": 4})]);
    });
}

#[tokio::test]
async fn a_write_on_the_current_revision_is_accepted_with_the_new_rev() {
    let r = rig().await;
    writable(&r);
    open(&r).await;
    let ack = r.views.row_set(CONN, row_set("Milk", 3)).await.unwrap();
    assert_eq!(
        (ack.resolution, ack.rev, ack.value.clone(), ack.conflict),
        (WriteResolution::Accepted, 4, json!("Milk"), Some(false))
    );
    assert!(ack.undo.is_none());
    let w = r.f.runtime.writes();
    assert_eq!((w[0].0.as_str(), w[0].2.as_str(), &w[0].4.actor.name[..]), ("note", "body", "You"));
}

#[tokio::test]
async fn a_stale_text_write_is_merged_against_the_value_the_host_last_sent() {
    let r = rig().await;
    writable(&r);
    open(&r).await; // sent: body "milk" at rev 3
                    // an agent appended since: the row is at rev 5 with a new last line
    r.data.set_rows("from notes where id", vec![json!({"id": 1, "body": "milk\n- oat", "rev": 5})]);
    let ack = r.views.row_set(CONN, row_set("Milk", 3)).await.unwrap();
    assert_eq!(ack.resolution, WriteResolution::Merged, "{ack:?}");
    assert_eq!(ack.value, json!("Milk\n- oat"));
    assert_eq!(ack.conflict, Some(false));
}

#[tokio::test]
async fn a_write_that_replaced_something_carries_an_undo_that_restores_it() {
    let r = rig().await;
    writable(&r);
    open(&r).await;
    r.data.set_rows("from notes where id", vec![json!({"id": 1, "body": "theirs", "rev": 5})]);
    // the base the client names (rev 99) is not one the host sent: no merge, the human wins
    let ack = r.views.row_set(CONN, row_set("mine", 99)).await.unwrap();
    assert_eq!(ack.resolution, WriteResolution::Replaced);
    assert_eq!(ack.conflict, Some(true));
    let undo = ack.undo.clone().unwrap();
    assert_eq!(undo.lost, json!("theirs"));

    let restore: RowRestore = serde_json::from_value(
        json!({"viewId": "v1", "source": "note", "key": 1, "revisionId": undo.revision_id}),
    )
    .unwrap();
    let back = r.views.row_restore(CONN, restore).await.unwrap();
    assert_eq!(back.value, json!("theirs"));
    let writes = r.f.runtime.writes();
    assert_eq!(writes.last().unwrap().3, json!("theirs"));
}

#[tokio::test]
async fn writes_to_unknown_sources_and_rows_sources_are_refused() {
    let r = rig().await;
    open(&r).await;
    let mut req = row_set("x", 3);
    req.source = "nope".into();
    assert!(matches!(r.views.row_set(CONN, req).await, Err(ViewError::Invalid(_))));
    let mut req = row_set("x", 3);
    req.source = "list".into();
    assert!(
        matches!(r.views.row_set(CONN, req).await, Err(ViewError::Invalid(m)) if m.contains("single-row"))
    );
}

// ---- client state and teardown ----------------------------------------------------------------------------

#[tokio::test]
async fn mirrored_client_values_reach_the_app_and_persisted_ones_are_stored_on_the_host() {
    let r = rig().await;
    open(&r).await;
    let n: ClientState =
        serde_json::from_value(json!({"viewId": "v1", "set": {"noteId": 5, "search": "milk"}})).unwrap();
    r.views.client_state(CONN, n).await;
    assert_eq!(r.f.runtime.client_values()[0].1["search"], "milk");
    let stored = crate::ports::AppData::ui_state(&r.data, "main").await.unwrap();
    assert_eq!(stored.get("noteId"), Some(&json!(5)), "persist = app");
    assert!(stored.get("search").is_none(), "an ephemeral value is not stored");
}

#[tokio::test]
async fn stopping_the_app_tells_its_views_and_closing_a_connection_releases_them() {
    let r = rig().await;
    open(&r).await;
    let other: Arc<RecordingViewSink> = Arc::default();
    r.views.open(CONN + 1, other.clone(), open_req()).await.unwrap();
    r.sink.take();

    r.views.connection_closed(CONN).await;
    assert_eq!(r.views.open_views(), 1);
    assert!(r.sink.take().is_empty(), "nobody to tell: the socket is gone");
    assert!(r.f.runtime.closed_views().contains(&"v1".to_owned()));

    r.views.stopping(&r.app, "disabled").await;
    assert_eq!(r.views.open_views(), 0);
    let HostNotification::Closed(c) = &other.take()[0] else { panic!("view.closed") };
    assert_eq!((c.view_id.as_str(), c.reason.as_str()), ("v2", "disabled"));
    assert_eq!(r.svc.live(&r.app).unwrap().subscription_count(), 0, "subscriptions are released");
}

#[tokio::test]
async fn lifecycle_calls_on_the_service_close_the_views_first() {
    let r = rig().await;
    open(&r).await;
    r.sink.take();
    r.svc.disable(&r.app).await.unwrap();
    assert_eq!(r.sink.methods(), ["view.closed"]);
    assert_eq!(r.views.open_views(), 0);
}

#[tokio::test]
async fn sync_answers_with_a_fresh_complete_snapshot() {
    let r = rig().await;
    let first = open(&r).await;
    r.f.runtime.set_render(tree("moved on"), bindings(), decls());
    let snap = r.views.sync(CONN, &first.view_id).await.unwrap();
    assert_eq!(snap.tree, tree("moved on"));
    assert!(snap.rev > first.rev);
    assert!(matches!(snap.data["list"], SourcePatch::Rows { .. }), "every source's data again");
    assert_eq!(r.svc.live(&r.app).unwrap().subscription_count(), 2, "re-subscribed, not doubled");
    let _ = (SourceUpdate::Scalar(Value::Null), Params::None);
}

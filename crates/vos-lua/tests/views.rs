#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Windows over the real runtime: the Calculator and Notes (the examples the
//! codegen contract test checks) run as real apps, render through the generated `ui` module, and every
//! tree passes the schema validator.

mod common;

use common::{agent_ctx, built_app, db, id};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use vos_application::{
    ports::{AppData, AppPackage, AppRuntime, LoadedApp, RenderOutput, ViewEffect, ViewEventCall},
    PortError,
};
use vos_domain::ui::{diff, protocol::Command, Node, Op};
use vos_lua::LuaRuntime;
use vos_sqlite::AppDb;

fn obj(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

struct Rig {
    runtime: LuaRuntime,
    db: Arc<AppDb>,
    shell: Arc<vos_application::testing::RecordingShell>,
}

fn runtime(shell: Arc<vos_application::testing::RecordingShell>) -> LuaRuntime {
    struct Quiet;
    #[async_trait::async_trait]
    impl vos_application::ports::Notifier for Quiet {
        async fn notify(&self, _: vos_domain::Notification) -> vos_application::ports::PortResult<()> {
            Ok(())
        }
    }
    LuaRuntime::new(Arc::new(Quiet), shell)
}

impl Rig {
    fn new() -> Self {
        let shell = Arc::new(vos_application::testing::RecordingShell::default());
        Self { runtime: runtime(shell.clone()), db: db(), shell }
    }
}

async fn load(app: &str, package: AppPackage) -> (Rig, LoadedApp) {
    let rig = Rig::new();
    let data: Arc<dyn AppData> = rig.db.clone();
    let loaded = rig.runtime.load(&id(app), package, data).await.unwrap();
    (rig, loaded)
}

async fn calculator() -> Rig {
    load("calculator", built_app("calculator")).await.0
}
async fn notes() -> (Rig, LoadedApp) {
    load("notes", built_app("notes")).await
}

async fn open(rig: &Rig, app: &str, view: &str) -> RenderOutput {
    rig.runtime.open_view(&id(app), view, "main", Map::new(), Map::new()).await.unwrap()
}

/// The path of the node with this key (the first match, depth first).
fn path_of(node: &Node, key: &str) -> Option<String> {
    fn walk(node: &Node, path: &str, key: &str) -> Option<String> {
        if node.key == key {
            return Some(path.to_owned());
        }
        for (slot, children) in node.containers() {
            for child in children {
                let p = vos_domain::ui::node::join_path(path, slot, &child.key);
                if let Some(found) = walk(child, &p, key) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(node, &node.key, key)
}

fn text_at(node: &Node, key: &str) -> Option<String> {
    fn find<'a>(node: &'a Node, key: &str) -> Option<&'a Node> {
        if node.key == key {
            return Some(node);
        }
        node.containers().flat_map(|(_, c)| c).find_map(|c| find(c, key))
    }
    find(node, key)?.props.get("text").and_then(Value::as_str).map(str::to_owned)
}

async fn tap(rig: &Rig, app: &str, view: &str, out: &RenderOutput, key: &str) -> bool {
    let path = path_of(&out.tree, key).unwrap_or_else(|| panic!("no node `{key}`"));
    let call = ViewEventCall { path, event: "tap".into(), row_key: None, row: None, payload: None };
    rig.runtime.dispatch_view_event(&id(app), view, call, common::user_ctx(0)).await.unwrap()
}

// ---- the Calculator ------------------------------------------------------------------------------------

#[tokio::test]
async fn the_calculator_renders_validates_and_reacts_to_a_key_press_with_one_set_op() {
    let rig = calculator().await;
    let first = open(&rig, "calculator", "v1").await;
    assert_eq!(first.tree.key, "main");
    assert_eq!(text_at(&first.tree, "display").as_deref(), Some("0"));
    assert!(first.sources.is_empty());

    assert!(tap(&rig, "calculator", "v1", &first, "key-7").await);
    // the handler wrote app.state: the view that read it is dirty
    assert_eq!(rig.runtime.dirty_views(&id("calculator")).await.unwrap(), ["v1"]);
    assert!(rig.runtime.dirty_views(&id("calculator")).await.unwrap().is_empty(), "taken once");

    let second = rig.runtime.render_view(&id("calculator"), "v1").await.unwrap();
    assert!(second.seq > first.seq);
    let ops = diff(&first.tree, &second.tree);
    assert!(
        ops.iter().any(|op| matches!(op, Op::Set { props, .. } if props.get("text") == Some(&json!("7")))),
        "{ops:?}"
    );
    assert!(ops.len() <= 3, "a key press is a handful of ops, not a new tree: {ops:?}");

    // 7 + 8 =
    for key in ["key-add", "key-8", "key-eq"] {
        tap(&rig, "calculator", "v1", &second, key).await;
    }
    let done = rig.runtime.render_view(&id("calculator"), "v1").await.unwrap();
    assert_eq!(text_at(&done.tree, "display").as_deref(), Some("15"));
    assert_eq!(text_at(&done.tree, "caption").as_deref(), Some("7 + 8"));
}

#[tokio::test]
async fn the_shared_state_reaches_every_open_window_and_the_agent_tool_drives_it() {
    let rig = calculator().await;
    let a = open(&rig, "calculator", "v1").await;
    let _b = open(&rig, "calculator", "v2").await;
    let out = rig
        .runtime
        .call_tool(
            &id("calculator"),
            "show_on_calculator",
            json!({"expression": "6 * 7"}),
            agent_ctx(0),
            false,
        )
        .await
        .unwrap();
    assert_eq!(out, json!({"shown": "42"}));
    let mut dirty = rig.runtime.dirty_views(&id("calculator")).await.unwrap();
    dirty.sort();
    assert_eq!(dirty, ["v1", "v2"], "one calculator however many windows are open");
    let after = rig.runtime.render_view(&id("calculator"), "v1").await.unwrap();
    assert_eq!(text_at(&after.tree, "display").as_deref(), Some("42"));
    assert!(!diff(&a.tree, &after.tree).is_empty());
    // app:openWindow("main") went to the shell
    assert_eq!(rig.shell.windows.lock().unwrap()[0].1, "main");

    // arithmetic (not readOnly: it records the sum in the history), then the history through app.kv (readOnly)
    let r = rig
        .runtime
        .call_tool(
            &id("calculator"),
            "calculate",
            json!({"expression": "(12.5 + 7) * 3"}),
            agent_ctx(0),
            false,
        )
        .await
        .unwrap();
    assert_eq!(r["result"], json!(58.5));
    let h = rig
        .runtime
        .call_tool(&id("calculator"), "history", json!({"limit": 5}), agent_ctx(0), false)
        .await
        .unwrap();
    assert_eq!(h["entries"].as_array().unwrap().len(), 2, "show_on_calculator and calculate both recorded");
    let bad = rig
        .runtime
        .call_tool(&id("calculator"), "calculate", json!({"expression": "1 / 0"}), agent_ctx(0), false)
        .await;
    assert!(matches!(bad, Err(PortError::Rejected(_))), "division by zero is a tool error: {bad:?}");
}

// ---- Notes ----------------------------------------------------------------------------------------------

#[tokio::test]
async fn notes_declares_its_sources_and_client_state_and_renders_a_valid_tree() {
    let (rig, loaded) = notes().await;
    assert_eq!(loaded.sources.len(), 3);
    let out = open(&rig, "notes", "v1").await;
    assert_eq!(
        out.sources.keys().collect::<Vec<_>>(),
        ["s1", "s2", "s3"],
        "one id per (declaration, params)"
    );
    let s2 = &out.sources["s2"].decl;
    let vos_domain::ui::SourceDecl::Rows { page, params, .. } = s2 else { panic!("rows") };
    assert_eq!(*page, Some(60));
    assert_eq!(params["folder"], json!({"$ref": "client.folder"}));
    assert_eq!(params["q"], json!({"$ref": "client.search"}));
    let vos_domain::ui::SourceDecl::Row { writable, rev, .. } = &out.sources["s3"].decl else {
        panic!("row")
    };
    assert_eq!(writable, &["body"], "derived from the two-way binding in the tree");
    assert_eq!(rev.as_deref(), Some("rev"));
    assert_eq!(out.client_decls["folder"].persist, vos_domain::ui::source::Persist::App);
    assert!(out.client.is_empty(), "nothing restored yet");
}

#[tokio::test]
async fn a_tap_can_make_a_note_and_point_the_client_at_it() {
    let (rig, _) = notes().await;
    let out = open(&rig, "notes", "v1").await;
    assert!(tap(&rig, "notes", "v1", &out, "new-note").await);
    let effects = rig.runtime.take_view_effects(&id("notes")).await.unwrap();
    assert_eq!(
        effects,
        vec![(
            "v1".to_owned(),
            ViewEffect::ClientSet { path: "noteId".into(), value: json!(1), force: false }
        )]
    );
    let rows = AppData::query(
        rig.db.as_ref(),
        "select author from notes".into(),
        vos_application::data::Params::None,
    )
    .await
    .unwrap();
    assert_eq!(rows[0]["author"], "You", "the handler saw the person as e.actor");
}

#[tokio::test]
async fn restored_values_a_stale_event_and_closing() {
    let (rig, _) = notes().await;
    let restored: Map<String, Value> = obj(&[("noteId", json!(7)), ("ghost", json!(1))]);
    let out = rig.runtime.open_view(&id("notes"), "v1", "main", Map::new(), restored).await.unwrap();
    assert_eq!(out.client.get("noteId"), Some(&json!(7)), "a declared, restored value");
    assert!(out.client.get("ghost").is_none(), "an undeclared one is ignored");

    let stale = ViewEventCall {
        path: "main/nope".into(),
        event: "tap".into(),
        row_key: None,
        row: None,
        payload: None,
    };
    assert!(!rig.runtime.dispatch_view_event(&id("notes"), "v1", stale, common::user_ctx(0)).await.unwrap());

    rig.runtime.close_view(&id("notes"), "v1").await.unwrap();
    assert!(matches!(rig.runtime.render_view(&id("notes"), "v1").await, Err(PortError::NotFound(_))));
    assert!(matches!(
        rig.runtime.open_view(&id("notes"), "v2", "nope", Map::new(), Map::new()).await,
        Err(PortError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_synced_client_value_is_readable_by_handlers_and_the_alert_deletes_the_note() {
    let (rig, _) = notes().await;
    AppData::exec(
        rig.db.as_ref(),
        "insert into notes(folder_id, body) values (1, 'x')".into(),
        vos_application::data::Params::None,
    )
    .await
    .unwrap();
    let out = open(&rig, "notes", "v1").await;
    // the client set deleteId (ui.act) and mirrors it (sync = commit); then taps Delete in the alert
    rig.runtime.set_client_values(&id("notes"), "v1", obj(&[("deleteId", json!(1))])).await.unwrap();
    rig.runtime.set_client_values(&id("notes"), "v1", obj(&[("noteId", json!(1))])).await.unwrap();
    assert!(tap(&rig, "notes", "v1", &out, "confirm").await);
    let left = AppData::query(
        rig.db.as_ref(),
        "select count(*) as n from notes".into(),
        vos_application::data::Params::None,
    )
    .await
    .unwrap();
    assert_eq!(left[0]["n"], 0);
    let effects = rig.runtime.take_view_effects(&id("notes")).await.unwrap();
    assert!(effects.iter().any(|(_, e)| matches!(e, ViewEffect::ClientSet { path, value, .. } if path == "noteId" && *value == json!(0))), "{effects:?}");
}

// ---- failures stay in the view ---------------------------------------------------------------------------

const BROKEN: &str = r#"
local app = require("vos:app")
local ui = require("vos:ui")
app.manifest { id = "broken", name = "Broken", icon = "gear" }
local s = app.state { n = 0, bad = false }
app.window("main", {
  state = { title = "hello" },
  render = function(w)
    if s.bad then error("render exploded") end
    if w.state.title == "invalid" then
      return ui.Button { key = "b", onTap = function() end }   -- no title or label: fails the schema
    end
    return ui.VStack {
      ui.Text { tostring(s.n), key = "n" },
      ui.Button { key = "inc", title = "Inc", onTap = function() s.n = s.n + 1 end },
      ui.Button { key = "bad", title = "Bad", onTap = function() s.bad = true end },
      ui.Button { key = "toast", title = "Toast", onTap = function() w:toast { text = "Saved", kind = "success" } w:announce("done") end },
      ui.Button { key = "fail", title = "Fail", onTap = function() error("handler exploded") end },
      ui.Button { key = "retitle", title = "Retitle", onTap = function() w.state.title = "invalid" end },
    }
  end,
})
"#;

#[tokio::test]
async fn a_render_that_fails_or_is_invalid_is_an_error_and_the_view_survives() {
    let package = AppPackage {
        entry: "app.lua".into(),
        files: [("app.lua".to_owned(), BROKEN.as_bytes().to_vec())].into(),
        ..AppPackage::default()
    };
    let (rig, _) = load("broken", package).await;
    let first = open(&rig, "broken", "v1").await;

    tap(&rig, "broken", "v1", &first, "toast").await;
    let effects = rig.runtime.take_view_effects(&id("broken")).await.unwrap();
    assert!(
        matches!(&effects[0].1, ViewEffect::Command(Command::Toast(t)) if t.text == "Saved" && t.kind.as_deref() == Some("success"))
    );
    assert!(matches!(&effects[1].1, ViewEffect::Command(Command::Announce { text }) if text == "done"));

    let handler_error = {
        let path = path_of(&first.tree, "fail").unwrap();
        let call = ViewEventCall { path, event: "tap".into(), row_key: None, row: None, payload: None };
        rig.runtime.dispatch_view_event(&id("broken"), "v1", call, common::user_ctx(0)).await.unwrap_err()
    };
    assert!(
        matches!(&handler_error, PortError::Rejected(m) if m.contains("broken/app.lua:") && m.contains("handler exploded")),
        "{handler_error:?}"
    );

    // an invalid tree is rejected with every problem, and the old handlers keep working
    tap(&rig, "broken", "v1", &first, "retitle").await;
    let invalid = rig.runtime.render_view(&id("broken"), "v1").await.unwrap_err();
    assert!(
        matches!(&invalid, PortError::Rejected(m) if m.contains("invalid") && m.contains("main")),
        "{invalid:?}"
    );
    assert!(tap(&rig, "broken", "v1", &first, "inc").await, "handlers of the last good tree still answer");

    // a render that raises an error carries the app's file and line
    tap(&rig, "broken", "v1", &first, "bad").await;
    let raised = rig.runtime.render_view(&id("broken"), "v1").await.unwrap_err();
    assert!(
        matches!(&raised, PortError::Rejected(m) if m.contains("render exploded") && m.contains("broken/app.lua:")),
        "{raised:?}"
    );
}

#[tokio::test]
async fn render_is_pure_a_handler_style_write_raises_inside_it() {
    let src = r#"
local app = require("vos:app")
local ui = require("vos:ui")
app.manifest { id = "impure", name = "Impure", icon = "gear" }
local s = app.state { n = 0 }
app.window("main", { render = function(w) s.n = s.n + 1 return ui.Text { "x", key = "t" } end })
"#;
    let package = AppPackage {
        entry: "app.lua".into(),
        files: [("app.lua".to_owned(), src.as_bytes().to_vec())].into(),
        ..AppPackage::default()
    };
    let (rig, _) = load("impure", package).await;
    let err = rig.runtime.open_view(&id("impure"), "v1", "main", Map::new(), Map::new()).await.unwrap_err();
    assert!(matches!(&err, PortError::Rejected(m) if m.contains("must be pure")), "{err:?}");
}

/// What a view sends on `view.open`, minus the snapshot envelope: the shape of `vos-codegen/tests/fixtures/trees`.
fn wire(out: &RenderOutput) -> Value {
    json!({
        "tree": out.tree,
        "client": out.client_decls,
        "sources": out.sources.iter().map(|(id, binding)| (id.clone(), &binding.decl)).collect::<BTreeMap<_, _>>(),
    })
}

/// The trees under `crates/vos-codegen/tests/fixtures/trees` are what the real runtime renders for the Calculator and
/// Notes: `vos-codegen` checks them against the schema and `types/tests/trees.test.ts` against the component table, so
/// they must not drift from the apps. Re-bless with `UPDATE_FIXTURES=1 cargo test -p vos-lua --test views`.
#[tokio::test]
async fn the_committed_example_trees_are_what_the_real_runtime_renders() {
    let bless = std::env::var_os("UPDATE_FIXTURES").is_some();
    let calculator = calculator().await;
    let (notes, _) = notes().await;
    for (app, rig) in [("calculator", &calculator), ("notes", &notes)] {
        let fresh = wire(&open(rig, app, "v1").await);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../vos-codegen/tests/fixtures/trees/{app}.json"));
        if bless {
            std::fs::write(&path, serde_json::to_string_pretty(&fresh).unwrap() + "\n").unwrap();
        } else {
            let committed: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                fresh,
                committed,
                "{app}: the tree the app renders changed; review it, then run \
                 `UPDATE_FIXTURES=1 cargo test -p vos-lua --test views` to re-bless {}",
                path.display()
            );
        }
    }
}

/// The generated `ui.lua` constructors do what the docs promise (positional children, slots, auto keys, handlers,
/// bindings, errors that name the constructor and the prop), checked by an app on the real runtime.
#[tokio::test]
async fn the_ui_constructors_behave_as_documented() {
    let source = include_str!("fixtures/constructors.lua");
    let (rig, _) = load("constructors", common::package(source, &[])).await;
    let checked = id("constructors");
    let out = rig.runtime.call_tool(&checked, "constructors_checked", json!({}), agent_ctx(0), false).await;
    assert_eq!(out.unwrap(), json!({ "ok": true }), "every check of constructors.lua ran");

    // the checks bite: a constructor that stopped doing what is documented fails the load
    let broken = source.replace("check(stack.props.axis == \"v\"", "check(stack.props.axis == \"h\"");
    assert_ne!(broken, source);
    let rig = Rig::new();
    let data: Arc<dyn AppData> = rig.db.clone();
    let err = rig.runtime.load(&id("constructors"), common::package(&broken, &[]), data).await.unwrap_err();
    assert!(err.to_string().contains("VStack sets axis"), "{err}");
}

const TOASTER: &str = r#"
local app = require("vos:app")
local ui = require("vos:ui")
app.manifest { id = "toaster", name = "Toaster", icon = "plus", window = { w = 400, h = 300, minW = 320, minH = 240 } }
local s = app.state { n = 0, undone = 0 }
app.window("main", {
  render = function(w)
    return ui.VStack {
      ui.Text { key = "n", tostring(s.n) .. "/" .. tostring(s.undone) },
      ui.Button {
        key = "add",
        title = "Add",
        onTap = function()
          s.n = s.n + 1
          w:toast { text = "Added", action = { title = "Undo", onTap = function() s.undone = s.undone + 1 end } }
        end,
      },
    }
  end,
})
"#;

#[tokio::test]
async fn a_toast_action_runs_the_apps_closure_once() {
    let rig = Rig::new();
    let package = common::package(TOASTER, &[]);
    let data: Arc<dyn AppData> = rig.db.clone();
    rig.runtime.load(&id("toaster"), package, data).await.unwrap();
    let out = open(&rig, "toaster", "v1").await;
    assert!(tap(&rig, "toaster", "v1", &out, "add").await);

    let effects = rig.runtime.take_view_effects(&id("toaster")).await.unwrap();
    let Some((_, ViewEffect::Command(Command::Toast(toast)))) = effects.first() else {
        panic!("a toast was queued: {effects:?}")
    };
    let action = toast.action.clone().expect("the toast carries its action");
    assert_eq!((action.title.as_str(), action.id.as_str()), ("Undo", "t1"));

    let press = |path: &str| ViewEventCall {
        path: path.into(),
        event: "tap".into(),
        row_key: None,
        row: None,
        payload: None,
    };
    let path = format!("$toast/{}", action.id);
    let ran = rig.runtime.dispatch_view_event(&id("toaster"), "v1", press(&path), common::user_ctx(0)).await;
    assert!(ran.unwrap(), "the action's onTap ran");
    let again =
        rig.runtime.dispatch_view_event(&id("toaster"), "v1", press(&path), common::user_ctx(0)).await;
    assert!(!again.unwrap(), "a toast action is one-shot: the second tap is stale");
    let after = rig.runtime.render_view(&id("toaster"), "v1").await.unwrap();
    assert_eq!(text_at(&after.tree, "n").as_deref(), Some("1/1"), "the handler changed app state once");
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! Todos (`apps/todos`, a Vite project whose backend is written in TypeScript) under the real runtime: its handlers
//! through the generated router, and its agent tools. The backend is compiled as the app's own build compiles it.

mod common;

use common::{agent_ctx, built_project, db, id, user_ctx};
use serde_json::{json, Value};
use std::sync::Arc;
use vos_application::{
    ports::{AppRuntime, LoadedApp, Notifier, PortResult},
    testing::RecordingShell,
    PortError,
};
use vos_domain::Notification;
use vos_lua::LuaRuntime;
use vos_sqlite::{AppDb, Migration};

const APP: &str = "todos";

struct Quiet;
#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

struct Todos {
    rt: LuaRuntime,
    shell: Arc<RecordingShell>,
    loaded: LoadedApp,
}

impl Todos {
    async fn open() -> Self {
        Self::open_on(db()).await
    }

    /// The app loaded on a database that may already hold its data.
    async fn open_on(db: Arc<AppDb>) -> Self {
        let shell = Arc::new(RecordingShell::default());
        let rt = LuaRuntime::new(Arc::new(Quiet), shell.clone());
        let loaded = rt.load(&id(APP), built_project("apps/todos"), db).await.unwrap();
        Self { rt, shell, loaded }
    }

    async fn call(&self, method: &str, args: Value) -> PortResult<Value> {
        self.rt.call_handler(&id(APP), method, args, user_ctx(0)).await
    }

    async fn tool(&self, name: &str, args: Value) -> PortResult<Value> {
        self.rt.call_tool(&id(APP), name, args, agent_ctx(0), false).await
    }

    fn events(&self) -> Vec<String> {
        self.shell.events.lock().unwrap().iter().map(|(_, name, _)| name.clone()).collect()
    }
}

fn titles(board: &Value) -> Vec<&str> {
    board["todos"].as_array().unwrap().iter().map(|todo| todo["title"].as_str().unwrap()).collect()
}

/// Migration 1 as the Luau backend had it, byte for byte: what every Todos database made before the backend was
/// TypeScript was made from. The database keeps a checksum of these bytes and refuses a migration that differs.
const LUAU_MIGRATION_1: &str = include_str!("fixtures/todos-migration-1-luau.sql");

#[tokio::test]
async fn a_database_the_luau_backend_made_opens_with_its_todos() {
    let db = Arc::new(AppDb::open(None, &[]).unwrap());
    db.migrate(vec![Migration::new(1, "1", LUAU_MIGRATION_1)]).await.unwrap();
    db.exec("insert into todos(list_id, title) values (1, 'Kept from before')".to_owned(), vec![])
        .await
        .unwrap();

    let todos = Todos::open_on(db).await;
    let board = todos.call("board", Value::Null).await.unwrap();
    assert_eq!(titles(&board), ["Kept from before"]);
    assert_eq!(board["lists"], json!([{ "id": 1, "name": "Reminders", "color": "blue" }]));
}

#[tokio::test]
async fn a_person_keeps_todos_in_lists() {
    let todos = Todos::open().await;
    let board = todos.call("board", Value::Null).await.unwrap();
    assert_eq!(board, json!({ "lists": [{ "id": 1, "name": "Reminders", "color": "blue" }], "todos": [] }));

    let board = todos
        .call("add", json!({ "list": 1, "title": "  Water the plants  ", "due": "2026-06-09" }))
        .await
        .unwrap();
    assert_eq!(
        board["todos"],
        json!([{ "id": 1, "list": 1, "title": "Water the plants", "notes": "", "due": "2026-06-09", "flagged": false, "done": false }])
    );
    todos.call("add", json!({ "list": 1, "title": "Call the plumber", "flagged": true })).await.unwrap();

    // only what is given changes
    let board =
        todos.call("update", json!({ "todo": 1, "notes": "The fern first", "due": "" })).await.unwrap();
    assert_eq!(board["todos"][0]["title"], "Water the plants");
    assert_eq!(board["todos"][0]["notes"], "The fern first");
    assert_eq!(board["todos"][0]["due"], "");

    // a completed todo goes after the open ones, and comes back
    let board = todos.call("toggle", json!({ "todo": 1 })).await.unwrap();
    assert_eq!(titles(&board), ["Call the plumber", "Water the plants"]);
    assert_eq!(board["todos"][1]["done"], true);
    let board = todos.call("toggle", json!({ "todo": 1 })).await.unwrap();
    assert_eq!(titles(&board), ["Water the plants", "Call the plumber"]);

    let board = todos.call("remove", json!({ "todo": 2 })).await.unwrap();
    assert_eq!(titles(&board), ["Water the plants"]);

    // a list goes with its todos, and the last list stays
    let board = todos.call("addList", json!({ "name": "Shopping", "color": "green" })).await.unwrap();
    assert_eq!(board["lists"][1], json!({ "id": 2, "name": "Shopping", "color": "green" }));
    let board = todos.call("removeList", json!({ "list": 1 })).await.unwrap();
    assert_eq!(board, json!({ "lists": [{ "id": 2, "name": "Shopping", "color": "green" }], "todos": [] }));
}

#[tokio::test]
async fn what_cannot_be_done_is_refused_in_words() {
    let todos = Todos::open().await;
    for (method, args, why) in [
        ("add", json!({ "list": 1, "title": "   " }), "A todo needs a title."),
        ("add", json!({ "list": 1, "title": "Milk", "due": "9 June" }), "A day is written like 2026-06-09."),
        // digits of another script are not a day (the host's patterns are Unicode ones)
        (
            "add",
            json!({ "list": 1, "title": "Milk", "due": "٢٠٢٦-٠٦-٠٩" }),
            "A day is written like 2026-06-09.",
        ),
        (
            "add",
            json!({ "list": 1, "title": "Milk", "due": "2026-13-01" }),
            "A day is written like 2026-06-09.",
        ),
        ("add", json!({ "list": 7, "title": "Milk" }), "There is no such list."),
        ("update", json!({ "todo": 9, "title": "Milk" }), "There is no such todo."),
        ("toggle", json!({ "todo": 9 }), "There is no such todo."),
        ("remove", json!({ "todo": 9 }), "There is no such todo."),
        ("addList", json!({ "name": " ", "color": "red" }), "A list needs a name."),
        ("removeList", json!({ "list": 1 }), "The last list cannot be deleted."),
        ("removeList", json!({ "list": 7 }), "There is no such list."),
    ] {
        assert_eq!(
            todos.call(method, args.clone()).await.unwrap_err(),
            PortError::Rejected(why.into()),
            "{method} {args}"
        );
    }
    // the router holds the call to the definition before the handler sees it
    let refused = todos.call("addList", json!({ "name": "Work", "color": "pink" })).await.unwrap_err();
    assert!(matches!(refused, PortError::Rpc { code: -32602, .. }), "{refused:?}");
    assert_eq!(todos.call("board", Value::Null).await.unwrap()["todos"], json!([]), "nothing was added");
}

#[tokio::test]
async fn an_agent_reads_adds_and_completes_todos() {
    let todos = Todos::open().await;
    let names: Vec<&str> = todos.loaded.tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(names, ["list_todos", "add_todo", "complete_todo"]);

    todos.call("addList", json!({ "name": "Shopping", "color": "green" })).await.unwrap();
    let added = todos
        .tool("add_todo", json!({ "title": "Milk", "list": "shopping", "due": "2026-06-09" }))
        .await
        .unwrap();
    assert_eq!(added, json!({ "id": 1, "list": "Shopping" }), "a list is named whatever its capitals");
    let added = todos.tool("add_todo", json!({ "title": "Post the letter" })).await.unwrap();
    assert_eq!(added, json!({ "id": 2, "list": "Reminders" }), "the first list when none is named");

    let listed = todos.tool("list_todos", json!({ "list": "Shopping" })).await.unwrap();
    assert_eq!(
        listed,
        json!({ "todos": [{ "id": 1, "title": "Milk", "list": "Shopping", "notes": "", "due": "2026-06-09", "flagged": false, "done": false }] })
    );

    let done = todos.tool("complete_todo", json!({ "id": 1 })).await.unwrap();
    assert_eq!(done, json!({ "id": 1, "title": "Milk", "done": true }));
    let open = todos.tool("list_todos", json!({})).await.unwrap();
    assert_eq!(open["todos"].as_array().unwrap().len(), 1);
    let completed = todos.tool("list_todos", json!({ "completed": true })).await.unwrap();
    assert_eq!(completed["todos"][0]["title"], "Milk");

    // each change an agent made told the open windows; reading told them nothing
    assert_eq!(todos.events(), ["todos.changed", "todos.changed", "todos.changed"]);

    assert_eq!(
        todos.tool("add_todo", json!({ "title": "Eggs", "list": "Nowhere" })).await.unwrap_err(),
        PortError::Rejected("There is no list called Nowhere.".into())
    );
    assert_eq!(
        todos.tool("complete_todo", json!({ "id": 9 })).await.unwrap_err(),
        PortError::Rejected("There is no such todo.".into())
    );
}

/// A database the Luau backend made (`fixtures/todos-luau/store.luau`: its migration and a todo kept its way) is
/// the same database to today's backend. The database records a hash of each migration's bytes and refuses a step
/// that no longer matches what it ran, so the migration must be sent exactly as the Luau backend sent it. A fresh
/// database cannot show this: it has recorded nothing.
#[tokio::test]
async fn a_database_of_the_luau_backend_opens_with_its_todos() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("todos.db");
    let open = || Arc::new(vos_sqlite::AppDb::open(Some(&file), &[]).unwrap());

    let before = include_str!("fixtures/todos-luau/store.luau");
    drop(common::try_load_with(APP, before, open()).await.unwrap());

    let shell = Arc::new(RecordingShell::default());
    let rt = LuaRuntime::new(Arc::new(Quiet), shell.clone());
    let loaded = rt.load(&id(APP), built_project("apps/todos"), open()).await.unwrap();
    let todos = Todos { rt, shell, loaded };

    let board = todos.call("board", Value::Null).await.unwrap();
    assert_eq!(board["lists"], json!([{ "id": 1, "name": "Reminders", "color": "blue" }]));
    assert_eq!(
        board["todos"],
        json!([{ "id": 1, "list": 1, "title": "Kept since the Luau backend", "notes": "", "due": "2026-06-09", "flagged": true, "done": false }])
    );
    // and it is a working app on that database: the next todo follows the kept one
    let board = todos.call("add", json!({ "list": 1, "title": "Added since" })).await.unwrap();
    assert_eq!(titles(&board), ["Kept since the Luau backend", "Added since"]);
}

use super::*;
use crate::ui::source::Persist;
use serde::de::DeserializeOwned;
use serde_json::json;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T, expected: &Value) {
    let wire = serde_json::to_value(value).unwrap();
    assert_eq!(&wire, expected);
    assert_eq!(&serde_json::from_value::<T>(wire).unwrap(), value);
}

fn tree() -> Node {
    Node::new("Group", "main").child(Node::new("Text", "t").prop("text", json!("hi")))
}

#[test]
fn hello_and_welcome() {
    let hello = SessionHello {
        proto: 1,
        kit: "1.0".into(),
        locale: "en-GB".into(),
        time_zone: "Europe/London".into(),
        features: vec!["offline".into()],
        resume: Some(SessionResume {
            session_id: "s1".into(),
            views: vec![ResumeView { view_id: "v1".into(), rev: 4 }],
        }),
    };
    round_trip(
        &hello,
        &json!({
            "proto": 1, "kit": "1.0", "locale": "en-GB", "timeZone": "Europe/London", "features": ["offline"],
            "resume": { "sessionId": "s1", "views": [{ "viewId": "v1", "rev": 4 }] }
        }),
    );
    round_trip(
        &SessionWelcome {
            proto: 1,
            kit: "1.0".into(),
            host_version: "0.1.0".into(),
            session_id: "s1".into(),
            resumed: false,
        },
        &json!({ "proto": 1, "kit": "1.0", "hostVersion": "0.1.0", "sessionId": "s1", "resumed": false }),
    );
}

#[test]
fn view_open_and_snapshot() {
    let open = ViewOpen {
        app: "notes".into(),
        window: "main".into(),
        window_id: "w7".into(),
        size: Size { w: 960, h: 640 },
        params: Some(json!({ "noteId": 12 }).as_object().cloned().unwrap()),
        resume: None,
    };
    round_trip(
        &open,
        &json!({ "app": "notes", "window": "main", "windowId": "w7", "size": { "w": 960, "h": 640 }, "params": { "noteId": 12 } }),
    );

    let snapshot = ViewSnapshot {
        view_id: "v1".into(),
        proto: 1,
        kit: "1.0".into(),
        rev: 1,
        client_decls: [(
            "noteId".to_owned(),
            ClientDecl { init: json!(0), persist: Persist::App, ..ClientDecl::default() },
        )]
        .into(),
        client: json!({ "noteId": 7 }).as_object().cloned().unwrap(),
        state: [("draft.title".to_owned(), StateValue { v: json!("Groceries"), r: 14 })].into(),
        sources: [(
            "s_note".to_owned(),
            SourceDecl::Row {
                key: "id".into(),
                writable: vec!["body".into()],
                rev: None,
                conflict: None,
                params: json!({ "id": { "$ref": "client.noteId" } }).as_object().cloned().unwrap(),
            },
        )]
        .into(),
        tree: tree(),
        data: [("s_note".to_owned(), SourcePatch::Row { rev: 9, row: Some(json!({ "id": 7 })) })].into(),
    };
    let wire = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(wire["clientDecls"]["noteId"], json!({ "init": 0, "persist": "app" }));
    assert_eq!(wire["data"]["s_note"]["kind"], "row");
    assert_eq!(serde_json::from_value::<ViewSnapshot>(wire).unwrap(), snapshot);
}

#[test]
fn events_and_acks() {
    let event = ViewEvent {
        view_id: "v1".into(),
        rev: 42,
        seq: 17,
        path: "main/list/@row/item/delete".into(),
        row_key: Some(json!(7)),
        event: "tap".into(),
        payload: None,
    };
    round_trip(
        &event,
        &json!({ "viewId": "v1", "rev": 42, "seq": 17, "path": "main/list/@row/item/delete", "rowKey": 7, "event": "tap" }),
    );
    round_trip(
        &EventAck { ok: false, reason: Some("stale".into()) },
        &json!({ "ok": false, "reason": "stale" }),
    );
    round_trip(&EventAck { ok: true, reason: None }, &json!({ "ok": true }));
}

#[test]
fn write_backs_carry_base_rev_and_acks_carry_conflicts() {
    round_trip(
        &RowSet {
            view_id: "v1".into(),
            source: "s_note".into(),
            key: json!(7),
            field: "body".into(),
            value: json!("Milk"),
            base_rev: 9,
        },
        &json!({ "viewId": "v1", "source": "s_note", "key": 7, "field": "body", "value": "Milk", "baseRev": 9 }),
    );
    round_trip(
        &StateSet { view_id: "v1".into(), path: "draft.title".into(), value: json!("x"), base_rev: 3 },
        &json!({ "viewId": "v1", "path": "draft.title", "value": "x", "baseRev": 3 }),
    );
    round_trip(
        &WriteAck {
            rev: 11,
            value: json!("Milk\n- buy oat milk"),
            resolution: WriteResolution::Merged,
            conflict: Some(false),
            undo: None,
        },
        &json!({ "rev": 11, "value": "Milk\n- buy oat milk", "resolution": "merged", "conflict": false }),
    );
    round_trip(
        &WriteAck {
            rev: 12,
            value: json!("Mine"),
            resolution: WriteResolution::Replaced,
            conflict: Some(true),
            undo: Some(Undo { revision_id: 4, lost: json!("Theirs") }),
        },
        &json!({
            "rev": 12, "value": "Mine", "resolution": "replaced", "conflict": true,
            "undo": { "revisionId": 4, "lost": "Theirs" }
        }),
    );
    round_trip(
        &RowRestore { view_id: "v1".into(), source: "s_note".into(), key: json!(7), revision_id: 4 },
        &json!({ "viewId": "v1", "source": "s_note", "key": 7, "revisionId": 4 }),
    );
}

#[test]
fn source_set_changes_params_or_the_window() {
    round_trip(
        &SourceSet {
            view_id: "v1".into(),
            source: "s_notes".into(),
            params: Some(json!({ "q": "milk" }).as_object().cloned().unwrap()),
            range: Some(Range { from: 60, to: 120 }),
            anchor: None,
        },
        &json!({ "viewId": "v1", "source": "s_notes", "params": { "q": "milk" }, "range": { "from": 60, "to": 120 } }),
    );
}

#[test]
fn data_patches() {
    let patch = DataPatch {
        view_id: "v1".into(),
        state: Some([("state.display".to_owned(), StateValue { v: json!("7"), r: 3 })].into()),
        sources: Some(
            [(
                "s_notes".to_owned(),
                SourcePatch::Rows {
                    total: 214,
                    from: 0,
                    keys: vec![json!(41), json!(7)],
                    rows: [("41".to_owned(), json!({ "id": 41, "title": "Plan" }))].into(),
                    drop: vec![json!(3)],
                },
            )]
            .into(),
        ),
        declare: None,
    };
    let wire = serde_json::to_value(&patch).unwrap();
    assert_eq!(wire["state"]["state.display"], json!({ "v": "7", "r": 3 }));
    assert_eq!(wire["sources"]["s_notes"]["kind"], "rows");
    assert_eq!(wire["sources"]["s_notes"]["drop"], json!([3]));
    assert_eq!(serde_json::from_value::<DataPatch>(wire).unwrap(), patch);
}

#[test]
fn text_and_thread_patches() {
    round_trip(
        &SourcePatch::Text {
            rev: 5,
            length: 100,
            value: None,
            splices: vec![TextSplice { at: 4, del: 0, ins: "x".into(), rev: 5 }],
        },
        &json!({ "kind": "text", "rev": 5, "length": 100, "splices": [{ "at": 4, "del": 0, "ins": "x", "rev": 5 }] }),
    );
    round_trip(
        &SourcePatch::Thread {
            rows: BTreeMap::new(),
            append: vec![ThreadAppend { id: "m1".into(), path: "parts.0.text".into(), text: "hel".into() }],
        },
        &json!({ "kind": "thread", "rows": {}, "append": [{ "id": "m1", "path": "parts.0.text", "text": "hel" }] }),
    );
}

#[test]
fn view_ops_carry_base_and_rev() {
    let ops = ViewOps {
        view_id: "v1".into(),
        base: 3,
        rev: 4,
        ops: vec![Op::Set { path: "main/display".into(), props: [("text".to_owned(), json!("7"))].into() }],
    };
    round_trip(
        &ops,
        &json!({ "viewId": "v1", "base": 3, "rev": 4, "ops": [{ "op": "set", "path": "main/display", "props": { "text": "7" } }] }),
    );
}

#[test]
fn every_op_round_trips() {
    let ops = vec![
        Op::Set { path: "a".into(), props: [("x".to_owned(), Value::Null)].into() },
        Op::On {
            path: "a".into(),
            on: [("tap".to_owned(), Some(Handler::Emit)), ("long".to_owned(), None)].into(),
        },
        Op::Insert { parent: "a".into(), slot: Some("row".into()), before: Some("k".into()), node: tree() },
        Op::Remove { path: "a/b".into() },
        Op::Move { path: "a/b".into(), before: None },
        Op::Replace { path: "a/b".into(), node: tree() },
    ];
    for op in ops {
        let wire = serde_json::to_value(&op).unwrap();
        assert_eq!(serde_json::from_value::<Op>(wire).unwrap(), op);
    }
}

#[test]
fn commands_are_cmd_plus_args() {
    let toast = ViewCommand {
        view_id: "v1".into(),
        command: Command::Toast(Toast {
            text: "Your edit replaced an agent change.".into(),
            icon: None,
            kind: Some("info".into()),
            action: Some(ToastAction { title: "Undo".into(), id: "t1".into() }),
            duration: None,
        }),
    };
    round_trip(
        &toast,
        &json!({
            "viewId": "v1", "cmd": "toast",
            "args": { "text": "Your edit replaced an agent change.", "kind": "info", "action": { "title": "Undo", "id": "t1" } }
        }),
    );
    let focus = ViewCommand { view_id: "v1".into(), command: Command::Focus { key: "q".into() } };
    round_trip(&focus, &json!({ "viewId": "v1", "cmd": "focus", "args": { "key": "q" } }));
    for command in [Command::Haptic, Command::CloseWindow] {
        let wire =
            serde_json::to_value(ViewCommand { view_id: "v1".into(), command: command.clone() }).unwrap();
        assert_eq!(serde_json::from_value::<ViewCommand>(wire).unwrap().command, command);
    }
}

#[test]
fn the_rest_of_the_host_notifications() {
    round_trip(
        &ViewTree { view_id: "v1".into(), rev: 9, tree: tree() },
        &serde_json::to_value(ViewTree { view_id: "v1".into(), rev: 9, tree: tree() }).unwrap(),
    );
    round_trip(
        &ClientSet { view_id: "v1".into(), path: "noteId".into(), value: json!(12), force: None },
        &json!({ "viewId": "v1", "path": "noteId", "value": 12 }),
    );
    round_trip(
        &ViewError { view_id: "v1".into(), message: "boom".into(), dev: None },
        &json!({ "viewId": "v1", "message": "boom" }),
    );
    round_trip(
        &ViewClosed { view_id: "v1".into(), reason: "uninstalled".into() },
        &json!({ "viewId": "v1", "reason": "uninstalled" }),
    );
    round_trip(
        &ThemeSet {
            scheme: "dark".into(),
            tint: "blue".into(),
            text_scale: 1.0,
            reduce_motion: false,
            reduce_transparency: false,
            increase_contrast: true,
        },
        &json!({ "scheme": "dark", "tint": "blue", "textScale": 1.0, "reduceMotion": false, "reduceTransparency": false, "increaseContrast": true }),
    );
}

#[test]
fn window_open_carries_the_app_the_window_kind_and_optional_params() {
    round_trip(
        &WindowOpen { app: "calculator".into(), window: "main".into(), params: None },
        &json!({ "app": "calculator", "window": "main" }),
    );
    let note = HostNotification::WindowOpen(WindowOpen {
        app: "notes".into(),
        window: "main".into(),
        params: json!({ "noteId": 12 }).as_object().cloned(),
    });
    let frame = serde_json::to_value(note.to_message()).unwrap();
    assert_eq!(frame["method"], "window.open");
    assert_eq!(frame["params"]["params"]["noteId"], 12);
}

#[test]
fn the_client_notifications() {
    round_trip(
        &ClientState { view_id: "v1".into(), set: json!({ "folder": 3 }).as_object().cloned().unwrap() },
        &json!({ "viewId": "v1", "set": { "folder": 3 } }),
    );
    round_trip(
        &ViewResize { view_id: "v1".into(), size: Size { w: 400, h: 300 }, size_class: "compact".into() },
        &json!({ "viewId": "v1", "size": { "w": 400, "h": 300 }, "sizeClass": "compact" }),
    );
    round_trip(
        &ViewVisibility { view_id: "v1".into(), visible: false },
        &json!({ "viewId": "v1", "visible": false }),
    );
}

#[test]
fn drafts_are_tagged_by_kind() {
    let drafts = vec![
        Draft::State { path: "draft.title".into(), value: json!("a"), base_rev: 1 },
        Draft::Row {
            source: "s".into(),
            key: json!(7),
            field: "body".into(),
            value: json!("b"),
            base_rev: 2,
        },
    ];
    let wire = serde_json::to_value(&drafts).unwrap();
    assert_eq!(wire[0]["kind"], "state");
    assert_eq!(wire[1]["baseRev"], 2);
    assert_eq!(serde_json::from_value::<Vec<Draft>>(wire).unwrap(), drafts);
}

// ---- the method table -----------------------------------------------------------------------------------

#[test]
fn the_method_table_has_unique_namespaced_names() {
    let mut seen = std::collections::HashSet::new();
    for m in METHODS {
        assert!(seen.insert(m.name), "duplicate {}", m.name);
        assert!(m.name.contains('.'), "{} is not namespaced", m.name);
        assert_eq!(m.result.is_some(), m.kind == MethodKind::Request, "{}", m.name);
    }
    assert_eq!(METHODS.iter().filter(|m| m.kind == MethodKind::Request).count(), 9);
    assert_eq!(method("view.ops").map(|m| m.direction), Some(Direction::HostToClient));
    assert!(method("nope").is_none());
}

fn sample(method: &str) -> Value {
    match method {
        "session.hello" => json!({ "proto": 1, "kit": "1.0", "locale": "en", "timeZone": "UTC" }),
        "view.open" => {
            json!({ "app": "notes", "window": "main", "windowId": "w1", "size": { "w": 1, "h": 1 } })
        }
        "view.sync" | "view.close" | "data.patch" => json!({ "viewId": "v1" }),
        "view.event" => json!({ "viewId": "v1", "rev": 1, "seq": 1, "path": "main/x", "event": "tap" }),
        "state.set" => json!({ "viewId": "v1", "path": "p", "value": 1, "baseRev": 0 }),
        "row.set" => {
            json!({ "viewId": "v1", "source": "s", "key": 1, "field": "f", "value": 1, "baseRev": 0 })
        }
        "row.restore" => json!({ "viewId": "v1", "source": "s", "key": 1, "revisionId": 1 }),
        "source.set" => json!({ "viewId": "v1", "source": "s" }),
        "client.state" => json!({ "viewId": "v1", "set": {} }),
        "view.resize" => json!({ "viewId": "v1", "size": { "w": 1, "h": 1 }, "sizeClass": "compact" }),
        "view.visibility" => json!({ "viewId": "v1", "visible": true }),
        "session.bye" => json!({}),
        "view.tree" => json!({ "viewId": "v1", "rev": 1, "tree": { "type": "Group", "key": "main" } }),
        "view.ops" => json!({ "viewId": "v1", "base": 1, "rev": 2, "ops": [] }),
        "client.set" => json!({ "viewId": "v1", "path": "x", "value": 1 }),
        "view.command" => json!({ "viewId": "v1", "cmd": "haptic" }),
        "view.error" => json!({ "viewId": "v1", "message": "m" }),
        "view.closed" => json!({ "viewId": "v1", "reason": "r" }),
        "window.open" => json!({ "app": "calculator", "window": "main" }),
        "theme.set" => {
            json!({ "scheme": "light", "tint": "blue", "textScale": 1.0, "reduceMotion": false, "reduceTransparency": false, "increaseContrast": false })
        }
        other => panic!("no sample for {other}"),
    }
}

#[test]
fn every_method_in_the_table_decodes_through_its_enum() {
    for m in METHODS {
        let params = sample(m.name);
        match (m.direction, m.kind) {
            (Direction::ClientToHost, MethodKind::Request) => {
                let request = ClientRequest::from_rpc(m.name, params).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(request.method(), m.name);
            }
            (Direction::ClientToHost, MethodKind::Notification) => {
                ClientNotification::from_rpc(m.name, params).unwrap_or_else(|e| panic!("{e}"));
            }
            (Direction::HostToClient, _) => {
                let note = HostNotification::from_rpc(m.name, params).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(note.to_rpc().0, m.name);
            }
        }
    }
}

#[test]
fn params_are_a_by_name_object_and_an_array_is_refused() {
    assert_eq!(
        ClientRequest::from_rpc("view.sync", json!({ "viewId": "v1" })).unwrap(),
        ClientRequest::ViewSync(ViewRef { view_id: "v1".into() })
    );
    // the positional convention of the deleted SDK is not accepted any more
    let wrapped = ClientRequest::from_rpc("view.sync", json!([{ "viewId": "v1" }])).unwrap_err();
    assert_eq!(wrapped.to_rpc_error().code, codes::INVALID_PARAMS);
}

#[test]
fn bad_calls_become_json_rpc_errors() {
    let unknown = ClientRequest::from_rpc("view.explode", json!({})).unwrap_err();
    assert_eq!(unknown.to_rpc_error().code, codes::METHOD_NOT_FOUND);
    let invalid = ClientRequest::from_rpc("view.sync", json!({ "view": "v1" })).unwrap_err();
    assert_eq!(invalid.to_rpc_error().code, codes::INVALID_PARAMS);
    assert!(invalid.to_string().contains("view.sync"));
}

#[test]
fn a_host_notification_is_a_json_rpc_notification_frame() {
    let note = HostNotification::Closed(ViewClosed { view_id: "v1".into(), reason: "x".into() });
    let frame = serde_json::to_value(note.to_message()).unwrap();
    assert_eq!(
        frame,
        json!({ "jsonrpc": "2.0", "method": "view.closed", "params": { "viewId": "v1", "reason": "x" } })
    );
}

#[test]
fn the_versions_are_announced() {
    assert_eq!(crate::ui::PROTO_VERSION, 1);
    assert_eq!(crate::ui::KIT_VERSION.to_string(), "1.0");
}

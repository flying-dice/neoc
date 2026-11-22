#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Every host-level `/rpc` method parses its params into a typed struct: a missing, unknown or mistyped field is
//! a `-32602`, never a silent default, and nothing changes.

mod common;

use common::{browser, rpc, start};
use serde_json::{json, Value};

const INVALID_PARAMS: i64 = -32602;

async fn code_of(ws: &mut common::Ws, id: i64, method: &str, params: Value) -> Value {
    rpc(ws, id, method, params).await["error"]["code"].clone()
}

#[tokio::test]
async fn a_malformed_params_object_is_32602_for_every_agents_method() {
    let r = start(|_| {}).await;
    let mut ws = browser(&r).await;
    let created = rpc(&mut ws, 1, "agents.create", json!({"name": "Claude", "avatar": "sparkles"})).await;
    assert!(created.get("error").is_none(), "{created}");
    let id = created["result"]["agent"]["id"].as_str().unwrap().to_owned();

    let bad: Vec<(&str, Value)> = vec![
        ("agents.list", json!({"x": 1})),
        ("agents.list", json!([1])),
        ("agents.create", json!({"name": "x", "bogus": 1})),
        ("agents.create", json!({"role": "no name"})),
        // the host launches no agents: the launch settings of older versions are unknown fields now
        ("agents.create", json!({"name": "x", "runtime": {"command": "x"}})),
        ("agents.create", json!({"name": "x", "approval": "ask"})),
        ("agents.update", json!({"id": id, "runtime": {"command": "x"}})),
        ("agents.update", json!({"id": id, "approval": "ask"})),
        // `grants` belongs to agents.grants.set: update used to accept it and change nothing
        ("agents.update", json!({"id": id, "grants": {"all": true}})),
        ("agents.update", json!({"id": id, "avatar": 42})),
        ("agents.update", json!({"id": id, "name": 7})),
        ("agents.update", json!({"name": "no id"})),
        ("agents.update", json!([{"id": id}])),
        ("agents.delete", json!({"id": id, "force": true})),
        ("agents.delete", json!({})),
        ("agents.delete", json!({"id": 5})),
        ("agents.grants.set", json!({"id": id})),
        ("agents.grants.set", json!({"id": id, "grants": {"everything": true}})),
        ("agents.grants.set", json!({"id": id, "grants": {}, "x": 1})),
        ("agents.token.rotate", json!({"id": id, "x": 1})),
        ("agents.token.rotate", json!({})),
    ];
    for (n, (method, params)) in (10_i64..).zip(bad) {
        assert_eq!(code_of(&mut ws, n, method, params.clone()).await, INVALID_PARAMS, "{method} {params}");
    }

    // none of the refusals changed anything
    let listed = rpc(&mut ws, 99, "agents.list", json!({})).await;
    let agent = &listed["result"]["agents"][0];
    assert_eq!(agent["avatar"], "sparkles");
    assert_eq!(agent["name"], "Claude");
    assert_eq!(agent["grants"], json!({"all": false, "apps": []}));

    // the valid forms work, and `avatar: null` still clears it
    let ok = rpc(&mut ws, 100, "agents.update", json!({"id": id, "avatar": null})).await;
    assert!(ok["result"]["agent"].get("avatar").is_none(), "{ok}");
    r.stop().await;
}

#[tokio::test]
async fn a_malformed_params_object_is_32602_for_the_app_and_device_methods() {
    let r = start(|_| {}).await;
    let mut ws = browser(&r).await;
    let bad: Vec<(&str, Value)> = vec![
        ("app.install", json!({"url": "x", "force": true})),
        ("app.install", json!({})),
        ("app.install", json!({"url": "x"})), // the digest of what was previewed is required
        ("app.install", json!({"url": "x", "sha256": null})),
        ("app.install", json!({"url": 3})),
        ("app.preview", json!({"url": "x", "extra": 1})),
        ("app.preview", json!([])),
        ("app.uninstall", json!({"app": "notes", "purge": "yes"})),
        ("app.uninstall", json!({"app": "notes", "everything": true})),
        ("app.uninstall", json!({})),
        ("app.enable", json!({"app": "notes", "x": 1})),
        ("app.disable", json!({"app": "Not An Id"})),
        ("app.deleteData", json!({"app": "notes", "x": 1})),
        ("app.reload", json!({})),
        ("app.list", json!({"x": 1})),
        ("app.available", json!({"x": 1})),
        ("host.devices.list", json!({"x": 1})),
        ("host.devices.revoke", json!({"id": "d", "x": 1})),
        ("host.devices.revoke", json!({})),
        ("host.files.list", json!({"app": "notes", "path": "", "x": 1})),
        ("host.files.remove", json!({"app": "notes"})),
        ("host.files.createDir", json!({})),
        ("host.files.rename", json!({"app": "notes", "from": "a"})),
    ];
    for (n, (method, params)) in (10_i64..).zip(bad) {
        assert_eq!(code_of(&mut ws, n, method, params.clone()).await, INVALID_PARAMS, "{method} {params}");
    }
    // methods without params accept an absent or empty one
    assert!(rpc(&mut ws, 50, "app.list", Value::Null).await.get("error").is_none());
    assert!(rpc(&mut ws, 51, "host.devices.list", json!({})).await.get("error").is_none());
    r.stop().await;
}

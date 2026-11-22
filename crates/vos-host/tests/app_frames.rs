#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration tests
//! Apps with a client of their own, end to end over the real adapters: the frame route and its policy, a client's
//! `rpc` on `/rpc` reaching the handler of the app it names, a handler's error as a message, and an agent's tool
//! telling the app's open clients with `app.emit`.

mod common;

use common::{
    browser, call, claude, connect_agent, next_notification, publish_package_with_client, rpc, start, temp,
    Running, Ws,
};
use serde_json::json;
use std::path::Path;
use vos_host::startup::prepare_data_dir;

const CLIENT: &str = "window.vosApp = { mount: function () {} }";

/// A backend whose handlers say which app answered; `add_task` is an agent tool that also tells the clients.
fn backend(name: &str) -> String {
    format!(
        r#"
local db = app.db
db:migrate {{ [1] = "create table tasks(id integer primary key, title text not null)" }}

app.tool {{
  name = "add_task",
  input = {{ title = app.t.string {{}} }},
  run = function(input)
    db:exec("insert into tasks(title) values (:t)", {{ t = input.title }})
    app.emit("tasks.changed", {{ title = input.title }})
    return {{ ok = true }}
  end,
}}
return {{
  whose = function(ctx, args) return {{ app = "{name}", args = args }} end,
  add = function(ctx, args)
    if type(args) ~= "table" or args.title == nil then return ctx:fail("A task needs a title.") end
    db:exec("insert into tasks(title) values (:t)", {{ t = args.title }})
    return db:all("select id, title from tasks order by id")
  end,
  explode = function() error("boom") end,
}}
"#
    )
}

async fn host_with_apps(tmp: &Path, ids: &[&str]) -> Running {
    prepare_data_dir(&tmp.join("data")).unwrap();
    let r = start(|c| {
        c.data_dir = Some(tmp.join("data"));
        c.agents = vec![claude()];
    })
    .await;
    for id in ids {
        let files = [("app.luau", backend(id).into_bytes()), ("client.js", CLIENT.as_bytes().to_vec())];
        let url = publish_package_with_client(
            &tmp.join(format!("pkg-{id}")),
            id,
            "app.luau",
            Some("client.js"),
            &files,
        );
        r.host.service.install_from_url(&url).await.unwrap();
    }
    r
}

async fn get(r: &Running, path: &str, cookie: Option<&str>) -> reqwest::Response {
    let mut req = reqwest::Client::new().get(r.url(path));
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    req.send().await.unwrap()
}

async fn paired(r: &Running) -> (String, Ws) {
    // a second pairing for the HTTP side: the cookie of the browser the socket belongs to
    let cookie = common::pair(r, "Safari on iPad").await;
    (cookie, browser(r).await)
}

#[tokio::test]
async fn the_frame_document_needs_a_session_and_carries_a_policy_with_no_network() {
    let tmp = temp("frame-doc");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (cookie, _ws) = paired(&r).await;

    assert_eq!(
        get(&r, "/frame/tasks", None).await.status(),
        401,
        "the document names the app: it needs a session"
    );

    let res = get(&r, "/frame/tasks?theme=dark", Some(&cookie)).await;
    assert_eq!(res.status(), 200);
    let csp = res.headers()["content-security-policy"].to_str().unwrap().to_owned();
    for part in ["default-src 'none'", "script-src 'self'", "sandbox allow-scripts", "frame-ancestors 'self'"]
    {
        assert!(csp.contains(part), "{part} in {csp}");
    }
    assert!(!csp.contains("connect-src"), "no network for the client: {csp}");
    assert!(!csp.contains("allow-same-origin"), "an opaque origin: {csp}");
    assert_eq!(res.headers()["cache-control"], "no-store");
    let html = res.text().await.unwrap();
    assert!(html.contains(r#"<html lang="en" data-theme="dark" style="color-scheme:dark">"#), "{html}");
    assert!(html.contains(r#"<script src="/frame/bridge.js">"#), "{html}");
    assert!(html.contains("NEOC-252"), "the marked spot for the kit's two tags: {html}");

    // anything but `dark` is light
    let light = get(&r, "/frame/tasks?theme=%3Cscript%3E", Some(&cookie)).await.text().await.unwrap();
    assert!(light.contains(r#"data-theme="light""#) && !light.contains("<script>alert"), "{light}");
    r.stop().await;
}

#[tokio::test]
async fn the_clients_script_is_public_named_by_its_digest_and_only_for_an_app_that_has_one() {
    let tmp = temp("frame-scripts");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (cookie, mut ws) = paired(&r).await;
    let html = get(&r, "/frame/tasks", Some(&cookie)).await.text().await.unwrap();
    let src = html
        .split('"')
        .find(|part| part.starts_with("/frame/tasks/") && part.ends_with("/client.js"))
        .unwrap_or_else(|| panic!("the client script in {html}"))
        .to_owned();

    // a sandboxed frame sends no cookie
    let script = get(&r, &src, None).await;
    assert_eq!(script.status(), 200);
    assert!(script.headers()["content-type"].to_str().unwrap().starts_with("text/javascript"));
    assert!(script.headers()["cache-control"].to_str().unwrap().contains("immutable"));
    assert_eq!(script.text().await.unwrap(), CLIENT);

    assert_eq!(get(&r, &format!("/frame/tasks/{}/client.js", "0".repeat(64)), None).await.status(), 404);
    assert_eq!(get(&r, "/frame/ghost/abc/client.js", None).await.status(), 404);
    assert_eq!(get(&r, "/frame/Not%20An%20Id/abc/client.js", None).await.status(), 404);

    let bridge = get(&r, "/frame/bridge.js", None).await;
    assert_eq!(bridge.status(), 200);
    let bridge = bridge.text().await.unwrap();
    assert!(bridge.contains("vosApp") && bridge.contains("parent.postMessage"), "{bridge}");

    // an app with no client has no frame, and neither has one that is not installed
    let url = common::publish_package(
        &temp("frame-plain"),
        "plain",
        "app.luau",
        &[("app.luau", b"app.manifest { id = 'plain', name = 'Plain', icon = 'gear' }".to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    assert_eq!(get(&r, "/frame/plain", Some(&cookie)).await.status(), 404);
    assert_eq!(get(&r, "/frame/ghost", Some(&cookie)).await.status(), 404);

    // the summary tells the shell which windows are frames, and gives it the window from app.json
    let list = rpc(&mut ws, 1, "app.list", json!({})).await;
    let apps = list["result"]["apps"].as_array().unwrap();
    let tasks = apps.iter().find(|a| a["id"] == "tasks").unwrap();
    assert_eq!(tasks["client"], true);
    assert_eq!(tasks["window"], json!({"w": 420, "h": 480, "minW": 320, "minH": 320}));
    assert_eq!(apps.iter().find(|a| a["id"] == "plain").unwrap()["client"], false);
    r.stop().await;
}

#[tokio::test]
async fn a_client_call_reaches_the_handler_of_the_app_it_names_and_only_that_app() {
    let tmp = temp("frame-rpc");
    let r = host_with_apps(&tmp, &["alpha", "beta"]).await;
    let (_cookie, mut ws) = paired(&r).await;

    let a = rpc(&mut ws, 1, "app.call", json!({"app": "alpha", "method": "whose", "args": {"n": 1}})).await;
    assert_eq!(a["result"], json!({"app": "alpha", "args": {"n": 1}}), "{a}");
    let b = rpc(&mut ws, 2, "app.call", json!({"app": "beta", "method": "whose"})).await;
    assert_eq!(b["result"]["app"], "beta", "{b}");
    assert_eq!(b["result"].get("args"), None, "no args is nil");

    // an app that is not there, a handler that is not there, a method no handler can have
    let ghost = rpc(&mut ws, 3, "app.call", json!({"app": "ghost", "method": "whose"})).await;
    assert_eq!(ghost["error"]["code"], -32002, "{ghost}");
    let nope = rpc(&mut ws, 4, "app.call", json!({"app": "alpha", "method": "nope"})).await;
    assert_eq!(nope["error"]["code"], -32601, "{nope}");
    assert_eq!(nope["error"]["message"], "Method not found: nope");

    // the params are the shell's, exactly: a field it does not know is an invalid call
    let extra = rpc(&mut ws, 5, "app.call", json!({"app": "alpha", "method": "whose", "as": "beta"})).await;
    assert_eq!(extra["error"]["code"], -32602, "{extra}");
    let missing = rpc(&mut ws, 6, "app.call", json!({"method": "whose"})).await;
    assert_eq!(missing["error"]["code"], -32602, "{missing}");
    r.stop().await;
}

#[tokio::test]
async fn a_handlers_failure_comes_back_as_its_message() {
    let tmp = temp("frame-errors");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (_cookie, mut ws) = paired(&r).await;

    let failed = rpc(&mut ws, 1, "app.call", json!({"app": "tasks", "method": "add", "args": {}})).await;
    assert_eq!(failed["error"]["code"], -32010, "{failed}");
    assert_eq!(failed["error"]["message"], "A task needs a title.");

    let raised = rpc(&mut ws, 2, "app.call", json!({"app": "tasks", "method": "explode"})).await;
    assert_eq!(raised["error"]["code"], -32010, "{raised}");
    assert!(raised["error"]["message"].as_str().unwrap().contains("boom"), "{raised}");

    let added =
        rpc(&mut ws, 3, "app.call", json!({"app": "tasks", "method": "add", "args": {"title": "milk"}}))
            .await;
    assert_eq!(
        added["result"],
        json!([{"id": 1, "title": "milk"}]),
        "the person's client may write: {added}"
    );

    // an app that is disabled does not serve its client
    let off = rpc(&mut ws, 4, "app.disable", json!({"app": "tasks"})).await;
    assert!(off.get("error").is_none(), "{off}");
    let down =
        rpc(&mut ws, 5, "app.call", json!({"app": "tasks", "method": "add", "args": {"title": "x"}})).await;
    assert_eq!(down["error"]["code"], -32005, "{down}");
    r.stop().await;
}

#[tokio::test]
async fn an_agents_tool_tells_the_open_clients_of_its_own_app_and_the_data_is_the_same() {
    let tmp = temp("frame-events");
    let r = host_with_apps(&tmp, &["tasks", "notes"]).await;
    let (_cookie, mut ws) = paired(&r).await;
    let (agent, _) = connect_agent(r.addr, "tok-claude").await;

    let out = call(&agent, "tasks__add_task", json!({"title": "from the agent"})).await;
    assert_eq!(out.is_error, Some(false), "{out:?}");
    let event = next_notification(&mut ws, "app.event").await;
    assert_eq!(event, json!({"app": "tasks", "name": "tasks.changed", "data": {"title": "from the agent"}}));

    // the client reads it back through the same backend
    let listed =
        rpc(&mut ws, 1, "app.call", json!({"app": "tasks", "method": "add", "args": {"title": "and me"}}))
            .await;
    assert_eq!(listed["result"].as_array().unwrap().len(), 2, "{listed}");

    // the event named its app: the shell hands it to the frames of that app only
    assert!(ws.backlog.iter().all(|m| m["method"] != "app.event" || m["params"]["app"] == "tasks"));
    r.stop().await;
}

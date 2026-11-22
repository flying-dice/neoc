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

const PAGE: &str = "<!doctype html><title>tasks</title><script src=\"/apps/bridge.js\"></script>";
const SCRIPT: &str = "console.log('tasks')";

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
        let files = [
            ("app.luau", backend(id).into_bytes()),
            ("static/index.html", PAGE.as_bytes().to_vec()),
            ("static/other.html", b"<p>not the entry</p>".to_vec()),
            ("static/assets/app.js", SCRIPT.as_bytes().to_vec()),
            ("static/.secret", b"hidden".to_vec()),
        ];
        let url = publish_package_with_client(
            &tmp.join(format!("pkg-{id}")),
            id,
            "app.luau",
            Some("static"),
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

/// The status of a GET sent exactly as written: an HTTP client normalises `%2e%2e` and `..` out of a URL before it sends it (the URL
/// standard says so, and `url` 2.5.9 does it where 2.5.8 did not), which would test the client and not the host's route.
async fn raw_status(r: &Running, path: &str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(r.addr).await.unwrap();
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", r.addr.port());
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let head = String::from_utf8_lossy(&response);
    head.split_whitespace().nth(1).and_then(|code| code.parse().ok()).unwrap_or(0)
}

async fn paired(r: &Running) -> (String, Ws) {
    // a second pairing for the HTTP side: the cookie of the browser the socket belongs to
    let cookie = common::pair(r, "Safari on iPad").await;
    (cookie, browser(r).await)
}

#[tokio::test]
async fn an_apps_static_files_are_public_carry_the_policy_and_only_its_entry_is_a_document() {
    let tmp = temp("static-files");
    let r = host_with_apps(&tmp, &["tasks"]).await;

    // a sandboxed frame sends no cookie, and its module scripts send `Origin: null`
    let page = get(&r, "/apps/tasks/static/index.html", None).await;
    assert_eq!(page.status(), 200);
    let csp = page.headers()["content-security-policy"].to_str().unwrap().to_owned();
    for part in ["default-src 'none'", "script-src 'self'", "sandbox allow-scripts", "frame-ancestors 'self'"]
    {
        assert!(csp.contains(part), "{part} in {csp}");
    }
    assert!(!csp.contains("connect-src"), "no network for the client: {csp}");
    assert!(!csp.contains("allow-same-origin"), "an opaque origin: {csp}");
    assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
    assert_eq!(page.headers()["access-control-allow-origin"], "*");
    let etag = page.headers()["etag"].to_str().unwrap().to_owned();
    assert_eq!(page.text().await.unwrap(), PAGE);

    // `/static/` is the entry too, and the validator answers 304
    assert_eq!(get(&r, "/apps/tasks/static/", None).await.text().await.unwrap(), PAGE);
    let again = reqwest::Client::new()
        .get(r.url("/apps/tasks/static/index.html"))
        .header("if-none-match", &etag)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 304);

    // a script of the app: from the table, with the same policy on it
    let script = get(&r, "/apps/tasks/static/assets/app.js", None).await;
    assert_eq!(script.status(), 200);
    assert!(script.headers()["content-type"].to_str().unwrap().starts_with("text/javascript"));
    assert!(script.headers()["content-security-policy"].to_str().unwrap().contains("sandbox allow-scripts"));
    assert_eq!(script.text().await.unwrap(), SCRIPT);

    // only the entry is a document; no file is guessed at; hidden and climbing paths are not files
    let other = get(&r, "/apps/tasks/static/other.html", None).await;
    assert_eq!(other.headers()["content-type"], "text/plain; charset=utf-8");
    for missing in [
        "/apps/tasks/static/missing.js",
        "/apps/tasks/static/assets/",
        "/apps/tasks/static/.secret",
        "/apps/tasks/static/%2e%2e/app.luau",
        "/apps/tasks/static/assets/%2e%2e/%2e%2e/app.luau",
        "/apps/tasks/static/assets%2f..%2fapp.luau",
        "/apps/tasks/static/app.luau",
        "/apps/ghost/static/index.html",
        "/apps/Not%20An%20Id/static/index.html",
    ] {
        assert_eq!(raw_status(&r, missing).await, 404, "{missing}");
    }

    // the bridge is one public copy
    let bridge = get(&r, "/apps/bridge.js", None).await;
    assert_eq!(bridge.status(), 200);
    let bridge = bridge.text().await.unwrap();
    assert!(bridge.contains("ui/initialize") && bridge.contains("parent.postMessage"), "{bridge}");

    // an app with no client has no static files, and neither has one that is not installed
    let url = common::publish_package(
        &temp("static-plain"),
        "plain",
        "app.luau",
        &[("app.luau", b"app.manifest { id = 'plain', name = 'Plain', icon = 'gear' }".to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    assert_eq!(get(&r, "/apps/plain/static/index.html", None).await.status(), 404);
    r.stop().await;
}

#[tokio::test]
async fn origin_null_gets_through_the_guard_for_static_reads_and_nowhere_else() {
    let tmp = temp("static-origin");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (cookie, _ws) = paired(&r).await;
    let with_null = |method: reqwest::Method, path: &str| {
        reqwest::Client::new().request(method, r.url(path)).header("origin", "null").header("cookie", &cookie)
    };

    assert_eq!(
        with_null(reqwest::Method::GET, "/apps/tasks/static/assets/app.js").send().await.unwrap().status(),
        200
    );
    assert_eq!(
        with_null(reqwest::Method::HEAD, "/apps/tasks/static/index.html").send().await.unwrap().status(),
        200
    );
    // not a write, not another route, even with a paired device's cookie in the request
    assert_eq!(
        with_null(reqwest::Method::POST, "/apps/tasks/static/index.html").send().await.unwrap().status(),
        403
    );
    for path in ["/apps/tasks", "/apps/bridge.js", "/session", "/rpc", "/agui/threads", "/mcp", "/"] {
        assert_eq!(with_null(reqwest::Method::GET, path).send().await.unwrap().status(), 403, "{path}");
    }
    // another origin is still refused on the static route
    let foreign = reqwest::Client::new()
        .get(r.url("/apps/tasks/static/index.html"))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    r.stop().await;
}

#[tokio::test]
async fn the_host_route_is_a_wrapper_page_for_a_paired_device_and_says_to_pair_otherwise() {
    let tmp = temp("wrapper");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (cookie, _ws) = paired(&r).await;

    let unpaired = get(&r, "/apps/tasks", None).await;
    assert_eq!(unpaired.status(), 401);
    let body = unpaired.text().await.unwrap();
    assert!(body.contains("Pair this device first") && body.contains(r#"href="/""#), "{body}");

    let res = get(&r, "/apps/tasks", Some(&cookie)).await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["cache-control"], "no-store");
    let csp = res.headers()["content-security-policy"].to_str().unwrap().to_owned();
    assert!(csp.contains("frame-src 'self'") && csp.contains("script-src 'self'"), "{csp}");
    let html = res.text().await.unwrap();
    assert!(html.contains("<title>tasks</title>"), "{html}");
    assert!(
        html.contains(r#"sandbox="allow-scripts allow-forms""#) && !html.contains("allow-same-origin"),
        "{html}"
    );
    assert!(html.contains(r#"data-entry="/apps/tasks/static/index.html""#), "{html}");
    assert!(html.contains(r#"src="/wrapper.js""#), "{html}");
    assert_eq!(html.matches("<iframe").count(), 1, "one iframe and nothing else drawn: {html}");

    // an app that is not installed, or has no client, has no page
    assert_eq!(get(&r, "/apps/ghost", Some(&cookie)).await.status(), 404);
    let url = common::publish_package(
        &temp("wrapper-plain"),
        "plain",
        "app.luau",
        &[("app.luau", b"app.manifest { id = 'plain', name = 'Plain', icon = 'gear' }".to_vec())],
    );
    r.host.service.install_from_url(&url).await.unwrap();
    assert_eq!(get(&r, "/apps/plain", Some(&cookie)).await.status(), 404);
    r.stop().await;
}

/// Publish an app with a client whose app ref carries `ui` fields beyond the root (`csp`, `permissions`).
fn publish_with_ui(root: &Path, id: &str, ui: &serde_json::Value) -> String {
    let files = [("app.luau", backend(id).into_bytes()), ("static/index.html", PAGE.as_bytes().to_vec())];
    let url = publish_package_with_client(root, id, "app.luau", Some("static"), &files);
    let path = root.join("app-ref.json");
    let mut app_ref: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (key, value) in ui.as_object().unwrap() {
        app_ref["manifest"]["ui"][key] = value.clone();
    }
    std::fs::write(path, serde_json::to_vec(&app_ref).unwrap()).unwrap();
    url
}

#[tokio::test]
async fn what_an_app_declares_for_its_page_is_applied_and_what_opens_everything_is_refused_at_install() {
    let tmp = temp("static-declared");
    let r = host_with_apps(&tmp, &[]).await;
    let (cookie, mut ws) = paired(&r).await;
    let ui = json!({
        "csp": {
            "resourceDomains": ["https://cdn.example.com", "https://*.images.example.org"],
            "connectDomains": ["https://api.example.com"]
        },
        "permissions": { "camera": {}, "clipboardWrite": {} }
    });
    r.host.service.install_from_url(&publish_with_ui(&tmp.join("feeds"), "feeds", &ui)).await.unwrap();

    let page = get(&r, "/apps/feeds/static/index.html", None).await;
    let csp = page.headers()["content-security-policy"].to_str().unwrap().to_owned();
    assert!(
        csp.contains("img-src 'self' data: blob: https://cdn.example.com https://*.images.example.org"),
        "{csp}"
    );
    assert!(csp.contains("script-src 'self';"), "an app's code comes only from the host: {csp}");
    assert!(
        !csp.contains("connect-src") && !csp.contains("api.example.com"),
        "connectDomains are not applied: {csp}"
    );
    assert!(csp.contains("sandbox allow-scripts"), "{csp}");

    // the shell is told what the frame asks for, and the wrapper puts it in the iframe's `allow`
    let list = rpc(&mut ws, 1, "app.list", json!({})).await;
    let feeds =
        list["result"]["apps"].as_array().unwrap().iter().find(|a| a["id"] == "feeds").unwrap().clone();
    assert_eq!(feeds["permissions"], json!(["camera", "clipboard-write"]));
    let wrapper = get(&r, "/apps/feeds", Some(&cookie)).await.text().await.unwrap();
    assert!(wrapper.contains(r#"allow="camera; clipboard-write""#), "{wrapper}");

    // what opens everything, or weakens the host's own scripts, never installs
    for bad in [
        json!(["*"]),
        json!(["https:"]),
        json!(["https://*.com"]),
        json!(["'unsafe-inline'"]),
        json!(["https://a.example.com;script-src *"]),
    ] {
        let url = publish_with_ui(&tmp.join("bad"), "bad", &json!({ "csp": { "resourceDomains": bad } }));
        let err = r.host.service.install_from_url(&url).await.unwrap_err().to_string();
        assert!(err.contains("csp.resourceDomains"), "{bad}: {err}");
    }
    let url = publish_with_ui(&tmp.join("bad2"), "bad2", &json!({ "permissions": { "nfc": {} } }));
    assert!(r.host.service.install_from_url(&url).await.is_err());
    // an entry the app ref does not carry is refused too
    let url = publish_with_ui(&tmp.join("bad3"), "bad3", &json!({ "entry": "missing.html" }));
    let err = r.host.service.install_from_url(&url).await.unwrap_err().to_string();
    assert!(err.contains("ui.entry"), "{err}");
    r.stop().await;
}

#[tokio::test]
async fn the_summary_tells_the_shell_which_windows_are_frames_and_what_they_ask_for() {
    let tmp = temp("summary");
    let r = host_with_apps(&tmp, &["tasks"]).await;
    let (_cookie, mut ws) = paired(&r).await;
    let list = rpc(&mut ws, 1, "app.list", json!({})).await;
    let tasks =
        list["result"]["apps"].as_array().unwrap().iter().find(|a| a["id"] == "tasks").unwrap().clone();
    assert_eq!(tasks["client"], true);
    assert_eq!(tasks["window"], json!({"w": 420, "h": 480, "minW": 320, "minH": 320}));
    assert_eq!(tasks["permissions"], json!([]), "an app that asks for nothing asks for no feature");
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

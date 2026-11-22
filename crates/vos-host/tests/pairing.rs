#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Device pairing, sessions and the Origin/Host guard, through the real wiring over real sockets.
//! Every client route needs a paired device, including from loopback; `/mcp` has its own credentials.

mod common;

use common::{connect_rpc, next_json, pair, start, Running};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use vos_domain::Grants;
use vos_host::Interfaces;

const LAN_IP: &str = "192.168.1.20";

fn http() -> reqwest::Client {
    reqwest::Client::new()
}

async fn post_pair(r: &Running, body: &Value) -> reqwest::Response {
    http().post(r.url("/pair")).json(body).send().await.unwrap()
}

async fn session(r: &Running, cookie: Option<&str>) -> Value {
    let mut req = http().get(r.url("/session"));
    if let Some(c) = cookie {
        req = req.header("cookie", c);
    }
    req.send().await.unwrap().json().await.unwrap()
}

async fn rpc(ws: &mut common::Socket, id: i64, method: &str, params: Value) -> Value {
    ws.send(Message::text(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()))
        .await
        .unwrap();
    loop {
        let m = next_json(ws).await;
        if m["id"] == id {
            return m;
        }
    }
}

fn local_agent() -> (String, vos_domain::Agent) {
    ("agent-token".into(), common::agent_with("local", "Local", Grants::allow_all()))
}

#[tokio::test]
async fn a_pairing_code_becomes_an_http_only_strict_session_cookie() {
    let r = start(|_| {}).await;
    let code = r.devices().issue_pairing_code().await.unwrap();

    let res = post_pair(&r, &json!({"code": code.code.to_lowercase(), "deviceName": "Safari on iPad"})).await;
    assert_eq!(res.status(), 200);
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_owned();
    assert!(cookie.starts_with("vos_session="), "{cookie}");
    for attr in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.contains(attr), "{attr} in {cookie}");
    }
    assert!(!cookie.contains("Secure"), "plain http: no Secure attribute");
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["device"]["name"], "Safari on iPad");

    let pair_cookie = cookie.split(';').next().unwrap();
    let who = session(&r, Some(pair_cookie)).await;
    assert_eq!(who["paired"], true);
    assert_eq!(who["device"]["name"], "Safari on iPad");
    assert_eq!(session(&r, None).await, json!({"paired": false}));

    // single use
    assert_eq!(post_pair(&r, &json!({"code": code.code})).await.status(), 401);
    r.stop().await;
}

#[tokio::test]
async fn the_device_is_named_from_its_user_agent_when_the_client_gives_no_name() {
    let r = start(|_| {}).await;
    let code = r.devices().issue_pairing_code().await.unwrap();
    let res = http()
        .post(r.url("/pair"))
        .header("user-agent", "Mozilla/5.0 (iPad; CPU OS 18_0 like Mac OS X) AppleWebKit/605 Safari/605")
        .json(&json!({"code": code.code}))
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["device"]["name"], "Safari on iPad");
    r.stop().await;
}

#[tokio::test]
async fn a_pairing_code_expires_after_ten_minutes() {
    let r = start(|_| {}).await;
    let stale = r.devices().issue_pairing_code().await.unwrap();
    r.clock.advance(9 * 60_000);
    let fresh = r.devices().issue_pairing_code().await.unwrap();
    r.clock.advance(2 * 60_000); // `stale` is 11 minutes old, `fresh` 2

    assert_eq!(post_pair(&r, &json!({"code": stale.code})).await.status(), 401);
    assert_eq!(post_pair(&r, &json!({"code": fresh.code})).await.status(), 200);
    r.stop().await;
}

#[tokio::test]
async fn wrong_codes_are_rate_limited_even_for_the_right_code_until_the_window_passes() {
    let r = start(|_| {}).await;
    let code = r.devices().issue_pairing_code().await.unwrap();
    for _ in 0..5 {
        assert_eq!(post_pair(&r, &json!({"code": "AAAA-AAAA"})).await.status(), 401);
    }
    let limited = post_pair(&r, &json!({"code": code.code})).await;
    assert_eq!(limited.status(), 429);
    let wait: u64 = limited.headers().get("retry-after").unwrap().to_str().unwrap().parse().unwrap();
    assert!((1..=60).contains(&wait));
    assert_eq!(session(&r, None).await["paired"], false, "limited means no session was issued");

    r.clock.advance(61_000);
    assert_eq!(post_pair(&r, &json!({"code": code.code})).await.status(), 200);
    r.stop().await;
}

#[tokio::test]
async fn malformed_pair_requests_are_refused() {
    let r = start(|_| {}).await;
    assert!(http().post(r.url("/pair")).body("not json").send().await.unwrap().status().is_client_error());
    assert!(post_pair(&r, &json!({"nope": 1})).await.status().is_client_error());
    let huge = json!({"code": "x".repeat(10_000)});
    assert_eq!(post_pair(&r, &huge).await.status(), 413, "the body is capped");
    r.stop().await;
}

#[tokio::test]
async fn unpaired_requests_are_refused_on_rpc_even_from_loopback() {
    let r = start(|_| {}).await; // the test client IS on loopback
    let err = connect_rpc(r.addr, None, None).await.unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
    let err = connect_rpc(r.addr, Some("vos_session=made-up"), None).await.unwrap_err();
    assert!(err.to_string().contains("401"), "a forged cookie is not a session: {err}");

    // the guard answers before the WebSocket upgrade is looked at: a plain request is told why
    let plain = http().get(r.url("/rpc")).send().await.unwrap();
    assert_eq!(plain.status(), 401);
    assert_eq!(plain.json::<Value>().await.unwrap()["error"], "pairing required");

    // once paired, the same request works
    let cookie = pair(&r, "Mac").await;
    assert!(connect_rpc(r.addr, Some(&cookie), None).await.is_ok());
    r.stop().await;
}

#[tokio::test]
async fn the_client_files_load_without_pairing_so_the_shell_can_show_its_pairing_screen() {
    let dir = std::env::temp_dir().join(format!("vos-client-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<h1>shell</h1>").unwrap();
    let r = start(|c| c.client_dir = Some(dir.clone())).await;

    assert!(http().get(r.url("/")).send().await.unwrap().text().await.unwrap().contains("shell"));
    assert!(http().get(r.url("/some/route")).send().await.unwrap().text().await.unwrap().contains("shell"));
    r.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn devices_are_listed_and_revoking_one_ends_its_session_and_its_socket() {
    let r = start(|_| {}).await;
    let (ipad, mac) = (pair(&r, "iPad").await, pair(&r, "Mac").await);
    let mut ipad_ws = connect_rpc(r.addr, Some(&ipad), None).await.unwrap();
    let mut mac_ws = connect_rpc(r.addr, Some(&mac), None).await.unwrap();

    let listed = rpc(&mut mac_ws, 1, "host.devices.list", json!({})).await;
    let devices = listed["result"]["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    let ipad_row = devices.iter().find(|d| d["name"] == "iPad").unwrap();
    let mac_row = devices.iter().find(|d| d["name"] == "Mac").unwrap();
    assert_eq!(mac_row["current"], true, "the caller's own device is marked");
    assert_eq!(ipad_row["current"], false);
    assert!(ipad_row["createdAt"].is_u64() && ipad_row["lastSeenAt"].is_u64());

    // the Mac revokes the iPad
    let revoked = rpc(&mut mac_ws, 2, "host.devices.revoke", json!({"id": ipad_row["id"]})).await;
    assert!(revoked.get("error").is_none(), "{revoked}");
    // the iPad's socket is closed by the host ...
    let end = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ipad_ws.next().await {
                None | Some(Err(_) | Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(end.is_ok(), "the revoked device's socket was hung up on");
    // ... its cookie no longer works anywhere ...
    assert_eq!(session(&r, Some(&ipad)).await["paired"], false);
    let err = connect_rpc(r.addr, Some(&ipad), None).await.unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
    // ... and the Mac is untouched
    assert_eq!(session(&r, Some(&mac)).await["paired"], true);
    let again = rpc(&mut mac_ws, 3, "host.devices.revoke", json!({"id": ipad_row["id"]})).await;
    assert_eq!(again["error"]["code"], -32002, "revoking an unknown device is NotFound: {again}");
    let bad = rpc(&mut mac_ws, 4, "host.devices.revoke", json!({})).await;
    assert_eq!(bad["error"]["code"], -32602);
    r.stop().await;
}

#[tokio::test]
async fn a_device_can_revoke_itself_and_still_gets_its_answer() {
    let r = start(|_| {}).await;
    let cookie = pair(&r, "Mac").await;
    let mut ws = connect_rpc(r.addr, Some(&cookie), None).await.unwrap();
    let me = rpc(&mut ws, 1, "host.devices.list", json!({})).await["result"]["devices"][0]["id"].clone();
    let reply = rpc(&mut ws, 2, "host.devices.revoke", json!({"id": me})).await;
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(session(&r, Some(&cookie)).await["paired"], false);
    r.stop().await;
}

fn lan_host(c: &mut vos_host::HostConfig) {
    c.interfaces =
        Some(Interfaces { ips: vec![LAN_IP.parse().unwrap()], hostname: Some("studio-mac".into()) });
}

#[tokio::test]
async fn the_origin_and_host_guard_accepts_the_hosts_own_lan_address_and_names() {
    let r = start(lan_host).await;
    let port = r.addr.port();
    let get = |host: String, origin: Option<String>| {
        let mut req = http().get(r.url("/session")).header("host", host);
        if let Some(o) = origin {
            req = req.header("origin", o);
        }
        async move { req.send().await.unwrap().status() }
    };
    // a browser on another device opens the PWA by the LAN address, the hostname, or a `.local` name
    assert_eq!(get(format!("{LAN_IP}:{port}"), Some(format!("http://{LAN_IP}:{port}"))).await, 200);
    assert_eq!(get(format!("studio-mac:{port}"), Some(format!("http://studio-mac:{port}"))).await, 200);
    assert_eq!(
        get(format!("studio-mac.local:{port}"), Some(format!("http://studio-mac.local:{port}"))).await,
        200
    );
    assert_eq!(get(format!("127.0.0.1:{port}"), None).await, 200);

    // a foreign page cannot ride on a good Host, and a rebinding Host is refused outright
    assert_eq!(get(format!("{LAN_IP}:{port}"), Some("https://evil.example".into())).await, 403);
    assert_eq!(get(format!("{LAN_IP}:{port}"), Some(format!("http://192.168.1.99:{port}"))).await, 403);
    assert_eq!(get(format!("evil.example:{port}"), Some(format!("http://evil.example:{port}"))).await, 403);
    assert_eq!(get(format!("192.168.1.99:{port}"), None).await, 403, "an address that is not this machine's");
    r.stop().await;
}

#[tokio::test]
async fn a_paired_session_does_not_open_a_websocket_for_a_foreign_origin() {
    let r = start(lan_host).await;
    let cookie = pair(&r, "Mac").await;
    let ok = connect_rpc(r.addr, Some(&cookie), Some(&format!("http://127.0.0.1:{}", r.addr.port()))).await;
    assert!(ok.is_ok());
    let err = connect_rpc(r.addr, Some(&cookie), Some("https://evil.example")).await.unwrap_err();
    assert!(err.to_string().contains("403"), "cross-site WebSocket hijacking: {err}");
    r.stop().await;
}

async fn mcp_initialize(r: &Running, headers: &[(&str, &str)]) -> reqwest::StatusCode {
    let mut req = http()
        .post(r.url("/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"0"}}});
    req.json(&init).send().await.unwrap().status()
}

#[tokio::test]
async fn the_mcp_token_and_the_session_are_different_credentials() {
    let r = start(|c| c.agents = vec![local_agent()]).await;
    let cookie = pair(&r, "Mac").await;
    let session_token = cookie.strip_prefix("vos_session=").unwrap().to_owned();

    // /mcp: the agent's bearer token works; a paired session (cookie or as a bearer) does not
    assert_eq!(mcp_initialize(&r, &[]).await, 401, "no credential");
    assert_eq!(mcp_initialize(&r, &[("cookie", &cookie)]).await, 401, "a paired browser is not an agent");
    assert_eq!(mcp_initialize(&r, &[("authorization", &format!("Bearer {session_token}"))]).await, 401);
    assert_eq!(mcp_initialize(&r, &[("authorization", "Bearer agent-token")]).await, 200);

    // /rpc: the MCP token is not a session, as a cookie or as a bearer
    let as_cookie = "vos_session=agent-token";
    assert!(connect_rpc(r.addr, Some(as_cookie), None).await.is_err());
    let as_bearer =
        http().get(r.url("/rpc")).header("authorization", "Bearer agent-token").send().await.unwrap();
    assert_eq!(as_bearer.status(), 401, "a bearer header never authenticates a client route");
    assert_eq!(session(&r, None).await["paired"], false);
    r.stop().await;
}

#[tokio::test]
async fn mcp_is_behind_the_origin_and_host_guard_too() {
    let r = start(|c| {
        c.agents = vec![local_agent()];
        lan_host(c);
    })
    .await;
    let port = r.addr.port();
    let auth = ("authorization", "Bearer agent-token");
    // an agent reaching us by our LAN address is fine (rmcp's own loopback-only list is off) ...
    let lan = format!("{LAN_IP}:{port}");
    assert_eq!(mcp_initialize(&r, &[auth, ("host", &lan)]).await, 200);
    // ... a rebinding Host or a browser's foreign Origin is not, token or no token
    let evil = format!("evil.example:{port}");
    assert_eq!(mcp_initialize(&r, &[auth, ("host", &evil)]).await, 403);
    assert_eq!(mcp_initialize(&r, &[auth, ("origin", "https://evil.example")]).await, 403);
    r.stop().await;
}

/// Self-signed TLS files for `localhost` in a directory of their own.
fn tls_files(tag: &str) -> (vos_host::config::TlsFiles, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("vos-tls-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()]).unwrap();
    let files = vos_host::config::TlsFiles { cert: dir.join("cert.pem"), key: dir.join("key.pem") };
    std::fs::write(&files.cert, cert.cert.pem()).unwrap();
    std::fs::write(&files.key, cert.signing_key.serialize_pem()).unwrap();
    (files, dir)
}

#[tokio::test]
async fn over_tls_with_http2_the_authority_stands_in_for_the_host_header() {
    // a browser over ALPN h2 sends `:authority` and no `Host`; the guard used to refuse every such request
    let (files, dir) = tls_files("h2");
    let r = common::start_with_tls(|c| c.secure_cookies = true, Some(files)).await;
    let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let res = client.get(format!("https://{}/pair", r.addr)).send().await.unwrap();
    assert_eq!(res.version(), reqwest::Version::HTTP_2, "ALPN negotiated h2");
    assert_ne!(res.status(), 403, "an h2 request with no Host header is the browser's normal request");
    // a rebinding authority is still refused over h2 (the name is not ours)
    let evil = client
        .get(format!("https://localhost:{}/pair", r.addr.port()))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(evil.status(), 403, "an authority and a Host that differ are refused");
    r.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn over_tls_the_session_cookie_is_secure() {
    let (files, dir) = tls_files("cookie");
    let r = common::start_with_tls(|c| c.secure_cookies = true, Some(files)).await;
    let code = r.devices().issue_pairing_code().await.unwrap();
    let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let res = client
        .post(format!("https://{}/pair", r.addr))
        .json(&json!({"code": code.code, "deviceName": "Mac"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let cookie = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_owned();
    assert!(cookie.ends_with("; Secure") && cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.starts_with("__Host-vos_session="), "the __Host- prefix with TLS: {cookie}");
    // the browser sends it back under that name, and the host honours it (and only it)
    let sent = cookie.split(';').next().unwrap().to_owned();
    let paired =
        client.get(format!("https://{}/session", r.addr)).header("cookie", &sent).send().await.unwrap();
    assert_eq!(paired.json::<serde_json::Value>().await.unwrap()["paired"], true);
    let value = sent.split_once('=').unwrap().1;
    let tossed = client
        .get(format!("https://{}/session", r.addr))
        .header("cookie", format!("vos_session={value}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        tossed.json::<serde_json::Value>().await.unwrap()["paired"],
        false,
        "unprefixed name over TLS"
    );
    r.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use vos_domain::protocol::{RpcError, RpcMessage};
use vos_http::{origin_guard, rpc_router, static_router, HostGuard, RpcHandler, RpcPeer};

struct H;
#[async_trait]
impl RpcHandler for H {
    async fn request(&self, peer: &RpcPeer, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "ping" => Ok(json!({"pong": params})),
            // a bug in the host's own code, on the ordered path and on the concurrent one
            "view.explode" | "app.explode" => panic!("the handler broke"),
            // the first of a burst is the slowest: unordered handling would answer it last
            "view.event" | "app.slow" => {
                let n = params["n"].as_u64().unwrap_or(0);
                tokio::time::sleep(Duration::from_millis(60 / (n + 1))).await;
                Ok(json!({"n": n}))
            }
            // server -> client request inside a client request (bidirectional on ONE socket)
            "ask_client" => {
                let r = peer
                    .request("client/echo", json!({"q": 1}), Duration::from_secs(2))
                    .await
                    .map_err(|e| RpcError { code: -32000, message: e.to_string(), data: None })?;
                Ok(json!({"client_said": r}))
            }
            _ => Err(RpcError { code: -32601, message: "method not found".into(), data: None }),
        }
    }
    async fn notification(&self, peer: &RpcPeer, method: &str, _p: Value) {
        if method == "hello" {
            let _ = peer.notify("server/welcome", json!({"n": 1}));
        }
        if method == "explode" {
            panic!("the notification handler broke");
        }
    }
}

async fn start(dir: &std::path::Path) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let allowed = Arc::new(HostGuard::with_interfaces(addr.port(), Arc::new(Vec::new), None));
    let app = rpc_router(Arc::new(H))
        .merge(static_router(dir))
        .layer(axum::middleware::from_fn_with_state(allowed, origin_guard));
    (addr, tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }))
}

fn req(
    addr: std::net::SocketAddr,
    origin: Option<&str>,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut r = format!("ws://{addr}/rpc").into_client_request().unwrap();
    if let Some(o) = origin {
        r.headers_mut().insert("origin", o.parse().unwrap());
    }
    r
}

#[tokio::test]
async fn rpc_bidirectional_requests_and_notifications_on_one_socket() {
    let dir = std::env::temp_dir().join(format!("static-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<h1>client</h1>").unwrap();
    let (addr, server) = start(&dir).await;

    let (mut ws, _) =
        connect_async(req(addr, Some(&format!("http://127.0.0.1:{}", addr.port())))).await.unwrap();
    let send = |m: RpcMessage| serde_json::to_string(&m).unwrap();
    ws.send(Message::text(send(RpcMessage::request(1, "ping", json!([1, 2]))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::request(2, "ask_client", json!({}))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::notification("hello", json!({}))))).await.unwrap();

    let (mut got_ping, mut got_welcome, mut got_final) = (false, false, false);
    while !(got_ping && got_welcome && got_final) {
        let Message::Text(t) =
            tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap()
        else {
            continue;
        };
        match serde_json::from_str::<RpcMessage>(&t).unwrap() {
            RpcMessage::Response { id: vos_domain::protocol::RpcId::Num(1), result, .. } => {
                assert_eq!(result, json!({"pong":[1,2]}));
                got_ping = true;
            }
            RpcMessage::Request { id, method, .. } => {
                // the server is asking US something
                assert_eq!(method, "client/echo");
                ws.send(Message::text(send(RpcMessage::ok(id, json!("hi from client"))))).await.unwrap();
            }
            RpcMessage::Response { result, .. } => {
                assert_eq!(result, json!({"client_said":"hi from client"}));
                got_final = true;
            }
            RpcMessage::Notification { method, .. } => {
                assert_eq!(method, "server/welcome");
                got_welcome = true;
            }
            RpcMessage::Error { .. } => panic!("unexpected error"),
        }
    }
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// A request whose handler panics gets an internal error, and the connection serves on: the next `view.event`
/// (the ordered task) and the next request after a panicking notification are answered.
#[tokio::test]
async fn a_handler_that_panics_answers_an_internal_error_and_the_connection_serves_on() {
    let dir = std::env::temp_dir().join(format!("static3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (addr, server) = start(&dir).await;
    let (mut ws, _) =
        connect_async(req(addr, Some(&format!("http://127.0.0.1:{}", addr.port())))).await.unwrap();
    let send = |m: RpcMessage| serde_json::to_string(&m).unwrap();
    ws.send(Message::text(send(RpcMessage::request(1, "view.explode", json!({}))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::request(2, "view.event", json!({"n": 0}))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::request(3, "app.explode", json!({}))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::notification("explode", json!({}))))).await.unwrap();
    ws.send(Message::text(send(RpcMessage::request(4, "ping", json!(1))))).await.unwrap();

    let mut answered = std::collections::BTreeMap::new();
    while answered.len() < 4 {
        let Message::Text(t) =
            tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap()
        else {
            continue;
        };
        match serde_json::from_str::<RpcMessage>(&t).unwrap() {
            RpcMessage::Response { id: vos_domain::protocol::RpcId::Num(n), result, .. } => {
                answered.insert(n, Ok(result));
            }
            RpcMessage::Error { id: Some(vos_domain::protocol::RpcId::Num(n)), error, .. } => {
                answered.insert(n, Err((error.code, error.message)));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    let internal =
        |method: &str| Err((-32603, format!("internal error: the host panicked answering `{method}`")));
    assert_eq!(answered[&1], internal("view.explode"));
    assert_eq!(answered[&2], Ok(json!({"n": 0})), "the ordered task lived on");
    assert_eq!(answered[&3], internal("app.explode"));
    assert_eq!(answered[&4], Ok(json!({"pong": 1})), "the connection read on after the notification");
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn origin_check_blocks_foreign_pages_and_static_serves_the_client() {
    let dir = std::env::temp_dir().join(format!("static2-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<h1>client</h1>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    let (addr, server) = start(&dir).await;

    // a page on another origin cannot open the socket
    let err = connect_async(req(addr, Some("https://evil.example"))).await.unwrap_err();
    assert!(err.to_string().contains("403"), "{err}");
    // no Origin (native client) is allowed by this config
    assert!(connect_async(req(addr, None)).await.is_ok());

    let c = reqwest_get(addr, "/").await;
    assert!(c.contains("<h1>client</h1>"));
    assert_eq!(reqwest_get(addr, "/assets/app.js").await, "console.log(1)");
    assert!(reqwest_get(addr, "/some/spa/route").await.contains("<h1>client</h1>"), "SPA fallback");
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

/// minimal HTTP/1.1 GET without pulling reqwest into this crate's dev-deps
async fn reqwest_get(addr: std::net::SocketAddr, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_owned()
}

#[tokio::test]
async fn view_requests_are_handled_in_the_order_they_were_sent_and_others_are_not_held_up() {
    let dir = std::env::temp_dir().join(format!("static-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "x").unwrap();
    let (addr, server) = start(&dir).await;
    let (mut ws, _) =
        connect_async(req(addr, Some(&format!("http://127.0.0.1:{}", addr.port())))).await.unwrap();
    let send = |m: RpcMessage| serde_json::to_string(&m).unwrap();
    for n in 0..5u64 {
        ws.send(Message::text(send(RpcMessage::request(
            i64::try_from(n).unwrap(),
            "view.event",
            json!({"n": n}),
        ))))
        .await
        .unwrap();
    }
    let mut order = vec![];
    while order.len() < 5 {
        let Message::Text(t) =
            tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap()
        else {
            continue;
        };
        if let RpcMessage::Response { result, .. } = serde_json::from_str::<RpcMessage>(&t).unwrap() {
            order.push(result["n"].as_u64().unwrap());
        }
    }
    assert_eq!(order, [0, 1, 2, 3, 4], "keystrokes must not overtake one another");

    // the same burst of a non-view method is concurrent: the fast ones answer first
    for n in 0..3u64 {
        ws.send(Message::text(send(RpcMessage::request(
            10 + i64::try_from(n).unwrap(),
            "app.slow",
            json!({"n": n}),
        ))))
        .await
        .unwrap();
    }
    let mut order = vec![];
    while order.len() < 3 {
        let Message::Text(t) =
            tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap()
        else {
            continue;
        };
        if let RpcMessage::Response { result, .. } = serde_json::from_str::<RpcMessage>(&t).unwrap() {
            order.push(result["n"].as_u64().unwrap());
        }
    }
    assert_eq!(order, [2, 1, 0], "app management is not serialised");
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
}

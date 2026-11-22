#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What the hub pushes to every connected client, through a real `/rpc` socket.

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use vos_application::ports::Notifier;
use vos_domain::{
    protocol::RpcMessage,
    protocol::{AppNotification, HostNotification},
    AppId, Notification,
};
use vos_http::{rpc_router, RpcHub};

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A hub with nothing bound behind it, serving `/rpc` on an ephemeral loopback port.
async fn start() -> (Arc<RpcHub>, std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let hub = Arc::new(RpcHub::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = rpc_router(hub.clone());
    (hub, addr, tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }))
}

/// A client that the hub has registered: any answer to a request means `connected` has run. The answer is the
/// first frame it gets, so nothing was sent to it before it asked (a replayed notification would come first).
async fn connect(addr: std::net::SocketAddr) -> Ws {
    let (mut ws, _) = connect_async(format!("ws://{addr}/rpc").into_client_request().unwrap()).await.unwrap();
    let probe = RpcMessage::request(1, "nope", json!([]));
    ws.send(Message::text(serde_json::to_string(&probe).unwrap())).await.unwrap();
    let first = next(&mut ws).await;
    assert!(
        matches!(first, RpcMessage::Error { .. }),
        "the first frame is the probe's answer, got {first:?}"
    );
    ws
}

fn note(title: &str) -> Notification {
    Notification { app: AppId::parse("notes").unwrap(), title: title.into(), body: None }
}

async fn delivered_title(ws: &mut Ws) -> String {
    let RpcMessage::Notification { method, params, .. } = next(ws).await else {
        panic!("expected a notification")
    };
    assert_eq!(method, "notification.deliver");
    params["title"].as_str().unwrap().to_owned()
}

async fn next(ws: &mut Ws) -> RpcMessage {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), ws.next()).await.unwrap().unwrap().unwrap();
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// An app's notification reaches every connected client as the typed `notification.deliver`: the sender is the
/// host's id for the app, and a notification without a body carries no `body` key.
#[tokio::test]
async fn an_app_notification_reaches_every_client_as_notification_deliver() {
    let (hub, addr, server) = start().await;
    let (mut first, mut second) = (connect(addr).await, connect(addr).await);

    let notes = AppId::parse("notes").unwrap();
    hub.notify(Notification { app: notes.clone(), title: "Reminder".into(), body: Some("Call mum".into()) })
        .await
        .unwrap();
    hub.notify(Notification { app: notes, title: "Done".into(), body: None }).await.unwrap();

    for ws in [&mut first, &mut second] {
        let mut got = Vec::new();
        for _ in 0..2 {
            let RpcMessage::Notification { method, params, .. } = next(ws).await else {
                panic!("expected a notification")
            };
            assert_eq!(method, "notification.deliver");
            got.push((params.clone(), HostNotification::from_rpc(&method, params).unwrap()));
        }
        assert_eq!(
            got[0].1,
            HostNotification::Notify(AppNotification {
                app: "notes".into(),
                title: "Reminder".into(),
                body: Some("Call mum".into()),
            })
        );
        assert_eq!(got[1].0, json!({ "app": "notes", "title": "Done" }));
    }
    server.abort();
}

/// Nothing is queued: a notification sent while no client is connected, or before a client connects, never
/// reaches it. The client that connects later gets only what is sent after.
#[tokio::test]
async fn a_client_that_connects_later_gets_only_what_is_sent_after() {
    let (hub, addr, server) = start().await;
    hub.notify(note("nobody is listening")).await.unwrap();

    let mut early = connect(addr).await;
    hub.notify(note("before")).await.unwrap();
    assert_eq!(delivered_title(&mut early).await, "before");

    let mut late = connect(addr).await;
    hub.notify(note("after")).await.unwrap();
    assert_eq!(delivered_title(&mut late).await, "after");
    assert_eq!(delivered_title(&mut early).await, "after");
    server.abort();
}

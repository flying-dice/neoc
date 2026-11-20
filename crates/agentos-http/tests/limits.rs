#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What one `/rpc` connection may do: how many requests it may have in flight, and the id it is given.

use agentos_domain::protocol::{RpcError, RpcId, RpcMessage};
use agentos_http::{rpc_router, RpcHandler, RpcPeer, BUSY, MAX_IN_FLIGHT};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// `block` waits for the test to open the gate; `ping` answers at once.
struct Gate {
    open: Semaphore,
    connections: Mutex<Vec<u64>>,
}

#[async_trait]
impl RpcHandler for Gate {
    async fn request(&self, _peer: &RpcPeer, method: &str, _params: Value) -> Result<Value, RpcError> {
        match method {
            "block" | "view.block" => {
                let _ = self.open.acquire().await.unwrap();
                Ok(json!("released"))
            }
            "ping" => Ok(json!("pong")),
            _ => Err(RpcError { code: -32601, message: "no".into(), data: None }),
        }
    }
    async fn connected(&self, peer: RpcPeer) {
        self.connections.lock().push(peer.connection_id());
    }
}

async fn start(gate: Arc<Gate>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = rpc_router(gate);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn next(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> RpcMessage {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("a frame").unwrap();
        if let Message::Text(t) = frame.unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

fn send(m: &RpcMessage) -> Message {
    Message::text(serde_json::to_string(m).unwrap())
}

#[tokio::test]
async fn a_connection_over_its_in_flight_limit_gets_an_error_at_once_and_the_rest_still_answer() {
    let gate = Arc::new(Gate { open: Semaphore::new(0), connections: Mutex::default() });
    let addr = start(gate.clone()).await;
    let (mut ws, _) = connect_async(format!("ws://{addr}/rpc")).await.unwrap();
    let extra = 5;
    for id in 1..=(MAX_IN_FLIGHT + extra) {
        let method = if id % 2 == 0 { "view.block" } else { "block" }; // ordered and unordered both count
        ws.send(send(&RpcMessage::request(i64::try_from(id).unwrap(), method, json!({})))).await.unwrap();
    }
    // the ones over the limit are refused immediately, while every permit is still held
    let mut refused = vec![];
    for _ in 0..extra {
        let RpcMessage::Error { id: Some(RpcId::Num(id)), error, .. } = next(&mut ws).await else {
            panic!("expected a refusal")
        };
        assert_eq!(error.code, BUSY);
        refused.push(id);
    }
    refused.sort_unstable();
    let limit = i64::try_from(MAX_IN_FLIGHT).unwrap();
    assert_eq!(refused, ((limit + 1)..=(limit + 5)).collect::<Vec<_>>(), "the late ones");

    // open the gate: everything that was accepted is answered, ordered or not
    gate.open.add_permits(MAX_IN_FLIGHT + 1);
    let mut answered = 0;
    while answered < MAX_IN_FLIGHT {
        if let RpcMessage::Response { result, .. } = next(&mut ws).await {
            assert_eq!(result, json!("released"));
            answered += 1;
        }
    }
    // and permits are given back: the connection works again
    ws.send(send(&RpcMessage::request(1000, "ping", json!({})))).await.unwrap();
    let RpcMessage::Response { result, .. } = next(&mut ws).await else { panic!("a pong") };
    assert_eq!(result, json!("pong"));

    // another connection was never affected
    let (mut other, _) = connect_async(format!("ws://{addr}/rpc")).await.unwrap();
    other.send(send(&RpcMessage::request(1, "ping", json!({})))).await.unwrap();
    assert!(matches!(next(&mut other).await, RpcMessage::Response { .. }));
}

#[tokio::test]
async fn connection_ids_are_never_reused_even_after_earlier_connections_closed() {
    let gate = Arc::new(Gate { open: Semaphore::new(0), connections: Mutex::default() });
    let addr = start(gate.clone()).await;
    for _ in 0..20 {
        let (mut ws, _) = connect_async(format!("ws://{addr}/rpc")).await.unwrap();
        ws.send(send(&RpcMessage::request(1, "ping", json!({})))).await.unwrap();
        let _ = next(&mut ws).await;
        ws.close(None).await.unwrap();
        drop(ws);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let ids = gate.connections.lock().clone();
    assert_eq!(ids.len(), 20);
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "strictly increasing, so never reused: {ids:?}");
}

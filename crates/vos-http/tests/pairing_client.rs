#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! `POST /pair` counts attempts against the address the request came from: one noisy client is limited, the
//! others still pair.

use axum::{
    extract::{ConnectInfo, Request},
    middleware::{from_fn, Next},
    response::Response,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vos_application::{
    testing::{ManualClock, MemDevices, SeededRandom},
    DeviceService, PairingPolicy,
};
use vos_http::{session_router, SessionConfig};

/// The test's stand-in for `into_make_service_with_connect_info`: the peer address is the `x-peer` header.
async fn peer_from_header(mut req: Request, next: Next) -> Response {
    let peer: SocketAddr = req.headers().get("x-peer").unwrap().to_str().unwrap().parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    next.run(req).await
}

async fn pair(addr: SocketAddr, peer: &str, code: &str) -> u16 {
    let body = format!(r#"{{"code":"{code}"}}"#);
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(
        format!(
            "POST /pair HTTP/1.1\r\nHost: {addr}\r\nx-peer: {peer}:50000\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out.split(' ').nth(1).unwrap().parse().unwrap()
}

#[tokio::test]
async fn one_client_address_is_rate_limited_without_stalling_the_others() {
    let devices = Arc::new(DeviceService::new(
        Arc::new(MemDevices::default()),
        Arc::new(ManualClock::new(1_000_000)),
        Arc::new(SeededRandom::default()),
        PairingPolicy::default(),
    ));
    let code = devices.issue_pairing_code().await.unwrap().code;
    let app =
        session_router(SessionConfig { devices, secure_cookies: false }).layer(from_fn(peer_from_header));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for _ in 0..5 {
        assert_eq!(pair(addr, "192.168.1.50", "AAAA-AAAA").await, 401);
    }
    assert_eq!(pair(addr, "192.168.1.50", "AAAA-AAAA").await, 429, "the noisy client is limited");
    assert_eq!(pair(addr, "192.168.1.50", &code).await, 429, "even with the right code");
    assert_eq!(pair(addr, "192.168.1.60", &code).await, 200, "another client pairs at once");
}

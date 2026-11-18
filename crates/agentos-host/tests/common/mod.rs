#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the host's integration tests: start the REAL wiring on an ephemeral loopback port and
//! talk to it like a browser or an agent would.

use agentos_application::{ports::Clock, testing::ManualClock, DeviceService};
use agentos_host::{build, serve::serve, HostConfig};
use futures_util::StreamExt;
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use tokio_util::sync::CancellationToken;

pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Running {
    pub addr: SocketAddr,
    pub host: agentos_host::Host,
    pub clock: Arc<ManualClock>,
    shutdown: CancellationToken,
    task: JoinHandle<anyhow::Result<()>>,
}

impl Running {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
    pub fn devices(&self) -> &Arc<DeviceService> {
        &self.host.devices
    }
    /// Stop serving and wait for the server to drain.
    pub async fn stop(self) {
        self.shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.task).await;
    }
}

/// Start the host on `127.0.0.1:0`. `configure` may change anything before it is built.
pub async fn start(configure: impl FnOnce(&mut HostConfig)) -> Running {
    start_with_tls(configure, None).await
}

#[allow(clippy::unused_async)] // async so call sites read `start().await` and always run inside a runtime
pub async fn start_with_tls(
    configure: impl FnOnce(&mut HostConfig),
    tls: Option<agentos_host::config::TlsFiles>,
) -> Running {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let clock = Arc::new(ManualClock::new(now_ms()));
    let mut cfg = HostConfig::new(addr.port());
    cfg.clock = clock.clone() as Arc<dyn Clock>;
    configure(&mut cfg);
    let host = build(cfg).unwrap();
    let shutdown = host.shutdown.clone();
    let router = host.router.clone();
    let token = shutdown.clone();
    let task = tokio::spawn(async move { serve(listener, router, tls.as_ref(), token).await });
    Running { addr, host, clock, shutdown, task }
}

fn now_ms() -> u64 {
    u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis())
        .unwrap()
}

/// Issue a pairing code at the host and exchange it over HTTP like a browser would; returns the
/// `Cookie` header value (`agentos_session=...`) of the new session.
pub async fn pair(r: &Running, device_name: &str) -> String {
    let code = r.devices().issue_pairing_code().await.unwrap();
    let res = reqwest::Client::new()
        .post(r.url("/pair"))
        .json(&serde_json::json!({ "code": code.code, "deviceName": device_name }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "{:?}", res.text().await);
    let set = res.headers().get("set-cookie").unwrap().to_str().unwrap().to_owned();
    set.split(';').next().unwrap().to_owned()
}

/// Open `/rpc`. `cookie` is the session cookie; the Origin is the host's own.
pub async fn connect_rpc(
    addr: SocketAddr,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut req = format!("ws://{addr}/rpc").into_client_request().unwrap();
    if let Some(c) = cookie {
        req.headers_mut().insert("cookie", c.parse().unwrap());
    }
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    connect_async(req).await.map(|(ws, _)| ws)
}

pub async fn next_json(ws: &mut Socket) -> Value {
    loop {
        if let Message::Text(t) = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("frame in 5s")
            .unwrap()
            .unwrap()
        {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the host's integration tests: start the REAL wiring on an ephemeral loopback port and
//! talk to it like a browser or an agent would.

use agentos_application::{ports::Clock, testing::ManualClock, DeviceService};
use agentos_host::{build, serve::serve, HostConfig};
use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use tokio_util::sync::CancellationToken;

use agentos_domain::{Agent as Contact, AgentId, AgentRuntime, ApprovalPolicy, Grants, Permission};
use rmcp::{
    model::CallToolRequestParams,
    service::{NotificationContext, RunningService},
    transport::{streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport},
    ClientHandler, RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

pub mod agents;

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
    let host = build(cfg).await.unwrap();
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

// ---- the clients: an agent over MCP, a browser over /rpc ----------------------------------------------

#[derive(Clone, Default)]
pub struct Counting(pub Arc<AtomicUsize>);
impl ClientHandler for Counting {
    async fn on_tool_list_changed(&self, _: NotificationContext<RoleClient>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pub type Agent = RunningService<RoleClient, Counting>;

pub async fn connect_agent(addr: std::net::SocketAddr, token: &str) -> (Agent, Counting) {
    let counter = Counting::default();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")).auth_header(token),
    );
    (counter.clone().serve(transport).await.unwrap(), counter)
}

pub async fn tool_names(agent: &Agent) -> Vec<String> {
    agent.peer().list_tools(None).await.unwrap().tools.iter().map(|t| t.name.to_string()).collect()
}

pub async fn call(agent: &Agent, tool: &str, args: Value) -> rmcp::model::CallToolResult {
    let args = args.as_object().unwrap().clone();
    agent.peer().call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(args)).await.unwrap()
}

pub fn text_of(out: &rmcp::model::CallToolResult) -> String {
    format!("{:?}", out.content)
}

pub async fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// A test agent with a fixed MCP token. `grants` default to everything: tests that care say otherwise.
pub fn agent_with(id: &str, name: &str, grants: Grants) -> Contact {
    Contact {
        id: AgentId(id.into()),
        name: name.into(),
        avatar: None,
        role: String::new(),
        runtime: AgentRuntime::default(),
        grants,
        approval: ApprovalPolicy::default(),
    }
}

pub fn claude() -> (String, Contact) {
    ("tok-claude".into(), agent_with("claude", "Claude", Grants::allow_all()))
}

/// WS client that keeps notifications that arrive while it waits for a response (they can overtake it).
pub struct Ws {
    pub raw: Socket,
    pub backlog: Vec<Value>,
}
pub async fn rpc(ws: &mut Ws, id: i64, method: &str, params: Value) -> Value {
    ws.raw
        .send(Message::text(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()))
        .await
        .unwrap();
    loop {
        let m = next_json(&mut ws.raw).await;
        if m["id"] == id {
            return m;
        }
        ws.backlog.push(m);
    }
}
pub async fn next_notification(ws: &mut Ws, method: &str) -> Value {
    if let Some(i) = ws.backlog.iter().position(|m| m["method"] == method) {
        return ws.backlog.remove(i)["params"].clone();
    }
    loop {
        let m = next_json(&mut ws.raw).await;
        if m["method"] == method {
            return m["params"].clone();
        }
        ws.backlog.push(m);
    }
}

pub async fn browser(r: &Running) -> Ws {
    let cookie = pair(r, "Safari on iPad").await;
    let origin = format!("http://127.0.0.1:{}", r.addr.port());
    Ws { raw: connect_rpc(r.addr, Some(&cookie), Some(&origin)).await.unwrap(), backlog: vec![] }
}

pub fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("host-{name}-{}", std::process::id()))
}

// ---- packages and view helpers ----------------------------------------------------------------------------

/// Publish an app package (`files` by path) under `root` as an app ref; returns the `file://` URL of the ref.
pub fn publish_package(
    root: &std::path::Path,
    id: &str,
    main: &str,
    files: &[(&str, Vec<u8>)],
    grants: Vec<Permission>,
) -> String {
    use agentos_domain::{AppId, AppRef, FileRef, Manifest};
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(root).unwrap();
    let mut refs = vec![];
    for (path, bytes) in files {
        std::fs::write(root.join(path), bytes).unwrap();
        refs.push(FileRef {
            path: (*path).into(),
            url: format!("file://{}/{path}", root.display()),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        });
    }
    let r = AppRef {
        manifest: Manifest {
            id: AppId::parse(id).unwrap(),
            name: id.into(),
            version: "1.0.0".into(),
            main: main.into(),
            permissions: grants,
            migrations: vec![],
            icon: None,
            tint: None,
            description: None,
            requires: None,
        },
        files: refs,
    };
    std::fs::write(root.join("app-ref.json"), serde_json::to_vec(&r).unwrap()).unwrap();
    format!("file://{}/app-ref.json", root.display())
}

/// The path of the node with this key in a wire tree (depth first, `@slot` segments for named slots).
pub fn path_of(tree: &Value, key: &str) -> Option<String> {
    fn walk(node: &Value, path: &str, key: &str) -> Option<String> {
        if node["key"] == key {
            return Some(path.to_owned());
        }
        let children = node["children"].as_array().into_iter().flatten().map(|c| (None, c));
        let slots = node["slots"].as_object().into_iter().flatten().flat_map(|(name, list)| {
            list.as_array().into_iter().flatten().map(move |c| (Some(name.as_str()), c))
        });
        for (slot, child) in children.chain(slots) {
            let k = child["key"].as_str().unwrap_or_default();
            let p = match slot {
                Some(slot) => format!("{path}/@{slot}/{k}"),
                None => format!("{path}/{k}"),
            };
            if let Some(found) = walk(child, &p, key) {
                return Some(found);
            }
        }
        None
    }
    walk(tree, tree["key"].as_str()?, key)
}

/// Send a notification (no id, no answer).
pub async fn notify(ws: &mut Ws, method: &str, params: Value) {
    ws.raw
        .send(Message::text(json!({"jsonrpc":"2.0","method":method,"params":params}).to_string()))
        .await
        .unwrap();
}

/// Wait for a notification of `method` whose params satisfy `ok`; others stay in the backlog, in order.
pub async fn notification_where(ws: &mut Ws, method: &str, ok: impl Fn(&Value) -> bool) -> Value {
    if let Some(i) = ws.backlog.iter().position(|m| m["method"] == method && ok(&m["params"])) {
        return ws.backlog.remove(i)["params"].clone();
    }
    loop {
        let m = next_json(&mut ws.raw).await;
        if m["method"] == method && ok(&m["params"]) {
            return m["params"].clone();
        }
        ws.backlog.push(m);
    }
}

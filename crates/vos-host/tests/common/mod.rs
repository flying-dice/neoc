#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Shared by the host's integration tests: start the REAL wiring on an ephemeral loopback port and
//! talk to it like a browser or an agent would.

use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use tokio_util::sync::CancellationToken;
use vos_application::{ports::Clock, testing::ManualClock, DeviceService};
use vos_host::{build, serve::serve, HostConfig};

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
use vos_domain::{Agent as Contact, AgentId, Grants};

pub mod agents;

pub type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Running {
    pub addr: SocketAddr,
    pub host: vos_host::Host,
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
    tls: Option<vos_host::config::TlsFiles>,
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
/// `Cookie` header value (`vos_session=...`) of the new session.
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
    Contact { id: AgentId(id.into()), name: name.into(), avatar: None, role: String::new(), grants }
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

// ---- packages -------------------------------------------------------------------------------------------

/// The page of a package published without one of its own.
pub const DEFAULT_PAGE: &str = "<!doctype html><title>app</title>";

/// Publish an app package (`files` by path) under `root` as an app ref; returns the `file://` URL of the ref. Its
/// page is `static/index.html`: the one among `files`, or [`DEFAULT_PAGE`].
pub fn publish_package(root: &std::path::Path, id: &str, main: &str, files: &[(&str, Vec<u8>)]) -> String {
    publish_app_ref(root, json!({ "id": id, "name": id, "main": main, "ui": { "root": "static" } }), files)
}

/// Like [`publish_package`], with the page in the directory `ui_root` of `files` (which has an `index.html`) and a
/// window in the manifest, like a built app has.
pub fn publish_package_with_client(
    root: &std::path::Path,
    id: &str,
    main: &str,
    ui_root: &str,
    files: &[(&str, Vec<u8>)],
) -> String {
    let window = json!({ "w": 420, "h": 480, "minW": 320, "minH": 320 });
    let manifest = json!({ "id": id, "name": id, "main": main, "ui": { "root": ui_root }, "window": window });
    publish_app_ref(root, manifest, files)
}

/// Publish `files` under `root` with an app ref whose manifest is `manifest` (`version` defaults to `1.0.0`). When
/// the manifest's `ui.root` is `static` and no file is its `index.html`, [`DEFAULT_PAGE`] is published as one.
pub fn publish_app_ref(root: &std::path::Path, mut manifest: Value, files: &[(&str, Vec<u8>)]) -> String {
    use sha2::{Digest, Sha256};
    std::fs::create_dir_all(root).unwrap();
    let mut files = files.to_vec();
    let default_page =
        manifest["ui"]["root"] == "static" && !files.iter().any(|(path, _)| *path == "static/index.html");
    if default_page {
        files.push(("static/index.html", DEFAULT_PAGE.as_bytes().to_vec()));
    }
    let mut refs = vec![];
    for (path, bytes) in &files {
        let on_disk = root.join(path);
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(on_disk, bytes).unwrap();
        refs.push(json!({
            "path": path,
            "url": format!("file://{}/{path}", root.display()),
            "sha256": format!("{:x}", Sha256::digest(bytes)),
        }));
    }
    if manifest.get("version").is_none() {
        manifest["version"] = json!("1.0.0");
    }
    let app_ref = json!({ "manifest": manifest, "files": refs });
    std::fs::write(root.join("app-ref.json"), serde_json::to_vec(&app_ref).unwrap()).unwrap();
    format!("file://{}/app-ref.json", root.display())
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

/// The bundled Luau text of an app entry file, built with the product's own pipeline (`scripts/luau.ts`: darklua
/// with `.darklua.json`), so these tests run what `bun run build:apps` ships. Needs bun and darklua.
pub fn build_luau(entry: &std::path::Path) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("vos-host-test-{}-{}.luau", std::process::id(), nonce()));
    let run = std::process::Command::new("bun")
        .arg("scripts/luau-build.ts")
        .arg(entry)
        .arg(&out)
        .current_dir(&root)
        .output()
        .expect("bun is needed to build the apps these tests run (https://bun.sh)");
    assert!(
        run.status.success(),
        "building {} failed (is darklua 0.18.0 installed? see scripts/luau.ts):\n{}{}",
        entry.display(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let bytes = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_file(out);
    bytes
}

fn nonce() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The built Luau file of the host's own fixture backend `tests/fixtures/backends/<name>.luau`.
pub fn built_backend(name: &str) -> Vec<u8> {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/backends")
        .join(format!("{name}.luau"));
    assert!(fixture.exists(), "no fixture backend `{name}`");
    build_luau(&fixture)
}

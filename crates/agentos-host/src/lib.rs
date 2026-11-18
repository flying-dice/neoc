//! The composition root of the hexagon: the only crate that knows every adapter.
//!
//! [`build`] reads like a wiring diagram: build every adapter, hand each to the port it implements, mount
//! the driving adapters on one axum router. No business logic lives here. The binary (`main.rs`) adds
//! configuration, logging, the pairing banner and graceful shutdown around it.
//!
//! # Who may call what
//!
//! | route                        | credential                                                            |
//! |------------------------------|-----------------------------------------------------------------------|
//! | static client files          | none (the shell shows its pairing screen when the session is missing) |
//! | `POST /pair`, `GET /session` | none: a pairing code is the credential for `/pair`                    |
//! | `/rpc`, `/agui`              | a paired device's session cookie, **including from loopback**         |
//! | `/mcp`                       | a per-agent bearer token; a session cookie is not accepted here       |
//!
//! Protected is the default: `/rpc`, `/agui` and any client route added later sit in one router behind
//! the session guard, and only the routes listed above are mounted outside it. Every route is behind
//! the Origin/Host guard.

pub mod config;
pub mod net;
pub mod platform;
pub mod serve;
pub mod startup;

use agentos_acp::{AcpConnector, AgentLaunch};
use agentos_agui::{agui_router, AgUiEvent, Role, RunAgentInput, RunHandler};
use agentos_application::{
    ports::{
        AgentDirectory, AppRuntime, Clock, DataStore, DeviceRegistry, EventSink, Fetcher, InstalledAppStore,
        RandomSource, ToolRegistry,
    },
    AppService, DeviceService, PairingPolicy, Ports,
};
use agentos_domain::{AgentId, AgentIdentity};
use agentos_fetch::UrlFetcher;
use agentos_http::{
    origin_guard, require_session, rpc_router, session_router, static_router, HostGuard, RpcHub,
    SessionConfig,
};
use agentos_luau::{Limits, LuauRuntime};
use agentos_mcp::{mcp_router, McpState};
use agentos_sqlite::{SqliteDataStore, SqliteSystemDb};
use axum::{middleware::from_fn_with_state, Router};
use futures_util::{stream, stream::BoxStream, StreamExt};
use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

pub struct HostConfig {
    /// `None` = in-memory databases (tests).
    pub data_dir: Option<PathBuf>,
    /// Built Vite client; `None` = API only (dev: Vite serves the client).
    pub client_dir: Option<PathBuf>,
    /// The port the host listens on (the Origin/Host guard requires it in every `Host` header).
    pub port: u16,
    pub dev_client_port: Option<u16>,
    /// Extra `Host` names accepted on any port (a reverse proxy, a DNS name).
    pub allowed_hosts: Vec<String>,
    /// Mark the session cookie `Secure` (TLS is served here or by a proxy in front).
    pub secure_cookies: bool,
    /// The machine's own addresses and hostname for the Origin/Host guard; `None` = ask the OS.
    pub interfaces: Option<Interfaces>,
    /// (bearer token, identity) for `/mcp`.
    pub agents: Vec<(String, AgentIdentity)>,
    pub acp: HashMap<AgentId, AgentLaunch>,
    pub pairing: PairingPolicy,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
}

/// A fixed view of this machine's network identity (tests).
#[derive(Clone, Debug)]
pub struct Interfaces {
    pub ips: Vec<IpAddr>,
    pub hostname: Option<String>,
}

impl HostConfig {
    /// A config with in-memory state, the OS clock and randomness, and nothing mounted but the API.
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self {
            data_dir: None,
            client_dir: None,
            port,
            dev_client_port: None,
            allowed_hosts: vec![],
            secure_cookies: false,
            interfaces: None,
            agents: vec![],
            acp: HashMap::new(),
            pairing: PairingPolicy::default(),
            clock: Arc::new(platform::SystemClock),
            random: Arc::new(platform::OsRandom),
        }
    }
}

pub struct Host {
    pub router: Router,
    pub service: Arc<AppService>,
    pub devices: Arc<DeviceService>,
    pub shutdown: CancellationToken,
}

struct TokenDirectory(Vec<(String, AgentIdentity)>);
impl AgentDirectory for TokenDirectory {
    fn authenticate(&self, token: &str) -> Option<AgentIdentity> {
        // compare every entry (no early exit on the first match)
        let mut found = None;
        for (t, id) in &self.0 {
            if constant_time_eq(t.as_bytes(), token.as_bytes()) {
                found = Some(id.clone());
            }
        }
        found
    }
}
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Placeholder AG-UI run: the real one maps ACP `session/update` to AG-UI events (wave B3).
struct PlaceholderRun;
impl RunHandler for PlaceholderRun {
    fn run(&self, i: RunAgentInput) -> BoxStream<'static, AgUiEvent> {
        stream::iter([
            AgUiEvent::RunStarted {
                thread_id: i.thread_id.clone(),
                run_id: i.run_id.clone(),
                parent_run_id: None,
            },
            AgUiEvent::TextMessageStart { message_id: "m1".into(), role: Role::Assistant },
            AgUiEvent::TextMessageContent {
                message_id: "m1".into(),
                delta: "ACP -> AG-UI mapping goes here".into(),
            },
            AgUiEvent::TextMessageEnd { message_id: "m1".into() },
            AgUiEvent::RunFinished { thread_id: i.thread_id, run_id: i.run_id, result: None },
        ])
        .boxed()
    }
}

/// The device service alone, over the system database in `data_dir` (or in memory). The `pair` command
/// uses this to issue a code for a running host without starting a second one.
pub fn open_devices(
    data_dir: Option<&std::path::Path>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    policy: PairingPolicy,
) -> anyhow::Result<Arc<DeviceService>> {
    let registry: Arc<dyn DeviceRegistry> = open_system(data_dir)?;
    Ok(Arc::new(DeviceService::new(registry, clock, random, policy)))
}

/// The system database (`<data>/system.db`): paired devices and the installed apps. In memory when
/// there is no data directory.
pub fn open_system(data_dir: Option<&std::path::Path>) -> anyhow::Result<Arc<SqliteSystemDb>> {
    Ok(Arc::new(SqliteSystemDb::open(data_dir.map(|d| d.join("system.db")).as_deref())?))
}

pub fn build(cfg: HostConfig) -> anyhow::Result<Host> {
    let shutdown = CancellationToken::new();

    // driven adapters -> ports
    let hub = Arc::new(RpcHub::default()); // EventSink + Notifier + the /rpc handler
    let mcp_state = Arc::new(McpState::default()); // ToolRegistry
    let fetcher: Arc<dyn Fetcher> = Arc::new(UrlFetcher::new(8 * 1024 * 1024)?);
    let data: Arc<dyn DataStore> =
        Arc::new(SqliteDataStore::new(cfg.data_dir.as_ref().map(|d| d.join("apps"))));
    let runtime: Arc<dyn AppRuntime> =
        Arc::new(LuauRuntime::new(hub.clone(), Arc::new(platform::LogShell), Limits::default()));
    let tools: Arc<dyn ToolRegistry> = mcp_state.clone();
    let events: Arc<dyn EventSink> = hub.clone();
    let agents = Arc::new(AcpConnector::new(cfg.acp));
    let system = open_system(cfg.data_dir.as_deref())?;
    let installed: Arc<dyn InstalledAppStore> = system.clone();
    let registry: Arc<dyn DeviceRegistry> = system;
    let devices = Arc::new(DeviceService::new(registry, cfg.clock.clone(), cfg.random, cfg.pairing));

    // application
    let service = Arc::new(AppService::new(Ports {
        fetcher,
        data,
        runtime,
        tools,
        events,
        agents,
        installed,
        clock: cfg.clock,
        background: Arc::new(platform::TokioBackground),
    }));
    hub.bind(service.clone(), devices.clone());

    // the Origin/Host guard knows the machine's own network identity
    let mut guard = match cfg.interfaces {
        Some(i) => HostGuard::with_interfaces(cfg.port, Arc::new(move || i.ips.clone()), i.hostname.clone()),
        None => HostGuard::new(cfg.port),
    }
    .allow_hosts(cfg.allowed_hosts);
    if let Some(port) = cfg.dev_client_port {
        guard = guard.allow_port(port);
    }

    // driving adapters. Everything a client may call is in `protected`; the few public routes are
    // listed below it, and /mcp has its own per-agent bearer authentication.
    let protected = rpc_router(hub)
        .merge(agui_router(Arc::new(PlaceholderRun)))
        .layer(from_fn_with_state(devices.clone(), require_session));
    let public =
        session_router(SessionConfig { devices: devices.clone(), secure_cookies: cfg.secure_cookies });
    let directory: Arc<dyn AgentDirectory> = Arc::new(TokenDirectory(cfg.agents));
    let mcp = mcp_router(mcp_state, service.clone(), directory, shutdown.child_token());

    let mut router = Router::new().merge(protected).merge(public).merge(mcp);
    if let Some(dir) = cfg.client_dir {
        router = router.merge(static_router(&dir)); // the client's files are public: the shell pairs itself
    }
    let router = router.layer(from_fn_with_state(Arc::new(guard), origin_guard));

    Ok(Host { router, service, devices, shutdown })
}

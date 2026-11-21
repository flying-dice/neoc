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
pub mod runtime;
pub mod serve;
pub mod startup;

use vos_acp::AcpConnector;
use vos_agui::{agui_router_with, DeviceStreams};
use vos_application::{
    ports::{
        AgentConnector, AgentDirectory, AppActivity, AppRuntime, Clock, DataStore, DeviceRegistry, EventSink,
        Fetcher, InstalledAppStore, RandomSource, ThreadStore, ToolRegistry,
    },
    AgentProcesses, AgentService, AppService, ChatPorts, ChatService, DeviceService, NoAgentProcesses,
    PairingPolicy, Ports, ViewService,
};
use vos_domain::{limits, Agent};
use vos_fetch::UrlFetcher;
use vos_http::{
    origin_guard, require_session, rpc_router, session_router, static_router, HostGuard, RpcHub,
    SessionConfig,
};
use vos_lua::LuaRuntime;
use vos_mcp::{mcp_router, McpState};
use vos_sqlite::{SqliteDataStore, SqliteSystemDb};
use axum::{middleware::from_fn_with_state, Router};
use std::{
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, Weak},
};
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
    /// The catalog Discover browses (`app.available`): registry files or app refs, by URL.
    pub registries: Vec<String>,
    /// Agents to add to the roster at start if they are not there yet, each with a fixed persistent MCP
    /// bearer token (tests; the binary seeds from `agentos.toml` and mints tokens at runtime).
    pub agents: Vec<(String, Agent)>,
    /// The `/mcp` URL as agents reach it (handed to every agent session); `None` starts sessions without tools.
    pub mcp_url: Option<String>,
    /// The working directory handed to agent sessions.
    pub agent_cwd: PathBuf,
    /// Replace the ACP connector (tests that script an agent in-process); `None` launches real processes.
    pub connector: Option<Arc<dyn AgentConnector>>,
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
            registries: vec![],
            agents: vec![],
            mcp_url: Some(format!("http://127.0.0.1:{port}/mcp")),
            agent_cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            connector: None,
            pairing: PairingPolicy::default(),
            clock: Arc::new(platform::SystemClock),
            random: Arc::new(platform::OsRandom),
        }
    }
}

pub struct Host {
    pub router: Router,
    pub service: Arc<AppService>,
    pub views: Arc<ViewService>,
    pub devices: Arc<DeviceService>,
    /// The agent roster (contacts, grants, tokens).
    pub agents: Arc<AgentService>,
    /// Agent chats: what `/agui` serves.
    pub chat: Arc<ChatService>,
    pub shutdown: CancellationToken,
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

/// The connector the chats use, and (for the real ACP one) the handle that stops an agent's process.
fn agent_processes(
    scripted: Option<Arc<dyn AgentConnector>>,
    cwd: PathBuf,
) -> (Arc<dyn AgentConnector>, Arc<dyn AgentProcesses>) {
    if let Some(connector) = scripted {
        return (connector, Arc::new(NoAgentProcesses));
    }
    let acp = Arc::new(AcpConnector::new(cwd));
    (acp.clone(), acp)
}

pub async fn build(cfg: HostConfig) -> anyhow::Result<Host> {
    let shutdown = CancellationToken::new();

    // driven adapters -> ports
    let hub = Arc::new(RpcHub::default()); // EventSink + Notifier + the /rpc handler
    let mcp_state = Arc::new(McpState::default()); // ToolRegistry
    let fetcher: Arc<dyn Fetcher> = Arc::new(UrlFetcher::new(limits::MAX_FILE_BYTES as u64)?); // one fetch is one package file
    let data: Arc<dyn DataStore> =
        Arc::new(SqliteDataStore::new(cfg.data_dir.as_ref().map(|d| d.join("apps"))));
    let system = open_system(cfg.data_dir.as_deref())?;
    let random = cfg.random;
    // the agent processes: real ACP ones unless a test scripts the connector. The roster stops a process when
    // its agent is deleted or relaunched with other settings.
    let (connector, processes) = agent_processes(cfg.connector, cfg.agent_cwd);
    // the roster is also the MCP adapter's directory, tells it when an agent's tool list changes, and is what
    // apps list
    let contacts = Arc::new(AgentService::new(system.clone(), mcp_state.clone(), random.clone(), processes));
    contacts.load().await?;
    let lua = Arc::new(LuaRuntime::new(hub.clone(), hub.clone()).with_roster(contacts.clone()));
    let runtime: Arc<dyn AppRuntime> = lua.clone();
    let tools: Arc<dyn ToolRegistry> = mcp_state.clone();
    let events: Arc<dyn EventSink> = hub.clone();
    let installed: Arc<dyn InstalledAppStore> = system.clone();
    let thread_store: Arc<dyn ThreadStore> = system.clone();
    let registry: Arc<dyn DeviceRegistry> = system;
    let devices = Arc::new(DeviceService::new(registry, cfg.clock.clone(), random.clone(), cfg.pairing));

    // application
    let service = Arc::new(AppService::new(Ports {
        fetcher,
        data,
        runtime,
        tools,
        events,
        installed,
        clock: cfg.clock.clone(),
        background: Arc::new(platform::TokioBackground),
        registries: cfg.registries,
    }));
    // what an app's background work changes reaches its open windows: the runtime tells the application
    lua.bind_activity(Arc::downgrade(&service) as Weak<dyn AppActivity>);
    for (token, agent) in cfg.agents {
        contacts.ensure(agent, Some(&token)).await?;
    }
    let chat = ChatService::new(ChatPorts {
        connector,
        threads: thread_store,
        background: Arc::new(platform::TokioBackground),
        clock: cfg.clock,
        random,
        agents: contacts.clone(),
        apps: service.clone(),
        mcp_url: cfg.mcp_url,
    });
    hub.bind(service.clone(), devices.clone());
    hub.bind_agents(contacts.clone());
    let views = Arc::new(ViewService::new(service.clone()));
    service.add_observer(views.clone());
    hub.bind_views(views.clone());

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
    // revoking a device ends its open /agui streams (its /rpc sockets are closed by the hub)
    let streams = Arc::new(DeviceStreams::default());
    let to_end = streams.clone();
    devices.on_revoked(move |device| to_end.revoke(device));
    let session = SessionConfig { devices: devices.clone(), secure_cookies: cfg.secure_cookies };
    let protected = rpc_router(hub)
        .merge(agui_router_with(chat.clone(), streams))
        .layer(from_fn_with_state(session.clone(), require_session));
    let public = session_router(session);
    let directory: Arc<dyn AgentDirectory> = contacts.clone();
    let mcp = mcp_router(mcp_state, service.clone(), directory, shutdown.child_token());

    let mut router = Router::new().merge(protected).merge(public).merge(mcp);
    if let Some(dir) = cfg.client_dir {
        router = router.merge(static_router(&dir)); // the client's files are public: the shell pairs itself
    }
    let router = router.layer(from_fn_with_state(Arc::new(guard), origin_guard));

    Ok(Host { router, service, views, devices, agents: contacts, chat, shutdown })
}

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
//! | `/rpc`                       | a paired device's session cookie, **including from loopback**         |
//! | `GET`, `PUT /files/host/{path}`, `/files/app/{app}/{path}` | a paired device's session cookie: download (always `attachment`, `nosniff`, never a page) or upload a file of This Computer (the host's whole file system, unless `host_files = false`) or of an app's Documents. The Origin/Host guard refuses a `PUT` from any other origin |
//! | `GET /apps/{app}`            | a paired device's session cookie (an unpaired visitor gets a page that says to pair) |
//! | `/apps/bridge.js`, `/apps/{app}/static/...` | none (a sandboxed frame sends no cookie, its module scripts send `Origin: null`: the origin guard lets those reads through, GET and HEAD under this prefix only). An app's static content is public like the shell's own files: nothing secret or personal belongs in it |
//! | `/mcp`                       | a per-agent bearer token; a session cookie is not accepted here       |
//!
//! Protected is the default: `/rpc` and any client route added later sit in one router behind
//! the session guard, and only the routes listed above are mounted outside it. Every route is behind
//! the Origin/Host guard.

// One app's failure stays that app's: a panic in a module an app calls ends that call with an error (`vos_lua`
// catches it where the host calls into a VM, `neoc_vm::panics` where a task or a request handler runs). Under
// `panic = "abort"` every one of those catches is dead and the first panic ends the host and every app with it
// (NEOC-125, Decision NEOC-258), so no profile may build the host that way.
#[cfg(panic = "abort")]
compile_error!(
    "vos-host must be built with panic = \"unwind\": under panic = \"abort\" a panic in one app's module ends every app"
);

pub mod config;
pub mod migrate;
pub mod net;
pub mod platform;
pub mod runtime;
pub mod serve;
pub mod startup;

use axum::{middleware::from_fn_with_state, Router};
use std::{net::IpAddr, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
use vos_application::{
    ports::{
        AgentDirectory, AppRuntime, AppTools, Clock, DataStore, DeviceRegistry, EventSink, Fetcher,
        InstalledAppStore, RandomSource, ToolRegistry,
    },
    AgentService, AppService, DeviceService, FileStore, FilesService, HostFiles, McpEndpoint, PairingPolicy,
    Ports,
};
use vos_domain::{limits, Agent};
use vos_fetch::UrlFetcher;
use vos_http::{
    app_routes, files_router, origin_guard, require_session, rpc_router, session_router, static_router,
    HostGuard, RpcHub, SessionConfig,
};
use vos_lua::{DiskFiles, LocalFiles, LuaRuntime};
use vos_mcp::{mcp_router, McpState};
use vos_sqlite::{SqliteDataStore, SqliteSystemDb};

pub struct HostConfig {
    /// `None` = in-memory databases (tests).
    pub data_dir: Option<PathBuf>,
    /// Built Vite client; `None` = API only (dev: Vite serves the client).
    pub client_dir: Option<PathBuf>,
    /// The port the host listens on (the Origin/Host guard requires it in every `Host` header).
    pub port: u16,
    /// The address it listens on: the URL of a token an app leases for its agent names its local address when the
    /// app names none (`app.tools.lease`).
    pub bind: IpAddr,
    /// `https` when TLS is served here, else `http`.
    pub scheme: &'static str,
    pub dev_client_port: Option<u16>,
    /// Extra `Host` names accepted on any port (a reverse proxy, a DNS name).
    pub allowed_hosts: Vec<String>,
    /// Mark the session cookie `Secure` (TLS is served here or by a proxy in front).
    pub secure_cookies: bool,
    /// The machine's own addresses and hostname for the Origin/Host guard; `None` = ask the OS.
    pub interfaces: Option<Interfaces>,
    /// Whether the Files page may browse this machine's own file system ("This Computer"): all of it, as the user
    /// running the host.
    pub host_files: bool,
    /// That user's home directory, the Files page's shortcut (`None`: there is none to offer).
    pub home: Option<PathBuf>,
    /// The catalog Discover browses (`app.available`): registry files or app refs, by URL.
    pub registries: Vec<String>,
    /// Agents to add to the roster at start if they are not there yet, each with a fixed persistent MCP
    /// bearer token (tests; the binary seeds from `vos.toml` and mints tokens at runtime).
    pub agents: Vec<(String, Agent)>,
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
            bind: IpAddr::from([127, 0, 0, 1]),
            scheme: "http",
            dev_client_port: None,
            allowed_hosts: vec![],
            secure_cookies: false,
            interfaces: None,
            host_files: true,
            home: None,
            registries: vec![],
            agents: vec![],
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
    /// The agent roster (contacts, grants, tokens).
    pub agents: Arc<AgentService>,
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

pub async fn build(cfg: HostConfig) -> anyhow::Result<Host> {
    let shutdown = CancellationToken::new();

    // driven adapters -> ports
    let hub = Arc::new(RpcHub::default()); // EventSink + Notifier + the /rpc handler
    let mcp_state = Arc::new(McpState::default()); // ToolRegistry
    let fetcher: Arc<dyn Fetcher> = Arc::new(UrlFetcher::new(limits::MAX_FILE_BYTES as u64)?); // one fetch is one package file
    let data: Arc<dyn DataStore> =
        Arc::new(SqliteDataStore::new(cfg.data_dir.as_ref().map(|d| d.join("apps"))));
    // each app's managed files sit next to its database (`<data>/apps/<id>.files`), and are purged with it
    let files: Arc<dyn FileStore> = Arc::new(DiskFiles::new(cfg.data_dir.as_ref().map(|d| d.join("apps"))));
    let computer: Option<Arc<dyn HostFiles>> =
        cfg.host_files.then(|| Arc::new(LocalFiles::new(cfg.home.as_deref())) as Arc<dyn HostFiles>);
    let system = open_system(cfg.data_dir.as_deref())?;
    let random = cfg.random;
    // the roster is also the MCP adapter's directory, tells it when an agent's tool list changes, is what apps list,
    // and keeps the tokens apps lease for agents of their own (`app.tools.lease`) at the host's own `/mcp`
    let mcp_endpoint = McpEndpoint {
        scheme: cfg.scheme.to_owned(),
        host: net::local_ip(cfg.bind).to_string(),
        port: cfg.port,
    };
    let contacts =
        Arc::new(AgentService::new(system.clone(), mcp_state.clone(), random.clone()).with_mcp(mcp_endpoint));
    contacts.load().await?;
    let lua = Arc::new(
        LuaRuntime::new(hub.clone(), hub.clone())
            .with_roster(contacts.clone())
            .with_leases(contacts.clone())
            .with_files(files.clone()),
    );
    let runtime: Arc<dyn AppRuntime> = lua.clone();
    let tools: Arc<dyn ToolRegistry> = mcp_state.clone();
    let events: Arc<dyn EventSink> = hub.clone();
    let installed: Arc<dyn InstalledAppStore> = system.clone();
    let registry: Arc<dyn DeviceRegistry> = system;
    let devices = Arc::new(DeviceService::new(registry, cfg.clock.clone(), random.clone(), cfg.pairing));

    // application
    let service = Arc::new(AppService::new(Ports {
        fetcher,
        data,
        files: files.clone(),
        runtime,
        tools,
        events,
        installed,
        clock: cfg.clock.clone(),
        registries: cfg.registries,
    }));
    for (token, agent) in cfg.agents {
        contacts.ensure(agent, Some(&token)).await?;
    }
    // every app can list and call the tools of the running apps (the Agents app offers them to its agents)
    let app_tools: Arc<dyn AppTools> = service.clone();
    lua.bind_tools(Arc::downgrade(&app_tools));
    // the one RPC bus: every app's backend sends its requests and notifications through it, and the pages' handlers
    // (kept by the `/rpc` adapter) answer requests on it beside the backends'
    let bus: Arc<dyn vos_application::ports::AppBus> = service.clone();
    lua.bind_bus(Arc::downgrade(&bus));
    let pages: Arc<dyn vos_application::ports::PageHandlers> = hub.clone();
    service.bind_pages(Arc::downgrade(&pages));
    hub.bind(service.clone(), devices.clone());
    hub.bind_agents(contacts.clone());
    // the Files page: every installed app's Documents, and this machine's own file system
    let installed_apps = service.clone();
    let browsable = Arc::new(FilesService::new(files, move |app| installed_apps.is_installed(app), computer));
    hub.bind_files(browsable.clone());

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
    let session = SessionConfig { devices: devices.clone(), secure_cookies: cfg.secure_cookies };
    let apps = app_routes(service.clone(), session.clone());
    let protected = rpc_router(hub)
        .merge(files_router(browsable))
        .layer(from_fn_with_state(session.clone(), require_session));
    // an app's static files are public: a sandboxed frame sends no cookie. The wrapper checks the session itself, to
    // answer an unpaired visitor with a page rather than the guard's JSON.
    let public = session_router(session).merge(apps.files).merge(apps.wrapper);
    let directory: Arc<dyn AgentDirectory> = contacts.clone();
    let mcp = mcp_router(mcp_state, service.clone(), directory, shutdown.child_token());

    let mut router = Router::new().merge(protected).merge(public).merge(mcp);
    if let Some(dir) = cfg.client_dir {
        router = router.merge(static_router(&dir)); // the client's files are public: the shell pairs itself
    }
    let router = router.layer(from_fn_with_state(Arc::new(guard), origin_guard));

    Ok(Host { router, service, devices, agents: contacts, shutdown })
}

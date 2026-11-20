//! `agentos-host`: serve the desktop, `/rpc`, `/agui` and `/mcp`, or print a fresh pairing code.
//!
//! ```text
//! agentos-host                       serve (config: agentos.toml, environment, see --help)
//! agentos-host pair                  print a fresh pairing code (also: --new-pairing-code)
//! agentos-host --config <file>       use this config file
//! ```
//! Logging is `tracing` with `RUST_LOG` (default `info`).

use agentos_application::{mint_agent_token, PairingPolicy};
use agentos_domain::{Agent, AgentId, AgentRuntime, ApprovalPolicy, Grants};
use agentos_host::{
    build,
    config::Settings,
    net::{local_url, reachable_urls},
    open_devices,
    platform::{OsRandom, SystemClock},
    serve::{serve, termination_signal},
    startup::{prepare_data_dir, spawn_revision_purge, start_apps, watch_for_reload, write_mcp_json},
    HostConfig,
};
use agentos_http::{system_hostname, system_interface_ips};
use anyhow::Context;
use std::{
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing_subscriber::EnvFilter;

const HELP: &str = "\
agentos-host: the AgentOS host

USAGE:
    agentos-host [--config <file>]       serve the desktop, /rpc, /agui and /mcp
    agentos-host pair [--config <file>]  print a fresh pairing code (alias: --new-pairing-code)

Every client must be paired: open the desktop, enter the pairing code the host prints at start.
Settings come from agentos.toml and AGENTOS_* environment variables; see docs/architecture/host-stack.md.
Logging: RUST_LOG (default info).
";

enum Command {
    Serve,
    Pair,
}

struct Cli {
    command: Command,
    config: Option<PathBuf>,
}

fn parse_cli(args: impl IntoIterator<Item = String>) -> anyhow::Result<Option<Cli>> {
    let mut cli = Cli { command: Command::Serve, config: None };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "pair" | "--new-pairing-code" => cli.command = Command::Pair,
            "--config" => cli.config = Some(args.next().context("--config needs a path")?.into()),
            "-h" | "--help" => return Ok(None),
            other => anyhow::bail!("unknown argument `{other}` (try --help)"),
        }
    }
    Ok(Some(cli))
}

/// The stack of each runtime worker thread (the default is 2 MiB). Apps' Lua runs on these threads, and a stack
/// overflow aborts the whole host. The guards in `neoc-vm` (template nesting, value depth: `hostile_inputs.rs` runs them on a
/// 2 MiB thread) and the engine's own pattern limits are meant to hold at 2 MiB without this; the slack is for the
/// paths nobody has swept. It is not a defence against the abort NEOC-124 closes: an error value that recurses in the
/// engine's message handler overflows whatever stack it has (see AGENTS.md).
const WORKER_STACK_BYTES: usize = 16 << 20;

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK_BYTES)
        .build()
        .context("start the async runtime")?
        .block_on(run_cli())
}

async fn run_cli() -> anyhow::Result<()> {
    let Some(cli) = parse_cli(std::env::args().skip(1))? else {
        print!("{HELP}");
        return Ok(());
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .init();

    let cwd = std::env::current_dir().context("working directory")?;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let settings = Settings::load(cli.config.as_deref(), &|k| std::env::var(k).ok(), &cwd, home.as_deref())?;
    match cli.command {
        Command::Pair => pair(&settings).await,
        Command::Serve => run(settings, &cwd).await,
    }
}

/// Issue a code into the system database; a running host reads the same file.
async fn pair(settings: &Settings) -> anyhow::Result<()> {
    prepare_data_dir(&settings.data_dir)?;
    let devices = open_devices(
        Some(&settings.data_dir),
        Arc::new(SystemClock),
        Arc::new(OsRandom),
        PairingPolicy::default(),
    )?;
    let code = devices.issue_pairing_code().await?;
    print_code(&code.code, &settings.data_dir);
    print_urls(settings, settings.port);
    Ok(())
}

async fn run(settings: Settings, cwd: &Path) -> anyhow::Result<()> {
    prepare_data_dir(&settings.data_dir)?;
    let client_dir = settings.resolve_client_dir(cwd)?;
    match &client_dir {
        Some(dir) => tracing::info!(dir = %dir.display(), "serving the client"),
        None => {
            tracing::warn!("no built client found (client/dist): serving the API only");
        }
    }

    let listener = TcpListener::bind(SocketAddr::new(settings.bind, settings.port))
        .with_context(|| format!("binding {}:{}", settings.bind, settings.port))?;
    let port = listener.local_addr()?.port();

    // /mcp: one bearer token for local MCP clients, regenerated on every start
    let mcp_token = mint_agent_token(&OsRandom);
    let local_agent = Agent {
        id: AgentId("local".into()),
        name: "Local MCP client".into(),
        avatar: None,
        role: "Your own MCP client, through <data>/mcp.json".into(),
        runtime: AgentRuntime::default(),
        // the one seeded exception to deny-by-default: this is the token the person's own tools use
        grants: Grants::allow_all(),
        approval: ApprovalPolicy::default(),
    };
    let mut config = HostConfig::new(port);
    config.data_dir = Some(settings.data_dir.clone());
    config.client_dir = client_dir;
    config.dev_client_port = settings.dev_client_port;
    config.allowed_hosts = settings.allowed_hosts.clone();
    config.registries = settings.catalog(cwd);
    config.secure_cookies = settings.secure_cookies;
    config.agents = vec![(mcp_token.clone(), local_agent)];
    // the one URL for host-launched agents and for mcp.json: reachable for whatever `bind` is
    let mcp_url = local_url(settings.scheme(), settings.bind, port, "/mcp");
    config.mcp_url = Some(mcp_url.clone());

    let host = build(config).await?;
    for agent in settings.agents.clone() {
        let id = agent.id.clone();
        if let Err(e) = host.agents.ensure(agent, None).await {
            tracing::warn!(agent = %id.0, error = %e, "could not add the configured agent");
        }
    }
    start_apps(&host.service, &settings.install).await;
    spawn_revision_purge(host.service.clone(), host.shutdown.clone());
    if settings.dev_reload {
        for dir in watch_for_reload(&host.service, &host.shutdown) {
            tracing::info!(dir = %dir.display(), "dev reload: watching");
        }
    }

    let mcp_file = write_mcp_json(&settings.data_dir, &mcp_url, &mcp_token)?;

    let code = host.devices.issue_pairing_code().await.context("issuing the first pairing code")?;
    println!("AgentOS host listening (data: {})", settings.data_dir.display());
    print_urls(&settings, port);
    println!("  MCP:      {mcp_url}  (token in {})", mcp_file.display());
    print_code(&code.code, &settings.data_dir);

    // stop on SIGINT/SIGTERM
    let shutdown = host.shutdown.clone();
    tokio::spawn(async move {
        termination_signal().await;
        tracing::info!("shutting down");
        shutdown.cancel();
    });

    serve(listener, host.router, settings.tls.as_ref(), host.shutdown.clone()).await?;

    host.service.shutdown().await;
    host.chat.shutdown().await;
    tracing::info!("stopped");
    Ok(())
}

fn print_urls(settings: &Settings, port: u16) {
    let urls = reachable_urls(
        settings.scheme(),
        settings.bind,
        port,
        &system_interface_ips(),
        system_hostname().as_deref(),
    );
    println!("  Open:");
    for url in urls {
        println!("    {url}");
    }
}

fn print_code(code: &str, data_dir: &Path) {
    println!("  Pairing code: {code}   (valid 10 minutes, single use)");
    println!("  Enter it on the device you want to pair. New code any time: agentos-host pair");
    tracing::debug!(data_dir = %data_dir.display(), "pairing codes live in the system database");
}

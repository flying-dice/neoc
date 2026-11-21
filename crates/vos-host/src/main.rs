//! `vos`: serve the desktop, `/rpc`, `/agui` and `/mcp`, or print a fresh pairing code.
//!
//! ```text
//! vos                       serve (config: vos.toml, environment, see --help)
//! vos pair                  print a fresh pairing code (also: --new-pairing-code)
//! vos --config <file>       use this config file
//! ```
//! Logging is `tracing` with `RUST_LOG` (default `info`).

use anyhow::Context;
use std::{
    net::{SocketAddr, TcpListener},
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing_subscriber::EnvFilter;
use vos_application::{mint_agent_token, PairingPolicy};
use vos_domain::{Agent, AgentId, AgentRuntime, ApprovalPolicy, Grants};
use vos_host::{
    build,
    config::Settings,
    migrate::{migrate_if_default, reject_legacy_env, Migration},
    net::{local_url, reachable_urls},
    open_devices,
    platform::{OsRandom, SystemClock},
    serve::{serve, termination_signal},
    startup::{prepare_data_dir, spawn_revision_purge, start_apps, watch_for_reload, write_mcp_json},
    HostConfig,
};
use vos_http::{system_hostname, system_interface_ips};

const HELP: &str = "\
vos: the host

USAGE:
    vos [--config <file>]       serve the desktop, /rpc, /agui and /mcp
    vos pair [--config <file>]  print a fresh pairing code (alias: --new-pairing-code)

Every client must be paired: open the desktop, enter the pairing code the host prints at start.
Settings come from vos.toml and VOS_* environment variables; the table is in crates/vos-host/src/config.rs.
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

fn main() -> anyhow::Result<()> {
    // every thread that polls an app's VM has `VM_THREAD_STACK`, and the top-level future runs on a worker: see
    // `vos_host::runtime`
    let runtime = vos_host::runtime::build()?;
    vos_host::runtime::run(&runtime, run_cli())
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

    // the old environment variable names are no longer read: stop rather than start with different settings
    reject_legacy_env(std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()))?;

    let cwd = std::env::current_dir().context("working directory")?;
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let load = || Settings::load(cli.config.as_deref(), &|k| std::env::var(k).ok(), &cwd, home.as_deref());
    let mut settings = load()?;
    if let Some(home) = home.as_deref() {
        // once, on the first start after the rename: the old data directory becomes `~/.vos` (see `vos_host::migrate`)
        let moved = migrate_if_default(&settings.data_dir, home)
            .context("moving the data directory to its new name (no data was lost)")?;
        if let Migration::Moved { from, to, config_renamed } = moved {
            tracing::info!(from = %from.display(), to = %to.display(), config_renamed, "moved the data directory to its new name");
            // the config file may have come with it
            settings = load()?;
            if settings.data_dir != to {
                // the config file came with the directory; the data is where its `data_dir` points
                if settings.data_dir == from {
                    tracing::warn!(
                        data_dir = %settings.data_dir.display(),
                        moved_to = %to.display(),
                        "the moved config file's data_dir names the old directory, whose contents are now in the moved-to directory: set data_dir to it"
                    );
                } else {
                    tracing::warn!(
                        data_dir = %settings.data_dir.display(),
                        config_moved_to = %to.display(),
                        "only the config file moved: the moved config file's data_dir points elsewhere, so the host keeps using the data there"
                    );
                }
            }
        }
    }
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
    println!("vos host listening (data: {})", settings.data_dir.display());
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
    println!("  Enter it on the device you want to pair. New code any time: vos pair");
    tracing::debug!(data_dir = %data_dir.display(), "pairing codes live in the system database");
}

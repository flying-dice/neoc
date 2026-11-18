//! `agentos` binary: config from env, bind 127.0.0.1, serve, shut down on Ctrl-C.
use agentos_host::{build, HostConfig};
use anyhow::Context;
use std::{collections::HashMap, path::PathBuf};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::var("AGENTOS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7000);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.context("bind 127.0.0.1")?;
    let cfg = HostConfig {
        data_dir: std::env::var_os("AGENTOS_DATA").map(PathBuf::from),
        client_dir: std::env::var_os("AGENTOS_CLIENT").map(PathBuf::from),
        port: listener.local_addr()?.port(),
        dev_client_port: Some(5173),
        agents: vec![],
        acp: HashMap::new(),
    };
    let host = build(cfg)?;
    let shutdown = host.shutdown.clone();
    eprintln!("agentos listening on http://{}", listener.local_addr()?);
    axum::serve(listener, host.router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await?;
    Ok(())
}

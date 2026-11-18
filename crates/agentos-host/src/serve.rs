//! Serving the router on a listener, over plain HTTP or TLS, until a shutdown is requested.

use crate::config::TlsFiles;
use anyhow::Context;
use axum::Router;
use axum_server::{tls_rustls::RustlsConfig, Handle};
use std::{net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;

/// How long open connections (a streaming MCP session, a socket) get to finish after a shutdown request.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// Serve `router` on `listener` until `shutdown` is cancelled, then drain for [`SHUTDOWN_GRACE`].
///
/// With `tls` the server speaks HTTPS (rustls; the PWA's service worker needs a secure context on any
/// origin that is not `localhost`).
pub async fn serve(
    listener: std::net::TcpListener,
    router: Router,
    tls: Option<&TlsFiles>,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let handle = Handle::<SocketAddr>::new();
    let on_shutdown = handle.clone();
    tokio::spawn(async move {
        shutdown.cancelled().await;
        on_shutdown.graceful_shutdown(Some(SHUTDOWN_GRACE));
    });
    let make_service = router.into_make_service();
    match tls {
        None => axum_server::from_tcp(listener)?.handle(handle).serve(make_service).await?,
        Some(files) => {
            let config = RustlsConfig::from_pem_file(&files.cert, &files.key).await.with_context(|| {
                format!("loading TLS certificate {} and key {}", files.cert.display(), files.key.display())
            })?;
            axum_server::from_tcp_rustls(listener, config)?.handle(handle).serve(make_service).await?;
        }
    }
    Ok(())
}

/// Resolves when the process is asked to stop: Ctrl-C (SIGINT) or SIGTERM.
pub async fn termination_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()))
        else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

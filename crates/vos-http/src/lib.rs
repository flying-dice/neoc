//! Driving adapter: axum. Owns `/rpc` (WebSocket), the Origin/Host guard, device pairing and sessions
//! (`/pair`, `/session`, the session guard) and static serving of the built client. `/mcp` and `/agui`
//! routers come from their own adapter crates; the composition root merges them and decides which routes
//! sit behind the session guard.

mod guard;
mod hub;
mod rpc;
mod session;

pub use guard::{origin_guard, system_hostname, system_interface_ips, HostGuard, InterfaceIps};
pub use hub::RpcHub;
pub use rpc::{
    rpc_router, RpcCallError, RpcHandler, RpcPeer, BUSY, MAX_IN_FLIGHT, MAX_QUEUED_BYTES,
    MIN_WRITE_BYTES_PER_SEC, WRITE_TIMEOUT,
};
pub use session::{require_session, session_router, SessionConfig, SECURE_SESSION_COOKIE, SESSION_COOKIE};

use axum::Router;
use std::path::Path;
use tower_http::services::{ServeDir, ServeFile};

/// Serve a built Vite app: files from `dir`, unknown paths fall back to `index.html` (SPA routing).
pub fn static_router(dir: &Path) -> Router {
    let index = ServeFile::new(dir.join("index.html"));
    Router::new().fallback_service(ServeDir::new(dir).precompressed_gzip().fallback(index))
}

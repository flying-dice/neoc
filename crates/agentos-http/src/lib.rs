//! Driving adapter: axum. Owns `/rpc` (WebSocket), the Origin/Host guard and static serving of the built
//! client. `/mcp` and `/agui` routers come from their own adapter crates; the composition root
//! merges them.

mod guard;
mod hub;
mod rpc;

pub use guard::{origin_guard, AllowedOrigins};
pub use hub::RpcHub;
pub use rpc::{rpc_router, RpcCallError, RpcHandler, RpcPeer};

use axum::Router;
use std::path::Path;
use tower_http::services::{ServeDir, ServeFile};

/// Serve a built Vite app: files from `dir`, unknown paths fall back to `index.html` (SPA routing).
pub fn static_router(dir: &Path) -> Router {
    let index = ServeFile::new(dir.join("index.html"));
    Router::new().fallback_service(ServeDir::new(dir).precompressed_gzip().fallback(index))
}

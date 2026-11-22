//! Driving adapter: axum. Owns `/rpc` (WebSocket), the Origin/Host guard, device pairing and sessions
//! (`/pair`, `/session`, the session guard), an installed app as a sub-resource (`/apps/<id>`) and static serving of
//! the built client. `/mcp` and `/agui`
//! routers come from their own adapter crates; the composition root merges them and decides which routes
//! sit behind the session guard.

mod apps;
mod guard;
mod hub;
mod rpc;
mod session;

pub use apps::{app_routes, page_csp, AppRoutes};
pub use guard::{origin_guard, system_hostname, system_interface_ips, HostGuard, InterfaceIps};
pub use hub::RpcHub;
pub use rpc::{
    rpc_router, PreparedNotification, RpcCallError, RpcHandler, RpcPeer, BUSY, MAX_IN_FLIGHT,
    MAX_QUEUED_BYTES, MIN_WRITE_BYTES_PER_SEC, WRITE_TIMEOUT,
};
pub use session::{
    is_paired, require_session, session_router, SessionConfig, SECURE_SESSION_COOKIE, SESSION_COOKIE,
};

use axum::http::{header, HeaderValue};
use axum::Router;
use std::path::Path;
use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

/// Serve a built Vite app: files from `dir`, unknown paths fall back to `index.html` (SPA routing).
///
/// `/kit/` is the app kit (`/kit/kit.js`, `/kit/tokens.css`, `/kit/custom-elements.json`): there a missing file is a
/// 404, never the shell's page, which a `<script>` would otherwise be handed in place of the kit. It is read by app pages
/// in a sandboxed frame (an opaque origin), so it answers with `Access-Control-Allow-Origin: *`, as an app's own static
/// files do: public files, read with `crossorigin` so the page can read the stylesheet's rules.
pub fn static_router(dir: &Path) -> Router {
    let index = ServeFile::new(dir.join("index.html"));
    let kit = Router::new().fallback_service(ServeDir::new(dir.join("kit")).precompressed_gzip()).layer(
        SetResponseHeaderLayer::overriding(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        ),
    );
    Router::new()
        .nest_service("/kit", kit)
        .fallback_service(ServeDir::new(dir).precompressed_gzip().fallback(index))
}

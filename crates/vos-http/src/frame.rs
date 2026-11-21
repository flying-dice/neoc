//! The frame route: what an app's client runs in.
//!
//! An app with a client opens in an `<iframe sandbox="allow-scripts">` of the shell. The frame's document comes
//! from here, not from the shell's page, so it can carry a `Content-Security-Policy` header of its own: the
//! shell's `<meta>` policy would otherwise be inherited by an inline document, and a `<meta>` policy cannot carry
//! `sandbox`. The policy allows no network (`default-src 'none'`, no `connect-src`), so the only thing a client can
//! reach is the shell it is embedded in, through `postMessage` (see `bridge.js`).
//!
//! | route                              | what                                                              |
//! |------------------------------------|-------------------------------------------------------------------|
//! | `GET /frame/{app}`                 | the frame document (behind the session guard: it names the app)   |
//! | `GET /frame/bridge.js`             | the host's bridge: `mount(el, { rpc, host })` and the messages     |
//! | `GET /frame/{app}/{sha}/client.js` | the app's client bundle, named by its digest, so it caches forever |
//!
//! A sandboxed frame has an opaque origin and sends no cookie, so the two scripts are public (like the shell's own
//! files); the document is what needs the session. This is sensible isolation, not sandboxing: it keeps an app's
//! client to its own backend and its styles and scripts out of the shell and other apps.

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use vos_application::AppService;
use vos_domain::AppId;

/// The host's side of the mount contract, served to every frame.
const BRIDGE: &str = include_str!("frame/bridge.js");

/// The frame document's policy: no network at all, scripts and styles only from the host and inline styles (a
/// framework injects its CSS), images and fonts from the host or `data:`. `sandbox` gives the frame an opaque
/// origin, so it has no access to the shell's page, its storage or its cookie.
pub const FRAME_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self' data:; base-uri 'none'; form-action 'none'; \
frame-ancestors 'self'; sandbox allow-scripts";

#[derive(Deserialize)]
struct FrameQuery {
    theme: Option<String>,
}

/// The frame routes. `document` is returned separately so the composition root can put it behind the session guard
/// and leave the two scripts public.
pub struct FrameRoutes {
    pub document: Router,
    pub scripts: Router,
}

pub fn frame_routes(service: Arc<AppService>) -> FrameRoutes {
    FrameRoutes {
        document: Router::new().route("/frame/{app}", get(document)).with_state(service.clone()),
        scripts: Router::new()
            .route("/frame/bridge.js", get(bridge))
            .route("/frame/{app}/{version}/client.js", get(client))
            .with_state(service),
    }
}

fn not_found(what: &str) -> Response {
    (StatusCode::NOT_FOUND, format!("{what} not found")).into_response()
}

/// The frame document for a running app with a client. `?theme=dark` (or `light`) is the shell's appearance at the
/// time it opened the frame; the shell keeps it current afterwards with an event.
async fn document(
    State(service): State<Arc<AppService>>,
    Path(app): Path<String>,
    Query(query): Query<FrameQuery>,
) -> Response {
    let Ok(app) = AppId::parse(&app) else { return not_found("app") };
    let Ok(bundle) = service.client_bundle(&app) else { return not_found("client") };
    let theme = if query.theme.as_deref() == Some("dark") { "dark" } else { "light" };
    // `app` is a validated id (lowercase letters, digits, `-`) and the digest is hex: neither needs escaping
    let html = format!(
        "<!doctype html>\n<html lang=\"en\" data-theme=\"{theme}\" style=\"color-scheme:{theme}\">\n<head>\n\
<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{app}</title>\n\
<!-- NEOC-252: the kit's `<link rel=\"stylesheet\" href=\"/kit/tokens.css\">` and `<script src=\"/kit/kit.js\"></script>` go here once those files exist -->\n\
<style>html,body{{margin:0;height:100%}}#root{{height:100%}}</style>\n\
</head>\n<body>\n<div id=\"root\"></div>\n\
<script src=\"/frame/bridge.js\"></script>\n<script src=\"/frame/{app}/{}/client.js\"></script>\n</body>\n</html>\n",
        bundle.sha256
    );
    with_headers(html.into_bytes(), "text/html; charset=utf-8", "no-store", true)
}

async fn bridge() -> Response {
    with_headers(BRIDGE.as_bytes().to_vec(), "text/javascript; charset=utf-8", "no-cache", false)
}

/// The client bundle. Its URL carries the digest it was installed with: a request for any other digest (a frame
/// of an older version after a reload) finds nothing rather than a script it did not ask for.
async fn client(
    State(service): State<Arc<AppService>>,
    Path((app, version)): Path<(String, String)>,
) -> Response {
    let Ok(app) = AppId::parse(&app) else { return not_found("app") };
    match service.client_bundle(&app) {
        Ok(bundle) if bundle.sha256 == version => with_headers(
            bundle.bytes.to_vec(),
            "text/javascript; charset=utf-8",
            "public, max-age=31536000, immutable",
            false,
        ),
        _ => not_found("client"),
    }
}

fn with_headers(body: Vec<u8>, content_type: &'static str, cache: &'static str, csp: bool) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    if csp {
        headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(FRAME_CSP));
    }
    response
}

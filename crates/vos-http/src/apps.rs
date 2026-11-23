//! An installed app as a sub-resource of the host: `/apps/<id>`.
//!
//! Two things live here, and they are different. **The host route** (`GET /apps/<id>`) is a host page: a tiny wrapper
//! with one full-viewport iframe and a script that is the host's side of the bridge, so an app opened on its own
//! page gets what the shell gives it (its backend, its events, the host context) and nothing the shell draws.
//! **The app** is its built static content, served as files at `/apps/<id>/static/...` (what `vite build` wrote), in
//! a frame whose policy the host sets on every file. The policy leaves the page no request of its own; it cannot stop the
//! frame navigating itself, which the host page notices (`AppFrame.svelte`).
//!
//! | route                              | what                                                              | credential |
//! |------------------------------------|-------------------------------------------------------------------|------------|
//! | `GET /apps/{app}`                  | the wrapper page, or "pair this device first"                      | session    |
//! | `GET /apps/{app}/static/{path}`    | a file of the app's client (`/static/` is its entry page)          | none       |
//! | `GET /apps/bridge.js`              | the frame's side of the bridge, injected into the app's page       | none       |
//!
//! A sandboxed frame has an opaque origin and sends no cookie, and its module scripts are fetched with
//! `Origin: null`, so the static route is public, like the shell's own files, and the origin guard lets `Origin: null`
//! through for GET and HEAD under that prefix alone (`guard.rs`). **Nothing secret or personal belongs in an app's
//! static content.** Every file carries the content security policy with `sandbox`, so a file opened directly in a tab
//! is as contained as the frame; only the entry page is ever served as a document.
//!
//! This is sensible isolation, not sandboxing: it keeps an app's client to its own backend and its styles and
//! scripts out of the shell and other apps.

use crate::session::{is_paired, SessionConfig};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::Arc;
use vos_application::{AppService, StaticFile, StaticSite};
use vos_domain::{AppId, Csp};

/// The host's side of the bridge, served to every frame (the app's page loads it before its own scripts).
const BRIDGE: &str = include_str!("apps/bridge.js");

/// The routes of `/apps`. `wrapper` is returned separately so the composition root can give it the session it
/// checks; `files` and `bridge` are public.
pub struct AppRoutes {
    pub wrapper: Router,
    pub files: Router,
}

#[derive(Clone)]
struct AppsState {
    service: Arc<AppService>,
    session: SessionConfig,
}

#[must_use]
pub fn app_routes(service: Arc<AppService>, session: SessionConfig) -> AppRoutes {
    let state = AppsState { service: service.clone(), session };
    AppRoutes {
        wrapper: Router::new().route("/apps/{app}", get(wrapper)).with_state(state),
        files: Router::new()
            .route("/apps/bridge.js", get(bridge))
            .route("/apps/{app}/static/", get(entry))
            .route("/apps/{app}/static/{*path}", get(file))
            .with_state(service),
    }
}

/// The policy of an app's page and of every file served with it: no network at all (`default-src 'none'`, no
/// `connect-src`), scripts only from the host, and images, media, fonts and styles from the host plus the origins the
/// app declared as `resourceDomains` (never scripts: an app's code comes only from the host). `sandbox` gives the
/// frame an opaque origin, so it has no access to the shell's page, its storage or its cookie.
#[must_use]
pub fn page_csp(csp: &Csp) -> String {
    let more: String = csp.resource_domains.iter().flat_map(|origin| [" ", origin.as_str()]).collect();
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'{more}; \
img-src 'self' data: blob:{more}; media-src 'self' data: blob:{more}; font-src 'self' data:{more}; \
base-uri 'none'; form-action 'none'; frame-ancestors 'self'; sandbox allow-scripts allow-forms"
    )
}

/// The wrapper page's own policy: the shell's, plus the frame it holds.
const WRAPPER_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; connect-src 'self'; frame-src 'self'; base-uri 'none'; form-action 'none'";

fn plain(status: StatusCode, message: &str) -> Response {
    (status, message.to_owned()).into_response()
}

fn not_found(what: &str) -> Response {
    plain(StatusCode::NOT_FOUND, &format!("{what} not found"))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The host page of an app opened on its own: one iframe holding the app's page, loaded at once, and the script that
/// is the host's side of the bridge. Draws nothing of its own (no chrome, no sheets, no toasts): it is not the shell.
async fn wrapper(State(state): State<AppsState>, Path(app): Path<String>, headers: HeaderMap) -> Response {
    let Ok(id) = AppId::parse(&app) else { return not_found("app") };
    if !is_paired(&state.session, &headers).await {
        return with_page_headers(
            Html(UNPAIRED.to_owned()).into_response(),
            StatusCode::UNAUTHORIZED,
            WRAPPER_CSP,
        );
    }
    let Some(info) = state.service.apps().into_iter().find(|a| a.id == id) else { return not_found("app") };
    let Ok(site) = state.service.static_site(&id) else { return not_found("app") };
    let allow = site.permissions.features().join("; ");
    let html = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n\
<title>{name}</title>\n\
<meta name=\"mobile-web-app-capable\" content=\"yes\">\n\
<meta name=\"apple-mobile-web-app-capable\" content=\"yes\">\n\
<meta name=\"apple-mobile-web-app-title\" content=\"{name}\">\n\
<meta name=\"apple-mobile-web-app-status-bar-style\" content=\"black-translucent\">\n\
<style>{style}</style>\n\
</head>\n<body>\n<iframe id=\"app\" title=\"{name}\" sandbox=\"allow-scripts allow-forms\" allow=\"{allow}\" \
referrerpolicy=\"no-referrer\" data-entry=\"/apps/{id}/static/{entry}\" data-icon=\"{icon}\" data-tint=\"{tint}\" \
data-testid=\"{id}-frame\"></iframe>\n\
<script type=\"module\" src=\"/wrapper.js\"></script>\n</body>\n</html>\n",
        name = escape(&info.name),
        entry = escape(site.entry()),
        icon = escape(info.display.icon.as_deref().unwrap_or_default()),
        tint = escape(info.display.tint.as_deref().unwrap_or_default()),
        style = WRAPPER_STYLE,
    );
    with_page_headers(Html(html).into_response(), StatusCode::OK, WRAPPER_CSP)
}

/// The wrapper page's whole style: the frame fills the page, inside the device's safe areas (an app's page is in a
/// frame and cannot read them, and added to a phone's home screen the page is drawn under the status bar and the home
/// indicator). Installed, the page is as tall as the screen (`100vh`): iOS makes `100%` shorter than the screen by the
/// status bar there. `touch-action`: an app is not a document to pinch or double-tap larger.
const WRAPPER_STYLE: &str =
    "html,body{margin:0;height:100%;overflow:hidden;color-scheme:light dark;background:Canvas;\
touch-action:pan-x pan-y;-webkit-text-size-adjust:100%}\
@media(display-mode:standalone),(display-mode:fullscreen){html,body{height:100vh}}\
body{box-sizing:border-box;padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px) \
env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px)}\
iframe{display:block;width:100%;height:100%;border:0}";

const UNPAIRED: &str ="<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Pair this device</title>\
<style>body{font:16px system-ui,sans-serif;margin:0;display:grid;place-items:center;height:100vh;text-align:center;\
background:#fff;color:#111}@media(prefers-color-scheme:dark){body{background:#111;color:#eee}a{color:#6cf}}</style></head>\
<body><main><h1>Pair this device first</h1><p>This app opens once this device is paired with vos.</p>\
<p><a href=\"/\">Open vos to pair</a></p></main></body></html>\n";

fn with_page_headers(mut response: Response, status: StatusCode, csp: &'static str) -> Response {
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(csp));
    response
}

async fn bridge() -> Response {
    let mut response = (StatusCode::OK, BRIDGE).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/javascript; charset=utf-8"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

async fn entry(
    State(service): State<Arc<AppService>>,
    Path(app): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    serve(&service, &app, None, &method, &headers)
}

async fn file(
    State(service): State<Arc<AppService>>,
    Path((app, path)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    serve(&service, &app, Some(&path), &method, &headers)
}

/// A request path inside an app's static content, when it is a plain relative path (no empty, `.`, `..` or hidden
/// segment, no backslash or control character). The lookup is an exact match in what the host verified, so a path
/// that is not a file of the app is a 404 whatever it looks like.
fn plain_path(path: &str) -> bool {
    vos_domain::app_ui::is_relative_path(path)
}

fn serve(
    service: &AppService,
    app: &str,
    path: Option<&str>,
    method: &Method,
    headers: &HeaderMap,
) -> Response {
    let Ok(id) = AppId::parse(app) else { return not_found("app") };
    let Ok(site) = service.static_site(&id) else { return not_found("app") };
    let path = path.unwrap_or_else(|| site.entry());
    if !plain_path(path) {
        return not_found("file");
    }
    let Some(found) = site.get(path) else { return not_found("file") };
    let is_entry = path == site.entry();
    let validator = format!("\"{}\"", found.sha256);
    let mut response =
        if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(validator.as_str()) {
            StatusCode::NOT_MODIFIED.into_response()
        } else if *method == Method::HEAD {
            StatusCode::OK.into_response()
        } else {
            (StatusCode::OK, found.bytes.to_vec()).into_response()
        };
    set_file_headers(&mut response, path, is_entry, found, &validator, &site);
    response
}

fn set_file_headers(
    response: &mut Response,
    path: &str,
    is_entry: bool,
    found: &StaticFile,
    validator: &str,
    site: &StaticSite,
) {
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(path, is_entry)));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(found.bytes.len()));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(value) = HeaderValue::from_str(validator) {
        headers.insert(header::ETAG, value);
    }
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    if let Ok(value) = HeaderValue::from_str(&page_csp(&site.csp)) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
}

/// The content type of a file, from a fixed table by extension. Only the app's entry page is a document: any other
/// `.html` is served as text, and a type the table does not know is opaque bytes (with `nosniff`, the browser keeps
/// to it).
fn content_type(path: &str, is_entry: bool) -> &'static str {
    let ext = path.rsplit_once('.').map_or_else(String::new, |(_, e)| e.to_ascii_lowercase());
    match ext.as_str() {
        "html" | "htm" if is_entry => "text/html; charset=utf-8",
        "html" | "htm" | "txt" => "text/plain; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" | "webmanifest" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_adds_declared_origins_to_resources_and_never_to_scripts() {
        let none = page_csp(&Csp::default());
        assert!(none.contains("default-src 'none'") && none.contains("sandbox allow-scripts"), "{none}");
        assert!(!none.contains("connect-src"), "no network: {none}");
        let csp = Csp {
            resource_domains: vec!["https://cdn.example.com".into(), "https://*.img.example.org".into()],
            connect_domains: vec!["https://api.example.com".into()],
            ..Csp::default()
        };
        let with = page_csp(&csp);
        assert!(
            with.contains("img-src 'self' data: blob: https://cdn.example.com https://*.img.example.org"),
            "{with}"
        );
        assert!(with.contains("font-src 'self' data: https://cdn.example.com"), "{with}");
        assert!(with.contains("script-src 'self';"), "an app's code comes only from the host: {with}");
        assert!(
            !with.contains("api.example.com") && !with.contains("connect-src"),
            "connectDomains are not applied: {with}"
        );
    }

    #[test]
    fn only_the_entry_is_a_document_and_unknown_types_are_opaque() {
        assert_eq!(content_type("index.html", true), "text/html; charset=utf-8");
        assert_eq!(content_type("other.html", false), "text/plain; charset=utf-8");
        assert_eq!(content_type("assets/a.JS", false), "text/javascript; charset=utf-8");
        assert_eq!(content_type("assets/font.woff2", false), "font/woff2");
        assert_eq!(content_type("assets/blob.bin", false), "application/octet-stream");
        assert_eq!(content_type("noextension", false), "application/octet-stream");
    }

    #[test]
    fn paths_with_climbing_hidden_or_odd_segments_are_not_files() {
        for ok in ["index.html", "assets/a-1b2c.js", "a/b/c.css"] {
            assert!(plain_path(ok), "{ok}");
        }
        for bad in ["", "..", "../x", "a/../b", "a//b", ".env", "a/.git/x", "a\\b", "a\0b", "/abs"] {
            assert!(!plain_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_name_is_escaped_in_the_wrapper() {
        assert_eq!(escape("<b>\"x\" & y</b>"), "&lt;b&gt;&quot;x&quot; &amp; y&lt;/b&gt;");
    }
}

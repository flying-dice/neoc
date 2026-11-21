//! Device sessions over HTTP: the pairing endpoint, the session cookie and the guard that makes every
//! client route require a paired device.
//!
//! - `POST /pair` `{ "code": "ABCD-EFGH", "deviceName": "Safari on iPad" }` exchanges a pairing code for a
//!   session cookie. Unauthenticated by design; rate limited by the `DeviceService`.
//! - `GET /session` says whether this browser is paired (`{"paired":false}` or `{"paired":true,"device":..}`),
//!   so the shell knows to show its pairing screen. Unauthenticated: it reveals nothing to an unpaired client.
//! - [`require_session`] is the middleware for everything else a client may call (`/rpc`, `/agui`, any
//!   future data endpoint). **Protected is the default**: public routes are the exception, listed in
//!   one place (the host's router).
//!
//! The cookie is `HttpOnly; SameSite=Strict`, and with TLS `Secure` and named `__Host-agentos_session`: the
//! `__Host-` prefix makes browsers refuse a copy set by another (plain-HTTP or sub-domain) origin and
//! refuse any `Domain`, so only this host can set it; the host accepts only that name then. It carries a random
//! 256-bit token; the database holds only its SHA-256. A paired session is **not** an MCP credential:
//! `/mcp` has its own per-agent bearer tokens and never looks at this cookie.

use vos_application::{DeviceService, PairError};
use vos_domain::device::device_name_from_user_agent;
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};

/// Name of the session cookie over plain HTTP.
pub const SESSION_COOKIE: &str = "agentos_session";
/// Name of the session cookie when the host serves TLS (`Secure`, `Path=/`, no `Domain`).
pub const SECURE_SESSION_COOKIE: &str = "__Host-agentos_session";
/// 400 days is the longest lifetime browsers honour. The session itself lasts until it is revoked.
const COOKIE_MAX_AGE_SECS: u64 = 400 * 24 * 60 * 60;
const MAX_PAIR_BODY: usize = 4 * 1024;

#[derive(Clone)]
pub struct SessionConfig {
    pub devices: Arc<DeviceService>,
    /// Set the `Secure` cookie attribute (the host serves TLS, or a TLS proxy fronts it).
    pub secure_cookies: bool,
}

impl SessionConfig {
    /// The cookie's name: the `__Host-` one with TLS, the plain one without.
    #[must_use]
    pub fn cookie_name(&self) -> &'static str {
        cookie_name(self.secure_cookies)
    }
}

fn cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairBody {
    code: String,
    #[serde(default)]
    device_name: Option<String>,
}

/// The unauthenticated session endpoints: `POST /pair` and `GET /session`.
pub fn session_router(config: SessionConfig) -> Router {
    Router::new()
        .route("/pair", post(pair).layer(DefaultBodyLimit::max(MAX_PAIR_BODY)))
        .route("/session", get(session))
        .with_state(config)
}

async fn pair(
    State(config): State<SessionConfig>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Result<Json<PairBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        // 413 when the body is over the cap, 415/422 for a wrong content type or shape
        Err(rejection) => return error(rejection.status(), "expected {\"code\": \"...\"}"),
    };
    let name = body.device_name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
        let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or_default();
        device_name_from_user_agent(ua)
    });
    let client = peer.map(|Extension(ConnectInfo(addr))| addr.ip());
    match config.devices.pair(client, &body.code, &name).await {
        Ok(paired) => {
            let cookie = set_cookie(&paired.token, config.secure_cookies);
            no_store((
                StatusCode::OK,
                [(header::SET_COOKIE, cookie)],
                Json(json!({ "device": paired.device })),
            ))
        }
        Err(PairError::InvalidCode) => error(StatusCode::UNAUTHORIZED, "invalid or expired pairing code"),
        Err(PairError::RateLimited { retry_after_secs }) => no_store((
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, retry_after_secs.to_string())],
            Json(json!({ "error": "too many attempts", "retryAfterSecs": retry_after_secs })),
        )),
        Err(PairError::Port(e)) => {
            tracing::error!(error = %e, "pairing failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, "pairing is unavailable")
        }
    }
}

async fn session(State(config): State<SessionConfig>, headers: HeaderMap) -> Response {
    let device = match session_token(&headers, config.cookie_name()) {
        Some(token) => config.devices.authenticate(&token).await,
        None => None,
    };
    no_store(match device {
        Some(device) => Json(json!({ "paired": true, "device": device })),
        None => Json(json!({ "paired": false })),
    })
}

/// Middleware: the request must carry a valid session cookie. On success the [`Device`] is attached as a
/// request extension (`Extension<Device>`); otherwise `401`.
pub async fn require_session(State(config): State<SessionConfig>, mut req: Request, next: Next) -> Response {
    let device = match session_token(req.headers(), config.cookie_name()) {
        Some(token) => config.devices.authenticate(&token).await,
        None => None,
    };
    match device {
        Some(device) => {
            req.extensions_mut().insert(device);
            next.run(req).await
        }
        None => error(StatusCode::UNAUTHORIZED, "pairing required"),
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    no_store((status, Json(json!({ "error": message }))))
}

fn no_store(res: impl IntoResponse) -> Response {
    let mut res = res.into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// The session token from the cookie called `name` in the `Cookie` header(s), if any.
fn session_token(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(cookie, _)| *cookie == name)
        .map(|(_, value)| value.to_owned())
}

fn set_cookie(token: &str, secure: bool) -> HeaderValue {
    let name = cookie_name(secure);
    let secure = if secure { "; Secure" } else { "" };
    // the token is lowercase hex: always a valid header value
    HeaderValue::from_str(&format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE_SECS}{secure}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_token_is_read_from_among_other_cookies() {
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, HeaderValue::from_static("theme=dark; agentos_session=abc123; other=1"));
        assert_eq!(session_token(&h, SESSION_COOKIE).as_deref(), Some("abc123"));
        assert_eq!(session_token(&HeaderMap::new(), SESSION_COOKIE), None);
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, HeaderValue::from_static("agentos_session_extra=nope"));
        assert_eq!(
            session_token(&h, SESSION_COOKIE),
            None,
            "a cookie that merely starts with the name is not ours"
        );
    }

    #[test]
    fn over_tls_only_the_host_prefixed_cookie_counts() {
        // a copy set from a plain-HTTP origin (cookie tossing) under the unprefixed name is ignored
        let mut h = HeaderMap::new();
        h.append(
            header::COOKIE,
            HeaderValue::from_static("agentos_session=tossed; __Host-agentos_session=real"),
        );
        assert_eq!(session_token(&h, SECURE_SESSION_COOKIE).as_deref(), Some("real"));
        let mut tossed = HeaderMap::new();
        tossed.append(header::COOKIE, HeaderValue::from_static("agentos_session=tossed"));
        assert_eq!(session_token(&tossed, SECURE_SESSION_COOKIE), None);
        // and without TLS the prefixed name is not accepted either
        assert_eq!(session_token(&h, SESSION_COOKIE).as_deref(), Some("tossed"));
        let set = set_cookie("tok", true);
        let set = set.to_str().unwrap();
        assert!(set.starts_with("__Host-agentos_session=tok; Path=/;") && !set.contains("Domain"), "{set}");
        assert!(set_cookie("tok", false).to_str().unwrap().starts_with("agentos_session=tok;"));
    }

    #[test]
    fn the_cookie_is_http_only_strict_and_secure_over_tls() {
        let plain = set_cookie("tok", false);
        let plain = plain.to_str().unwrap();
        assert!(plain.contains("HttpOnly") && plain.contains("SameSite=Strict") && plain.contains("Path=/"));
        assert!(!plain.contains("Secure"));
        assert!(set_cookie("tok", true).to_str().unwrap().ends_with("; Secure"));
    }
}

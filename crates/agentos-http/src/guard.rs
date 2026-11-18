//! DNS-rebinding / cross-site WebSocket hijacking guard for a localhost server.
//! Browsers always send `Origin` on WebSocket handshakes; a page on evil.com can open
//! `ws://127.0.0.1:7000/rpc`, so the Origin MUST be checked (CORS does not apply to `WebSockets`).

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct AllowedOrigins {
    /// full origins, e.g. `http://127.0.0.1:7000`, `http://localhost:5173`
    pub origins: Vec<String>,
    /// accepted `Host` header values (defeats DNS rebinding where Origin == attacker host)
    pub hosts: Vec<String>,
    /// requests with no Origin (curl, native clients) pass when true; browsers never omit it
    pub allow_missing_origin: bool,
}

impl AllowedOrigins {
    pub fn loopback(port: u16, dev_client_port: Option<u16>) -> Self {
        let mut origins = vec![format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
        if let Some(p) = dev_client_port {
            origins.push(format!("http://localhost:{p}"));
            origins.push(format!("http://127.0.0.1:{p}"));
        }
        Self {
            origins,
            hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
            allow_missing_origin: true,
        }
    }

    fn check(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        let host_ok = host.is_some_and(|h| self.hosts.iter().any(|a| a == h));
        let origin_ok = match origin {
            Some(o) => self.origins.iter().any(|a| a == o),
            None => self.allow_missing_origin,
        };
        host_ok && origin_ok
    }
}

/// `axum::middleware::from_fn_with_state(Arc<AllowedOrigins>, origin_guard)`.
pub async fn origin_guard(State(allowed): State<Arc<AllowedOrigins>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok());
    if allowed.check(origin, host) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "forbidden origin").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules() {
        let a = AllowedOrigins::loopback(7000, Some(5173));
        assert!(a.check(Some("http://localhost:5173"), Some("127.0.0.1:7000")));
        assert!(a.check(None, Some("127.0.0.1:7000")));
        assert!(!a.check(Some("https://evil.example"), Some("127.0.0.1:7000")));
        assert!(!a.check(Some("http://127.0.0.1:7000"), Some("evil.example:7000")), "DNS rebinding");
        assert!(!a.check(Some("http://127.0.0.1:7000"), None));
    }
}

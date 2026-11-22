//! DNS-rebinding and cross-site WebSocket hijacking guard.
//!
//! Browsers always send `Origin` on WebSocket handshakes and on cross-origin requests, and a page on
//! evil.example can open `ws://192.168.1.20:7400/rpc` (CORS does not apply to `WebSocket`s). Separately,
//! DNS rebinding makes a hostile name resolve to this machine, so the `Host` header must name this host.
//!
//! What counts as "this host" (so the PWA works from other devices on the LAN, and nothing else does):
//!
//! - any loopback address, `localhost`;
//! - every IP address of this machine's network interfaces (refreshed every few seconds);
//! - the machine's own hostname, and any `*.local` name (mDNS cannot be pointed at us from the internet);
//! - names the operator lists in `allowed_hosts` (for a reverse proxy or a DNS name), on any port.
//!
//! Own names and addresses must carry the port the host listens on (or the dev client's port).
//! A request with no `Origin` (curl, a native or MCP client) passes the Origin check; browsers never omit it.

use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use parking_lot::Mutex;
use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
    time::{Duration, Instant},
};

/// How long the interface list is cached.
const INTERFACE_TTL: Duration = Duration::from_secs(10);

/// Where the host's own IP addresses come from. A function so tests can pretend to have a LAN address.
pub type InterfaceIps = Arc<dyn Fn() -> Vec<IpAddr> + Send + Sync>;

/// The IP addresses of every network interface of this machine.
#[must_use]
pub fn system_interface_ips() -> Vec<IpAddr> {
    if_addrs::get_if_addrs().map_or_default(|all| all.into_iter().map(|i| i.ip()).collect())
}

/// The machine's hostname, lowercased (`None` if it has none we can read).
#[must_use]
pub fn system_hostname() -> Option<String> {
    gethostname::gethostname().into_string().ok().map(|h| h.to_ascii_lowercase())
}

pub struct HostGuard {
    ports: BTreeSet<u16>,
    /// own names, matched on one of `ports`
    names: BTreeSet<String>,
    /// operator-allowed names, matched on any port (including none)
    extra_names: BTreeSet<String>,
    interfaces: InterfaceIps,
    cached: Mutex<(Instant, BTreeSet<IpAddr>)>,
}

#[derive(Debug, PartialEq, Eq)]
enum Name {
    Ip(IpAddr),
    Dns(String),
}

impl HostGuard {
    /// Guard for a host listening on `port`, discovering its interface addresses and hostname.
    #[must_use]
    pub fn new(port: u16) -> Self {
        Self::with_interfaces(port, Arc::new(system_interface_ips), system_hostname())
    }

    /// As [`HostGuard::new`], with the interface addresses and hostname given (tests).
    #[must_use]
    pub fn with_interfaces(port: u16, interfaces: InterfaceIps, hostname: Option<String>) -> Self {
        let mut names = BTreeSet::from(["localhost".to_owned()]);
        names
            .extend(hostname.map(|h| h.trim_end_matches('.').to_ascii_lowercase()).filter(|h| !h.is_empty()));
        let ips = interfaces().into_iter().collect();
        Self {
            ports: BTreeSet::from([port]),
            names,
            extra_names: BTreeSet::new(),
            interfaces,
            cached: Mutex::new((Instant::now(), ips)),
        }
    }

    /// Also accept the dev client (Vite) on this port.
    #[must_use]
    pub fn allow_port(mut self, port: u16) -> Self {
        self.ports.insert(port);
        self
    }

    /// Also accept these names (a reverse proxy, a DNS name) on any port.
    #[must_use]
    pub fn allow_hosts<I: IntoIterator<Item = String>>(mut self, hosts: I) -> Self {
        self.extra_names
            .extend(hosts.into_iter().map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase()));
        self
    }

    /// Is a request with these `Origin` and `Host` header values acceptable?
    #[must_use]
    pub fn permits(&self, origin: Option<&str>, host: Option<&str>) -> bool {
        let host_ok = host.and_then(parse_authority).is_some_and(|(name, port)| self.is_own(&name, port));
        let origin_ok = origin.is_none_or(|o| self.origin_permitted(o));
        host_ok && origin_ok
    }

    fn origin_permitted(&self, origin: &str) -> bool {
        let Some((scheme, authority)) = origin.split_once("://") else { return false };
        let default_port = match scheme {
            "http" => 80,
            "https" => 443,
            _ => return false,
        };
        parse_authority(authority)
            .is_some_and(|(name, port)| self.is_own(&name, Some(port.unwrap_or(default_port))))
    }

    fn is_own(&self, name: &Name, port: Option<u16>) -> bool {
        let on_our_port = port.is_some_and(|p| self.ports.contains(&p));
        match name {
            Name::Ip(ip) => on_our_port && (ip.is_loopback() || self.interface_ips().contains(ip)),
            Name::Dns(dns) => {
                self.extra_names.contains(dns)
                    || (on_our_port && (self.names.contains(dns) || is_mdns_name(dns)))
            }
        }
    }

    fn interface_ips(&self) -> BTreeSet<IpAddr> {
        let mut cached = self.cached.lock();
        if cached.0.elapsed() > INTERFACE_TTL {
            *cached = (Instant::now(), (self.interfaces)().into_iter().collect());
        }
        cached.1.clone()
    }
}

/// `something.local`: multicast DNS names, resolved on the local link only.
fn is_mdns_name(name: &str) -> bool {
    name.strip_suffix(".local").is_some_and(|prefix| !prefix.is_empty() && !prefix.starts_with('.'))
}

/// `host`, `host:port`, `1.2.3.4:port`, `[::1]:port` -> name and optional port. Anything else (userinfo,
/// path, spaces, an empty host, a bad port) is `None`.
fn parse_authority(authority: &str) -> Option<(Name, Option<u16>)> {
    if authority.is_empty() || authority.bytes().any(|b| b.is_ascii_whitespace() || b == b'/' || b == b'@') {
        return None;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (ip, after) = rest.split_once(']')?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':')?),
        };
        (format!("[{ip}]"), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h.to_owned(), Some(p)),
            Some(_) => return None, // a bare IPv6 address is not a valid authority
            None => (authority.to_owned(), None),
        }
    };
    let port = match port {
        None => None,
        Some(p) => Some(p.parse::<u16>().ok()?),
    };
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return inner.parse::<Ipv6Addr>().ok().map(|ip| (Name::Ip(IpAddr::V6(ip)), port));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some((Name::Ip(ip), port));
    }
    let dns = host.trim_end_matches('.').to_ascii_lowercase();
    let valid = !dns.is_empty()
        && dns.split('.').all(|label| {
            !label.is_empty() && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    valid.then_some((Name::Dns(dns), port))
}

/// The authority a request is addressed to: the `Host` header (HTTP/1.1), the URI authority (`:authority`
/// of HTTP/2, which has no `Host`), or either when they agree. `None` when there is none or they differ
/// (two names for one request is an attempt to confuse the check).
fn effective_host<'a>(header_host: Option<&'a str>, uri_authority: Option<&'a str>) -> Option<&'a str> {
    match (header_host, uri_authority) {
        (Some(h), Some(a)) => h.eq_ignore_ascii_case(a).then_some(h),
        (Some(h), None) => Some(h),
        (None, authority) => authority,
    }
}

/// `axum::middleware::from_fn_with_state(Arc<HostGuard>, origin_guard)`; layer it over every route.
pub async fn origin_guard(State(guard): State<Arc<HostGuard>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let header_host = match h.get(header::HOST) {
        Some(v) => match v.to_str() {
            Ok(text) => Some(text),
            Err(_) => return forbidden(),
        },
        None => None,
    };
    let uri_authority = req.uri().authority().map(axum::http::uri::Authority::as_str);
    let host = effective_host(header_host, uri_authority);
    if origin.is_none() && h.contains_key(header::ORIGIN) {
        return forbidden(); // an Origin header that is not text is not a browser's
    }
    // A sandboxed app page has an opaque origin: its module scripts and styles are fetched with `Origin: null`. Those
    // reads of the app's own static files and of the kit (public, like the shell's) are let through; nothing else is.
    let origin = if origin == Some("null") && is_public_frame_read(req.method(), req.uri().path()) {
        None
    } else {
        origin
    };
    if guard.permits(origin, host) {
        next.run(req).await
    } else {
        forbidden()
    }
}

/// Is this a read (`GET` or `HEAD`) that an app's sandboxed page makes of public files: its own static content,
/// `/apps/<id>/static/...`, or the kit, `/kit/...` (its stylesheet is loaded `crossorigin`, so that the page, and a
/// checker such as axe running in it, can read its rules)? Judged on the path as it was sent: an encoded separator or a
/// segment that merely looks like `static` or `kit` is not one.
fn is_public_frame_read(method: &Method, path: &str) -> bool {
    if method != Method::GET && method != Method::HEAD {
        return false;
    }
    if let Some(rest) = path.strip_prefix("/kit/") {
        // the path as sent: no `..` segment, no escape (an encoded `.` or separator) and no backslash may take it out
        return !rest.contains(['%', '\\']) && !rest.split('/').any(|segment| segment == "..");
    }
    let Some(rest) = path.strip_prefix("/apps/") else { return false };
    let Some((app, after)) = rest.split_once('/') else { return false };
    !app.is_empty() && (after == "static" || after.starts_with("static/"))
}

fn forbidden() -> Response {
    (StatusCode::FORBIDDEN, "forbidden origin").into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAN: &str = "192.168.1.20";

    fn guard() -> HostGuard {
        HostGuard::with_interfaces(
            7400,
            Arc::new(|| vec![LAN.parse().unwrap(), "fd00::20".parse().unwrap()]),
            Some("Studio-Mac".into()),
        )
        .allow_port(5173)
        .allow_hosts(["vos.example.org".to_owned()])
    }

    #[test]
    fn loopback_and_the_lan_address_are_this_host() {
        let g = guard();
        assert!(g.permits(None, Some("127.0.0.1:7400")));
        assert!(g.permits(None, Some("localhost:7400")));
        assert!(g.permits(None, Some("[::1]:7400")));
        assert!(g.permits(Some("http://192.168.1.20:7400"), Some("192.168.1.20:7400")), "a LAN client");
        assert!(g.permits(Some("https://[fd00::20]:7400"), Some("[fd00::20]:7400")));
    }

    #[test]
    fn hostname_and_dot_local_names_are_this_host() {
        let g = guard();
        assert!(g.permits(Some("http://studio-mac:7400"), Some("Studio-Mac:7400")), "case-insensitive");
        assert!(g.permits(Some("http://studio-mac.local:7400"), Some("studio-mac.local:7400")));
        assert!(g.permits(None, Some("anything-else.local:7400")), "*.local is mDNS");
        assert!(!g.permits(None, Some(".local:7400")));
    }

    #[test]
    fn origin_null_is_let_through_only_for_reads_of_an_apps_static_files() {
        for (method, path, yes) in [
            (Method::GET, "/apps/tasks/static/index.html", true),
            (Method::HEAD, "/apps/tasks/static/assets/a.js", true),
            (Method::GET, "/apps/tasks/static/", true),
            (Method::GET, "/apps/tasks/static", true),
            (Method::POST, "/apps/tasks/static/index.html", false),
            (Method::PUT, "/apps/tasks/static/x", false),
            (Method::GET, "/apps/tasks", false),
            (Method::GET, "/apps/tasks/staticky/x", false),
            (Method::GET, "/apps/tasks%2Fstatic/x", false),
            (Method::GET, "/apps//static/x", false),
            (Method::GET, "/apps/bridge.js", false),
            (Method::GET, "/rpc", false),
            (Method::GET, "/session", false),
            (Method::GET, "/agui/threads", false),
            (Method::GET, "/mcp", false),
            (Method::GET, "/", false),
            (Method::GET, "/kit/tokens.css", true),
            (Method::HEAD, "/kit/kit.js", true),
            (Method::POST, "/kit/kit.js", false),
            (Method::GET, "/kit", false),
            (Method::GET, "/kits/x", false),
            (Method::GET, "/assets/index.js", false),
            (Method::GET, "/kit/../rpc", false),
            (Method::GET, "/kit/a/../../session", false),
            (Method::GET, "/kit/%2e%2e/rpc", false),
            (Method::GET, "/kit/x%2Fy", false),
            (Method::GET, "/kit/..\\rpc", false),
            (Method::GET, "/kit/fonts/a..b.woff2", true),
            (Method::GET, "/agui", false),
            (Method::GET, "/pair", false),
        ] {
            assert_eq!(is_public_frame_read(&method, path), yes, "{method} {path}");
        }
    }

    #[test]
    fn foreign_origins_are_refused_even_on_a_good_host() {
        let g = guard();
        assert!(!g.permits(Some("https://evil.example"), Some("192.168.1.20:7400")));
        assert!(!g.permits(Some("http://evil.example:7400"), Some("192.168.1.20:7400")));
        assert!(
            !g.permits(Some("http://192.168.1.99:7400"), Some("192.168.1.20:7400")),
            "someone else's LAN IP"
        );
        assert!(!g.permits(Some("null"), Some("127.0.0.1:7400")));
        assert!(!g.permits(Some("file://localhost:7400"), Some("127.0.0.1:7400")), "only http(s) origins");
        assert!(
            !g.permits(Some("http://192.168.1.20:7400/x"), Some("192.168.1.20:7400")),
            "an origin has no path"
        );
    }

    #[test]
    fn dns_rebinding_hosts_are_refused() {
        let g = guard();
        assert!(!g.permits(Some("http://evil.example:7400"), Some("evil.example:7400")), "attacker name");
        assert!(!g.permits(None, Some("192.168.1.99:7400")), "an address that is not ours");
        assert!(!g.permits(None, Some("127.0.0.1:9999")), "right host, wrong port");
        assert!(!g.permits(None, Some("127.0.0.1")), "no port");
        assert!(!g.permits(None, None), "no Host header");
        assert!(!g.permits(None, Some("user@127.0.0.1:7400")));
        assert!(!g.permits(None, Some("127.0.0.1:7400/evil")));
        assert!(!g.permits(None, Some("fe80::1:7400")), "an unbracketed IPv6 literal");
    }

    #[test]
    fn the_dev_client_port_and_configured_hosts_work() {
        let g = guard();
        assert!(g.permits(Some("http://localhost:5173"), Some("localhost:5173")), "Vite dev server");
        assert!(g.permits(Some("https://vos.example.org"), Some("vos.example.org")), "behind a proxy");
        assert!(!g.permits(Some("https://other.example.org"), Some("vos.example.org")));
    }

    #[test]
    fn the_uri_authority_stands_in_for_a_missing_host_header_and_must_agree_with_one() {
        // HTTP/2 has `:authority` and no `Host`
        assert_eq!(effective_host(None, Some("127.0.0.1:7400")), Some("127.0.0.1:7400"));
        assert_eq!(effective_host(Some("127.0.0.1:7400"), None), Some("127.0.0.1:7400"));
        assert_eq!(effective_host(Some("Localhost:7400"), Some("localhost:7400")), Some("Localhost:7400"));
        assert_eq!(effective_host(Some("localhost:7400"), Some("evil.example:7400")), None, "they differ");
        assert_eq!(effective_host(None, None), None);
        let g = guard();
        assert!(g.permits(None, effective_host(None, Some("192.168.1.20:7400"))));
        assert!(!g.permits(None, effective_host(None, Some("evil.example:7400"))));
        assert!(!g.permits(None, effective_host(Some("127.0.0.1:7400"), Some("evil.example:7400"))));
    }

    #[test]
    fn a_missing_origin_passes_when_the_host_is_ours() {
        assert!(guard().permits(None, Some("192.168.1.20:7400")));
    }

    #[test]
    fn interface_changes_are_picked_up_after_the_cache_expires() {
        let ips = Arc::new(Mutex::new(vec!["10.0.0.5".parse::<IpAddr>().unwrap()]));
        let source = ips.clone();
        let g = HostGuard::with_interfaces(7400, Arc::new(move || source.lock().clone()), None);
        assert!(!g.permits(None, Some("172.16.0.9:7400")));
        *ips.lock() = vec!["172.16.0.9".parse().unwrap()];
        g.cached.lock().0 = Instant::now().checked_sub(INTERFACE_TTL * 2).unwrap();
        assert!(g.permits(None, Some("172.16.0.9:7400")), "joined a new network");
    }
}

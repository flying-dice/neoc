//! Where a fetch may connect. A remote app ref must not be able to reach into the host's own network
//! (loopback services, the LAN, a cloud metadata endpoint) through the file URLs it lists or a redirect.
//!
//! The rule is by the *host name of the URL being fetched*: `localhost` and loopback addresses may reach
//! loopback (a ref served from the developer's own machine), nothing else may reach a private, link-local,
//! loopback, unspecified, multicast or otherwise non-public address.

use reqwest::{
    dns::{Addrs, Name, Resolve, Resolving},
    Url,
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

/// Redirects one fetch may follow (all of them to the same origin).
pub const MAX_REDIRECTS: usize = 5;

fn v4_blocked(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private()
        || ip.is_link_local() // 169.254.0.0/16, including the 169.254.169.254 metadata address
        || ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT, 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || a >= 240 // reserved
}

fn v6_blocked(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_blocked(v4);
    }
    let s = ip.segments();
    // NAT64 (64:ff9b::/96) embeds an IPv4 address in the last 32 bits
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        return v4_blocked(Ipv4Addr::from(u32::from(s[6]) << 16 | u32::from(s[7])));
    }
    // 6to4 (2002::/16) embeds an IPv4 address in the next 32 bits
    if s[0] == 0x2002 {
        return v4_blocked(Ipv4Addr::from(u32::from(s[1]) << 16 | u32::from(s[2])));
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || s[..6].iter().all(|x| *x == 0) // deprecated IPv4-compatible ::/96, which embeds any IPv4 address
        || (s[0] & 0xffc0) == 0xfec0 // deprecated site local fec0::/10
        || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7 (fd00:ec2::254 is the AWS IPv6 metadata address)
        || (s[0] & 0xffc0) == 0xfe80 // link local
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Whether the fetcher must not connect to `ip`; loopback is allowed when `loopback_ok` (the URL itself
/// names loopback).
#[must_use]
pub fn is_blocked(ip: IpAddr, loopback_ok: bool) -> bool {
    if loopback_ok && is_loopback(ip) {
        return false;
    }
    match ip {
        IpAddr::V4(v4) => v4_blocked(v4),
        IpAddr::V6(v6) => v6_blocked(v6),
    }
}

/// The host of a URL as an address, when it is one (`[::1]` loses its brackets).
fn literal_ip(url: &Url) -> Option<IpAddr> {
    url.host_str()?.trim_start_matches('[').trim_end_matches(']').parse().ok()
}

/// Whether a URL names this machine: `localhost` or a loopback address.
fn names_loopback(url: &Url) -> bool {
    literal_ip(url)
        .map_or_else(|| url.host_str().is_some_and(|h| h.eq_ignore_ascii_case("localhost")), is_loopback)
}

/// Refuse a URL whose host is an address the fetcher must not connect to. Names are checked when they
/// resolve ([`GuardedResolver`]).
pub fn check_url(url: &Url) -> Result<(), String> {
    match literal_ip(url) {
        Some(ip) if is_blocked(ip, names_loopback(url)) => {
            Err(format!("{ip} is not a public address: an app ref may not reach into the local network"))
        }
        _ => Ok(()),
    }
}

/// Keep `addrs` for `host`, or say why not: any non-public address among them refuses the whole answer, so
/// a name that also resolves to the LAN cannot be used to reach it.
pub fn check_addrs(host: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, String> {
    let loopback_ok = host.eq_ignore_ascii_case("localhost");
    if let Some(bad) = addrs.iter().find(|a| is_blocked(a.ip(), loopback_ok)) {
        return Err(format!("{host} resolves to {}, which is not a public address", bad.ip()));
    }
    if addrs.is_empty() {
        return Err(format!("{host} did not resolve"));
    }
    Ok(addrs)
}

/// The DNS resolver of the HTTP client: the system's, with [`check_addrs`] applied to the answer.
pub struct GuardedResolver;

impl GuardedResolver {
    #[must_use]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addrs = check_addrs(&host, found)?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// `scheme://host:port` equality, the notion of "same origin" a redirect must keep.
#[must_use]
pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str().map(str::to_ascii_lowercase) == b.host_str().map(str::to_ascii_lowercase)
        && a.port_or_known_default() == b.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_link_local_metadata_and_loopback_addresses_are_blocked_for_a_remote_ref() {
        for blocked in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.1.1",
            "127.0.0.1",
            "0.0.0.0",
            "100.64.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fd00:ec2::254",
            "fe80::1",
            "ff02::1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
            "::10.0.0.1",
            "::a9fe:a9fe",
            "::7f00:1",
            "fec0::1",
            "feff::1",
            "2002:a00:1::",
            "2002:a9fe:a9fe::1",
            "2002:7f00:1::1",
        ] {
            assert!(is_blocked(ip(blocked), false), "{blocked} must be blocked");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111", "::ffff:8.8.8.8", "100.128.0.1", "2002:808:808::1"] {
            assert!(!is_blocked(ip(public), false), "{public} must be allowed");
        }
    }

    #[test]
    fn loopback_is_allowed_only_when_the_url_names_loopback_and_nothing_else_is() {
        assert!(!is_blocked(ip("127.0.0.1"), true));
        assert!(!is_blocked(ip("::1"), true));
        assert!(is_blocked(ip("10.0.0.1"), true), "a loopback ref may not reach the LAN");
        assert!(is_blocked(ip("169.254.169.254"), true));
    }

    #[test]
    fn urls_with_an_address_for_a_host_are_checked_by_address() {
        let ok = |u: &str| check_url(&Url::parse(u).unwrap()).is_ok();
        assert!(ok("http://127.0.0.1:7400/a"));
        assert!(ok("http://[::1]:7400/a"));
        assert!(ok("http://localhost/a"));
        assert!(ok("https://apps.example.org/a"), "names are checked when they resolve");
        assert!(ok("https://93.184.216.34/a"));
        for bad in [
            "http://169.254.169.254/latest/meta-data",
            "http://10.1.2.3/a",
            "http://192.168.0.10:8080/a",
            "http://[fd00:ec2::254]/a",
            "http://[::ffff:10.0.0.1]/a",
            "http://0.0.0.0/a",
        ] {
            assert!(!ok(bad), "{bad} must be refused");
        }
        // 127.0.0.1 written as one number is the same loopback address, so it is as allowed
        assert!(ok("http://2130706433/a"));
    }

    #[test]
    fn a_name_that_resolves_to_a_private_address_is_refused_unless_it_is_localhost() {
        let a = |s: &str| SocketAddr::new(ip(s), 0);
        assert!(check_addrs("apps.example.org", vec![a("93.184.216.34")]).is_ok());
        assert!(check_addrs("rebind.example.org", vec![a("10.0.0.5")]).is_err());
        assert!(
            check_addrs("mixed.example.org", vec![a("93.184.216.34"), a("169.254.169.254")]).is_err(),
            "one bad address spoils the answer"
        );
        assert!(check_addrs("loop.example.org", vec![a("127.0.0.1")]).is_err());
        assert!(check_addrs("localhost", vec![a("127.0.0.1"), a("::1")]).is_ok());
        assert!(check_addrs("localhost", vec![a("10.0.0.5")]).is_err());
        assert!(check_addrs("nothing.example.org", vec![]).is_err());
    }

    #[test]
    fn same_origin_means_scheme_host_and_port() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(same_origin(&u("https://a.org/x"), &u("https://A.org:443/y")));
        assert!(!same_origin(&u("https://a.org/x"), &u("http://a.org/x")));
        assert!(!same_origin(&u("https://a.org/x"), &u("https://b.org/x")));
        assert!(!same_origin(&u("http://a.org/x"), &u("http://a.org:8080/x")));
    }
}

//! Which URLs can people actually open this host on? Printed at startup next to the pairing code.

use std::net::IpAddr;

/// The URLs the host is reachable on, loopback first, then each LAN address, then the hostname.
///
/// `bind` decides the set: an unspecified address (`0.0.0.0`, `::`) is every interface; a specific one
/// is just that address. Link-local IPv6 addresses are left out (browsers cannot use them without a zone).
#[must_use]
pub fn reachable_urls(
    scheme: &str,
    bind: IpAddr,
    port: u16,
    interface_ips: &[IpAddr],
    hostname: Option<&str>,
) -> Vec<String> {
    let url = |ip: &IpAddr| match ip {
        IpAddr::V4(v4) => format!("{scheme}://{v4}:{port}"),
        IpAddr::V6(v6) => format!("{scheme}://[{v6}]:{port}"),
    };
    let usable = |ip: &IpAddr| !is_link_local(ip);

    let mut urls: Vec<String> = Vec::new();
    if bind.is_unspecified() {
        let (mut loopback, mut lan): (Vec<&IpAddr>, Vec<&IpAddr>) =
            interface_ips.iter().filter(|ip| usable(ip)).partition(|ip| ip.is_loopback());
        // only the family the socket serves (a `0.0.0.0` socket is IPv4 only)
        loopback.retain(|ip| ip.is_ipv4() == bind.is_ipv4());
        lan.retain(|ip| ip.is_ipv4() == bind.is_ipv4());
        urls.extend(loopback.into_iter().chain(lan).map(url));
        if urls.is_empty() {
            urls.push(url(&if bind.is_ipv4() {
                IpAddr::from([127, 0, 0, 1])
            } else {
                IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1])
            }));
        }
        if let Some(h) = hostname {
            urls.push(format!("{scheme}://{h}:{port}"));
        }
    } else {
        urls.push(url(&bind));
    }
    urls.dedup();
    urls
}

/// The URL under which a process on this machine reaches the host it listens on: loopback when `bind` is
/// loopback or a wildcard (a wildcard socket answers on loopback), else the one address it is bound to (a
/// host bound to a LAN address does not listen on loopback at all). IPv6 addresses are bracketed.
#[must_use]
pub fn local_url(scheme: &str, bind: IpAddr, port: u16, path: &str) -> String {
    let ip = match bind {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::from([127, 0, 0, 1]),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]),
        other => other,
    };
    match ip {
        IpAddr::V4(v4) => format!("{scheme}://{v4}:{port}{path}"),
        IpAddr::V6(v6) => format!("{scheme}://[{v6}]:{port}{path}"),
    }
}

fn is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn an_unspecified_bind_lists_loopback_each_lan_address_and_the_hostname() {
        let urls = reachable_urls(
            "http",
            "0.0.0.0".parse().unwrap(),
            7400,
            &ips(&["192.168.1.20", "127.0.0.1", "fe80::1", "169.254.3.4", "fd00::20"]),
            Some("studio.local"),
        );
        assert_eq!(urls, ["http://127.0.0.1:7400", "http://192.168.1.20:7400", "http://studio.local:7400"]);
    }

    #[test]
    fn a_local_process_reaches_the_host_on_loopback_or_on_the_one_address_it_is_bound_to() {
        let url = |bind: &str| local_url("http", bind.parse().unwrap(), 7400, "/mcp");
        assert_eq!(url("0.0.0.0"), "http://127.0.0.1:7400/mcp");
        assert_eq!(url("127.0.0.1"), "http://127.0.0.1:7400/mcp");
        assert_eq!(url("192.168.1.20"), "http://192.168.1.20:7400/mcp", "not listening on loopback");
        assert_eq!(url("::"), "http://[::1]:7400/mcp");
        assert_eq!(url("::1"), "http://[::1]:7400/mcp");
        assert_eq!(url("fd00::20"), "http://[fd00::20]:7400/mcp", "an IPv6 address is bracketed");
        assert_eq!(
            local_url("https", "10.0.0.5".parse().unwrap(), 8443, "/mcp"),
            "https://10.0.0.5:8443/mcp"
        );
    }

    #[test]
    fn a_specific_bind_is_just_that_address() {
        let urls =
            reachable_urls("https", "127.0.0.1".parse().unwrap(), 8443, &ips(&["192.168.1.20"]), Some("h"));
        assert_eq!(urls, ["https://127.0.0.1:8443"]);
        let v6 = reachable_urls("http", "::1".parse().unwrap(), 7400, &[], None);
        assert_eq!(v6, ["http://[::1]:7400"]);
    }

    #[test]
    fn an_ipv6_wildcard_lists_the_ipv6_addresses() {
        let urls = reachable_urls(
            "http",
            "::".parse().unwrap(),
            7400,
            &ips(&["192.168.1.20", "::1", "fd00::20"]),
            None,
        );
        assert_eq!(urls, ["http://[::1]:7400", "http://[fd00::20]:7400"]);
    }

    #[test]
    fn with_no_interfaces_known_loopback_is_still_listed() {
        assert_eq!(
            reachable_urls("http", "0.0.0.0".parse().unwrap(), 7400, &[], None),
            ["http://127.0.0.1:7400"]
        );
    }
}

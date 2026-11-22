//! What a redirect does to the next request, after Swift's async-http-client (`RedirectState.swift`), the rules
//! NEOC-259 adopted. Pure functions: the loop that follows redirects is in `exchange`.
//!
//! - **Which statuses.** 301, 302, 303, 307 and 308 with a `Location` are followed. Any other 3xx, and a 3xx without
//!   a `Location`, is the response.
//! - **The method.** 303 becomes `GET` (a `HEAD` stays `HEAD`) and drops the body. 301 and 302 turn a `POST` into a
//!   `GET` and drop the body; every other method keeps both. 307 and 308 keep the method and the body.
//! - **The body's headers.** A request that no longer has a body loses the headers that describe one:
//!   `Content-Length`, `Content-Type`, `Content-Encoding`, `Content-Language`, `Content-Location` and
//!   `Transfer-Encoding` (the Fetch standard's list, plus the two that frame it).
//! - **Another origin.** When the next URL's origin (scheme, host and port) is not the current one, the request loses
//!   `Authorization`, `Cookie`, `Proxy-Authorization` and `Origin`, which belong to the site they were written for,
//!   and a caller-set `Host`, which named the old server. A hop from `https` to `http` is another origin, so it
//!   loses them too.
//! - **The address.** A relative `Location` is resolved against the current URL by the `url` crate (the WHATWG
//!   algorithm); anything that is not an `http` or `https` URL is refused by the caller, before its origin is looked at.

use hyper::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LENGTH, CONTENT_LOCATION,
    CONTENT_TYPE, COOKIE, HOST, HeaderMap, ORIGIN, PROXY_AUTHORIZATION, TRANSFER_ENCODING,
};
use url::Url;

/// Is `status` one of the five that are followed (when the response has a `Location`)?
pub(super) fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// The method of the request a `status` redirect makes of one sent with `method`, and whether it keeps the body.
pub(super) fn rewrite_method(status: u16, method: &str) -> (String, bool) {
    match status {
        303 if method == "HEAD" => ("HEAD".to_owned(), false),
        303 => ("GET".to_owned(), false),
        301 | 302 if method == "POST" => ("GET".to_owned(), false),
        _ => (method.to_owned(), true),
    }
}

/// Take from `headers` what the next request must not carry: the body's headers when the body is dropped, and the
/// origin-bound ones when the next URL is another origin.
pub(super) fn strip_headers(headers: &mut HeaderMap, keeps_body: bool, same_origin: bool) {
    if !keeps_body {
        for name in [
            CONTENT_LENGTH,
            CONTENT_TYPE,
            CONTENT_ENCODING,
            CONTENT_LANGUAGE,
            CONTENT_LOCATION,
            TRANSFER_ENCODING,
        ] {
            headers.remove(name);
        }
    }
    if !same_origin {
        for name in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, ORIGIN, HOST] {
            headers.remove(name);
        }
    }
}

/// The URL a `Location` value names, resolved against `current`; `None` when it does not parse.
pub(super) fn resolve(current: &Url, location: &str) -> Option<Url> {
    current.join(location).ok()
}

/// Are the two URLs the same origin (scheme, host and port)? Compared from the parsed parts, not with
/// `Url::origin()`, which recurses over a nested `blob:` URL (RUSTSEC-2026-0592) and so must not meet a URL a
/// server chose; both URLs are `http` or `https` here, which the caller has checked.
pub(super) fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    #[test]
    fn five_statuses_are_followed_and_the_rest_are_not() {
        for status in [301, 302, 303, 307, 308] {
            assert!(is_redirect(status), "{status}");
        }
        for status in [200, 300, 304, 305, 306, 309, 404] {
            assert!(!is_redirect(status), "{status}");
        }
    }

    #[test]
    fn methods_are_rewritten_as_async_http_client_does() {
        let cases = [
            (303, "POST", "GET", false),
            (303, "PUT", "GET", false),
            (303, "GET", "GET", false),
            (303, "HEAD", "HEAD", false),
            (301, "POST", "GET", false),
            (302, "POST", "GET", false),
            (301, "PUT", "PUT", true),
            (302, "DELETE", "DELETE", true),
            (301, "GET", "GET", true),
            (307, "POST", "POST", true),
            (308, "PUT", "PUT", true),
            (307, "GET", "GET", true),
        ];
        for (status, method, want, keeps) in cases {
            assert_eq!(
                rewrite_method(status, method),
                (want.to_owned(), keeps),
                "{status} {method}"
            );
        }
    }

    fn sample() -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("authorization", "Bearer t"),
            ("cookie", "a=b"),
            ("proxy-authorization", "Basic x"),
            ("origin", "https://a.example"),
            ("host", "custom.example"),
            ("content-length", "3"),
            ("content-type", "text/plain"),
            ("content-encoding", "gzip"),
            ("transfer-encoding", "chunked"),
            ("x-keep", "1"),
        ] {
            h.insert(
                hyper::header::HeaderName::from_static(k),
                HeaderValue::from_static(v),
            );
        }
        h
    }

    fn names(h: &HeaderMap) -> Vec<&str> {
        let mut v: Vec<_> = h.keys().map(|k| k.as_str()).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn a_dropped_body_takes_its_headers_with_it_and_nothing_else() {
        let mut h = sample();
        strip_headers(&mut h, false, true);
        assert_eq!(
            names(&h),
            [
                "authorization",
                "cookie",
                "host",
                "origin",
                "proxy-authorization",
                "x-keep"
            ]
        );
    }

    #[test]
    fn another_origin_loses_the_credentials_and_the_host() {
        let mut h = sample();
        strip_headers(&mut h, true, false);
        assert_eq!(
            names(&h),
            [
                "content-encoding",
                "content-length",
                "content-type",
                "transfer-encoding",
                "x-keep"
            ]
        );
        let mut h = sample();
        strip_headers(&mut h, false, false);
        assert_eq!(names(&h), ["x-keep"]);
    }

    #[test]
    fn the_same_origin_keeps_what_it_had() {
        let mut h = sample();
        strip_headers(&mut h, true, true);
        assert_eq!(h.len(), sample().len());
    }

    #[test]
    fn a_location_is_resolved_against_the_current_url() {
        let base = Url::parse("https://a.example/dir/page?q=1").unwrap();
        let cases = [
            ("/abs", "https://a.example/abs"),
            ("rel", "https://a.example/dir/rel"),
            ("../up", "https://a.example/up"),
            ("?z=2", "https://a.example/dir/page?z=2"),
            ("//b.example/p", "https://b.example/p"),
            ("http://c.example:8080/x", "http://c.example:8080/x"),
        ];
        for (location, want) in cases {
            assert_eq!(
                resolve(&base, location).unwrap().as_str(),
                want,
                "{location}"
            );
        }
    }

    #[test]
    fn origins_compare_scheme_host_and_port() {
        let a = Url::parse("https://a.example/x").unwrap();
        for (other, same) in [
            ("https://a.example/y?q", true),
            ("https://a.example:443/y", true),
            ("https://A.EXAMPLE/y", true),
            ("http://a.example/x", false),
            ("https://a.example:8443/x", false),
            ("https://b.example/x", false),
        ] {
            assert_eq!(
                same_origin(&a, &Url::parse(other).unwrap()),
                same,
                "{other}"
            );
        }
    }
}

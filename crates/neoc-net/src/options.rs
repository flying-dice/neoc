//! A request's options, and the request they describe once checked.
//!
//! A module reads its options table by its own type (so a misspelt option is refused as a wrong argument there) and
//! hands the values here. What a type cannot say (a timeout that is not positive and finite, PEM that holds no
//! certificate, a header name that is not one, a `Content-Length` that is not the body's) is checked here, before
//! anything is sent, and is an `invalid_input` error.

use std::collections::BTreeMap;
use std::time::Duration;

use bytes::Bytes;
use hyper::Method;
use hyper::header::{CONTENT_LENGTH, HeaderMap, HeaderName, HeaderValue, TRANSFER_ENCODING};
use tokio::sync::mpsc;
use tokio_rustls::rustls::RootCertStore;
use tokio_rustls::rustls::pki_types::{CertificateDer, pem::PemObject};

use crate::error::NetError;

/// No data for this long ends a request (`idle_timeout`).
pub const DEFAULT_IDLE: Duration = Duration::from_secs(60);
/// Connecting, TLS handshake included, takes at most this long (`connect_timeout`).
pub const DEFAULT_CONNECT: Duration = Duration::from_secs(10);
/// The most redirects followed (`max_redirects`), `URLSession`'s documented threshold.
pub const DEFAULT_REDIRECTS: u32 = 16;
/// The largest response body read (`max_body`): 64 MiB.
pub const DEFAULT_MAX_BODY: u64 = 64 << 20;

/// The methods HTTP defines (RFC 9110 section 9.1). They are case-sensitive on the wire, so a script that writes one
/// in another case gets the standard method, as Node, Python and the Fetch standard do.
const STANDARD_METHODS: &[&str] =
    &["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH"];

/// What a request sends besides its method, URL and body, and the limits it keeps to.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Headers to send, by name. A `Content-Length` that is not the body's length is refused, a `Host` replaces
    /// the built-in one, and a name given twice in any case or a `Transfer-Encoding` is refused: the module frames
    /// the body.
    pub headers: BTreeMap<String, String>,
    /// PEM certificates to trust instead of the platform's, on every hop.
    pub ca_pem: Option<String>,
    /// Seconds the TCP connect and the TLS handshake may take.
    pub connect_timeout: f64,
    /// Seconds without data moving in either direction; reset whenever any does, so a server that trickles never
    /// trips it.
    pub idle_timeout: f64,
    /// Seconds the whole request may take, redirects and body included, which is what bounds a trickle. No limit
    /// when not given.
    pub timeout: Option<f64>,
    /// Follow 301, 302, 303, 307 and 308; `false` returns the 3xx. A 303 becomes a GET, a POST becomes a GET on 301
    /// and 302, 307 and 308 keep method and body. A hop to another origin drops `Authorization`, `Cookie`,
    /// `Proxy-Authorization`, `Origin` and a caller-set `Host`, and only those: a credential in a header of your
    /// own (`X-Api-Key`) goes to every hop. Nothing refuses a redirect to a host on the local network or from
    /// `https` to `http`: check the response's `url`, or follow redirects yourself.
    pub follow_redirects: bool,
    /// The most redirects to follow; one more is a `too_many_redirects` error.
    pub max_redirects: u32,
    /// The most bytes of response body to read; a longer one fails with `too_large`. The module never
    /// decompresses, so this bounds the bytes on the wire.
    pub max_body: u64,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            headers: BTreeMap::new(),
            ca_pem: None,
            connect_timeout: DEFAULT_CONNECT.as_secs_f64(),
            idle_timeout: DEFAULT_IDLE.as_secs_f64(),
            timeout: None,
            follow_redirects: true,
            max_redirects: DEFAULT_REDIRECTS,
            max_body: DEFAULT_MAX_BODY,
        }
    }
}

/// The body of a request.
#[derive(Debug, Default)]
pub enum RequestBody {
    /// None: a request that carries content (`POST`, `PUT`, `PATCH`) says `Content-Length: 0`.
    #[default]
    Empty,
    /// The whole body, sent with its length.
    Full(Bytes),
    /// A body written as it goes, sent chunked: the chunks in the order they come, until the sender is dropped. An
    /// error ends the request. It is sent once, so a 307 or 308 redirect (which sends the body again) fails.
    Stream(mpsc::Receiver<Result<Bytes, std::io::Error>>),
}

/// The limits of one request.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub connect: Duration,
    pub idle: Duration,
    /// For the whole request, redirects and body included; none by default.
    pub total: Option<Duration>,
    pub max_body: u64,
    /// The most redirects to follow; 0 follows none.
    pub redirects: u32,
}

/// A request as the arguments describe it, checked and ready to send.
pub struct Prepared {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: RequestBody,
    /// The certificates of `ca_pem`, checked: the only ones trusted, on every hop.
    pub(crate) ca_pem: Option<RootCertStore>,
    pub(crate) limits: Limits,
}

impl Prepared {
    /// The request `method url` with `body`, as `opts` says, after the checks the types cannot make.
    pub fn new(method: &str, url: String, body: RequestBody, opts: &ClientOptions) -> Result<Self, NetError> {
        let method = normalize_method(method)?;
        let headers = headers_of(&opts.headers)?;
        // checked before anything is sent and whatever the URL's scheme, so a bad value is never silently unused
        let ca_pem = opts.ca_pem.as_deref().map(roots_of).transpose()?;
        if opts.max_body == 0 {
            return Err(NetError::invalid_input("max_body must be a positive whole number of bytes, got 0"));
        }
        if opts.max_redirects == 0 {
            return Err(NetError::invalid_input(
                "max_redirects must be a positive whole number of redirects, got 0 (follow_redirects = false \
                 follows none)",
            ));
        }
        let limits = Limits {
            connect: seconds("connect_timeout", opts.connect_timeout)?,
            idle: seconds("idle_timeout", opts.idle_timeout)?,
            total: opts.timeout.map(|secs| seconds("timeout", secs)).transpose()?,
            max_body: opts.max_body,
            redirects: if opts.follow_redirects { opts.max_redirects } else { 0 },
        };
        let prepared = Self { method, url, headers, body, ca_pem, limits };
        prepared.check_content_length()?;
        Ok(prepared)
    }

    /// A `Content-Length` the caller set must be the length of the body: a shorter one would cut the body off
    /// silently and a longer one would hang the request.
    fn check_content_length(&self) -> Result<(), NetError> {
        let Some(value) = self.headers.get(CONTENT_LENGTH) else {
            return Ok(());
        };
        let declared: u64 = value
            .to_str()
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| NetError::invalid_input("the Content-Length header is not a whole number"))?;
        let actual = match &self.body {
            RequestBody::Empty => 0,
            RequestBody::Full(bytes) => bytes.len() as u64,
            RequestBody::Stream(_) => {
                return Err(NetError::invalid_input(
                    "a streamed body is sent chunked and has no Content-Length; drop the header",
                ));
            }
        };
        if declared == actual {
            Ok(())
        } else {
            Err(NetError::invalid_input(format!(
                "the Content-Length header says {declared} but the body is {actual} bytes; \
                 drop the header and the module sets it"
            )))
        }
    }
}

/// A number of seconds as a duration: positive and finite.
pub(crate) fn seconds(name: &str, secs: f64) -> Result<Duration, NetError> {
    Duration::try_from_secs_f64(secs).ok().filter(|d| !d.is_zero()).ok_or_else(|| {
        NetError::invalid_input(format!(
            "{name} must be a positive, finite number of seconds that a duration can hold, got {secs}"
        ))
    })
}

/// The certificates of `ca_pem`, each usable as a trust anchor.
pub(crate) fn roots_of(pem: &str) -> Result<RootCertStore, NetError> {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(pem.as_bytes()) {
        let cert = cert.map_err(|e| NetError::invalid_input(format!("ca_pem is not valid PEM: {e}")))?;
        roots
            .add(cert)
            .map_err(|e| NetError::invalid_input(format!("ca_pem holds an unusable certificate: {e}")))?;
    }
    if roots.is_empty() {
        return Err(NetError::invalid_input("ca_pem holds no certificate"));
    }
    Ok(roots)
}

/// A method as the script wrote it: a standard one in any case is the standard one (`put` is `PUT`), any other is
/// sent as written if it is a valid method token.
fn normalize_method(method: &str) -> Result<String, NetError> {
    let upper = method.to_ascii_uppercase();
    let method = if STANDARD_METHODS.contains(&upper.as_str()) { upper } else { method.to_owned() };
    Method::from_bytes(method.as_bytes())
        .map(|_| method)
        .map_err(|_| NetError::invalid_input("method is not a valid HTTP method name"))
}

/// The headers a map of names and values says, checked as header names and values. Names are case-insensitive, so
/// one written twice (`Host` and `host`) is refused rather than sent twice, and `Transfer-Encoding` is refused: the
/// module frames the body, and a second framing would make the request something other than what was asked for.
pub(crate) fn headers_of(given: &BTreeMap<String, String>) -> Result<HeaderMap, NetError> {
    let mut headers = HeaderMap::new();
    for (name, value) in given {
        let header = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| NetError::invalid_input(format!("`{name}` is not a valid header name")))?;
        // `HeaderValue` refuses control characters, CR and LF included
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| NetError::invalid_input(format!("the value of header `{name}` is not valid")))?;
        if header == TRANSFER_ENCODING {
            return Err(NetError::invalid_input(
                "the Transfer-Encoding header is set by the module; drop it",
            ));
        }
        if headers.contains_key(&header) {
            return Err(NetError::invalid_input(format!(
                "header `{header}` is given twice (header names are case-insensitive)"
            )));
        }
        headers.insert(header, value);
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(prepared: Result<Prepared, NetError>) -> String {
        match prepared {
            Err(NetError::InvalidInput { reason }) => reason,
            Err(other) => panic!("expected invalid_input, got {other:?}"),
            Ok(_) => panic!("expected invalid_input, got a request"),
        }
    }

    fn get(opts: ClientOptions) -> Result<Prepared, NetError> {
        Prepared::new("GET", "http://localhost/".to_owned(), RequestBody::Empty, &opts)
    }

    #[test]
    fn the_defaults_are_the_documented_limits() {
        let prepared = get(ClientOptions::default()).unwrap();
        assert_eq!(prepared.limits.connect, Duration::from_secs(10));
        assert_eq!(prepared.limits.idle, Duration::from_secs(60));
        assert_eq!(prepared.limits.total, None);
        assert_eq!(prepared.limits.max_body, 64 << 20);
        assert_eq!(prepared.limits.redirects, 16);
        assert!(prepared.headers.is_empty() && prepared.ca_pem.is_none());
        assert!(matches!(prepared.body, RequestBody::Empty));
    }

    #[test]
    fn follow_redirects_false_follows_none_whatever_the_limit() {
        let opts = ClientOptions { follow_redirects: false, max_redirects: 5, ..ClientOptions::default() };
        assert_eq!(get(opts).unwrap().limits.redirects, 0);
        let opts = ClientOptions { max_redirects: 5, ..ClientOptions::default() };
        assert_eq!(get(opts).unwrap().limits.redirects, 5);
    }

    #[test]
    fn a_time_limit_is_positive_and_finite() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            let reason = refused(get(ClientOptions { connect_timeout: bad, ..ClientOptions::default() }));
            assert!(reason.starts_with("connect_timeout must be"), "{reason}");
            let reason = refused(get(ClientOptions { idle_timeout: bad, ..ClientOptions::default() }));
            assert!(reason.starts_with("idle_timeout must be"), "{reason}");
            let reason = refused(get(ClientOptions { timeout: Some(bad), ..ClientOptions::default() }));
            assert!(reason.starts_with("timeout must be"), "{reason}");
        }
        // a very large one is in effect no limit, and is taken
        let opts = ClientOptions {
            timeout: Some(1e9),
            idle_timeout: 1e9,
            connect_timeout: 1e9,
            ..Default::default()
        };
        assert_eq!(get(opts).unwrap().limits.total, Some(Duration::from_secs(1_000_000_000)));
    }

    #[test]
    fn a_size_or_hop_limit_of_zero_is_refused() {
        let reason = refused(get(ClientOptions { max_body: 0, ..ClientOptions::default() }));
        assert!(reason.starts_with("max_body must be"), "{reason}");
        let reason = refused(get(ClientOptions { max_redirects: 0, ..ClientOptions::default() }));
        assert!(reason.starts_with("max_redirects must be"), "{reason}");
    }

    #[test]
    fn a_standard_method_in_any_case_is_the_standard_method() {
        assert_eq!(normalize_method("put").unwrap(), "PUT");
        assert_eq!(normalize_method("Patch").unwrap(), "PATCH");
        assert_eq!(normalize_method("PuRgE").unwrap(), "PuRgE");
        assert!(normalize_method("GE T").is_err() && normalize_method("").is_err());
    }

    #[test]
    fn headers_are_checked_as_names_and_values() {
        let with = |pairs: &[(&str, &str)]| {
            let headers = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            Prepared::new(
                "POST",
                "http://localhost/".to_owned(),
                RequestBody::Full(Bytes::from_static(b"abc")),
                &ClientOptions { headers, ..ClientOptions::default() },
            )
        };
        assert!(refused(with(&[("bad name", "x")])).contains("is not a valid header name"));
        assert!(refused(with(&[("x-a", "line\r\nx-b: injected")])).contains("is not valid"));
        assert!(refused(with(&[("Host", "a.example"), ("host", "b.example")])).contains("twice"));
        assert!(refused(with(&[("Transfer-Encoding", "chunked")])).contains("Transfer-Encoding"));
        assert!(refused(with(&[("Content-Length", "1")])).contains("Content-Length"));
        assert!(refused(with(&[("Content-Length", "three")])).contains("Content-Length"));
        let sent = with(&[("content-LENGTH", "3"), ("X-Keep", "yes")]).unwrap();
        assert_eq!(sent.headers.len(), 2);
    }

    #[test]
    fn ca_pem_must_hold_a_usable_certificate() {
        let with =
            |pem: &str| get(ClientOptions { ca_pem: Some(pem.to_owned()), ..ClientOptions::default() });
        assert_eq!(refused(with("nonsense")), "ca_pem holds no certificate");
        let reason = refused(with("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"));
        assert!(reason.starts_with("ca_pem holds an unusable certificate"), "{reason}");
    }
}

//! One request, from the first connection to the last byte of the body: connect, send, follow redirects, read the
//! body under the cap. The rules each step follows are in the module docs of `redirect` and `idle`; the limits
//! (`options::Limits`) apply as follows.
//!
//! - `connect_timeout` bounds each connection: the name lookup, the TCP connect and the TLS handshake. The TCP part is
//!   hyper-util's `HttpConnector`, which races the addresses a name resolves to (Happy Eyeballs, RFC 8305), so a host
//!   whose IPv6 address does not answer is reached over IPv4 instead of using up the whole timeout on the first address.
//! - `idle_timeout` is measured on each socket (see `idle`).
//! - `timeout`, when given, bounds [`run`] as a whole: every hop and the body.
//! - `max_body` bounds the body of the response that is returned. The bodies of redirect responses are not read: the
//!   connection is dropped, so a hop cannot make the client buffer anything.
//!
//! The module never decodes `Content-Encoding` and sends no `Accept-Encoding`: the body is the bytes on the wire,
//! so the cap is a cap on what is read from the socket and a small response cannot expand past it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body as _, Incoming};
use hyper::header::{HOST, HeaderMap, HeaderValue, LOCATION};
use hyper::{Request as HyperRequest, Response as HyperResponse};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::task::AbortHandle;
use tokio_rustls::rustls::pki_types::ServerName;
use url::{Position, Url};

use super::error::{HyperError, TimeLimit, chain};
use super::idle::{Fired, IdleIo};
use super::options::{Limits, Prepared};
use super::redirect;
use super::tls::Tls;
use neoc_vm_core::lifecycle::Life;

/// A finished response: the last one of the redirect chain, its body read in full.
pub(crate) struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The URL the response came from: the last one of the chain.
    pub url: String,
}

/// Perform the request `prepared` describes. The connections it opens belong to `life`: a shut-down engine closes
/// them.
pub(crate) async fn run(prepared: Prepared, life: Life) -> Result<Response, HyperError> {
    let total = prepared.limits.total;
    let work = exchange(prepared, life);
    match total {
        None => work.await,
        // dropping `work` drops its connection, which stops the driver
        Some(limit) => tokio::time::timeout(limit, work).await.unwrap_or_else(|_| {
            Err(HyperError::timed_out(
                TimeLimit::Timeout,
                format!("the request took longer than {}s", secs(limit)),
            ))
        }),
    }
}

async fn exchange(prepared: Prepared, life: Life) -> Result<Response, HyperError> {
    let max_body = prepared.limits.max_body;
    let Head { resp, conn, url } = head(prepared, life).await?;
    let status = resp.status().as_u16();
    let status_headers = super::super::collect_headers(resp.headers());
    let body = read_body(resp, max_body, &conn).await?;
    Ok(Response { status, headers: status_headers, body, url: url.to_string() })
}

/// The response the request ends at (the last of the redirect chain), its body not read yet, on its connection.
struct Head {
    resp: HyperResponse<Incoming>,
    conn: Connection,
    url: Url,
}

async fn head(prepared: Prepared, life: Life) -> Result<Head, HyperError> {
    let Prepared { mut method, url, mut headers, mut body, ca_pem, limits } = prepared;
    let mut url = Url::parse(&url).map_err(|e| HyperError::invalid_url(e.to_string()))?;
    let mut tls = Tls::new(ca_pem);
    let mut followed = 0_u32;
    loop {
        let target = Target::new(&url)?;
        let mut conn = Connection::open(&target, &mut tls, &limits, &life).await?;
        let resp = conn.send(build_request(&method, &target, &headers, body.clone())?).await?;
        let status = resp.status().as_u16();
        if limits.redirects > 0
            && redirect::is_redirect(status)
            && let Some(location) = resp.headers().get(LOCATION)
        {
            if followed >= limits.redirects {
                return Err(HyperError::TooManyRedirects { max_redirects: limits.redirects });
            }
            followed += 1;
            let next = next_url(&url, status, location)?;
            let (next_method, keeps_body) = redirect::rewrite_method(status, &method);
            redirect::strip_headers(&mut headers, keeps_body, redirect::same_origin(&url, &next));
            method = next_method;
            if !keeps_body {
                body = None;
            }
            url = next;
            // `resp` and `conn` drop here: the redirect's body is never read and its connection closes
            continue;
        }
        return Ok(Head { resp, conn, url });
    }
}

/// The URL a redirect's `Location` names, which must be an `http` or `https` URL.
fn next_url(current: &Url, status: u16, location: &HeaderValue) -> Result<Url, HyperError> {
    let invalid = |why: String| HyperError::invalid_url(format!("the {status} response's Location {why}"));
    // UTF-8, not only visible ASCII: servers do send non-ASCII paths, and the `url` crate percent-encodes them
    let location =
        std::str::from_utf8(location.as_bytes()).map_err(|_| invalid("is not valid UTF-8".to_owned()))?;
    let next =
        redirect::resolve(current, location).ok_or_else(|| invalid("is not a valid URL".to_owned()))?;
    match next.scheme() {
        "http" | "https" => Ok(next),
        other => Err(invalid(format!("is a {other}: URL; only http:// and https:// redirects are followed"))),
    }
}

/// Read the body of `resp`, at most `max` bytes.
async fn read_body(
    resp: HyperResponse<Incoming>,
    max: u64,
    conn: &Connection,
) -> Result<Vec<u8>, HyperError> {
    let too_large = || HyperError::TooLarge { max_body: max };
    let mut body = resp.into_body();
    // a declared length over the cap is refused before a byte is read; it is only a hint, so the cap is enforced
    // on what actually arrives too (a chunked body has no length)
    let declared = body.size_hint().exact();
    if declared.is_some_and(|n| n > max) {
        return Err(too_large());
    }
    let mut buf = Vec::with_capacity(usize::try_from(declared.unwrap_or(0).min(1 << 20)).unwrap_or(0));
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| HyperError::from_hyper(&e, conn.fired.after()))?;
        if let Ok(data) = frame.into_data() {
            if (buf.len() as u64).saturating_add(data.len() as u64) > max {
                return Err(too_large());
            }
            buf.extend_from_slice(&data);
        }
    }
    // hyper ends a body the same way whether it arrived in full or its connection's driver was aborted (the engine
    // shut down, or stopped its background work): only the driver knows, and a cut body is never a whole one
    conn.ended_early()?;
    Ok(buf)
}

/// The request to send: the method, the target in origin form, the caller's headers and a `Host` of ours when they
/// have none, and the body.
fn build_request(
    method: &str,
    target: &Target<'_>,
    headers: &HeaderMap,
    body: Option<Bytes>,
) -> Result<HyperRequest<Full<Bytes>>, HyperError> {
    // hyper writes `content-length` for a non-empty body only. A request that carries content (a body was given, or
    // the method defines content: POST, PUT, PATCH) says `0` when it is empty, as RFC 9110 section 8.6 asks: a
    // strict server answers 411 without it. A length the caller set is left alone (it was checked against the body).
    let carries_content = body.is_some() || matches!(method, "POST" | "PUT" | "PATCH");
    let bytes = body.unwrap_or_default();
    let mut req = HyperRequest::builder()
        .method(method)
        .uri(target.request_target())
        .body(Full::new(bytes.clone()))
        .map_err(|e| HyperError::invalid_input(e.to_string()))?;
    let out = req.headers_mut();
    // a caller-set Host replaces ours: one `Host` line, as HTTP/1.1 requires
    if !headers.contains_key(HOST) {
        let host = HeaderValue::from_str(&target.host_header())
            .map_err(|e| HyperError::invalid_url(e.to_string()))?;
        out.insert(HOST, host);
    }
    for (name, value) in headers {
        out.append(name.clone(), value.clone());
    }
    if bytes.is_empty() && carries_content && !out.contains_key(hyper::header::CONTENT_LENGTH) {
        out.insert(hyper::header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    }
    Ok(req)
}

/// A parsed request URL: `http://` or `https://`, with a host.
struct Target<'u> {
    url: &'u Url,
    /// The host without the brackets of an IPv6 literal: what to connect to and what the certificate names.
    host: &'u str,
    port: u16,
    tls: bool,
}

impl<'u> Target<'u> {
    fn new(url: &'u Url) -> Result<Self, HyperError> {
        let (tls, default_port) = match url.scheme() {
            "http" => (false, 80),
            "https" => (true, 443),
            _ => {
                return Err(HyperError::invalid_url("only http:// and https:// URLs are supported"));
            }
        };
        let host = url
            .host_str()
            .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
            .filter(|h| !h.is_empty())
            .ok_or_else(|| HyperError::invalid_url("URL has no host"))?;
        Ok(Self { url, host, port: url.port().unwrap_or(default_port), tls })
    }

    /// The path and query: the request target in origin form (the fragment is never sent).
    fn request_target(&self) -> &str {
        &self.url[Position::BeforePath..Position::AfterQuery]
    }

    /// The `Host` header: the host, and the port when it is not the scheme's own.
    fn host_header(&self) -> String {
        let host = self.url.host_str().unwrap_or(self.host);
        match self.url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }
}

/// An open connection: the sender, and the driver task that moves its bytes, which stops when this is dropped.
struct Connection {
    sender: hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    driver: AbortHandle,
    fired: Fired,
    /// Set when the driver ended without finishing (aborted with the engine's `Life`, or panicked), before the
    /// response body it was feeding learns of it.
    lost: Arc<AtomicBool>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

impl Connection {
    /// Connect to the target (over TLS for `https`) within `limits.connect` and complete the HTTP/1.1 handshake.
    async fn open(
        target: &Target<'_>,
        tls: &mut Tls,
        limits: &Limits,
        life: &Life,
    ) -> Result<Self, HyperError> {
        let connector = if target.tls { Some(tls.connector()?) } else { None };
        let fired = Fired::new(limits.idle);
        let attempt = async {
            let stream = tcp_connect(target).await?;
            let io = IdleIo::new(stream, fired.clone());
            match connector {
                None => handshake(TokioIo::new(io), life, &fired).await,
                Some(connector) => {
                    let name = ServerName::try_from(target.host.to_owned())
                        .map_err(|e| HyperError::invalid_url(format!("{e}")))?;
                    let io = connector.connect(name, io).await.map_err(|e| match fired.after() {
                        Some(idle) => HyperError::idle(idle),
                        None => HyperError::tls(chain(&e)),
                    })?;
                    handshake(TokioIo::new(io), life, &fired).await
                }
            }
        };
        tokio::time::timeout(limits.connect, attempt).await.unwrap_or_else(|_| {
            Err(HyperError::timed_out(
                TimeLimit::ConnectTimeout,
                format!("connecting took longer than {}s", secs(limits.connect)),
            ))
        })
    }

    /// `connection_lost` when the driver ended without finishing: a body that "ended" then was cut off.
    fn ended_early(&self) -> Result<(), HyperError> {
        if self.lost.load(Ordering::SeqCst) {
            return Err(HyperError::connection_lost(
                "the connection was closed before the response body ended",
            ));
        }
        Ok(())
    }

    async fn send(&mut self, req: HyperRequest<Full<Bytes>>) -> Result<HyperResponse<Incoming>, HyperError> {
        self.sender.send_request(req).await.map_err(|e| HyperError::from_hyper(&e, self.fired.after()))
    }
}

/// The TCP connection to `target`, through hyper-util's `HttpConnector`: the name lookup, then the addresses raced
/// (the other address family 300 ms after the first). No timeout of its own: `connect_timeout` bounds the whole attempt.
async fn tcp_connect(target: &Target<'_>) -> Result<tokio::net::TcpStream, HyperError> {
    use tower_service::Service as _;
    let mut connector = HttpConnector::new();
    // the scheme is ours to check (`Target::new`); the connector only connects
    connector.enforce_http(false);
    // requests are small and one-shot: no waiting to coalesce them
    connector.set_nodelay(true);
    let authority = match target.url.host() {
        Some(url::Host::Ipv6(ip)) => format!("[{ip}]:{}", target.port),
        _ => format!("{}:{}", target.host, target.port),
    };
    let uri: hyper::Uri = format!("http://{authority}/")
        .parse()
        .map_err(|e: hyper::http::uri::InvalidUri| HyperError::invalid_url(e.to_string()))?;
    std::future::poll_fn(|cx| connector.poll_ready(cx)).await.map_err(|e| HyperError::connect(chain(&e)))?;
    let io = connector.call(uri).await.map_err(|e| HyperError::connect(chain(&e)))?;
    Ok(io.into_inner())
}

async fn handshake<T>(io: TokioIo<T>, life: &Life, fired: &Fired) -> Result<Connection, HyperError>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| HyperError::from_hyper(&e, fired.after()))?;
    // the driver is registered with the engine's `Life`, so shutting the engine down closes the connection; a panic
    // in it ends this connection only
    let lost = Arc::new(AtomicBool::new(false));
    let panicked = Arc::clone(&lost);
    let driver = life.spawn(Driver {
        conn: Box::pin(async move {
            if let Err(message) = neoc_vm_core::panics::caught(conn).await {
                panicked.store(true, Ordering::SeqCst);
                tracing::error!(target: "lua::lib::hyper", "connection driver panicked: {message}");
            }
        }),
        finished: false,
        lost: Arc::clone(&lost),
    });
    Ok(Connection { sender, driver: driver.abort_handle(), fired: fired.clone(), lost })
}

/// The task that drives a connection. Dropped before it finished (aborted), it sets `lost` first, and only then drops
/// the connection and with it the sender of the response body: whoever reads the body sees `lost` when the body
/// ends.
struct Driver<F> {
    conn: Pin<Box<F>>,
    finished: bool,
    lost: Arc<AtomicBool>,
}

impl<F: Future<Output = ()>> Future for Driver<F> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let done = self.conn.as_mut().poll(cx);
        if done.is_ready() {
            self.finished = true;
        }
        done
    }
}

impl<F> Drop for Driver<F> {
    fn drop(&mut self) {
        if !self.finished {
            self.lost.store(true, Ordering::SeqCst);
        }
        // `conn` (and the body's sender in it) drops after this
    }
}

fn secs(d: Duration) -> f64 {
    d.as_secs_f64()
}

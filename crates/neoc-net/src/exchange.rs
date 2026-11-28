//! One request, from the first connection to the last byte of the body: connect, send, follow redirects, read the
//! body. The rules each step follows are in the module docs of `redirect` and `idle`; the limits
//! (`options::Limits`) apply as follows.
//!
//! - `connect_timeout` bounds each connection: the name lookup, the TCP connect and the TLS handshake. The TCP part is
//!   hyper-util's `HttpConnector`, which races the addresses a name resolves to (Happy Eyeballs, RFC 8305), so a host
//!   whose IPv6 address does not answer is reached over IPv4 instead of using up the whole timeout on the first address.
//! - `idle_timeout` is measured on each socket (see `idle`).
//! - `timeout`, when given, bounds the request as a whole: every hop, and every read of the body until its end.
//! - `max_body` bounds a body read whole ([`Body::read_all`]). A body read chunk by chunk ([`Body::chunk`]) has no
//!   cap: the reader decides how much it keeps. The bodies of redirect responses are not read: the connection is
//!   dropped, so a hop cannot make the client buffer anything.
//!
//! The module never decodes `Content-Encoding` and sends no `Accept-Encoding`: the body is the bytes on the wire,
//! so the cap is a cap on what is read from the socket and a small response cannot expand past it.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Body as _, Frame, Incoming};
use hyper::header::{HOST, HeaderMap, HeaderValue, LOCATION};
use hyper::{Request as HyperRequest, Response as HyperResponse};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio::time::Instant;
use tokio_rustls::rustls::pki_types::ServerName;
use url::{Position, Url};

use crate::Spawner;
use crate::error::{NetError, TimeLimit, chain};
use crate::idle::{Fired, IdleIo};
use crate::options::{Limits, Prepared, RequestBody};
use crate::redirect;
use crate::tls::Tls;

/// The body a request is sent with: hyper's, boxed, whatever it was made from.
type SendBody = BoxBody<Bytes, std::io::Error>;

/// A response: the last one of the redirect chain, its body not read yet.
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The headers, one entry per name (see [`crate::collect_headers`]).
    pub headers: Vec<(String, String)>,
    /// The URL the response came from: the last one of the chain.
    pub url: String,
    /// At least one redirect was followed.
    pub redirected: bool,
    /// The body, read as it arrives.
    pub body: Body,
}

/// A finished response: the last one of the redirect chain, its body read in full.
pub struct FullResponse {
    /// The status code.
    pub status: u16,
    /// The headers, one entry per name.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
    /// The URL the response came from: the last one of the chain.
    pub url: String,
}

/// Perform the request `prepared` describes and read the whole body, at most `max_body` bytes. The connections it
/// opens are spawned with `spawn`: a spawner that aborts them (an engine shutting down) closes them.
pub async fn fetch(prepared: Prepared, spawn: &Spawner) -> Result<FullResponse, NetError> {
    let max_body = prepared.limits.max_body;
    let response = send(prepared, spawn).await?;
    let Response { status, headers, url, body, .. } = response;
    let body = body.read_all(max_body).await?;
    Ok(FullResponse { status, headers, body, url })
}

/// Perform the request `prepared` describes, up to the head of the response the chain ends at. The body is read
/// with [`Body::chunk`] or [`Body::read_all`]; the whole-request `timeout` goes on applying to those reads.
pub async fn send(prepared: Prepared, spawn: &Spawner) -> Result<Response, NetError> {
    let total = prepared.limits.total;
    let deadline = total.map(|limit| (Instant::now() + limit, limit));
    let work = head(prepared, spawn);
    let head = match deadline {
        None => work.await?,
        // dropping `work` drops its connection, which stops the driver
        Some((at, limit)) => tokio::time::timeout_at(at, work).await.unwrap_or_else(|_| Err(whole(limit)))?,
    };
    let Head { resp, conn, url, redirected } = head;
    let status = resp.status().as_u16();
    let headers = crate::collect_headers(resp.headers());
    Ok(Response {
        status,
        headers,
        url: url.to_string(),
        redirected,
        body: Body { incoming: resp.into_body(), conn, deadline, done: false },
    })
}

fn whole(limit: Duration) -> NetError {
    NetError::timed_out(TimeLimit::Timeout, format!("the request took longer than {}s", secs(limit)))
}

/// A response body on its connection. Dropping it closes the connection.
pub struct Body {
    incoming: Incoming,
    conn: Connection,
    /// When the whole-request `timeout` passes, and how long it was.
    deadline: Option<(Instant, Duration)>,
    done: bool,
}

impl Body {
    /// The length the response declares (`Content-Length`), when it declares one.
    pub fn declared_len(&self) -> Option<u64> {
        self.incoming.size_hint().exact()
    }

    /// The next piece of the body as it arrived; `None` at its end.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, NetError> {
        if self.done {
            return Ok(None);
        }
        let next = async {
            loop {
                match self.incoming.frame().await {
                    None => {
                        // hyper ends a body the same way whether it arrived in full or its connection's driver was
                        // aborted (the engine shut down): only the driver knows, and a cut body is never a whole one
                        self.conn.ended_early()?;
                        return Ok(None);
                    }
                    Some(Err(e)) => return Err(NetError::from_hyper(&e, self.conn.fired.after())),
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data()
                            && !data.is_empty()
                        {
                            return Ok(Some(data));
                        }
                    }
                }
            }
        };
        let read = match self.deadline {
            None => next.await,
            Some((at, limit)) => {
                tokio::time::timeout_at(at, next).await.unwrap_or_else(|_| Err(whole(limit)))
            }
        };
        if !matches!(read, Ok(Some(_))) {
            self.done = true;
        }
        read
    }

    /// The rest of the body, at most `max` bytes: a longer one is `too_large`, a declared length over the cap
    /// before a byte is read.
    pub async fn read_all(mut self, max: u64) -> Result<Vec<u8>, NetError> {
        let too_large = || NetError::TooLarge { max_body: max };
        // a declared length is only a hint, so the cap is enforced on what actually arrives too (a chunked body has
        // no length)
        let declared = self.declared_len();
        if declared.is_some_and(|n| n > max) {
            return Err(too_large());
        }
        let mut buf = Vec::with_capacity(usize::try_from(declared.unwrap_or(0).min(1 << 20)).unwrap_or(0));
        while let Some(data) = self.chunk().await? {
            if (buf.len() as u64).saturating_add(data.len() as u64) > max {
                return Err(too_large());
            }
            buf.extend_from_slice(&data);
        }
        Ok(buf)
    }
}

/// The response the request ends at (the last of the redirect chain), its body not read yet, on its connection.
struct Head {
    resp: HyperResponse<Incoming>,
    conn: Connection,
    url: Url,
    redirected: bool,
}

/// The body of each hop: a whole body is sent again on a redirect that keeps it; a streamed one only once.
enum Sending {
    Empty,
    Full(Bytes),
    Stream(Option<mpsc::Receiver<Result<Bytes, std::io::Error>>>),
}

impl Sending {
    fn has_content(&self) -> bool {
        !matches!(self, Self::Empty)
    }

    /// The body of the next hop. A stream that was sent already is gone: `None`.
    fn next(&mut self) -> Option<(SendBody, bool)> {
        match self {
            Self::Empty => Some((Full::new(Bytes::new()).map_err(never).boxed(), true)),
            Self::Full(bytes) => Some((Full::new(bytes.clone()).map_err(never).boxed(), bytes.is_empty())),
            Self::Stream(rx) => {
                let rx = rx.take()?;
                let frames = futures_util::stream::unfold(rx, |mut rx| async move {
                    rx.recv().await.map(|chunk| (chunk.map(Frame::data), rx))
                });
                Some((BodyExt::boxed(StreamBody::new(frames)), false))
            }
        }
    }
}

fn never(e: Infallible) -> std::io::Error {
    match e {}
}

async fn head(prepared: Prepared, spawn: &Spawner) -> Result<Head, NetError> {
    let Prepared { mut method, url, mut headers, body, ca_pem, limits } = prepared;
    let mut url = Url::parse(&url).map_err(|e| NetError::invalid_url(e.to_string()))?;
    let mut tls = Tls::new(ca_pem);
    let mut body = match body {
        RequestBody::Empty => Sending::Empty,
        RequestBody::Full(bytes) => Sending::Full(bytes),
        RequestBody::Stream(rx) => Sending::Stream(Some(rx)),
    };
    let mut followed = 0_u32;
    loop {
        let target = Target::new(&url)?;
        let carries_content = body.has_content() || matches!(method.as_str(), "POST" | "PUT" | "PATCH");
        let (sent, empty) = body.next().ok_or_else(|| NetError::Other {
            reason: "the redirect asks for the body again, and a streamed body is sent only once".to_owned(),
        })?;
        let mut conn = Connection::open(&target, &mut tls, &limits, spawn).await?;
        let resp =
            conn.send(build_request(&method, &target, &headers, sent, carries_content && empty)?).await?;
        let status = resp.status().as_u16();
        if limits.redirects > 0
            && redirect::is_redirect(status)
            && let Some(location) = resp.headers().get(LOCATION)
        {
            if followed >= limits.redirects {
                return Err(NetError::TooManyRedirects { max_redirects: limits.redirects });
            }
            followed += 1;
            let next = next_url(&url, status, location)?;
            let (next_method, keeps_body) = redirect::rewrite_method(status, &method);
            redirect::strip_headers(&mut headers, keeps_body, redirect::same_origin(&url, &next));
            method = next_method;
            if !keeps_body {
                body = Sending::Empty;
            }
            url = next;
            // `resp` and `conn` drop here: the redirect's body is never read and its connection closes
            continue;
        }
        return Ok(Head { resp, conn, url, redirected: followed > 0 });
    }
}

/// The URL a redirect's `Location` names, which must be an `http` or `https` URL.
fn next_url(current: &Url, status: u16, location: &HeaderValue) -> Result<Url, NetError> {
    let invalid = |why: String| NetError::invalid_url(format!("the {status} response's Location {why}"));
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

/// The request to send: the method, the target in origin form, the caller's headers and a `Host` of ours when they
/// have none, and the body. `says_empty` adds `Content-Length: 0`.
fn build_request(
    method: &str,
    target: &Target<'_>,
    headers: &HeaderMap,
    body: SendBody,
    says_empty: bool,
) -> Result<HyperRequest<SendBody>, NetError> {
    // hyper writes `content-length` for a non-empty body of known length only, and sends a body of unknown length
    // chunked. A request that carries content (a body was given, or the method defines content: POST, PUT, PATCH)
    // says `0` when it is empty, as RFC 9110 section 8.6 asks: a strict server answers 411 without it. A length the
    // caller set is left alone (it was checked against the body).
    let mut req = HyperRequest::builder()
        .method(method)
        .uri(target.request_target())
        .body(body)
        .map_err(|e| NetError::invalid_input(e.to_string()))?;
    let out = req.headers_mut();
    // a caller-set Host replaces ours: one `Host` line, as HTTP/1.1 requires
    if !headers.contains_key(HOST) {
        let host =
            HeaderValue::from_str(&target.host_header()).map_err(|e| NetError::invalid_url(e.to_string()))?;
        out.insert(HOST, host);
    }
    for (name, value) in headers {
        out.append(name.clone(), value.clone());
    }
    if says_empty && !out.contains_key(hyper::header::CONTENT_LENGTH) {
        out.insert(hyper::header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    }
    Ok(req)
}

/// A parsed request URL: `http://` or `https://`, with a host.
pub(crate) struct Target<'u> {
    url: &'u Url,
    /// The host without the brackets of an IPv6 literal: what to connect to and what the certificate names.
    pub(crate) host: &'u str,
    port: u16,
    tls: bool,
}

impl<'u> Target<'u> {
    pub(crate) fn new(url: &'u Url) -> Result<Self, NetError> {
        let (tls, default_port) = match url.scheme() {
            "http" => (false, 80),
            "https" => (true, 443),
            _ => {
                return Err(NetError::invalid_url("only http:// and https:// URLs are supported"));
            }
        };
        let host = url
            .host_str()
            .map(|h| h.trim_start_matches('[').trim_end_matches(']'))
            .filter(|h| !h.is_empty())
            .ok_or_else(|| NetError::invalid_url("URL has no host"))?;
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
    sender: hyper::client::conn::http1::SendRequest<SendBody>,
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
        spawn: &Spawner,
    ) -> Result<Self, NetError> {
        let connector = if target.tls { Some(tls.connector()?) } else { None };
        let fired = Fired::new(limits.idle);
        let attempt = async {
            let stream = tcp_connect(target).await?;
            let io = IdleIo::new(stream, fired.clone());
            match connector {
                None => handshake(TokioIo::new(io), spawn, &fired).await,
                Some(connector) => {
                    let name = ServerName::try_from(target.host.to_owned())
                        .map_err(|e| NetError::invalid_url(format!("{e}")))?;
                    let io = connector.connect(name, io).await.map_err(|e| match fired.after() {
                        Some(idle) => NetError::idle(idle),
                        None => NetError::tls(chain(&e)),
                    })?;
                    handshake(TokioIo::new(io), spawn, &fired).await
                }
            }
        };
        tokio::time::timeout(limits.connect, attempt).await.unwrap_or_else(|_| {
            Err(NetError::timed_out(
                TimeLimit::ConnectTimeout,
                format!("connecting took longer than {}s", secs(limits.connect)),
            ))
        })
    }

    /// `connection_lost` when the driver ended without finishing: a body that "ended" then was cut off.
    fn ended_early(&self) -> Result<(), NetError> {
        if self.lost.load(Ordering::SeqCst) {
            return Err(NetError::connection_lost(
                "the connection was closed before the response body ended",
            ));
        }
        Ok(())
    }

    async fn send(&mut self, req: HyperRequest<SendBody>) -> Result<HyperResponse<Incoming>, NetError> {
        self.sender.send_request(req).await.map_err(|e| NetError::from_hyper(&e, self.fired.after()))
    }
}

/// The TCP connection to `target`, through hyper-util's `HttpConnector`: the name lookup, then the addresses raced
/// (the other address family 300 ms after the first). No timeout of its own: `connect_timeout` bounds the whole attempt.
pub(crate) async fn tcp_connect(target: &Target<'_>) -> Result<tokio::net::TcpStream, NetError> {
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
        .map_err(|e: hyper::http::uri::InvalidUri| NetError::invalid_url(e.to_string()))?;
    std::future::poll_fn(|cx| connector.poll_ready(cx)).await.map_err(|e| NetError::connect(chain(&e)))?;
    let io = connector.call(uri).await.map_err(|e| NetError::connect(chain(&e)))?;
    Ok(io.into_inner())
}

async fn handshake<T>(io: TokioIo<T>, spawn: &Spawner, fired: &Fired) -> Result<Connection, NetError>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| NetError::from_hyper(&e, fired.after()))?;
    // the driver is spawned by the caller's spawner (an engine registers it, so shutting the engine down closes the
    // connection); a panic in it ends this connection only
    let lost = Arc::new(AtomicBool::new(false));
    let panicked = Arc::clone(&lost);
    let driver = spawn(Box::pin(Driver {
        conn: Box::pin(async move {
            if let Err(message) = crate::caught(conn).await {
                panicked.store(true, Ordering::SeqCst);
                tracing::error!(target: "neoc_net", "connection driver panicked: {message}");
            }
        }),
        finished: false,
        lost: Arc::clone(&lost),
    }));
    Ok(Connection { sender, driver, fired: fired.clone(), lost })
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

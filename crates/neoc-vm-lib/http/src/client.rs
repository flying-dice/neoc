//! The client half of `lib:http`: what `fetch`, `request` and `events` are made of. The requests are `neoc-net`'s (the
//! limits NEOC-259 decided, TLS by the platform verifier); this file holds the module's error, the line and event
//! streams over a response body, and the reconnecting event source.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use mlua::Lua;
use mlua_ir::{LuaError, Shape};
use neoc_net::{ClientOptions, NetError, Prepared, RequestBody, Spawner};
use serde::{Deserialize, Serialize};

use crate::sse;
use neoc_vm_core::lifecycle::Life;

/// Which time limit a `timed_out` request passed: the option that sets it, or the operating system's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "lib:http")]
pub enum TimeLimit {
    /// Connecting (the name lookup, the TCP connect and the TLS handshake) took longer than `connect_timeout`.
    ConnectTimeout,
    /// No data moved in either direction for `idle_timeout`.
    IdleTimeout,
    /// The whole request, redirects and body included, took longer than `timeout`.
    Timeout,
    /// The operating system gave up on the connection by a limit of its own.
    System,
}

impl From<neoc_net::TimeLimit> for TimeLimit {
    fn from(limit: neoc_net::TimeLimit) -> Self {
        match limit {
            neoc_net::TimeLimit::ConnectTimeout => Self::ConnectTimeout,
            neoc_net::TimeLimit::IdleTimeout => Self::IdleTimeout,
            neoc_net::TimeLimit::Timeout => Self::Timeout,
            neoc_net::TimeLimit::System => Self::System,
        }
    }
}

/// Why a request failed. The kind means the same on every platform; `reason` is for a person or a log. A response
/// with a 4xx or 5xx status is a response, not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "lib:http")]
pub enum FetchError {
    /// An option, a header, the method or the body was refused before anything was sent, or a call came after the
    /// thing it acts on was done (a write after `finish`).
    #[error("{reason}")]
    InvalidInput {
        /// What was refused, and why.
        reason: String,
    },
    /// The URL (or a redirect's `Location`) is not an `http` or `https` URL with a host.
    #[error("{reason}")]
    InvalidUrl {
        /// What is wrong with it.
        reason: String,
    },
    /// No connection could be made: the name did not resolve, or the host refused or could not be reached.
    #[error("{reason}")]
    Connect {
        /// What the transport said.
        reason: String,
    },
    /// The TLS handshake failed, usually over the certificate.
    #[error("{reason}")]
    Tls {
        /// What the handshake said.
        reason: String,
    },
    /// A time limit passed: connecting, no data arriving, or the whole request.
    #[error("{reason}")]
    TimedOut {
        /// Which limit.
        limit: TimeLimit,
        /// The limit and how long it was.
        reason: String,
    },
    /// More redirects than `max_redirects` allows.
    #[error("more than {max_redirects} redirects")]
    TooManyRedirects {
        /// The limit that was passed.
        max_redirects: u32,
    },
    /// A body read whole, or a line or a server-sent event being read from it, is longer than `max_body`.
    #[error("the response body, or a line or event of it, is longer than max_body ({max_body} bytes)")]
    TooLarge {
        /// The limit that was passed, in bytes.
        max_body: u64,
    },
    /// The connection ended or failed after it was made, before the response was complete.
    #[error("{reason}")]
    ConnectionLost {
        /// What the transport said.
        reason: String,
    },
    /// The server answered with something that is not what was asked for: not valid HTTP, a body that is not JSON
    /// for `json()`, an event stream that is not `text/event-stream`.
    #[error("{reason}")]
    BadResponse {
        /// What is wrong.
        reason: String,
    },
    /// An event stream was answered with a status other than 200.
    #[error("the server answered {code}")]
    Status {
        /// The status code.
        code: u16,
    },
    /// The response was cancelled (`cancel()`) while this read waited on it.
    #[error("the response was cancelled")]
    Cancelled {},
    /// Anything else, reported and never hidden.
    #[error("{reason}")]
    Other {
        /// What went wrong.
        reason: String,
    },
}

impl FetchError {
    pub(crate) fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput { reason: reason.into() }
    }
}

impl From<NetError> for FetchError {
    fn from(error: NetError) -> Self {
        match error {
            NetError::InvalidInput { reason } => Self::InvalidInput { reason },
            NetError::InvalidUrl { reason } => Self::InvalidUrl { reason },
            NetError::Connect { reason } => Self::Connect { reason },
            NetError::Tls { reason } => Self::Tls { reason },
            NetError::TimedOut { limit, reason } => Self::TimedOut { limit: limit.into(), reason },
            NetError::TooManyRedirects { max_redirects } => Self::TooManyRedirects { max_redirects },
            NetError::TooLarge { max_body } => Self::TooLarge { max_body },
            NetError::ConnectionLost { reason } => Self::ConnectionLost { reason },
            NetError::BadResponse { reason } | NetError::Protocol { reason } => Self::BadResponse { reason },
            NetError::Status { code, .. } => Self::Status { code },
            NetError::Other { reason } => Self::Other { reason },
        }
    }
}

/// The spawner of the engine's [`Life`]: a shut-down engine closes the connections.
pub(crate) fn spawner(lua: &Lua) -> Spawner {
    let life = Life::of(lua);
    Arc::new(move |future| life.spawn(future).abort_handle())
}

/// The options every request shares, as `neoc-net` takes them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn client_options(
    headers: BTreeMap<String, String>,
    ca_pem: Option<String>,
    connect_timeout: f64,
    idle_timeout: f64,
    timeout: Option<f64>,
    follow_redirects: bool,
    max_redirects: u32,
    max_body: u64,
) -> ClientOptions {
    ClientOptions {
        headers,
        ca_pem,
        connect_timeout,
        idle_timeout,
        timeout,
        follow_redirects,
        max_redirects,
        max_body,
    }
}

/// A response's cancellation, as each of its readers watches it.
#[derive(Clone)]
pub(crate) struct Cancel(tokio::sync::watch::Receiver<bool>);

impl Cancel {
    /// A cancellation, and what sets it.
    pub(crate) fn new() -> (tokio::sync::watch::Sender<bool>, Self) {
        let (tx, rx) = tokio::sync::watch::channel(false);
        (tx, Self(rx))
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    /// Wait until the response is cancelled (forever, when it never is).
    async fn cancelled(&mut self) {
        if self.0.wait_for(|cancelled| *cancelled).await.is_err() {
            std::future::pending::<()>().await;
        }
    }

    /// `work`, unless the response is cancelled first: then `cancelled`, and `work` (and the body it reads) is
    /// dropped.
    pub(crate) async fn or_cancelled<T>(
        &self,
        work: impl Future<Output = Result<T, FetchError>>,
    ) -> Result<T, FetchError> {
        let mut cancel = self.clone();
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(FetchError::Cancelled {}),
            done = work => done,
        }
    }
}

/// The lines of `body`, without their `\n` or `\r\n`; the last one too when the body does not end with a newline.
/// Bytes that are not UTF-8 become U+FFFD. Cancelled, the stream gives `cancelled` and ends, and the body goes; so
/// does a line longer than `max_body` (`too_large`), which is not kept.
pub(crate) fn lines(
    body: neoc_net::Body,
    cancel: Cancel,
    max_body: u64,
) -> impl futures_util::Stream<Item = Result<String, FetchError>> + Send {
    struct State {
        body: Option<neoc_net::Body>,
        pending: Vec<u8>,
        cancel: Cancel,
    }
    fn line(mut bytes: Vec<u8>) -> String {
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
    futures_util::stream::unfold(
        State { body: Some(body), pending: Vec::new(), cancel },
        move |mut state| async move {
            loop {
                if state.body.is_some() && state.cancel.is_cancelled() {
                    state.body = None;
                    state.pending.clear();
                    return Some((Err(FetchError::Cancelled {}), state));
                }
                if let Some(end) = state.pending.iter().position(|&b| b == b'\n') {
                    let rest = state.pending.split_off(end + 1);
                    let mut taken = std::mem::replace(&mut state.pending, rest);
                    taken.pop();
                    return Some((Ok(line(taken)), state));
                }
                let Some(body) = state.body.as_mut() else {
                    if state.pending.is_empty() {
                        return None;
                    }
                    let last = std::mem::take(&mut state.pending);
                    return Some((Ok(line(last)), state));
                };
                // no newline in what is kept: it is all one line, which a peer must not grow without end
                if state.pending.len() as u64 > max_body {
                    state.body = None;
                    state.pending = Vec::new();
                    return Some((Err(FetchError::TooLarge { max_body }), state));
                }
                let read = async { body.chunk().await.map_err(FetchError::from) };
                match state.cancel.or_cancelled(read).await {
                    Ok(Some(chunk)) => state.pending.extend_from_slice(&chunk),
                    Ok(None) => state.body = None,
                    Err(e) => {
                        state.body = None;
                        state.pending.clear();
                        return Some((Err(e), state));
                    }
                }
            }
        },
    )
}

/// One server-sent event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[shape(module = "lib:http")]
pub struct Event {
    /// Its type: the stream's `event:` field, or `"message"`.
    pub event: String,
    /// Its data: the `data:` lines, joined with `"\n"`.
    pub data: String,
    /// The last event id the stream set (`id:`), which a reconnect sends as `Last-Event-ID`; `""` when none.
    pub id: String,
}

/// How an event source connects, and reconnects.
pub(crate) struct Source {
    pub url: String,
    pub options: ClientOptions,
    pub reconnect: bool,
    pub retry: Duration,
    pub spawn: Spawner,
}

impl Source {
    /// One connection: the response's body, once the server has answered 200 with `text/event-stream`.
    async fn open(&self, last_id: &str) -> Result<neoc_net::Body, FetchError> {
        let mut options = self.options.clone();
        options.headers.entry("accept".to_owned()).or_insert_with(|| "text/event-stream".to_owned());
        options.headers.entry("cache-control".to_owned()).or_insert_with(|| "no-store".to_owned());
        if !last_id.is_empty() {
            options.headers.insert("last-event-id".to_owned(), last_id.to_owned());
        }
        let prepared = Prepared::new("GET", self.url.clone(), RequestBody::Empty, &options)?;
        let response = neoc_net::send(prepared, &self.spawn).await?;
        if response.status != 200 {
            return Err(FetchError::Status { code: response.status });
        }
        let content_type = response
            .headers
            .iter()
            .find(|(name, _)| name == "content-type")
            .map(|(_, value)| value.to_ascii_lowercase())
            .unwrap_or_default();
        if content_type.split(';').next().map(str::trim) != Some("text/event-stream") {
            return Err(FetchError::BadResponse {
                reason: format!("the server answered with {content_type:?}, not text/event-stream"),
            });
        }
        Ok(response.body)
    }

    /// The events of `first` (a connection that is open), then of each reconnection, after the HTML standard's
    /// `EventSource`: a stream that ends, or a connection that fails on the way, is opened again after `retry` (or
    /// what the server's `retry:` said), with the last event id; a reconnection answered with another status or
    /// another type ends the stream with that error. With `reconnect = false` the stream ends with the first
    /// connection: at its end, or with its error.
    pub(crate) fn events(
        self,
        first: neoc_net::Body,
    ) -> impl futures_util::Stream<Item = Result<Event, FetchError>> + Send {
        /// One connection: its body, and the parser of what it sends.
        struct Connection {
            body: neoc_net::Body,
            parser: sse::Parser,
        }
        struct State {
            source: Source,
            connection: Option<Connection>,
            /// What the connection's lines did and the stream has not acted on yet.
            ready: std::collections::VecDeque<sse::Item>,
            /// The last event id, across connections, as the HTML standard's `EventSource` keeps it.
            last_id: String,
            /// A line or an event outgrew `max_body`: once what came before it is given, the stream ends so.
            too_large: bool,
            ended: bool,
        }
        let parser = sse::Parser::new(self.options.max_body);
        let state = State {
            source: self,
            connection: Some(Connection { body: first, parser }),
            ready: std::collections::VecDeque::new(),
            last_id: String::new(),
            too_large: false,
            ended: false,
        };
        futures_util::stream::unfold(state, |mut state| async move {
            loop {
                if state.ended {
                    return None;
                }
                // the fields take effect in the order they came, an `id:` or a `retry:` with no event after it too
                if let Some(item) = state.ready.pop_front() {
                    match item {
                        sse::Item::Id(id) => state.last_id = id,
                        sse::Item::Retry(retry) => state.source.retry = retry,
                        sse::Item::Event { event, data } => {
                            let id = state.last_id.clone();
                            return Some((Ok(Event { event, data, id }), state));
                        }
                    }
                    continue;
                }
                if state.too_large {
                    state.ended = true;
                    let max_body = state.source.options.max_body;
                    return Some((Err(FetchError::TooLarge { max_body }), state));
                }
                let Some(connection) = state.connection.as_mut() else {
                    tokio::time::sleep(state.source.retry).await;
                    match state.source.open(&state.last_id).await {
                        Ok(body) => {
                            let parser = sse::Parser::new(state.source.options.max_body);
                            state.connection = Some(Connection { body, parser });
                        }
                        // the network: try again, as a browser does
                        Err(
                            FetchError::Connect { .. }
                            | FetchError::ConnectionLost { .. }
                            | FetchError::TimedOut { .. },
                        ) => {}
                        Err(e) => {
                            state.ended = true;
                            return Some((Err(e), state));
                        }
                    }
                    continue;
                };
                match connection.body.chunk().await {
                    Ok(Some(chunk)) => {
                        let items = connection.parser.feed(&chunk);
                        state.ready.extend(items);
                        // a line or an event that outgrew the limit: the connection goes, and the stream ends (a
                        // reconnection would only be sent the same)
                        if connection.parser.too_large() {
                            state.connection = None;
                            state.too_large = true;
                        }
                    }
                    // what was not ended by a blank line is not dispatched
                    Ok(None) => {
                        state.connection = None;
                        if !state.source.reconnect {
                            return None;
                        }
                    }
                    Err(e) => {
                        state.connection = None;
                        if !state.source.reconnect {
                            state.ended = true;
                            return Some((Err(e.into()), state));
                        }
                    }
                }
            }
        })
    }

    /// The first connection, whose failure is the caller's error.
    pub(crate) async fn connect(
        self,
    ) -> Result<impl futures_util::Stream<Item = Result<Event, FetchError>> + Send, FetchError> {
        let first = self.open("").await?;
        Ok(self.events(first))
    }
}

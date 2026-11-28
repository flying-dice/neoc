//! The client half of `lib:http`: what `fetch`, `request` and `events` are made of. The requests are `neoc-net`'s (the
//! limits NEOC-259 decided, TLS by the platform verifier); this file holds the module's error, the line and event
//! streams over a response body, and the reconnecting event source.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eventsource_stream::Eventsource as _;
use futures_util::StreamExt as _;
use mlua::Lua;
use mlua_ir::{LuaError, Shape};
use neoc_net::{ClientOptions, NetError, Prepared, RequestBody, Spawner};
use serde::{Deserialize, Serialize};

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
    /// A body read whole is longer than `max_body`.
    #[error("the response body is longer than max_body ({max_body} bytes)")]
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

/// The lines of `body`, without their `\n` or `\r\n`; the last one too when the body does not end with a newline.
/// Bytes that are not UTF-8 become U+FFFD.
pub(crate) fn lines(body: neoc_net::Body) -> impl futures_util::Stream<Item = Result<String, FetchError>> + Send {
    struct State {
        body: Option<neoc_net::Body>,
        pending: Vec<u8>,
    }
    fn line(mut bytes: Vec<u8>) -> String {
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
    futures_util::stream::unfold(State { body: Some(body), pending: Vec::new() }, |mut state| async move {
        loop {
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
            match body.chunk().await {
                Ok(Some(chunk)) => state.pending.extend_from_slice(&chunk),
                Ok(None) => state.body = None,
                Err(e) => {
                    state.body = None;
                    state.pending.clear();
                    return Some((Err(e.into()), state));
                }
            }
        }
    })
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
        type Parsed = std::pin::Pin<
            Box<
                dyn futures_util::Stream<
                        Item = Result<
                            eventsource_stream::Event,
                            eventsource_stream::EventStreamError<FetchError>,
                        >,
                    > + Send,
            >,
        >;
        fn parse(body: neoc_net::Body) -> Parsed {
            let bytes = futures_util::stream::unfold(Some(body), |body| async move {
                let mut body = body?;
                match body.chunk().await {
                    Ok(Some(chunk)) => Some((Ok::<Bytes, FetchError>(chunk), Some(body))),
                    Ok(None) => None,
                    Err(e) => Some((Err(e.into()), None)),
                }
            });
            Box::pin(bytes.eventsource())
        }
        struct State {
            source: Source,
            parsed: Option<Parsed>,
            /// The last event id, across connections: a new connection's parser starts with none.
            last_id: String,
            /// The current connection set an id of its own.
            set_id: bool,
            ended: bool,
        }
        let state =
            State { source: self, parsed: Some(parse(first)), last_id: String::new(), set_id: false, ended: false };
        futures_util::stream::unfold(state, |mut state| async move {
            loop {
                if state.ended {
                    return None;
                }
                let Some(parsed) = state.parsed.as_mut() else {
                    tokio::time::sleep(state.source.retry).await;
                    match state.source.open(&state.last_id).await {
                        Ok(body) => {
                            state.parsed = Some(parse(body));
                            state.set_id = false;
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
                match parsed.next().await {
                    Some(Ok(event)) => {
                        if let Some(retry) = event.retry {
                            state.source.retry = retry;
                        }
                        if state.set_id || !event.id.is_empty() {
                            state.set_id = true;
                            state.last_id.clone_from(&event.id);
                        }
                        let id = state.last_id.clone();
                        return Some((Ok(Event { event: event.event, data: event.data, id }), state));
                    }
                    Some(Err(e)) => {
                        state.parsed = None;
                        if !state.source.reconnect {
                            state.ended = true;
                            let error = match e {
                                eventsource_stream::EventStreamError::Transport(e) => e,
                                other => FetchError::BadResponse { reason: other.to_string() },
                            };
                            return Some((Err(error), state));
                        }
                    }
                    None => {
                        state.parsed = None;
                        if !state.source.reconnect {
                            return None;
                        }
                    }
                }
            }
        })
    }

    /// The first connection, whose failure is the caller's error.
    pub(crate) async fn connect(self) -> Result<impl futures_util::Stream<Item = Result<Event, FetchError>> + Send, FetchError> {
        let first = self.open("").await?;
        Ok(self.events(first))
    }
}

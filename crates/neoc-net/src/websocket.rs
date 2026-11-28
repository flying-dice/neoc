//! A WebSocket connection (RFC 6455): `ws://` over TCP, `wss://` over TLS, the upgrade done by tungstenite.
//!
//! The connection is made as an HTTP request is (`exchange`): the name lookup and the TCP connect race the addresses
//! (Happy Eyeballs), TLS is checked by the platform verifier or by `ca_pem`, and `connect_timeout` bounds the lot,
//! upgrade included. tungstenite's limits apply to what the peer sends: a message of at most 64 MiB, a frame of at
//! most 16 MiB.

use std::collections::BTreeMap;
use std::time::Duration;

use hyper::header::{CONNECTION, HeaderValue, SEC_WEBSOCKET_PROTOCOL, UPGRADE};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use url::Url;

use crate::error::{NetError, TimeLimit, chain};
use crate::exchange::{Target, tcp_connect};
use crate::options::{DEFAULT_CONNECT, headers_of, roots_of};
use crate::tls::Tls;

/// How much of a refused upgrade's body is quoted in the error, in characters.
const ERROR_BODY: usize = 2048;

/// A byte stream a WebSocket runs over: a TCP socket, or TLS over one.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// An open WebSocket.
pub type Socket = WebSocketStream<Box<dyn Io>>;

/// How to connect.
#[derive(Debug, Clone)]
pub struct Options {
    /// Headers to add to the upgrade request (`Authorization`, `Origin`). The ones the upgrade itself is made of
    /// (`Upgrade`, `Connection`, `Sec-WebSocket-*`) are refused.
    pub headers: BTreeMap<String, String>,
    /// The subprotocols to offer, in order of preference (`Sec-WebSocket-Protocol`).
    pub protocols: Vec<String>,
    /// PEM certificates to trust instead of the platform's.
    pub ca_pem: Option<String>,
    /// How long the TCP connect, the TLS handshake and the upgrade may take together.
    pub connect_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self { headers: BTreeMap::new(), protocols: Vec::new(), ca_pem: None, connect_timeout: DEFAULT_CONNECT }
    }
}

/// An open WebSocket, and what the upgrade settled.
pub struct Connected {
    /// The socket.
    pub socket: Socket,
    /// The subprotocol the server chose, when it chose one of those offered.
    pub protocol: Option<String>,
    /// The IP address of this end of the connection.
    pub local_address: String,
}

/// Connect to `url` (`ws://` or `wss://`) and complete the upgrade.
pub async fn connect(url: &str, opts: &Options) -> Result<Connected, NetError> {
    let parsed = Url::parse(url).map_err(|e| NetError::invalid_url(format!("{url}: {e}")))?;
    let tls = match parsed.scheme() {
        "ws" => false,
        "wss" => true,
        _ => return Err(NetError::invalid_url(format!("{url}: only ws:// and wss:// URLs are supported"))),
    };
    let mut request = url.into_client_request().map_err(|e| NetError::invalid_url(format!("{url}: {e}")))?;
    for (name, value) in &headers_of(&opts.headers)? {
        let lower = name.as_str();
        if name == UPGRADE || name == CONNECTION || lower.starts_with("sec-websocket-") {
            return Err(NetError::invalid_input(format!("the {name} header is set by the upgrade; drop it")));
        }
        request.headers_mut().insert(name.clone(), value.clone());
    }
    if !opts.protocols.is_empty() {
        let offered = HeaderValue::from_str(&opts.protocols.join(", "))
            .map_err(|_| NetError::invalid_input("a protocol is not a valid subprotocol name"))?;
        request.headers_mut().insert(SEC_WEBSOCKET_PROTOCOL, offered);
    }
    let ca_pem = opts.ca_pem.as_deref().map(roots_of).transpose()?;

    // the TCP part is the HTTP client's: the same lookup and the same races, to the same host and port
    let mut http = parsed.clone();
    http.set_scheme(if tls { "https" } else { "http" })
        .map_err(|()| NetError::invalid_url(format!("{url}: not a ws:// or wss:// URL")))?;
    let attempt = async {
        let target = Target::new(&http)?;
        let tcp = tcp_connect(&target).await?;
        let local_address =
            tcp.local_addr().map_err(|e| NetError::connect(e.to_string()))?.ip().to_string();
        let io: Box<dyn Io> = if tls {
            let name = ServerName::try_from(target.host.to_owned())
                .map_err(|e| NetError::invalid_url(format!("{url}: {e}")))?;
            let stream = Tls::new(ca_pem)
                .connector()?
                .connect(name, tcp)
                .await
                .map_err(|e| NetError::tls(chain(&e)))?;
            Box::new(stream)
        } else {
            Box::new(tcp)
        };
        let (socket, response) =
            tokio_tungstenite::client_async(request, io).await.map_err(handshake_error)?;
        let protocol = response
            .headers()
            .get(SEC_WEBSOCKET_PROTOCOL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Ok(Connected { socket, protocol, local_address })
    };
    tokio::time::timeout(opts.connect_timeout, attempt).await.unwrap_or_else(|_| {
        Err(NetError::timed_out(
            TimeLimit::ConnectTimeout,
            format!("connecting took longer than {}s", opts.connect_timeout.as_secs_f64()),
        ))
    })
}

/// What a failed upgrade is.
fn handshake_error(e: tungstenite::Error) -> NetError {
    match e {
        tungstenite::Error::Http(response) => {
            let body = response.body().as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            let body: String = body.chars().take(ERROR_BODY).collect();
            NetError::Status { code: response.status().as_u16(), body: body.trim().to_owned() }
        }
        tungstenite::Error::Io(e) => NetError::connection_lost(e.to_string()),
        tungstenite::Error::Tls(e) => NetError::tls(e.to_string()),
        other => NetError::Protocol { reason: format!("the WebSocket upgrade failed: {other}") },
    }
}

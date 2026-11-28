//! The error of the client: a stable `kind`, what the kind carries, and a message.
//!
//! A module that hands the error to a script declares its own error type with the same kinds and converts this one
//! (`From<NetError>`): the kind is the same on every platform, so a script branches on it; the message is for a log.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Which time limit a `timed_out` request passed: the option that sets it, or the operating system's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// Why a request failed. The kind means the same on every platform; `reason` is this module's or the
/// transport's wording, for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NetError {
    /// An option, a header, the method or the body was refused before anything was sent.
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
    /// The response body is longer than `max_body`.
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
    /// The server answered with something that is not valid HTTP.
    #[error("{reason}")]
    BadResponse {
        /// What the parser said.
        reason: String,
    },
    /// A WebSocket upgrade was answered with an HTTP status instead of `101 Switching Protocols`.
    #[error("the server answered {code}{}", said(.body))]
    Status {
        /// The status code.
        code: u16,
        /// The start of the response's body, as text.
        body: String,
    },
    /// The peer broke the protocol: a WebSocket upgrade or frame that is not what RFC 6455 says.
    #[error("{reason}")]
    Protocol {
        /// What is wrong.
        reason: String,
    },
    /// Anything else, reported and never hidden.
    #[error("{reason}")]
    Other {
        /// What went wrong.
        reason: String,
    },
}

impl NetError {
    /// The kind's name, as serde writes it (`timed_out`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. } => "invalid_input",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::Connect { .. } => "connect",
            Self::Tls { .. } => "tls",
            Self::TimedOut { .. } => "timed_out",
            Self::TooManyRedirects { .. } => "too_many_redirects",
            Self::TooLarge { .. } => "too_large",
            Self::ConnectionLost { .. } => "connection_lost",
            Self::BadResponse { .. } => "bad_response",
            Self::Status { .. } => "status",
            Self::Protocol { .. } => "protocol",
            Self::Other { .. } => "other",
        }
    }

    pub fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput { reason: reason.into() }
    }

    pub fn invalid_url(reason: impl Into<String>) -> Self {
        Self::InvalidUrl { reason: reason.into() }
    }

    pub fn connect(reason: impl Into<String>) -> Self {
        Self::Connect { reason: reason.into() }
    }

    pub(crate) fn tls(reason: impl Into<String>) -> Self {
        Self::Tls { reason: reason.into() }
    }

    pub(crate) fn timed_out(limit: TimeLimit, reason: impl Into<String>) -> Self {
        Self::TimedOut { limit, reason: reason.into() }
    }

    /// The idle timeout fired after `idle` without data.
    pub(crate) fn idle(idle: Duration) -> Self {
        Self::timed_out(TimeLimit::IdleTimeout, crate::idle::message(idle))
    }

    pub fn connection_lost(reason: impl Into<String>) -> Self {
        Self::ConnectionLost { reason: reason.into() }
    }

    /// An error of the connection after it was made. `idle_fired` is the idle timeout when the socket's idle timer
    /// fired: whatever hyper made of that (a closed connection, a cancelled request) it is a timeout, and it says so.
    pub(crate) fn from_hyper(e: &hyper::Error, idle_fired: Option<Duration>) -> Self {
        if let Some(idle) = idle_fired {
            return Self::idle(idle);
        }
        let reason = chain(e);
        if io_kind(e) == Some(io::ErrorKind::TimedOut) {
            Self::TimedOut { limit: TimeLimit::System, reason }
        } else if e.is_parse() || e.is_parse_status() || e.is_parse_too_large() {
            Self::BadResponse { reason }
        } else if e.is_user() && !e.to_string().contains("dispatch task is gone") {
            // a refusal of hyper's own (a method or URI it cannot send); when the connection's driver has gone,
            // hyper calls that a user error too, and it is a lost connection
            Self::Other { reason }
        } else {
            Self::ConnectionLost { reason }
        }
    }
}

/// The start of an error response's body, for the message: `: <text>`, or nothing.
fn said(body: &str) -> String {
    if body.is_empty() { String::new() } else { format!(": {body}") }
}

/// The first `io::Error` kind in the chain of causes of `e`.
fn io_kind(e: &(dyn std::error::Error + 'static)) -> Option<io::ErrorKind> {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(c) = cause {
        if let Some(io) = c.downcast_ref::<io::Error>() {
            return Some(io.kind());
        }
        cause = c.source();
    }
    None
}

/// `e` and its causes as one line (`error reading a body from connection: connection reset by peer`).
pub fn chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        let text = c.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        cause = c.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<NetError> {
        let reason = String::from("why");
        vec![
            NetError::InvalidInput { reason: reason.clone() },
            NetError::InvalidUrl { reason: reason.clone() },
            NetError::Connect { reason: reason.clone() },
            NetError::Tls { reason: reason.clone() },
            NetError::TimedOut { limit: TimeLimit::IdleTimeout, reason: reason.clone() },
            NetError::TooManyRedirects { max_redirects: 16 },
            NetError::TooLarge { max_body: 10 },
            NetError::ConnectionLost { reason: reason.clone() },
            NetError::BadResponse { reason: reason.clone() },
            NetError::Status { code: 404, body: reason.clone() },
            NetError::Protocol { reason: reason.clone() },
            NetError::Other { reason },
        ]
    }

    #[test]
    fn every_kind_name_is_snake_case_and_is_what_serde_writes() {
        let names: Vec<&str> = every_kind().iter().map(NetError::kind).collect();
        assert_eq!(
            names,
            [
                "invalid_input",
                "invalid_url",
                "connect",
                "tls",
                "timed_out",
                "too_many_redirects",
                "too_large",
                "connection_lost",
                "bad_response",
                "status",
                "protocol",
                "other"
            ]
        );
        for error in every_kind() {
            let written = serde_json::to_value(&error).unwrap();
            assert_eq!(written["kind"], error.kind());
        }
    }

    #[test]
    fn the_message_says_what_passed_the_limit() {
        assert_eq!(NetError::idle(Duration::from_secs(60)).to_string(), "no data for 60s");
        assert_eq!(NetError::TooManyRedirects { max_redirects: 16 }.to_string(), "more than 16 redirects");
        assert_eq!(
            NetError::TooLarge { max_body: 10 }.to_string(),
            "the response body is longer than max_body (10 bytes)"
        );
        assert_eq!(NetError::invalid_url("URL has no host").to_string(), "URL has no host");
    }

    #[test]
    fn a_time_limit_is_named_after_its_option() {
        let written = serde_json::to_value(NetError::idle(Duration::from_millis(300))).unwrap();
        assert_eq!(written["limit"], "idle_timeout");
        assert_eq!(written["reason"], "no data for 0.3s");
    }
}

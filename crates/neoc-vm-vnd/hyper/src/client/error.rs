//! The error of the `vnd:hyper` client: a stable `kind`, what the kind carries, and a message.
//!
//! Every failed request returns `(nil, err)` where `err` is `{ kind, message, op, <the kind's fields> }` and
//! `tostring(err)` is `vnd:hyper.<function>: <kind>: <message>` (`vnd:hyper.get: timed_out: no data for 60s`). The
//! `kind` is the same on every platform, so a script branches on it (`err.kind == "timed_out"`); the message is for
//! a log, and a script must not parse it.

use std::io;
use std::time::Duration;

use mlua_ir::{LuaError, Shape};
use serde::{Deserialize, Serialize};

/// Which time limit a `timed_out` request passed: the option that sets it, or the operating system's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(module = "vnd:hyper")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "vnd:hyper")]
pub enum HyperError {
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
    /// Anything else, reported and never hidden.
    #[error("{reason}")]
    Other {
        /// What went wrong.
        reason: String,
    },
}

impl HyperError {
    pub(super) fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput { reason: reason.into() }
    }

    pub(super) fn invalid_url(reason: impl Into<String>) -> Self {
        Self::InvalidUrl { reason: reason.into() }
    }

    pub(super) fn connect(reason: impl Into<String>) -> Self {
        Self::Connect { reason: reason.into() }
    }

    pub(super) fn tls(reason: impl Into<String>) -> Self {
        Self::Tls { reason: reason.into() }
    }

    pub(super) fn timed_out(limit: TimeLimit, reason: impl Into<String>) -> Self {
        Self::TimedOut { limit, reason: reason.into() }
    }

    /// The idle timeout fired after `idle` without data.
    pub(super) fn idle(idle: Duration) -> Self {
        Self::timed_out(TimeLimit::IdleTimeout, super::idle::message(idle))
    }

    pub(super) fn connection_lost(reason: impl Into<String>) -> Self {
        Self::ConnectionLost { reason: reason.into() }
    }

    /// An error of the connection after it was made. `idle_fired` is the idle timeout when the socket's idle timer
    /// fired: whatever hyper made of that (a closed connection, a cancelled request) it is a timeout, and it says so.
    pub(super) fn from_hyper(e: &hyper::Error, idle_fired: Option<Duration>) -> Self {
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
pub(super) fn chain(e: &(dyn std::error::Error + 'static)) -> String {
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

    fn every_kind() -> Vec<HyperError> {
        let reason = String::from("why");
        vec![
            HyperError::InvalidInput { reason: reason.clone() },
            HyperError::InvalidUrl { reason: reason.clone() },
            HyperError::Connect { reason: reason.clone() },
            HyperError::Tls { reason: reason.clone() },
            HyperError::TimedOut { limit: TimeLimit::IdleTimeout, reason: reason.clone() },
            HyperError::TooManyRedirects { max_redirects: 16 },
            HyperError::TooLarge { max_body: 10 },
            HyperError::ConnectionLost { reason: reason.clone() },
            HyperError::BadResponse { reason: reason.clone() },
            HyperError::Other { reason },
        ]
    }

    #[test]
    fn every_kind_name_is_snake_case_and_is_what_serde_writes() {
        let names: Vec<&str> = every_kind().iter().map(HyperError::kind).collect();
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
        assert_eq!(HyperError::idle(Duration::from_secs(60)).to_string(), "no data for 60s");
        assert_eq!(HyperError::TooManyRedirects { max_redirects: 16 }.to_string(), "more than 16 redirects");
        assert_eq!(
            HyperError::TooLarge { max_body: 10 }.to_string(),
            "the response body is longer than max_body (10 bytes)"
        );
        assert_eq!(HyperError::invalid_url("URL has no host").to_string(), "URL has no host");
    }

    #[test]
    fn a_time_limit_is_named_after_its_option() {
        let written = serde_json::to_value(HyperError::idle(Duration::from_millis(300))).unwrap();
        assert_eq!(written["limit"], "idle_timeout");
        assert_eq!(written["reason"], "no data for 0.3s");
    }
}

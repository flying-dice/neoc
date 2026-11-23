//! The error contract of the `vnd:hyper` client: a stable `kind` plus a message.
//!
//! Every failed request returns `(nil, err)` where `err` is one string, `hyper.<function>: <kind>: <message>`
//! (`hyper.get: timed_out: no data for 60s`). The `kind` is one of [`Kind`]'s names, the same on every platform, so
//! a script can branch on it (`hyper.error_kind(err) == "timed_out"`) or show the person a sentence of its own; the
//! message is for a log, and a script must not parse it. The shape is the one `std:fs` has (`fs.error_kind`).

use std::fmt;
use std::io;

use crate::lua::error_text;

/// What went wrong, stable across platforms. The names are the contract (`hyper.error_kind`); the mapping from
/// `URLSession`'s error codes is in each variant's doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An option, a header or the body was refused before anything was sent (`NSURLErrorBadURL`'s sibling for
    /// everything that is not the URL).
    InvalidInput,
    /// The URL (or a redirect's `Location`) is not an `http` or `https` URL with a host (`badURL`, `unsupportedURL`).
    InvalidUrl,
    /// No connection could be made: the name did not resolve, or the host refused or could not be reached
    /// (`cannotFindHost`, `cannotConnectToHost`).
    Connect,
    /// The TLS handshake failed, usually over the certificate (`secureConnectionFailed`, `serverCertificateUntrusted`).
    Tls,
    /// A time limit passed: connecting, no data arriving, or the whole request (`timedOut`).
    TimedOut,
    /// The redirect limit passed (`httpTooManyRedirects`).
    TooManyRedirects,
    /// The response body is longer than `max_body` (`dataLengthExceedsMaximum`).
    TooLarge,
    /// The connection ended or failed after it was made, before the response was complete (`networkConnectionLost`).
    ConnectionLost,
    /// The server answered with something that is not valid HTTP (`badServerResponse`).
    BadResponse,
    /// Anything else, reported and never hidden.
    Other,
}

impl Kind {
    /// Every kind, for the docs and the tests that hold them to each other.
    pub(super) const ALL: [Kind; 10] = [
        Kind::InvalidInput,
        Kind::InvalidUrl,
        Kind::Connect,
        Kind::Tls,
        Kind::TimedOut,
        Kind::TooManyRedirects,
        Kind::TooLarge,
        Kind::ConnectionLost,
        Kind::BadResponse,
        Kind::Other,
    ];

    /// The stable name a script sees.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::InvalidInput => "invalid_input",
            Kind::InvalidUrl => "invalid_url",
            Kind::Connect => "connect",
            Kind::Tls => "tls",
            Kind::TimedOut => "timed_out",
            Kind::TooManyRedirects => "too_many_redirects",
            Kind::TooLarge => "too_large",
            Kind::ConnectionLost => "connection_lost",
            Kind::BadResponse => "bad_response",
            Kind::Other => "other",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A failed request: its [`Kind`] and a message that says what was wrong.
#[derive(Debug)]
pub(crate) struct HyperError {
    kind: Kind,
    message: String,
}

impl HyperError {
    pub(crate) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput, message)
    }

    /// An error of the connection after it was made. `idle_fired` is the idle timeout when the socket's idle timer
    /// fired: whatever hyper made of that (a closed connection, a cancelled request) it is a timeout, and it says so.
    pub(super) fn from_hyper(e: &hyper::Error, idle_fired: Option<std::time::Duration>) -> Self {
        if let Some(idle) = idle_fired {
            return Self::new(Kind::TimedOut, super::idle::message(idle));
        }
        let kind = if io_kind(e) == Some(io::ErrorKind::TimedOut) {
            Kind::TimedOut
        } else if e.is_parse() || e.is_parse_status() || e.is_parse_too_large() {
            Kind::BadResponse
        } else if e.is_user() && !e.to_string().contains("dispatch task is gone") {
            // a refusal of hyper's own (a method or URI it cannot send); when the connection's driver has gone,
            // hyper calls that a user error too, and it is a lost connection
            Kind::Other
        } else {
            Kind::ConnectionLost
        };
        Self::new(kind, chain(e))
    }
}

impl fmt::Display for HyperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for HyperError {}

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

/// The kind name inside an error string a `vnd:hyper` function returned (`hyper.get: timed_out: ...`), or `None`
/// when the text is not one of ours.
pub(crate) fn kind_of(err: &str) -> Option<&'static str> {
    read(err).map(|(kind, _)| kind.as_str())
}

/// The message inside an error string a `vnd:hyper` function returned: everything after the kind. `None` when the
/// text is not one of ours.
pub(crate) fn message_of(err: &str) -> Option<&str> {
    read(err).map(|(_, message)| message)
}

/// The kind and the message of one of our error strings; the shape is [`error_text`]'s.
fn read(err: &str) -> Option<(Kind, &str)> {
    error_text::read("hyper", err, Kind::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_name_round_trips_and_is_snake_case() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
            assert!(
                kind.as_str()
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_')
            );
        }
    }

    #[test]
    fn display_is_kind_then_message() {
        let e = HyperError::new(Kind::TimedOut, "no data for 60s");
        assert_eq!(e.to_string(), "timed_out: no data for 60s");
    }

    #[test]
    fn kind_of_reads_our_strings_only() {
        assert_eq!(
            kind_of("hyper.get: timed_out: no data for 60s"),
            Some("timed_out")
        );
        assert_eq!(
            kind_of("hyper.request: invalid_url: no host"),
            Some("invalid_url")
        );
        assert_eq!(
            kind_of("hyper.read: connection_lost: reset"),
            Some("connection_lost")
        );
        assert_eq!(kind_of("hyper.get: nonsense: x"), None);
        assert_eq!(kind_of("fs.read: not_found: x"), None);
        assert_eq!(kind_of("timed_out: x"), None);
        assert_eq!(kind_of("hyper.: timed_out: x"), None);
        assert_eq!(kind_of("hyper.Get: timed_out: x"), None);
        assert_eq!(kind_of(""), None);
    }

    #[test]
    fn message_of_reads_our_strings_only() {
        assert_eq!(
            message_of("hyper.get: timed_out: no data for 60s"),
            Some("no data for 60s")
        );
        assert_eq!(message_of("hyper.get: timed_out"), Some(""));
        assert_eq!(message_of("hyper.get: nonsense: x"), None);
        assert_eq!(message_of("fs.read: not_found: x"), None);
        assert_eq!(message_of("no data for 60s"), None);
        assert_eq!(message_of(""), None);
    }
}

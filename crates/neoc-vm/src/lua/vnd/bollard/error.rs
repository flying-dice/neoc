//! The error contract of `vnd:bollard`: a stable `kind` plus a message.
//!
//! Every failed call returns `(nil, err)` where `err` is one string, `bollard.<function>: <kind>: <message>`
//! (`bollard.start_container: not_found: No such container: web`). The `kind` is one of [`Kind`]'s names, so a script
//! can branch on it (`bollard.error_kind(err) == "connect"`) or show the person a sentence of its own; the message is
//! Docker's own where Docker answered, and a script must not parse it. The shape is the one `vnd:hyper` and `std:fs`
//! have.

use std::fmt;

use ::bollard::errors::Error;

use crate::lua::error_text;

/// What went wrong. The names are the contract (`bollard.error_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// An argument or an option was refused before anything was sent, or Docker refused the request as malformed
    /// (HTTP 400).
    InvalidInput,
    /// No Docker daemon could be reached: there is no socket, nothing listens on it, or the host refused.
    Connect,
    /// The daemon did not answer within the client's `timeout`.
    TimedOut,
    /// Docker has no such container, image, volume or network (HTTP 404).
    NotFound,
    /// The object is in a state that does not allow the request: a name already in use, a running container that
    /// was to be removed, a volume still in use (HTTP 409).
    Conflict,
    /// Docker answered with any other error status. The message is Docker's.
    Api,
    /// Docker answered with something this client could not read.
    BadResponse,
    /// A result was longer than this module reads into memory.
    TooLarge,
    /// Anything else, reported and never hidden.
    Other,
}

impl Kind {
    /// Every kind, for the docs and the tests that hold them to each other.
    pub(super) const ALL: [Kind; 9] = [
        Kind::InvalidInput,
        Kind::Connect,
        Kind::TimedOut,
        Kind::NotFound,
        Kind::Conflict,
        Kind::Api,
        Kind::BadResponse,
        Kind::TooLarge,
        Kind::Other,
    ];

    /// The stable name a script sees.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Kind::InvalidInput => "invalid_input",
            Kind::Connect => "connect",
            Kind::TimedOut => "timed_out",
            Kind::NotFound => "not_found",
            Kind::Conflict => "conflict",
            Kind::Api => "api",
            Kind::BadResponse => "bad_response",
            Kind::TooLarge => "too_large",
            Kind::Other => "other",
        }
    }

    fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A failed call: its [`Kind`] and a message that says what was wrong.
#[derive(Debug)]
pub(super) struct BollardError {
    kind: Kind,
    message: String,
}

impl BollardError {
    pub(super) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput, message)
    }

    pub(super) fn kind(&self) -> Kind {
        self.kind
    }
}

impl From<Error> for BollardError {
    fn from(e: Error) -> Self {
        match e {
            Error::DockerResponseServerError {
                status_code,
                message,
            } => {
                let kind = match status_code {
                    400 => Kind::InvalidInput,
                    404 => Kind::NotFound,
                    409 => Kind::Conflict,
                    _ => Kind::Api,
                };
                let message = if message.is_empty() {
                    format!("Docker answered with status {status_code}")
                } else if kind == Kind::Api {
                    format!("{message} (status {status_code})")
                } else {
                    message
                };
                Self::new(kind, message)
            }
            // an error Docker reports inside a 200 response, as a line of a stream (a pull that failed)
            Error::DockerStreamError { error } => Self::new(Kind::Api, error),
            Error::SocketNotFoundError(path) => Self::new(
                Kind::Connect,
                format!("there is no Docker socket at {path}: is Docker installed and running?"),
            ),
            Error::HyperLegacyError { err } => Self::new(
                Kind::Connect,
                format!("the Docker daemon could not be reached: {}", chain(&err)),
            ),
            Error::RequestTimeoutError => {
                Self::new(Kind::TimedOut, "the Docker daemon did not answer in time")
            }
            e @ (Error::JsonDataError { .. }
            | Error::JsonSerdeError { .. }
            | Error::APIVersionParseError {}
            | Error::HyperResponseError { .. }) => Self::new(Kind::BadResponse, chain(&e)),
            e @ (Error::URLParseError { .. }
            | Error::InvalidURIError { .. }
            | Error::InvalidURIPartsError { .. }
            | Error::UnsupportedURISchemeError { .. }
            | Error::CertPathError { .. }
            | Error::CertMultipleKeys { .. }
            | Error::CertParseError { .. }) => Self::new(Kind::InvalidInput, chain(&e)),
            other => Self::new(Kind::Other, chain(&other)),
        }
    }
}

impl fmt::Display for BollardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for BollardError {}

/// `e` and its causes as one line.
fn chain(e: &(dyn std::error::Error + 'static)) -> String {
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

/// The kind name inside an error string a `vnd:bollard` function returned (`bollard.ping: connect: ...`), or `None`
/// when the text is not one of ours.
pub(super) fn kind_of(err: &str) -> Option<&'static str> {
    read(err).map(|(kind, _)| kind.as_str())
}

/// The message inside an error string a `vnd:bollard` function returned: everything after the kind. `None` when the
/// text is not one of ours.
pub(super) fn message_of(err: &str) -> Option<&str> {
    read(err).map(|(_, message)| message)
}

/// The kind and the message of one of our error strings; the shape is [`error_text`]'s.
fn read(err: &str) -> Option<(Kind, &str)> {
    error_text::read("bollard", err, Kind::parse)
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
    fn kind_of_reads_our_strings_only() {
        assert_eq!(
            kind_of("bollard.ping: connect: nothing listens"),
            Some("connect")
        );
        assert_eq!(
            kind_of("bollard.start_container: not_found: No such container: web"),
            Some("not_found")
        );
        assert_eq!(kind_of("bollard.ping: nonsense: x"), None);
        assert_eq!(kind_of("hyper.get: connect: x"), None);
        assert_eq!(kind_of("connect: x"), None);
        assert_eq!(kind_of("bollard.: connect: x"), None);
        assert_eq!(kind_of("bollard.Ping: connect: x"), None);
        assert_eq!(kind_of(""), None);
    }

    #[test]
    fn a_docker_status_maps_to_its_kind_and_keeps_dockers_message() {
        let of = |status_code: u16, message: &str| {
            BollardError::from(Error::DockerResponseServerError {
                status_code,
                message: message.to_owned(),
            })
        };
        assert_eq!(
            of(404, "No such container: web").to_string(),
            "not_found: No such container: web"
        );
        assert_eq!(of(409, "name in use").kind(), Kind::Conflict);
        assert_eq!(of(400, "bad parameter").kind(), Kind::InvalidInput);
        assert_eq!(of(500, "boom").to_string(), "api: boom (status 500)");
        assert_eq!(
            of(503, "").to_string(),
            "api: Docker answered with status 503"
        );
    }

    #[test]
    fn a_missing_socket_is_a_connect_error_that_names_the_path() {
        let e = BollardError::from(Error::SocketNotFoundError("/nowhere/docker.sock".into()));
        assert_eq!(e.kind(), Kind::Connect);
        assert!(e.to_string().contains("/nowhere/docker.sock"));
    }

    #[test]
    fn message_of_reads_our_strings_only() {
        assert_eq!(
            message_of("bollard.start_container: not_found: No such container: web"),
            Some("No such container: web")
        );
        assert_eq!(message_of("bollard.start_container: not_found"), Some(""));
        assert_eq!(message_of("bollard.ping: nonsense: x"), None);
        assert_eq!(message_of("hyper.get: connect: x"), None);
        assert_eq!(message_of("No such container: web"), None);
        assert_eq!(message_of(""), None);
    }
}

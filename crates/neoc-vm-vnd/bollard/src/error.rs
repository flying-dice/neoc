//! The error of `vnd:bollard`: a stable `kind`, and what was said.
//!
//! Every failed call returns `(nil, err)` where `err` is a [`DockerError`]. A script branches on `err.kind`; the
//! `reason` is Docker's own sentence where Docker answered, for a person and not for parsing.

use bollard::errors::Error;
use mlua_ir::LuaError;
use serde::Serialize;

/// Why a Docker call failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[shape(module = "vnd:bollard")]
pub enum DockerError {
    /// What was asked cannot be sent, or Docker refused it as malformed (HTTP 400): an option out of range, a host
    /// that is not a URL, a certificate that cannot be read.
    #[error("{reason}")]
    InvalidInput {
        /// What was refused, and why.
        reason: String,
    },
    /// No Docker daemon could be reached: there is no socket, nothing listens on it, or the host refused.
    #[error("{reason}")]
    Connect {
        /// What was tried, and how it failed.
        reason: String,
    },
    /// The daemon did not answer within the client's `timeout`.
    #[error("the Docker daemon did not answer in time")]
    TimedOut,
    /// Docker has no such container, image, volume or network (HTTP 404).
    #[error("{reason}")]
    NotFound {
        /// Docker's own sentence.
        reason: String,
    },
    /// The object's state does not allow the request (HTTP 409): a name in use, a running container that was to
    /// be removed, a volume still in use.
    #[error("{reason}")]
    Conflict {
        /// Docker's own sentence.
        reason: String,
    },
    /// Docker answered with any other error: an error status, or an error inside a stream (a pull that failed).
    #[error("{}", api_text(reason, *status))]
    Api {
        /// Docker's own sentence.
        reason: String,
        /// The HTTP status Docker answered with; none for an error Docker reported inside a stream.
        status: Option<u16>,
    },
    /// Docker answered with something this client could not read.
    #[error("{reason}")]
    BadResponse {
        /// What could not be read.
        reason: String,
    },
    /// An attached container's input cannot be written: `close_input()` was called, the connection broke, or the
    /// script stopped.
    #[error("{reason}")]
    Closed {
        /// Why.
        reason: String,
    },
    /// A result was longer than this module reads into memory.
    #[error("the command wrote more than {limit} bytes")]
    TooLarge {
        /// The most bytes that are read.
        limit: u64,
    },
    /// Anything else, reported and never hidden.
    #[error("{reason}")]
    Other {
        /// What went wrong.
        reason: String,
    },
}

/// What stands in for Docker's message when it sent none.
fn no_message(status: u16) -> String {
    format!("Docker answered with status {status}")
}

fn api_text(reason: &str, status: Option<u16>) -> String {
    match status {
        // the sentence that stands in for a message Docker did not send names the status already
        Some(status) if reason != no_message(status) => format!("{reason} (status {status})"),
        _ => reason.to_owned(),
    }
}

impl DockerError {
    pub(super) fn invalid_input(reason: impl Into<String>) -> Self {
        Self::InvalidInput { reason: reason.into() }
    }
}

impl From<Error> for DockerError {
    fn from(error: Error) -> Self {
        match error {
            Error::DockerResponseServerError { status_code, message } => {
                let reason = if message.is_empty() { no_message(status_code) } else { message };
                match status_code {
                    400 => Self::InvalidInput { reason },
                    404 => Self::NotFound { reason },
                    409 => Self::Conflict { reason },
                    _ => Self::Api { reason, status: Some(status_code) },
                }
            }
            // an error Docker reports inside a 200 response, as a line of a stream (a pull that failed)
            Error::DockerStreamError { error } => Self::Api { reason: error, status: None },
            Error::SocketNotFoundError(path) => Self::Connect {
                reason: format!("there is no Docker socket at {path}: is Docker installed and running?"),
            },
            Error::HyperLegacyError { err } => {
                Self::Connect { reason: format!("the Docker daemon could not be reached: {}", chain(&err)) }
            }
            Error::RequestTimeoutError => Self::TimedOut,
            error @ (Error::JsonDataError { .. }
            | Error::JsonSerdeError { .. }
            | Error::APIVersionParseError {}
            | Error::HyperResponseError { .. }) => Self::BadResponse { reason: chain(&error) },
            error @ (Error::URLParseError { .. }
            | Error::InvalidURIError { .. }
            | Error::InvalidURIPartsError { .. }
            | Error::UnsupportedURISchemeError { .. }
            | Error::CertPathError { .. }
            | Error::CertMultipleKeys { .. }
            | Error::CertParseError { .. }) => Self::InvalidInput { reason: chain(&error) },
            other => Self::Other { reason: chain(&other) },
        }
    }
}

/// `error` and its causes as one line.
fn chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut out = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        let text = inner.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        cause = inner.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(status_code: u16, message: &str) -> DockerError {
        DockerError::from(Error::DockerResponseServerError { status_code, message: message.to_owned() })
    }

    #[test]
    fn a_docker_status_maps_to_its_kind_and_keeps_dockers_message() {
        let not_found = of(404, "No such container: web");
        assert_eq!(
            (not_found.kind(), not_found.to_string().as_str()),
            ("not_found", "No such container: web")
        );
        assert_eq!(of(409, "name in use").kind(), "conflict");
        assert_eq!(of(400, "bad parameter").kind(), "invalid_input");
        assert_eq!(of(500, "boom"), DockerError::Api { reason: "boom".to_owned(), status: Some(500) });
        assert_eq!(of(500, "boom").to_string(), "boom (status 500)");
        assert_eq!(of(503, "").to_string(), "Docker answered with status 503");
        let in_a_stream =
            DockerError::from(Error::DockerStreamError { error: "pull access denied".to_owned() });
        assert_eq!(in_a_stream, DockerError::Api { reason: "pull access denied".to_owned(), status: None });
        assert_eq!(in_a_stream.to_string(), "pull access denied");
    }

    #[test]
    fn a_missing_socket_is_a_connect_error_that_names_the_path() {
        let error = DockerError::from(Error::SocketNotFoundError("/nowhere/docker.sock".into()));
        assert_eq!(error.kind(), "connect");
        assert!(error.to_string().contains("/nowhere/docker.sock"));
        assert_eq!(DockerError::from(Error::RequestTimeoutError), DockerError::TimedOut);
    }

    /// The kinds are the contract: snake_case, and a script reads the variant's own fields beside them.
    #[test]
    fn an_error_is_a_typed_table() {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let api = mlua_ir::error_value(&of(500, "boom"), &lua, "vnd:bollard.Client:ping").unwrap();
        lua.globals().set("api", api).unwrap();
        let large = DockerError::TooLarge { limit: 64 };
        lua.globals()
            .set("large", mlua_ir::error_value(&large, &lua, "vnd:bollard.Client:exec").unwrap())
            .unwrap();
        let (kind, reason, status, text, limit): (String, String, u16, String, u64) =
            lua.load("return api.kind, api.reason, api.status, tostring(api), large.limit").eval().unwrap();
        assert_eq!((kind.as_str(), reason.as_str(), status, limit), ("api", "boom", 500, 64));
        assert_eq!(text, "vnd:bollard.Client:ping: api: boom (status 500)");
    }
}

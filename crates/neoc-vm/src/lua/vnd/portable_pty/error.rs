//! The error contract of `vnd:portable_pty`: a stable `kind` plus a message.
//!
//! Every failed call returns `(nil, err)` where `err` is one string, `portable_pty.<function>: <kind>: <message>`
//! (`portable_pty.spawn_command: spawn: Unable to spawn nosuch because ...`). The `kind` is one of [`Kind`]'s names,
//! so a script can branch on it (`portable_pty.error_kind(err) == "closed"`); the message is the crate's or the
//! operating system's own, for a log, and a script must not parse it. The shape is the one `vnd:bollard`,
//! `vnd:hyper` and `std:fs` have.

use std::fmt;

use crate::lua::error_text;

/// What went wrong. The names are the contract (`portable_pty.error_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// An argument or an option was refused before anything was started.
    InvalidInput,
    /// The command could not be started: there is no such program, it is not executable, `cwd` is not a directory,
    /// or the operating system refused the terminal or the process.
    Spawn,
    /// The terminal was closed (by `close`, or with the engine).
    Closed,
    /// Reading from the terminal or writing to it failed.
    Io,
    /// Anything else, reported and never hidden.
    Other,
}

impl Kind {
    /// Every kind, for the docs and the tests that hold them to each other.
    pub(super) const ALL: [Kind; 5] = [
        Kind::InvalidInput,
        Kind::Spawn,
        Kind::Closed,
        Kind::Io,
        Kind::Other,
    ];

    /// The stable name a script sees.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Kind::InvalidInput => "invalid_input",
            Kind::Spawn => "spawn",
            Kind::Closed => "closed",
            Kind::Io => "io",
            Kind::Other => "other",
        }
    }

    fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A failed call: its [`Kind`] and a message that says what was wrong.
#[derive(Clone, Debug)]
pub(super) struct PtyError {
    kind: Kind,
    message: String,
}

impl PtyError {
    pub(super) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput, message)
    }

    pub(super) fn closed() -> Self {
        Self::new(Kind::Closed, "the terminal is closed")
    }

    #[cfg(test)]
    pub(super) fn kind(&self) -> Kind {
        self.kind
    }
}

impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for PtyError {}

/// The kind name inside an error string a `vnd:portable_pty` function returned
/// (`portable_pty.write: closed: ...`), or `None` when the text is not one of ours.
pub(super) fn kind_of(err: &str) -> Option<&'static str> {
    read(err).map(|(kind, _)| kind.as_str())
}

/// The message inside an error string a `vnd:portable_pty` function returned: everything after the kind. `None`
/// when the text is not one of ours.
pub(super) fn message_of(err: &str) -> Option<&str> {
    read(err).map(|(_, message)| message)
}

/// The kind and the message of one of our error strings; the shape is [`error_text`]'s.
fn read(err: &str) -> Option<(Kind, &str)> {
    error_text::read("portable_pty", err, Kind::parse)
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
            kind_of("portable_pty.write: closed: the terminal is closed"),
            Some("closed")
        );
        assert_eq!(
            kind_of("portable_pty.spawn_command: spawn: Unable to spawn nosuch: because"),
            Some("spawn")
        );
        assert_eq!(kind_of("portable_pty.write: nonsense: x"), None);
        assert_eq!(kind_of("bollard.ping: closed: x"), None);
        assert_eq!(kind_of("closed: x"), None);
        assert_eq!(kind_of("portable_pty.: closed: x"), None);
        assert_eq!(kind_of("portable_pty.Write: closed: x"), None);
        assert_eq!(kind_of(""), None);
    }

    #[test]
    fn message_of_reads_our_strings_only() {
        assert_eq!(
            message_of("portable_pty.spawn_command: spawn: Unable to spawn nosuch: because"),
            Some("Unable to spawn nosuch: because")
        );
        assert_eq!(message_of("portable_pty.spawn_command: spawn"), Some(""));
        assert_eq!(message_of("portable_pty.write: nonsense: x"), None);
        assert_eq!(message_of("bollard.ping: closed: x"), None);
        assert_eq!(message_of("Unable to spawn nosuch: because"), None);
        assert_eq!(message_of(""), None);
    }
}

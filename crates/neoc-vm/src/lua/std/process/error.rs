//! The error contract of `std:process`: a stable `kind` plus a message.
//!
//! Every failed call returns `(nil, err)` where `err` is one string, `process.<function>: <kind>: <message>`
//! (`process.spawn: spawn: No such file or directory (os error 2)`). The `kind` is one of [`Kind`]'s names,
//! so a script can branch on it (`process.error_kind(err) == "closed"`); the message is the operating
//! system's own, for a log, and a script must not parse it. The shape is the one `std:fs`, `vnd:portable_pty` and
//! `vnd:hyper` have.

use std::fmt;

/// What went wrong. The names are the contract (`process.error_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// An argument or an option was refused before anything was started.
    InvalidInput,
    /// The command could not be started: there is no such program, it is not executable, `cwd` is not a directory,
    /// or the operating system refused the process.
    Spawn,
    /// The stream is not a pipe or was closed (by `close_stdin` or `close`, or with the engine).
    Closed,
    /// Reading from a stream or writing to it failed, or the operating system did not say how the process ended.
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
pub(super) struct ProcessError {
    kind: Kind,
    message: String,
}

impl ProcessError {
    pub(super) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput, message)
    }

    pub(super) fn closed(what: &str) -> Self {
        Self::new(Kind::Closed, format!("{what} is closed"))
    }

    pub(super) fn not_piped(what: &str) -> Self {
        Self::new(Kind::Closed, format!("{what} is not a pipe"))
    }

    #[cfg(test)]
    pub(super) fn kind(&self) -> Kind {
        self.kind
    }
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for ProcessError {}

/// The kind name inside an error string a `std:process` function returned
/// (`process.write: closed: ...`), or `None` when the text is not one of ours.
pub(super) fn kind_of(err: &str) -> Option<&'static str> {
    let rest = err.strip_prefix("process.")?;
    let (function, rest) = rest.split_once(": ")?;
    if function.is_empty()
        || !function
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return None;
    }
    let name = rest.split_once(": ").map_or(rest, |(name, _)| name);
    Kind::parse(name).map(Kind::as_str)
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
            kind_of("process.write: closed: stdin is closed"),
            Some("closed")
        );
        assert_eq!(
            kind_of("process.spawn: spawn: No such file or directory (os error 2)"),
            Some("spawn")
        );
        assert_eq!(kind_of("process.write: nonsense: x"), None);
        assert_eq!(kind_of("portable_pty.write: closed: x"), None);
        assert_eq!(kind_of("closed: x"), None);
        assert_eq!(kind_of("process.: closed: x"), None);
        assert_eq!(kind_of("process.Write: closed: x"), None);
        assert_eq!(kind_of(""), None);
    }
}

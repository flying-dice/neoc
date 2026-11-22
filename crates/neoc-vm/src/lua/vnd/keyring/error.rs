//! The error contract of `vnd:keyring`: a stable `kind` plus a message.
//!
//! Every failed call returns `(nil, err)` where `err` is one string, `keyring.<function>: <kind>: <message>`
//! (`keyring.get_password: no_access: ...`). The `kind` is one of [`Kind`]'s names, so a script can branch on it
//! (`keyring.error_kind(err) == "no_store"`) or show the person a sentence of its own; the message is the store's
//! own where the store answered, and a script must not parse it. The shape is the one `vnd:bollard` and `std:fs`
//! have. No message ever holds a secret: the bytes a store hands back with an error are dropped here.

use std::fmt;

use keyring_core::Error;

/// What went wrong. The names are the contract (`keyring.error_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// An argument was refused: by this module before anything reached the store, or by the store (a service or
    /// user it does not accept, a value longer than it keeps).
    InvalidInput,
    /// This machine has no credential store that can be used: the platform has none, or it could not be opened (a
    /// Linux machine with no Secret Service running, say).
    NoStore,
    /// The store is there and would not give access: it is locked, or the person refused.
    NoAccess,
    /// The credential an entry stands for is gone. Only an entry that `search` returned says this (it wraps one
    /// credential): for an entry made from a service and a user, "no credential" is an answer, not an error.
    NotFound,
    /// More than one credential in the store matches the entry's service and user.
    Ambiguous,
    /// The credential's secret is not UTF-8 text: read it with `get_secret`.
    BadEncoding,
    /// What the store holds could not be read: the secret, or the store itself, is not in the store's format.
    BadData,
    /// The store does not do what was asked (`search`, on a store that has none).
    Unsupported,
    /// The platform's store failed. The message is the platform's.
    Platform,
    /// Anything else, reported and never hidden.
    Other,
}

impl Kind {
    /// Every kind, for the docs and the tests that hold them to each other.
    pub(super) const ALL: [Kind; 10] = [
        Kind::InvalidInput,
        Kind::NoStore,
        Kind::NoAccess,
        Kind::NotFound,
        Kind::Ambiguous,
        Kind::BadEncoding,
        Kind::BadData,
        Kind::Unsupported,
        Kind::Platform,
        Kind::Other,
    ];

    /// The stable name a script sees.
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Kind::InvalidInput => "invalid_input",
            Kind::NoStore => "no_store",
            Kind::NoAccess => "no_access",
            Kind::NotFound => "not_found",
            Kind::Ambiguous => "ambiguous",
            Kind::BadEncoding => "bad_encoding",
            Kind::BadData => "bad_data",
            Kind::Unsupported => "unsupported",
            Kind::Platform => "platform",
            Kind::Other => "other",
        }
    }

    fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }
}

/// A failed call: its [`Kind`] and a message that says what was wrong.
#[derive(Debug)]
pub(super) struct KeyringError {
    kind: Kind,
    message: String,
}

impl KeyringError {
    pub(super) fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidInput, message)
    }

    /// The platform's store could not be opened, and why.
    pub(super) fn no_store(why: &Error) -> Self {
        Self::new(
            Kind::NoStore,
            format!("this machine has no credential store that can be used: {why}"),
        )
    }

    #[cfg(test)]
    pub(super) fn kind(&self) -> Kind {
        self.kind
    }
}

impl From<Error> for KeyringError {
    fn from(e: Error) -> Self {
        match e {
            Error::Invalid(what, why) => Self::new(Kind::InvalidInput, format!("`{what}` {why}")),
            Error::TooLong(what, limit) => Self::new(
                Kind::InvalidInput,
                format!("`{what}` is longer than the {limit} characters this store keeps"),
            ),
            Error::NoDefaultStore => Self::new(
                Kind::NoStore,
                "this machine has no credential store that can be used",
            ),
            Error::NoStorageAccess(why) => Self::new(
                Kind::NoAccess,
                format!("the credential store would not give access: {why}"),
            ),
            Error::NoEntry => Self::new(Kind::NotFound, "the credential is no longer in the store"),
            // the entries are left out: their `Debug` is the store's, and names what the script already knows
            Error::Ambiguous(matches) => Self::new(
                Kind::Ambiguous,
                format!(
                    "{} credentials in the store match this service and user",
                    matches.len()
                ),
            ),
            // the bytes that came with these two are the secret: dropped
            Error::BadEncoding(_) => Self::new(
                Kind::BadEncoding,
                "the secret is not UTF-8 text: read it with `get_secret`",
            ),
            Error::BadDataFormat(_, why) => Self::new(
                Kind::BadData,
                format!("the secret is not in the store's format: {why}"),
            ),
            Error::BadStoreFormat(why) => Self::new(
                Kind::BadData,
                format!("the credential store could not be read: {why}"),
            ),
            Error::NotSupportedByStore(why) => Self::new(Kind::Unsupported, why),
            Error::PlatformFailure(why) => Self::new(Kind::Platform, why.to_string()),
            other => Self::new(Kind::Other, other.to_string()),
        }
    }
}

impl fmt::Display for KeyringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for KeyringError {}

/// The kind name inside an error string a `vnd:keyring` function returned (`keyring.store: no_store: ...`), or
/// `None` when the text is not one of ours.
pub(super) fn kind_of(err: &str) -> Option<&'static str> {
    let rest = err.strip_prefix("keyring.")?;
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
            kind_of("keyring.store: no_store: nothing to open"),
            Some("no_store")
        );
        assert_eq!(
            kind_of("keyring.get_password: bad_encoding: the secret is not UTF-8 text: x"),
            Some("bad_encoding")
        );
        assert_eq!(kind_of("keyring.store: nonsense: x"), None);
        assert_eq!(kind_of("bollard.ping: connect: x"), None);
        assert_eq!(kind_of("no_store: x"), None);
        assert_eq!(kind_of("keyring.: no_store: x"), None);
        assert_eq!(kind_of("keyring.Store: no_store: x"), None);
        assert_eq!(kind_of(""), None);
    }

    #[test]
    fn a_store_error_maps_to_its_kind() {
        let kind = |e: Error| KeyringError::from(e).kind();
        assert_eq!(
            kind(Error::Invalid("service".into(), "is empty".into())),
            Kind::InvalidInput
        );
        assert_eq!(kind(Error::TooLong("user".into(), 255)), Kind::InvalidInput);
        assert_eq!(kind(Error::NoDefaultStore), Kind::NoStore);
        assert_eq!(
            kind(Error::NoStorageAccess("locked".into())),
            Kind::NoAccess
        );
        assert_eq!(kind(Error::NoEntry), Kind::NotFound);
        assert_eq!(kind(Error::Ambiguous(Vec::new())), Kind::Ambiguous);
        assert_eq!(
            kind(Error::BadStoreFormat("truncated".into())),
            Kind::BadData
        );
        assert_eq!(
            kind(Error::NotSupportedByStore("no search".into())),
            Kind::Unsupported
        );
        assert_eq!(
            kind(Error::PlatformFailure("-25300".into())),
            Kind::Platform
        );
    }

    #[test]
    fn a_secret_that_came_with_an_error_is_not_in_the_message() {
        let secret = b"hunter2-not-utf8-\xff".to_vec();
        let not_text = KeyringError::from(Error::BadEncoding(secret.clone()));
        assert_eq!(not_text.kind(), Kind::BadEncoding);
        assert!(!not_text.to_string().contains("hunter2"));

        let malformed = KeyringError::from(Error::BadDataFormat(secret, "bad padding".into()));
        assert_eq!(malformed.kind(), Kind::BadData);
        assert!(!malformed.to_string().contains("hunter2"));
        assert!(malformed.to_string().contains("bad padding"));
    }
}

//! Schema versioning (spec §2.7): two independent numbers, both announced in `session.hello`.
//!
//! * `proto` (integer): the envelope, the method set and the semantics of ops and patches. Breaking
//!   changes bump it; host and client negotiate `min(proto)`.
//! * `kit` (`major.minor`): the component catalogue. Within a major, changes are additive only (new
//!   components, new optional props, new enum values). Every catalogue entry records `since`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The protocol version this build speaks.
pub const PROTO_VERSION: u32 = 1;

/// The component-catalogue version this build ships.
pub const KIT_VERSION: KitVersion = KitVersion::new(1, 0);

/// A catalogue version, `major.minor`. Serialized as the string `"1.0"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KitVersion {
    pub major: u16,
    pub minor: u16,
}

impl KitVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Does this version satisfy an app's `kit = ">=1.2"` requirement? Accepted forms: `1.2`,
    /// `>=1.2`, `^1.2` (same major, at least this minor).
    #[must_use]
    pub fn satisfies(self, requirement: &str) -> bool {
        let requirement = requirement.trim();
        let (caret, text) = match (requirement.strip_prefix(">="), requirement.strip_prefix('^')) {
            (Some(rest), _) => (false, rest),
            (None, Some(rest)) => (true, rest),
            (None, None) => (false, requirement),
        };
        let Ok(required) = text.trim().parse::<Self>() else { return false };
        if caret {
            self.major == required.major && self >= required
        } else {
            self >= required
        }
    }
}

impl fmt::Display for KitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A version string that is not `major.minor`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a kit version (expected `major.minor`)")]
pub struct ParseVersionError(String);

impl FromStr for KitVersion {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ParseVersionError(s.to_owned());
        let (major, minor) = s.split_once('.').ok_or_else(bad)?;
        Ok(Self { major: major.parse().map_err(|_| bad())?, minor: minor.parse().map_err(|_| bad())? })
    }
}

impl Serialize for KitVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KitVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Is the client's kit able to render a component introduced in `since`?
#[must_use]
pub fn kit_supports(client: KitVersion, since: KitVersion) -> bool {
    client.major == since.major && client >= since
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints() {
        let v: KitVersion = "1.12".parse().unwrap();
        assert_eq!(v, KitVersion::new(1, 12));
        assert_eq!(v.to_string(), "1.12");
        assert!("1".parse::<KitVersion>().is_err());
        assert!("a.b".parse::<KitVersion>().is_err());
    }

    #[test]
    fn requirements() {
        let v = KitVersion::new(1, 2);
        assert!(v.satisfies(">=1.0"));
        assert!(v.satisfies(">=1.2"));
        assert!(!v.satisfies(">=1.3"));
        assert!(v.satisfies("^1.1"));
        assert!(!KitVersion::new(2, 0).satisfies("^1.1"));
        assert!(!v.satisfies("nonsense"));
    }

    #[test]
    fn serde_is_a_string() {
        let json = serde_json::to_string(&KitVersion::new(1, 0)).unwrap();
        assert_eq!(json, "\"1.0\"");
        assert_eq!(serde_json::from_str::<KitVersion>(&json).unwrap(), KitVersion::new(1, 0));
    }

    #[test]
    fn support_is_per_major() {
        assert!(kit_supports(KitVersion::new(1, 3), KitVersion::new(1, 2)));
        assert!(!kit_supports(KitVersion::new(1, 1), KitVersion::new(1, 2)));
        assert!(!kit_supports(KitVersion::new(2, 0), KitVersion::new(1, 0)));
    }
}

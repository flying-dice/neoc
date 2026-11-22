//! The two version numbers the handshake (`session.hello`) announces.
//!
//! * `proto` (integer): the envelope, the method set and what the messages mean. A breaking change bumps it; host
//!   and client negotiate `min(proto)`.
//! * `kit` (`major.minor`): the version of the shell's kit the host was built against.

use std::fmt;

/// The protocol version this build speaks.
pub const PROTO_VERSION: u32 = 1;

/// The kit version this build ships.
pub const KIT_VERSION: KitVersion = KitVersion::new(1, 0);

/// A kit version, `major.minor`; printed as `1.0`.
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
}

impl fmt::Display for KitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_versions_are_announced() {
        assert_eq!(PROTO_VERSION, 1);
        assert_eq!(KIT_VERSION.to_string(), "1.0");
        assert_eq!(KitVersion::new(1, 12).to_string(), "1.12");
    }
}

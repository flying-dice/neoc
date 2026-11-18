//! The operating-system implementations of the small infrastructure ports: wall clock and secure randomness.

use agentos_application::ports::{Clock, RandomSource};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// The OS random number generator.
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill(&self, buf: &mut [u8]) {
        if let Err(e) = getrandom::fill(buf) {
            // Pairing codes and session tokens must never be predictable: if the OS cannot give us
            // randomness there is nothing safe to continue with.
            tracing::error!(error = %e, "the operating system random number generator failed");
            std::process::abort();
        }
    }
}

/// Lowercase hex of `bytes` random bytes.
#[must_use]
pub fn random_hex(random: &dyn RandomSource, bytes: usize) -> String {
    use std::fmt::Write as _;
    let mut buf = vec![0u8; bytes];
    random.fill(&mut buf);
    buf.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

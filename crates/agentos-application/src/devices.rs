//! Device pairing and sessions: how a browser becomes trusted to use this host.
//!
//! The host prints a short-lived **pairing code**; a client posts it and receives a **session token**
//! (a random secret) in exchange. Only SHA-256 hashes of codes and tokens are stored. Codes are
//! single-use, expire, and wrong guesses are rate limited and eventually burn every outstanding code.
//! There is no trusted-network shortcut: loopback clients pair like everyone else.

use crate::{
    ports::{Clock, DeviceRegistry, RandomSource},
    tokens::random_hex,
    PortError,
};
use agentos_domain::{hash_token, Device, DeviceId};
use parking_lot::Mutex;
use std::{collections::VecDeque, sync::Arc};

/// 31 symbols with no `0 O 1 I L`: safe to read aloud and to type from a screen.
const ALPHABET: &[u8; 31] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const CODE_LEN: usize = 8;
const MAX_DEVICE_NAME: usize = 64;
/// `last_seen` is written at most this often per session, so a busy socket does not hammer the database.
const TOUCH_INTERVAL_MS: u64 = 60_000;

/// Tunables of the pairing protocol (the defaults are what the host ships with).
#[derive(Clone, Debug)]
pub struct PairingPolicy {
    /// How long a code stays valid.
    pub code_ttl_ms: u64,
    /// Attempts allowed inside `attempt_window_ms` before the next one is refused with `RateLimited`.
    pub max_attempts: usize,
    pub attempt_window_ms: u64,
    /// Wrong guesses (inside `burn_window_ms`) after which every outstanding code is destroyed.
    pub burn_after_failures: usize,
    pub burn_window_ms: u64,
}

impl Default for PairingPolicy {
    fn default() -> Self {
        Self {
            code_ttl_ms: 10 * 60_000,
            max_attempts: 5,
            attempt_window_ms: 60_000,
            burn_after_failures: 10,
            burn_window_ms: 10 * 60_000,
        }
    }
}

/// A freshly issued pairing code, to be shown to the person at the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingCode {
    /// Display form, `ABCD-EFGH`.
    pub code: String,
    pub expires_at_ms: u64,
}

/// The result of a successful pairing: the device, and its session token (shown exactly once).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDevice {
    pub device: Device,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairError {
    /// Wrong, expired or already used. Deliberately one case: it must not tell a guesser which.
    #[error("invalid or expired pairing code")]
    InvalidCode,
    #[error("too many attempts; retry in {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error(transparent)]
    Port(#[from] PortError),
}

pub struct DeviceService {
    registry: Arc<dyn DeviceRegistry>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    policy: PairingPolicy,
    /// Timestamps (unix ms) of recent attempts, oldest first. In memory: a restart is a fresh window,
    /// but the codes themselves expire on their own.
    attempts: Mutex<VecDeque<u64>>,
    failures: Mutex<VecDeque<u64>>,
}

impl DeviceService {
    pub fn new(
        registry: Arc<dyn DeviceRegistry>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn RandomSource>,
        policy: PairingPolicy,
    ) -> Self {
        Self { registry, clock, random, policy, attempts: Mutex::default(), failures: Mutex::default() }
    }

    /// Issue a new pairing code (shown at the host: terminal or CLI). Older codes stay valid until they
    /// expire or are used.
    pub async fn issue_pairing_code(&self) -> Result<PairingCode, PortError> {
        let now = self.clock.now_unix_ms();
        let code = self.random_code();
        let expires_at_ms = now + self.policy.code_ttl_ms;
        self.registry.add_pairing_code(&hash_token(&code), expires_at_ms).await?;
        Ok(PairingCode {
            code: format!("{}-{}", &code[..CODE_LEN / 2], &code[CODE_LEN / 2..]),
            expires_at_ms,
        })
    }

    /// Exchange a pairing code for a session. `device_name` is what the client calls itself.
    pub async fn pair(&self, code: &str, device_name: &str) -> Result<PairedDevice, PairError> {
        let now = self.clock.now_unix_ms();
        // count the attempt BEFORE checking the code, so concurrent guesses cannot slip past the limit
        self.reserve_attempt(now)?;

        let normalised = normalise_code(code);
        let valid = match &normalised {
            Some(c) => self.registry.take_pairing_code(&hash_token(c), now).await?,
            None => false,
        };
        if !valid {
            self.record_failure(now).await?;
            return Err(PairError::InvalidCode);
        }
        self.release_attempt(now);

        let token = self.random_token();
        let device = Device {
            id: DeviceId(random_hex(self.random.as_ref(), 8)),
            name: clean_device_name(device_name),
            created_at: now,
            last_seen_at: now,
        };
        self.registry.add_session(&device, &hash_token(&token)).await?;
        Ok(PairedDevice { device, token })
    }

    /// Resolve a session token to its device, or `None` (unknown, revoked or malformed).
    pub async fn authenticate(&self, token: &str) -> Option<Device> {
        if token.is_empty() || token.len() > 128 {
            return None;
        }
        let mut device = match self.registry.find_session(&hash_token(token)).await {
            Ok(found) => found?,
            Err(e) => {
                tracing::error!(error = %e, "session lookup failed");
                return None;
            }
        };
        let now = self.clock.now_unix_ms();
        if now.saturating_sub(device.last_seen_at) >= TOUCH_INTERVAL_MS {
            if let Err(e) = self.registry.touch_session(&device.id, now).await {
                tracing::warn!(error = %e, "could not update last-seen");
            }
            device.last_seen_at = now;
        }
        Some(device)
    }

    pub async fn list(&self) -> Result<Vec<Device>, PortError> {
        self.registry.list_devices().await
    }

    /// Revoke a device's session. `NotFound` if it is not paired.
    pub async fn revoke(&self, id: &DeviceId) -> Result<(), PortError> {
        if self.registry.revoke_device(id).await? {
            Ok(())
        } else {
            Err(PortError::NotFound(format!("device {}", id.0)))
        }
    }

    fn reserve_attempt(&self, now: u64) -> Result<(), PairError> {
        let mut attempts = self.attempts.lock();
        while attempts.front().is_some_and(|t| now.saturating_sub(*t) >= self.policy.attempt_window_ms) {
            attempts.pop_front();
        }
        if attempts.len() >= self.policy.max_attempts {
            let oldest = attempts.front().copied().unwrap_or(now);
            let wait_ms = (oldest + self.policy.attempt_window_ms).saturating_sub(now);
            return Err(PairError::RateLimited { retry_after_secs: wait_ms.div_ceil(1000).max(1) });
        }
        attempts.push_back(now);
        Ok(())
    }

    /// A correct code does not count against the limit.
    fn release_attempt(&self, now: u64) {
        let mut attempts = self.attempts.lock();
        if let Some(i) = attempts.iter().rposition(|t| *t == now) {
            attempts.remove(i);
        }
    }

    async fn record_failure(&self, now: u64) -> Result<(), PairError> {
        let burn = {
            let mut failures = self.failures.lock();
            while failures.front().is_some_and(|t| now.saturating_sub(*t) >= self.policy.burn_window_ms) {
                failures.pop_front();
            }
            failures.push_back(now);
            let burn = failures.len() >= self.policy.burn_after_failures;
            if burn {
                failures.clear();
            }
            burn
        };
        if burn {
            tracing::warn!("too many wrong pairing codes: every outstanding code was destroyed");
            self.registry.clear_pairing_codes().await?;
        }
        Ok(())
    }

    fn random_token(&self) -> String {
        random_hex(self.random.as_ref(), 32)
    }

    /// Uniform over the alphabet (rejection sampling: 248 = 31 * 8 is the largest multiple below 256).
    fn random_code(&self) -> String {
        let mut out = String::with_capacity(CODE_LEN);
        let mut buf = [0u8; 16];
        while out.len() < CODE_LEN {
            self.random.fill(&mut buf);
            for b in buf {
                if b < 248 && out.len() < CODE_LEN {
                    out.push(char::from(ALPHABET[usize::from(b) % ALPHABET.len()]));
                }
            }
        }
        out
    }
}

/// `abcd-efgh`, ` ABCD EFGH `, ... -> `ABCDEFGH`; `None` if it cannot be a code (wrong length or symbols).
fn normalise_code(raw: &str) -> Option<String> {
    let code: String =
        raw.chars().filter(|c| !c.is_whitespace() && *c != '-').map(|c| c.to_ascii_uppercase()).collect();
    let ok = code.len() == CODE_LEN && code.bytes().all(|b| ALPHABET.contains(&b));
    ok.then_some(code)
}

fn clean_device_name(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "Unnamed device".to_owned()
    } else {
        cleaned.chars().take(MAX_DEVICE_NAME).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ManualClock, MemDevices, SeededRandom};

    struct Rig {
        svc: DeviceService,
        clock: Arc<ManualClock>,
        registry: Arc<MemDevices>,
    }

    fn rig() -> Rig {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let registry = Arc::new(MemDevices::default());
        let svc = DeviceService::new(
            registry.clone(),
            clock.clone(),
            Arc::new(SeededRandom::default()),
            PairingPolicy::default(),
        );
        Rig { svc, clock, registry }
    }

    #[tokio::test]
    async fn a_code_pairs_a_device_once_and_the_session_authenticates() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        assert_eq!(code.code.len(), 9, "ABCD-EFGH");
        assert_eq!(code.expires_at_ms, 1_000_000 + 600_000);

        let paired = r.svc.pair(&code.code.to_lowercase(), "  Safari on iPad ").await.unwrap();
        assert_eq!(paired.device.name, "Safari on iPad");
        assert_eq!(r.svc.authenticate(&paired.token).await, Some(paired.device.clone()));
        assert_eq!(r.svc.authenticate("not-a-session").await, None);

        // single use
        assert_eq!(r.svc.pair(&code.code, "again").await, Err(PairError::InvalidCode));
        assert_eq!(r.svc.list().await.unwrap(), vec![paired.device]);
    }

    #[tokio::test]
    async fn only_hashes_are_stored() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(&code.code, "d").await.unwrap();
        let stored = r.registry.dump();
        assert!(!stored.contains(&paired.token), "the session token must not be stored");
        assert!(!stored.contains(&code.code.replace('-', "")), "the pairing code must not be stored");
    }

    #[tokio::test]
    async fn a_code_expires_after_ten_minutes() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        r.clock.advance(599_000);
        let again = r.svc.issue_pairing_code().await.unwrap();
        r.clock.advance(2_000); // first code is now 601 s old, the second 2 s
        assert_eq!(r.svc.pair(&code.code, "late").await, Err(PairError::InvalidCode));
        assert!(r.svc.pair(&again.code, "on time").await.is_ok());
    }

    #[tokio::test]
    async fn wrong_codes_are_rate_limited_and_a_right_code_is_refused_while_limited() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        for _ in 0..5 {
            assert_eq!(r.svc.pair("AAAA-AAAA", "x").await, Err(PairError::InvalidCode));
        }
        // the sixth attempt is refused outright, even with the right code
        assert!(matches!(
            r.svc.pair(&code.code, "x").await,
            Err(PairError::RateLimited { retry_after_secs }) if (1..=60).contains(&retry_after_secs)
        ));
        r.clock.advance(61_000);
        assert!(r.svc.pair(&code.code, "x").await.is_ok(), "the window slid past the failures");
    }

    #[tokio::test]
    async fn persistent_guessing_burns_every_outstanding_code() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        for _ in 0..2 {
            for _ in 0..5 {
                let _ = r.svc.pair("AAAA-AAAA", "x").await;
            }
            r.clock.advance(61_000);
        }
        assert_eq!(r.svc.pair(&code.code, "x").await, Err(PairError::InvalidCode), "burned");
    }

    #[tokio::test]
    async fn malformed_codes_count_as_failures() {
        let r = rig();
        for bad in ["", "short", "ABCD-EFG0", "ÅÄÖÅ-ÄÖÅÄ"] {
            assert_eq!(r.svc.pair(bad, "x").await, Err(PairError::InvalidCode), "{bad}");
        }
    }

    #[tokio::test]
    async fn revoking_a_device_ends_its_session() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(&code.code, "d").await.unwrap();
        r.svc.revoke(&paired.device.id).await.unwrap();
        assert_eq!(r.svc.authenticate(&paired.token).await, None);
        assert!(r.svc.list().await.unwrap().is_empty());
        assert!(matches!(r.svc.revoke(&paired.device.id).await, Err(PortError::NotFound(_))));
    }

    #[tokio::test]
    async fn last_seen_is_refreshed_at_most_once_a_minute() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(&code.code, "d").await.unwrap();
        r.clock.advance(10_000);
        assert_eq!(r.svc.authenticate(&paired.token).await.unwrap().last_seen_at, 1_000_000);
        r.clock.advance(60_000);
        assert_eq!(r.svc.authenticate(&paired.token).await.unwrap().last_seen_at, 1_070_000);
        assert_eq!(r.svc.list().await.unwrap()[0].last_seen_at, 1_070_000);
    }

    #[test]
    fn device_names_are_cleaned() {
        assert_eq!(clean_device_name("\n\t"), "Unnamed device");
        assert_eq!(clean_device_name(&"x".repeat(200)).len(), MAX_DEVICE_NAME);
        assert_eq!(clean_device_name("a\u{0}b"), "ab");
    }

    #[test]
    fn codes_use_only_the_unambiguous_alphabet() {
        let svc = rig().svc;
        for _ in 0..200 {
            let c = svc.random_code();
            assert!(c.bytes().all(|b| ALPHABET.contains(&b)), "{c}");
            assert!(!c.contains(['0', 'O', '1', 'I', 'L']));
        }
    }
}

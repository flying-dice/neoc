//! Device pairing and sessions: how a browser becomes trusted to use this host.
//!
//! The host prints a short-lived **pairing code**; a client posts it and receives a **session token**
//! (a random secret) in exchange. Only SHA-256 hashes of codes and tokens are stored. Codes are
//! single-use, expire, and wrong guesses are rate limited (per client address, under a ceiling for all
//! clients together). Wrong guesses are also counted per client, each client counting for at most
//! `burn_failures_per_client` of them, and once the clients together reach `burn_after_failures` every outstanding
//! code is burned: a distributed guesser destroys the codes, but one client inside its own limit cannot.
//! There is no trusted-network shortcut: loopback clients pair like everyone else.

use crate::{
    ports::{Clock, DeviceRegistry, RandomSource},
    tokens::random_hex,
    PortError,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};
use vos_domain::{hash_token, Device, DeviceId};

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
    /// Attempts one client address gets inside `attempt_window_ms` before its next one is refused with
    /// `RateLimited`. One noisy client cannot stall the others.
    pub max_attempts: usize,
    /// Attempts all clients together get inside the window: the ceiling that bounds a guesser who has many
    /// addresses.
    pub max_attempts_total: usize,
    pub attempt_window_ms: u64,
    /// Wrong guesses (inside `burn_window_ms`, counted per client, each client at most
    /// `burn_failures_per_client`) after which every outstanding code is destroyed: a ceiling for guessers
    /// with many addresses. It should be above `burn_failures_per_client`, or one client alone could burn the
    /// codes.
    pub burn_after_failures: usize,
    /// How many wrong guesses one client address contributes to that ceiling.
    pub burn_failures_per_client: usize,
    pub burn_window_ms: u64,
}

impl Default for PairingPolicy {
    fn default() -> Self {
        Self {
            code_ttl_ms: 10 * 60_000,
            max_attempts: 5,
            max_attempts_total: 20,
            attempt_window_ms: 60_000,
            burn_after_failures: 10,
            burn_failures_per_client: 5,
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

/// Who is guessing: the client's address (an IPv6 one by its /64, so a whole subnet is one client), or
/// `None` when the transport cannot say.
type Client = Option<IpAddr>;

fn client_of(address: Option<IpAddr>) -> Client {
    match address {
        Some(IpAddr::V6(v6)) => Some(match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & (u128::MAX << 64))),
        }),
        other => other,
    }
}

/// Recent pairing attempts: per client, and all together.
#[derive(Default)]
struct Attempts {
    per_client: HashMap<Client, VecDeque<u64>>,
    all: VecDeque<u64>,
}

pub struct DeviceService {
    registry: Arc<dyn DeviceRegistry>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    policy: PairingPolicy,
    /// Timestamps (unix ms) of recent attempts, oldest first. In memory: a restart is a fresh window,
    /// but the codes themselves expire on their own.
    attempts: Mutex<Attempts>,
    /// Recent wrong guesses per client (at most `burn_failures_per_client` kept each), oldest first.
    failures: Mutex<HashMap<Client, VecDeque<u64>>>,
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

    /// Exchange a pairing code for a session. `device_name` is what the client calls itself; `client` is the
    /// address it connects from (what the attempt limit is counted against), `None` if unknown.
    pub async fn pair(
        &self,
        client: Option<IpAddr>,
        code: &str,
        device_name: &str,
    ) -> Result<PairedDevice, PairError> {
        let now = self.clock.now_unix_ms();
        let client = client_of(client);
        // count the attempt BEFORE checking the code, so concurrent guesses cannot slip past the limit
        self.reserve_attempt(client, now)?;

        let normalised = normalise_code(code);
        let valid = match &normalised {
            Some(c) => self.registry.take_pairing_code(&hash_token(c), now).await?,
            None => false,
        };
        if !valid {
            self.record_failure(client, now).await?;
            return Err(PairError::InvalidCode);
        }
        self.release_attempt(client, now);

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

    fn reserve_attempt(&self, client: Client, now: u64) -> Result<(), PairError> {
        let window = self.policy.attempt_window_ms;
        let expired = |queue: &mut VecDeque<u64>| {
            while queue.front().is_some_and(|t| now.saturating_sub(*t) >= window) {
                queue.pop_front();
            }
        };
        let mut attempts = self.attempts.lock();
        let Attempts { per_client, all } = &mut *attempts;
        expired(all);
        // a client with nothing recent is forgotten, so the map stays as small as the traffic
        per_client.retain(|_, queue| {
            expired(queue);
            !queue.is_empty()
        });
        let mine = per_client.entry(client).or_default();
        let (limit, queue) = if mine.len() >= self.policy.max_attempts {
            (true, &*mine)
        } else {
            (all.len() >= self.policy.max_attempts_total, &*all)
        };
        if limit {
            let oldest = queue.front().copied().unwrap_or(now);
            let wait_ms = (oldest + window).saturating_sub(now);
            return Err(PairError::RateLimited { retry_after_secs: wait_ms.div_ceil(1000).max(1) });
        }
        mine.push_back(now);
        all.push_back(now);
        Ok(())
    }

    /// A correct code does not count against the limit.
    fn release_attempt(&self, client: Client, now: u64) {
        let mut attempts = self.attempts.lock();
        if let Some(queue) = attempts.per_client.get_mut(&client) {
            if let Some(i) = queue.iter().rposition(|t| *t == now) {
                queue.remove(i);
            }
        }
        if let Some(i) = attempts.all.iter().rposition(|t| *t == now) {
            attempts.all.remove(i);
        }
    }

    async fn record_failure(&self, client: Client, now: u64) -> Result<(), PairError> {
        let burn = {
            let window = self.policy.burn_window_ms;
            let kept = self.policy.burn_failures_per_client.max(1);
            let mut failures = self.failures.lock();
            // a client with nothing recent is forgotten, so the map stays as small as the traffic
            failures.retain(|_, queue| {
                while queue.front().is_some_and(|t| now.saturating_sub(*t) >= window) {
                    queue.pop_front();
                }
                !queue.is_empty()
            });
            let mine = failures.entry(client).or_default();
            mine.push_back(now);
            while mine.len() > kept {
                mine.pop_front();
            }
            let counted: usize = failures.values().map(VecDeque::len).sum();
            let burn = counted >= self.policy.burn_after_failures;
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

        let paired = r.svc.pair(None, &code.code.to_lowercase(), "  Safari on iPad ").await.unwrap();
        assert_eq!(paired.device.name, "Safari on iPad");
        assert_eq!(r.svc.authenticate(&paired.token).await, Some(paired.device.clone()));
        assert_eq!(r.svc.authenticate("not-a-session").await, None);

        // single use
        assert_eq!(r.svc.pair(None, &code.code, "again").await, Err(PairError::InvalidCode));
        assert_eq!(r.svc.list().await.unwrap(), vec![paired.device]);
    }

    #[tokio::test]
    async fn only_hashes_are_stored() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(None, &code.code, "d").await.unwrap();
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
        assert_eq!(r.svc.pair(None, &code.code, "late").await, Err(PairError::InvalidCode));
        assert!(r.svc.pair(None, &again.code, "on time").await.is_ok());
    }

    #[tokio::test]
    async fn wrong_codes_are_rate_limited_and_a_right_code_is_refused_while_limited() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        for _ in 0..5 {
            assert_eq!(r.svc.pair(None, "AAAA-AAAA", "x").await, Err(PairError::InvalidCode));
        }
        // the sixth attempt is refused outright, even with the right code
        assert!(matches!(
            r.svc.pair(None, &code.code, "x").await,
            Err(PairError::RateLimited { retry_after_secs }) if (1..=60).contains(&retry_after_secs)
        ));
        r.clock.advance(61_000);
        assert!(r.svc.pair(None, &code.code, "x").await.is_ok(), "the window slid past the failures");
    }

    #[allow(clippy::unnecessary_wraps)] // the shape `pair` takes, so the calls read plainly
    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[tokio::test]
    async fn one_noisy_client_cannot_stall_everyone_else() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        for _ in 0..5 {
            assert_eq!(r.svc.pair(ip("192.168.1.50"), "AAAA-AAAA", "x").await, Err(PairError::InvalidCode));
        }
        assert!(
            matches!(
                r.svc.pair(ip("192.168.1.50"), "AAAA-AAAA", "x").await,
                Err(PairError::RateLimited { .. })
            ),
            "the guesser is limited"
        );
        // another address pairs with the right code at once
        assert!(r.svc.pair(ip("192.168.1.60"), &code.code, "iPad").await.is_ok());
    }

    #[tokio::test]
    async fn all_clients_together_stay_under_a_ceiling() {
        let r = rig();
        let total = PairingPolicy::default().max_attempts_total;
        // that many different addresses guess once each (the wrong guesses burn the codes issued so far)
        for n in 0..total {
            let _ = r.svc.pair(ip(&format!("10.0.0.{n}")), "AAAA-AAAA", "x").await;
        }
        let code = r.svc.issue_pairing_code().await.unwrap();
        let late = r.svc.pair(ip("10.0.1.1"), &code.code, "x").await;
        assert!(matches!(late, Err(PairError::RateLimited { .. })), "{late:?}");
        r.clock.advance(61_000);
        assert!(r.svc.pair(ip("10.0.1.1"), &code.code, "x").await.is_ok());
    }

    #[test]
    fn an_ipv6_client_is_its_64_and_a_mapped_address_is_its_ipv4() {
        let key = |s: &str| client_of(ip(s));
        assert_eq!(key("2001:db8:1:2:aaaa::1"), key("2001:db8:1:2:bbbb::9"), "one /64, one client");
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
        assert_eq!(key("::ffff:192.168.1.5"), key("192.168.1.5"));
        assert_eq!(client_of(None), None);
    }

    #[tokio::test]
    async fn guessers_with_many_addresses_burn_every_outstanding_code() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        // two addresses, each inside its own limit, together reach the ceiling
        for client in ["10.0.0.1", "10.0.0.2"] {
            for _ in 0..5 {
                let _ = r.svc.pair(ip(client), "AAAA-AAAA", "x").await;
            }
        }
        assert_eq!(r.svc.pair(ip("10.0.0.3"), &code.code, "x").await, Err(PairError::InvalidCode), "burned");
    }

    #[tokio::test]
    async fn one_client_inside_its_own_limit_cannot_burn_the_codes() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        // 40 wrong guesses over eight minutes, always at the limit: each window of the rate limit is allowed
        for _ in 0..8 {
            for _ in 0..5 {
                assert_eq!(
                    r.svc.pair(ip("192.168.1.50"), "AAAA-AAAA", "x").await,
                    Err(PairError::InvalidCode)
                );
            }
            r.clock.advance(61_000);
        }
        let paired = r.svc.pair(ip("192.168.1.60"), &code.code, "iPad").await;
        assert!(paired.is_ok(), "the code the person is about to type still works: {paired:?}");
    }

    #[tokio::test]
    async fn failures_leave_the_count_when_the_window_passes() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        for client in ["10.0.0.1", "10.0.0.2"] {
            for _ in 0..4 {
                let _ = r.svc.pair(ip(client), "AAAA-AAAA", "x").await;
            }
        }
        r.clock.advance(11 * 60_000); // 8 failures: below the ceiling, and now older than the window
        for client in ["10.0.0.3", "10.0.0.4"] {
            for _ in 0..4 {
                let _ = r.svc.pair(ip(client), "AAAA-AAAA", "x").await;
            }
        }
        let fresh = r.svc.issue_pairing_code().await.unwrap();
        assert!(r.svc.pair(ip("10.0.0.5"), &fresh.code, "x").await.is_ok(), "{code:?} aged out, not burned");
    }

    #[tokio::test]
    async fn malformed_codes_count_as_failures() {
        let r = rig();
        for bad in ["", "short", "ABCD-EFG0", "ÅÄÖÅ-ÄÖÅÄ"] {
            assert_eq!(r.svc.pair(None, bad, "x").await, Err(PairError::InvalidCode), "{bad}");
        }
    }

    #[tokio::test]
    async fn revoking_a_device_ends_its_session() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(None, &code.code, "d").await.unwrap();
        r.svc.revoke(&paired.device.id).await.unwrap();
        assert_eq!(r.svc.authenticate(&paired.token).await, None);
        assert!(r.svc.list().await.unwrap().is_empty());
        assert!(matches!(r.svc.revoke(&paired.device.id).await, Err(PortError::NotFound(_))));
    }

    #[tokio::test]
    async fn last_seen_is_refreshed_at_most_once_a_minute() {
        let r = rig();
        let code = r.svc.issue_pairing_code().await.unwrap();
        let paired = r.svc.pair(None, &code.code, "d").await.unwrap();
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

//! The OS CSPRNG and UUIDs.
//!
//! All randomness comes from `SysRng` (the OS entropy source via `rand`), never from the seedable `math.random`.
//! UUIDs use the `uuid` crate's v4 (random) and v7 (timestamp-ordered) generators, which draw from the same source.
//!
//! Ported from the old VM's `lib:random`. What changed on purpose: counts are `u32` (a negative one is refused as an
//! argument), the alphabet of `random_string` is a `String` (not valid UTF-8 is refused as an argument), and every
//! failure is a typed error table.

use rand::TryRng;
use rand::rngs::SysRng;

/// The largest buffer, counted in bytes or in characters, that one call may size: 1 Gi. A bigger request is an
/// error rather than an allocation failure that takes the whole process down.
const MAX_BUFFER: u32 = 1 << 30;

/// The default alphabet of `random_string`: digits, upper and lower case letters (about 5.95 bits a character).
const DEFAULT_ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Fill `buf` from the OS CSPRNG; an entropy-source failure is the reason, as text.
pub(crate) fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    SysRng.try_fill_bytes(buf).map_err(|e| e.to_string())
}

/// Unbiased uniform draw in `[0, span)` by rejection sampling on `u128`. `span` is at least 1.
fn uniform_u128(span: u128) -> Result<u128, String> {
    if span == 1 {
        return Ok(0);
    }
    // Reject draws in the partial bucket above `limit`, so what is left is an exact multiple of `span`.
    let limit = u128::MAX - (u128::MAX % span);
    loop {
        let mut buf = [0u8; 16];
        fill_random(&mut buf)?;
        let draw = u128::from_le_bytes(buf);
        if draw < limit {
            return Ok(draw % span);
        }
    }
}

/// Batches OS entropy for exact uniform draws below a small span: one `getrandom` call fills a chunk of `u32`
/// draws instead of one call (and 16 bytes) per character. Rejection sampling is unchanged, so every draw is
/// exactly uniform, and a failed refill is still an error.
struct SmallDraws {
    buf: Vec<u8>,
    pos: usize,
    chunk: usize,
}

impl SmallDraws {
    /// `expected` is the number of draws the caller will need (a hint that sizes the first chunk, capped at 4 KiB).
    fn new(expected: usize) -> Self {
        Self { buf: Vec::new(), pos: 0, chunk: expected.saturating_mul(4).clamp(16, 4096) }
    }

    fn next_u32(&mut self) -> Result<u32, String> {
        self.next_u32_from(&mut fill_random)
    }

    /// `next_u32` with the entropy source passed in, so a test can inject a failing one.
    fn next_u32_from(
        &mut self,
        fill: &mut dyn FnMut(&mut [u8]) -> Result<(), String>,
    ) -> Result<u32, String> {
        if self.pos + 4 > self.buf.len() {
            self.buf.resize(self.chunk, 0);
            if let Err(e) = fill(&mut self.buf) {
                // Drop the zeroed, never-filled buffer so a retry refills instead of handing out zeros as entropy.
                self.buf.clear();
                self.pos = 0;
                return Err(e);
            }
            self.pos = 0;
        }
        let bytes =
            [self.buf[self.pos], self.buf[self.pos + 1], self.buf[self.pos + 2], self.buf[self.pos + 3]];
        self.pos += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Exactly uniform index in `[0, len)`; `len` is at least 1.
    fn below(&mut self, len: usize) -> Result<usize, String> {
        let span = len as u64;
        if span > 1 << 32 {
            return Ok(uniform_u128(u128::from(span))? as usize);
        }
        // Reject draws in the partial bucket above `limit` (no modulo bias).
        let limit = (1u64 << 32) - ((1u64 << 32) % span);
        loop {
            let draw = u64::from(self.next_u32()?);
            if draw < limit {
                return Ok((draw % span) as usize);
            }
        }
    }
}

/// A zeroed buffer of `len` bytes, allocated fallibly.
fn zeroed(len: usize) -> Result<Vec<u8>, usize> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(len).map_err(|_| len)?;
    buf.resize(len, 0);
    Ok(buf)
}

#[mlua_ir::module("lib:random")]
pub mod random {
    use ::base64::Engine as _;
    use ::base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use mlua_ir::{Bytes, LuaError};
    use serde::Serialize;
    use uuid::Uuid;

    use super::{DEFAULT_ALPHABET, MAX_BUFFER, SmallDraws, fill_random, uniform_u128, zeroed};

    /// Why random data could not be made.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum RandomError {
        /// The request is bigger than one call may size.
        #[error("{requested} is too large (the most one call may size is {max})")]
        TooLarge {
            /// What was asked for.
            requested: u32,
            /// The most a call may size.
            max: u32,
        },
        /// `random_int` was given `min` greater than `max`.
        #[error("min ({min}) must be <= max ({max})")]
        EmptyRange {
            /// The lower bound given.
            min: i64,
            /// The upper bound given.
            max: i64,
        },
        /// `random_string` was given an empty alphabet.
        #[error("alphabet must be non-empty")]
        EmptyAlphabet,
        /// The buffer could not be allocated.
        #[error("cannot allocate {bytes} bytes")]
        OutOfMemory {
            /// The size that failed.
            bytes: u32,
        },
        /// The OS entropy source failed.
        #[error("OS entropy source failed: {reason}")]
        EntropyUnavailable {
            /// What the OS said.
            reason: String,
        },
    }

    /// Why bytes and a UUID could not be converted.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum UuidError {
        /// The text is not a UUID.
        #[error("{reason}")]
        InvalidUuid {
            /// Why not, as the parser says it.
            reason: String,
        },
        /// A UUID is 16 bytes.
        #[error("expected 16 bytes, got {length}")]
        WrongLength {
            /// The number of bytes given.
            length: u32,
        },
    }

    fn entropy(reason: String) -> RandomError {
        RandomError::EntropyUnavailable { reason }
    }

    /// `count` as a length, refused when beyond what one call may size.
    fn sized(count: u32) -> Result<usize, RandomError> {
        if count > MAX_BUFFER {
            return Err(RandomError::TooLarge { requested: count, max: MAX_BUFFER });
        }
        Ok(count as usize)
    }

    fn buffer(count: u32) -> Result<Vec<u8>, RandomError> {
        zeroed(sized(count)?).map_err(|_| RandomError::OutOfMemory { bytes: count })
    }

    /// `n_bytes` cryptographically random bytes, as a raw string.
    ///
    /// ```luau
    /// assert(#random.random_bytes(32) == 32)
    /// ```
    pub fn random_bytes(n_bytes: u32) -> Result<Bytes, RandomError> {
        let mut buf = buffer(n_bytes)?;
        fill_random(&mut buf).map_err(entropy)?;
        Ok(Bytes(buf))
    }

    /// A uniform random integer in the closed interval `[min, max]`: both ends can be drawn.
    ///
    /// ```luau
    /// local roll = assert(random.random_int(1, 6))
    /// assert(roll >= 1 and roll <= 6)
    /// ```
    pub fn random_int(min: i64, max: i64) -> Result<i64, RandomError> {
        if min > max {
            return Err(RandomError::EmptyRange { min, max });
        }
        // The span is 2^64 at the extreme (i64::MIN, i64::MAX), so compute in i128: the sum is always in i64 range.
        let span = (i128::from(max) - i128::from(min) + 1) as u128;
        let value = uniform_u128(span).map_err(entropy)?;
        Ok((i128::from(min) + value as i128) as i64)
    }

    /// A random string of `n` characters drawn uniformly from `alphabet` (default: digits, upper and lower case
    /// letters). The unit is a character, not a byte, so a multi-byte alphabet is safe.
    ///
    /// ```luau
    /// local code = assert(random.random_string(6, "01"))
    /// assert(#code == 6)
    /// ```
    pub fn random_string(n: u32, alphabet: Option<String>) -> Result<String, RandomError> {
        let len = sized(n)?;
        let alphabet: Vec<char> = alphabet.as_deref().unwrap_or(DEFAULT_ALPHABET).chars().collect();
        if alphabet.is_empty() {
            return Err(RandomError::EmptyAlphabet);
        }
        let mut out = String::new();
        out.try_reserve_exact(len).map_err(|_| RandomError::OutOfMemory { bytes: n })?;
        let mut draws = SmallDraws::new(len);
        for _ in 0..len {
            out.push(alphabet[draws.below(alphabet.len()).map_err(entropy)?]);
        }
        Ok(out)
    }

    /// A URL-safe base64 token (no padding) built from `n_bytes` random bytes.
    pub fn token_urlsafe(n_bytes: u32) -> Result<String, RandomError> {
        let mut buf = buffer(n_bytes)?;
        fill_random(&mut buf).map_err(entropy)?;
        Ok(URL_SAFE_NO_PAD.encode(&buf))
    }

    /// A random (version 4) UUID, as a canonical hyphenated string.
    pub fn uuid_v4() -> String {
        Uuid::new_v4().to_string()
    }

    /// A timestamp-ordered (version 7) UUID, as a canonical hyphenated string.
    pub fn uuid_v7() -> String {
        Uuid::now_v7().to_string()
    }

    /// The 16 raw bytes of a UUID written as text. Case does not matter, and braces (`{...}`) and the plain
    /// 32-digit form are read too.
    ///
    /// ```luau
    /// local bytes = assert(random.uuid_parse("550e8400-e29b-41d4-a716-446655440000"))
    /// assert(#bytes == 16)
    /// local _, err = random.uuid_parse("not-a-uuid")
    /// assert(err ~= nil and err.kind == "invalid_uuid")
    /// ```
    pub fn uuid_parse(input: String) -> Result<Bytes, UuidError> {
        Uuid::parse_str(&input)
            .map(|u| Bytes(u.as_bytes().to_vec()))
            .map_err(|e| UuidError::InvalidUuid { reason: e.to_string() })
    }

    /// 16 raw bytes as a canonical hyphenated UUID string.
    pub fn uuid_format(input: Bytes) -> Result<String, UuidError> {
        let arr: [u8; 16] =
            input.0.as_slice().try_into().map_err(|_| UuidError::WrongLength {
                length: u32::try_from(input.len()).unwrap_or(u32::MAX),
            })?;
        Ok(Uuid::from_bytes(arr).to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mlua::Lua;

    use super::random::{RandomError, random_int};

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:random")"#).eval().unwrap();
        lua.globals().set("rng", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    fn bytes(code: &str) -> Vec<u8> {
        let lua = engine();
        let s: mlua::LuaString = lua.load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"));
        s.as_bytes().to_vec()
    }

    /// `(kind, op, tostring)` of the typed error `code` returns as its second value.
    fn failure(code: &str) -> (String, String, String) {
        let lua = engine();
        let (value, err): (mlua::Value, mlua::Table) = lua.load(code).eval().unwrap();
        assert!(value.is_nil(), "{code} succeeded");
        let text: String = lua.load("return tostring(...)").call(err.clone()).unwrap();
        (err.get("kind").unwrap(), err.get("op").unwrap(), text)
    }

    fn refused(code: &str) -> String {
        engine().load(code).eval::<mlua::Value>().unwrap_err().to_string()
    }

    // random_bytes ----------------------------------------------------------

    #[test]
    fn random_bytes_length_matches_request() {
        assert_eq!(bytes("return rng.random_bytes(32)").len(), 32);
    }

    #[test]
    fn random_bytes_zero_is_empty() {
        assert!(bytes("return rng.random_bytes(0)").is_empty());
    }

    #[test]
    fn random_bytes_two_draws_differ() {
        // 32 bytes: a collision is negligible. If this ever flakes the CSPRNG is not being called.
        assert!(eval::<bool>("return rng.random_bytes(32) ~= rng.random_bytes(32)"));
    }

    #[test]
    fn a_negative_count_is_refused_with_its_place() {
        let err = refused("return rng.random_bytes(-1)");
        assert!(
            err.contains("lib:random.random_bytes: argument #1 'n_bytes': expected a whole number from 0 to"),
            "{err}"
        );
    }

    #[test]
    fn a_count_beyond_the_limit_is_a_typed_error() {
        let (kind, op, text) = failure("return rng.random_bytes(2 ^ 30 + 1)");
        assert_eq!((kind.as_str(), op.as_str()), ("too_large", "lib:random.random_bytes"));
        assert!(text.starts_with("lib:random.random_bytes: too_large:"), "{text}");
        let (requested, max): (u32, u32) =
            eval("local _, e = rng.token_urlsafe(2 ^ 30 + 5) return e.requested, e.max");
        assert_eq!((requested, max), ((1 << 30) + 5, 1 << 30));
    }

    // random_int ------------------------------------------------------------

    #[test]
    fn random_int_within_bounds() {
        let lua = engine();
        for _ in 0..500 {
            let v: i64 = lua.load("return rng.random_int(1, 6)").eval().unwrap();
            assert!((1..=6).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn random_int_inclusive_max() {
        // Width 1: every call yields the bound. Guards against an exclusive end.
        let lua = engine();
        for _ in 0..50 {
            assert_eq!(lua.load("return rng.random_int(7, 7)").eval::<i64>().unwrap(), 7);
        }
    }

    #[test]
    fn random_int_covers_full_range() {
        let lua = engine();
        let seen: HashSet<i64> =
            (0..500).map(|_| lua.load("return rng.random_int(0, 3)").eval().unwrap()).collect();
        for x in 0..=3 {
            assert!(seen.contains(&x), "value {x} never appeared in 500 draws");
        }
    }

    #[test]
    fn random_int_negative_range() {
        let lua = engine();
        for _ in 0..100 {
            let v: i64 = lua.load("return rng.random_int(-5, -1)").eval().unwrap();
            assert!((-5..=-1).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn random_int_min_greater_than_max_is_a_typed_error() {
        let (kind, op, _) = failure("return rng.random_int(10, 5)");
        assert_eq!((kind.as_str(), op.as_str()), ("empty_range", "lib:random.random_int"));
        let (min, max): (i64, i64) = eval("local _, e = rng.random_int(10, 5) return e.min, e.max");
        assert_eq!((min, max), (10, 5));
    }

    #[test]
    fn small_draws_reject_the_partial_bucket_and_stay_balanced() {
        // span = 2^31 + 1 leaves a partial bucket of 2^31 - 1 draws out of 2^32 (about half of all raw draws
        // are rejected), so this goes through the rejection branch thousands of times. Every result must be
        // below the span and the two halves roughly balanced.
        let span = (1usize << 31) + 1;
        let mut draws = super::SmallDraws::new(8);
        let mut low = 0u32;
        let n = 4000;
        for _ in 0..n {
            let v = draws.below(span).unwrap();
            assert!(v < span, "draw {v} outside span {span}");
            if v < span / 2 {
                low += 1;
            }
        }
        assert!((1700..=2300).contains(&low), "halves unbalanced: {low} of {n} below the midpoint");
        assert_eq!(draws.below(1).unwrap(), 0);
    }

    #[test]
    fn failed_refill_is_not_served_as_zero_entropy() {
        let mut draws = super::SmallDraws::new(8);
        let mut fail = |_: &mut [u8]| Err("boom".to_string());
        assert_eq!(draws.next_u32_from(&mut fail), Err("boom".to_string()));
        // The retry refills from the (now working) source, not from the zeroed buffer the failure left behind.
        let mut ones = |buf: &mut [u8]| {
            buf.fill(0xFF);
            Ok(())
        };
        assert_eq!(draws.next_u32_from(&mut ones), Ok(u32::MAX));
    }

    #[test]
    fn random_int_extreme_bounds_no_overflow() {
        // The full i64 span is 2^64: about half the draws land at or above 2^63, the path that wrapped before
        // the sum was done in i128.
        let mut saw_high_half = false;
        for _ in 0..200 {
            match random_int(i64::MIN, i64::MAX) {
                Ok(drawn) => saw_high_half |= drawn >= 0,
                Err(e) => panic!("no error path: {e}"),
            }
        }
        assert!(saw_high_half, "never drew a value >= 2^63 in 200 tries");
        assert!(matches!(random_int(1, 0), Err(RandomError::EmptyRange { min: 1, max: 0 })));
    }

    // random_string ---------------------------------------------------------

    #[test]
    fn random_string_default_alphabet() {
        let s: String = eval("return rng.random_string(20)");
        assert_eq!(s.len(), 20);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()), "{s}");
    }

    #[test]
    fn random_string_custom_alphabet() {
        let s: String = eval(r#"return rng.random_string(50, "01")"#);
        assert_eq!(s.len(), 50);
        assert!(s.chars().all(|c| c == '0' || c == '1'), "{s}");
    }

    #[test]
    fn random_string_multibyte_alphabet_stays_valid_utf8() {
        // Selecting bytes would split a multi-byte codepoint; characters are whole and exactly `n`.
        let s: String = eval(r#"return rng.random_string(64, "αβγ")"#);
        assert_eq!(s.chars().count(), 64);
        assert!(s.chars().all(|c| ['α', 'β', 'γ'].contains(&c)), "{s}");
    }

    #[test]
    fn random_string_invalid_utf8_alphabet_is_refused_with_its_place() {
        let err = refused(r#"return rng.random_string(8, "\xff\xfe")"#);
        assert!(err.contains("lib:random.random_string: argument #2 'alphabet'"), "{err}");
    }

    #[test]
    fn random_string_zero_length() {
        assert_eq!(eval::<String>("return rng.random_string(0)"), "");
    }

    #[test]
    fn random_string_empty_alphabet_is_a_typed_error() {
        let (kind, op, _) = failure(r#"return rng.random_string(8, "")"#);
        assert_eq!((kind.as_str(), op.as_str()), ("empty_alphabet", "lib:random.random_string"));
    }

    #[test]
    fn random_string_negative_length_is_refused() {
        let err = refused("return rng.random_string(-1)");
        assert!(err.contains("lib:random.random_string: argument #1 'n'"), "{err}");
    }

    // token_urlsafe ---------------------------------------------------------

    #[test]
    fn token_urlsafe_is_url_safe_and_unpadded() {
        let s: String = eval("return rng.token_urlsafe(32)");
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{s}");
        assert!(!s.contains('='));
        // 32 bytes -> ceil(32 * 4 / 3) = 43 characters without padding.
        assert_eq!(s.len(), 43);
    }

    #[test]
    fn token_urlsafe_zero_is_empty() {
        assert_eq!(eval::<String>("return rng.token_urlsafe(0)"), "");
    }

    #[test]
    fn token_urlsafe_negative_is_refused() {
        let err = refused("return rng.token_urlsafe(-1)");
        assert!(err.contains("lib:random.token_urlsafe: argument #1 'n_bytes'"), "{err}");
    }

    // UUIDs -----------------------------------------------------------------

    #[test]
    fn uuid_v4_canonical_form() {
        let s: String = eval("return rng.uuid_v4()");
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'), "v4 marker missing: {s}");
    }

    #[test]
    fn uuid_v4_two_draws_differ() {
        assert!(eval::<bool>("return rng.uuid_v4() ~= rng.uuid_v4()"));
    }

    #[test]
    fn uuid_v7_canonical_form_and_version_marker() {
        let s: String = eval("return rng.uuid_v7()");
        assert_eq!(s.len(), 36);
        assert!(s.split('-').nth(2).unwrap().starts_with('7'), "v7 marker missing: {s}");
    }

    #[test]
    fn uuid_v7_is_time_ordered() {
        // v7 ids start with a unix-ms timestamp: draws a few milliseconds apart sort in order.
        let lua = engine();
        let a: String = lua.load("return rng.uuid_v7()").eval().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b: String = lua.load("return rng.uuid_v7()").eval().unwrap();
        assert!(a < b, "v7 ordering broken: {a} !< {b}");
    }

    #[test]
    fn uuid_parse_round_trips_format() {
        let (orig, back, len): (String, String, i64) = eval(
            r#"
            local u = rng.uuid_v4()
            local bytes = assert(rng.uuid_parse(u))
            return u, rng.uuid_format(bytes), #bytes
            "#,
        );
        assert_eq!(orig, back);
        assert_eq!(len, 16);
    }

    #[test]
    fn uuid_parse_invalid_is_a_typed_error() {
        let (kind, op, text) = failure(r#"return rng.uuid_parse("not-a-uuid")"#);
        assert_eq!((kind.as_str(), op.as_str()), ("invalid_uuid", "lib:random.uuid_parse"));
        assert!(text.starts_with("lib:random.uuid_parse: invalid_uuid:"), "{text}");
    }

    #[test]
    fn uuid_format_wrong_length_is_a_typed_error() {
        let (kind, op, text) = failure(r#"return rng.uuid_format("too short")"#);
        assert_eq!((kind.as_str(), op.as_str()), ("wrong_length", "lib:random.uuid_format"));
        assert!(text.contains("expected 16 bytes, got 9"), "{text}");
        assert_eq!(eval::<u32>(r#"local _, e = rng.uuid_format("too short") return e.length"#), 9);
    }

    #[test]
    fn uuid_parse_accepts_uppercase_and_braces() {
        let (upper, braced): (String, String) = eval(
            r#"
            return rng.uuid_format((assert(rng.uuid_parse("550E8400-E29B-41D4-A716-446655440000")))),
                rng.uuid_format((assert(rng.uuid_parse("{550e8400-e29b-41d4-a716-446655440000}"))))
            "#,
        );
        assert_eq!(upper, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(braced, "550e8400-e29b-41d4-a716-446655440000");
    }
}

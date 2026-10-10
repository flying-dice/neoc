//! CSPRNG and UUIDs — `lib:random`.
//!
//! All randomness comes from `OsRng` (the OS getrandom syscall via the `rand`
//! crate), never from the seedable `math.random` PRNG. UUIDs use the `uuid`
//! crate's `v4` (random) and `v7` (timestamp-ordered) generators, which
//! themselves draw from the OS source.
//!
//! ## Surface
//!
//! Two concerns under one module — same split every major runtime ships:
//!
//! - Bytes / ints / strings: `random_bytes`, `random_int`, `random_string`,
//!   `token_urlsafe`. Matches Python `secrets`, Node `crypto`.
//! - UUIDs: `uuid_v4`, `uuid_v7`, `uuid_parse`, `uuid_format`. Universal naming.
//!
//! ## random_int is inclusive on both ends
//!
//! `random_int(min, max)` returns an integer in the closed interval `[min, max]`,
//! matching Lua's existing `math.random(m, n)` and Python's `random.randint`.
//! Picked over the half-open `[min, max)` convention (Node, Rust `gen_range`,
//! Go, Java) because the Lua audience already reads inclusive semantics from
//! `math.random`, and "between min and max" is the no-surprise reading for a
//! script writer.
//!
//! ## random_string default alphabet
//!
//! Default alphabet is `[0-9A-Za-z]` (62 chars) — what `secrets.token_urlsafe`,
//! GitHub tokens, and most session-id generators use when an alphabet isn't
//! specified. Callers wanting hex / base32 / custom can pass their own.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mlua::{Lua, Table, Value};
use rand::RngCore;
use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use tracing::instrument;
use uuid::Uuid;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};

/// Default alphabet for `random_string` when no alphabet is supplied.
/// 62 chars — digits + upper + lower. ~5.95 bits of entropy per char.
const DEFAULT_ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:random` surface through the facade.
///
/// `uuid_v4` / `uuid_v7` are infallible one-shot generators routed through
/// [`crate::lua::facade::Module::func`]; the rest are fallible and routed
/// through [`crate::lua::facade::Module::try_func`]. Each inner function
/// returns the bare failure reason, and the facade prepends the
/// `"random.<fn>: "` prefix, so the `(value, error)` shape, the exact error
/// strings, and the `lua::lib::random` spans are byte-identical to the
/// hand-written registration this replaces.
///
/// `random_bytes` and `uuid_parse` raise an `mlua::Error` from
/// `lua.create_string` (the bare `?`) when the decoded bytes can't be lifted
/// into a Lua string, while still routing their own validation/parse failures
/// to the `(value, error)` channel — a mixed shape `try_func` cannot model. Both
/// are registered verbatim with [`crate::lua::facade::Module::set`] (keeping
/// their `#[instrument]` span and `random.<fn>` channel prefix) and recorded for
/// the header with [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:random")?;
    m.set("random_bytes", lua.create_function(random_bytes)?)?
        .try_func(
            "random_int",
            &[("min", "number"), ("max", "number")],
            "Return a uniform random integer in the closed interval `[min, max]`.",
            |_, (min, max): (i64, i64)| random_int(min, max),
        )?
        .try_func(
            "random_string",
            &[("n", "number"), ("alphabet", "string?")],
            "Return a random string of `n` characters drawn from `alphabet` (default `[0-9A-Za-z]`).",
            |_, (n, alphabet): (i64, Option<mlua::String>)| random_string(n, alphabet),
        )?
        .try_func(
            "token_urlsafe",
            &[("n_bytes", "number")],
            "Return a URL-safe base64 token built from `n_bytes` random bytes.",
            |_, n_bytes: i64| token_urlsafe(n_bytes),
        )?
        .func(
            "uuid_v4",
            &[],
            "Generate a random (version 4) UUID as a canonical hyphenated string.",
            |_, _: ()| uuid_v4(),
        )?
        .func(
            "uuid_v7",
            &[],
            "Generate a timestamp-ordered (version 7) UUID as a canonical hyphenated string.",
            |_, _: ()| uuid_v7(),
        )?
        .set("uuid_parse", lua.create_function(uuid_parse)?)?
        .try_func(
            "uuid_format",
            &[("input", "string")],
            "Format 16 raw bytes as a canonical hyphenated UUID string.",
            |_, input: mlua::String| uuid_format(input),
        )?;

    m.record_fn(
        "random_bytes",
        &[("n_bytes", "number")],
        "Result<string, string>",
        "Return `n_bytes` cryptographically-random bytes as a raw string.",
    )
    .record_fn(
        "uuid_parse",
        &[("input", "string")],
        "Result<string, string>",
        "Parse a canonical UUID string and return its 16 raw bytes.",
    );

    Ok(m)
}

// ---------------------------------------------------------------------------
// CSPRNG
// ---------------------------------------------------------------------------

#[instrument(name = "random.random_bytes", target = "lua::lib::random", skip_all)]
fn random_bytes(lua: &Lua, n_bytes: i64) -> mlua::Result<(Value, Value)> {
    if n_bytes < 0 {
        return err(lua, "random.random_bytes: byte count must be non-negative");
    }
    let mut buf = vec![0u8; n_bytes as usize];
    OsRng.fill_bytes(&mut buf);
    ok(lua, lua.create_string(&buf)?)
}

fn random_int(min: i64, max: i64) -> Result<i64, String> {
    if min > max {
        return Err(format!("min ({min}) must be <= max ({max})"));
    }
    // Closed interval [min, max]: span = max - min + 1. Compute in i128 — at
    // the extreme (i64::MIN, i64::MAX) the span is 2^64 and `value` can reach
    // 2^64 - 1, so the final `min + value` must also stay in i128 before
    // narrowing. Casting `value as i64` first would wrap negative; adding in
    // i64 would overflow. The i128 sum is always within i64 range, so the
    // narrowing cast is lossless.
    let span = ((max as i128) - (min as i128) + 1) as u128;
    let value = uniform_u128(span);
    Ok(((min as i128) + (value as i128)) as i64)
}

/// Unbiased uniform draw in `[0, span)` via rejection sampling on `u128`.
/// `span` must be > 0; caller enforces (random_int's min<=max guard makes
/// span >= 1).
fn uniform_u128(span: u128) -> u128 {
    if span == 1 {
        return 0;
    }
    // Reject any draw landing in the partial bucket above `limit` so the
    // remaining range is an exact multiple of `span` — eliminates modulo bias.
    let limit = u128::MAX - (u128::MAX % span);
    loop {
        let mut buf = [0u8; 16];
        OsRng.fill_bytes(&mut buf);
        let draw = u128::from_le_bytes(buf);
        if draw < limit {
            return draw % span;
        }
    }
}

fn random_string(n: i64, alphabet: Option<mlua::String>) -> Result<String, String> {
    if n < 0 {
        return Err("length must be non-negative".to_string());
    }
    // Select whole characters, not bytes. Picking bytes out of a multi-byte
    // UTF-8 alphabet (e.g. "αβγ") splits codepoints and yields invalid UTF-8;
    // `chars()` is the unit the docs promise. ASCII alphabets are unaffected
    // (one byte per char), so the common case is unchanged.
    let alpha: Vec<char> = match alphabet.as_ref() {
        Some(s) => {
            let bytes = s.as_bytes();
            match std::str::from_utf8(&bytes) {
                Ok(st) => st.chars().collect(),
                Err(_) => return Err("alphabet must be valid UTF-8".to_string()),
            }
        }
        None => DEFAULT_ALPHABET.chars().collect(),
    };
    if alpha.is_empty() {
        return Err("alphabet must be non-empty".to_string());
    }
    let mut out = String::with_capacity(n as usize);
    for _ in 0..n {
        let pick = alpha
            .choose(&mut OsRng)
            .expect("alphabet non-empty (checked above)");
        out.push(*pick);
    }
    Ok(out)
}

fn token_urlsafe(n_bytes: i64) -> Result<String, String> {
    if n_bytes < 0 {
        return Err("byte count must be non-negative".to_string());
    }
    let mut buf = vec![0u8; n_bytes as usize];
    OsRng.fill_bytes(&mut buf);
    Ok(URL_SAFE_NO_PAD.encode(&buf))
}

// ---------------------------------------------------------------------------
// UUIDs
// ---------------------------------------------------------------------------

fn uuid_v4() -> mlua::Result<String> {
    Ok(Uuid::new_v4().to_string())
}

fn uuid_v7() -> mlua::Result<String> {
    Ok(Uuid::now_v7().to_string())
}

#[instrument(name = "random.uuid_parse", target = "lua::lib::random", skip_all)]
fn uuid_parse(lua: &Lua, input: mlua::String) -> mlua::Result<(Value, Value)> {
    let bytes = input.as_bytes();
    let s = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(e) => return from_err(lua, "random.uuid_parse", e),
    };
    match Uuid::parse_str(s) {
        Ok(u) => ok(lua, lua.create_string(u.as_bytes())?),
        Err(e) => from_err(lua, "random.uuid_parse", e),
    }
}

fn uuid_format(input: mlua::String) -> Result<String, String> {
    let bytes = input.as_bytes();
    if bytes.len() != 16 {
        return Err(format!("expected 16 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&bytes);
    Ok(Uuid::from_bytes(arr).to_string())
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");

    let mut g = c.benchmark_group("random");

    g.bench_function("library/random_bytes_32", |b| {
        b.iter(|| {
            let mut buf = [0u8; 32];
            OsRng.fill_bytes(black_box(&mut buf));
        });
    });

    g.bench_function("library/uuid_v4", |b| {
        b.iter(|| {
            let _ = black_box(Uuid::new_v4().to_string());
        });
    });

    g.bench_function("library/uuid_v7", |b| {
        b.iter(|| {
            let _ = black_box(Uuid::now_v7().to_string());
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function() return (require("lib:random")).random_bytes(32) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua/random_bytes_32", |b| {
        b.iter(|| {
            let _: mlua::String = chunk.call::<mlua::String>(()).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function() return (require("lib:random")).uuid_v4() end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua/uuid_v4", |b| {
        b.iter(|| {
            let _: String = chunk.call::<String>(()).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function() return (require("lib:random")).uuid_v7() end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua/uuid_v7", |b| {
        b.iter(|| {
            let _: String = chunk.call::<String>(()).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::Lua;
    use std::collections::HashSet;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:random")"#).eval().unwrap();
        lua.globals().set("rng", m).unwrap();
        lua
    }

    // random_bytes ----------------------------------------------------------

    #[test]
    fn random_bytes_length_matches_request() {
        let lua = engine();
        let s: mlua::String = lua.load(r#"return rng.random_bytes(32)"#).eval().unwrap();
        assert_eq!(s.as_bytes().len(), 32);
    }

    #[test]
    fn random_bytes_zero_is_empty() {
        let lua = engine();
        let s: mlua::String = lua.load(r#"return rng.random_bytes(0)"#).eval().unwrap();
        assert_eq!(s.as_bytes().len(), 0);
    }

    #[test]
    fn random_bytes_two_draws_differ() {
        // 32 bytes — collision probability negligible. If this ever flakes,
        // the CSPRNG is broken or this isn't being called.
        let lua = engine();
        let (a, b): (mlua::String, mlua::String) = lua
            .load(r#"return rng.random_bytes(32), rng.random_bytes(32)"#)
            .eval()
            .unwrap();
        assert_ne!(a.as_bytes().to_vec(), b.as_bytes().to_vec());
    }

    #[test]
    fn random_bytes_negative_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) =
            lua.load(r#"return rng.random_bytes(-1)"#).eval().unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.random_bytes:"));
    }

    // random_int ------------------------------------------------------------

    #[test]
    fn random_int_within_bounds() {
        let lua = engine();
        for _ in 0..500 {
            let v: i64 = lua.load(r#"return rng.random_int(1, 6)"#).eval().unwrap();
            assert!((1..=6).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn random_int_inclusive_max() {
        // Width 1 — every call must yield the bound. Guards against
        // exclusive-end regressions sneaking in.
        let lua = engine();
        for _ in 0..50 {
            let v: i64 = lua.load(r#"return rng.random_int(7, 7)"#).eval().unwrap();
            assert_eq!(v, 7);
        }
    }

    #[test]
    fn random_int_covers_full_range() {
        // Over many draws, both endpoints must appear. Catches off-by-one.
        let lua = engine();
        let mut seen = HashSet::new();
        for _ in 0..500 {
            let v: i64 = lua.load(r#"return rng.random_int(0, 3)"#).eval().unwrap();
            seen.insert(v);
        }
        for x in 0..=3 {
            assert!(seen.contains(&x), "value {x} never appeared in 500 draws");
        }
    }

    #[test]
    fn random_int_negative_range() {
        let lua = engine();
        for _ in 0..100 {
            let v: i64 = lua.load(r#"return rng.random_int(-5, -1)"#).eval().unwrap();
            assert!((-5..=-1).contains(&v), "out of range: {v}");
        }
    }

    #[test]
    fn random_int_min_greater_than_max_errors() {
        let lua = engine();
        let (v, e): (Option<i64>, Option<String>) =
            lua.load(r#"return rng.random_int(10, 5)"#).eval().unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.random_int:"));
    }

    #[test]
    fn random_int_extreme_bounds_no_overflow() {
        // Unreachable from Lua (number is f64; exact ints cap at 2^53) but a
        // Rust-direct caller can ask for the full i64 span. With span = 2^64,
        // ~half the draws land >= 2^63 — the exact path that wrapped/panicked
        // before the i128 fix. Loop so a wrap, if present, is near-certain.
        let mut saw_high_half = false;
        for _ in 0..200 {
            let drawn = super::random_int(i64::MIN, i64::MAX).expect("no error path");
            if drawn >= 0 {
                saw_high_half = true;
            }
        }
        assert!(saw_high_half, "never drew value >= 2^63 in 200 tries");
    }

    // random_string ---------------------------------------------------------

    #[test]
    fn random_string_default_alphabet_length() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.random_string(20)"#).eval().unwrap();
        assert_eq!(s.len(), 20);
        for c in s.chars() {
            assert!(c.is_ascii_alphanumeric(), "non-alphanumeric: {c}");
        }
    }

    #[test]
    fn random_string_custom_alphabet() {
        let lua = engine();
        let s: String = lua
            .load(r#"return rng.random_string(50, "01")"#)
            .eval()
            .unwrap();
        assert_eq!(s.len(), 50);
        for c in s.chars() {
            assert!(c == '0' || c == '1', "non-binary char: {c}");
        }
    }

    #[test]
    fn random_string_multibyte_alphabet_stays_valid_utf8() {
        // The bug: byte-level selection split multi-byte codepoints, emitting
        // invalid UTF-8. Decoding the result as a Rust `String` would fail.
        // Char-level selection must emit only whole chars from the alphabet,
        // and exactly `n` of them.
        let lua = engine();
        let s: String = lua
            .load(r#"return rng.random_string(64, "αβγ")"#)
            .eval()
            .unwrap();
        let alphabet = ['α', 'β', 'γ'];
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(
            chars.len(),
            64,
            "expected 64 characters, got {}",
            chars.len()
        );
        for c in chars {
            assert!(alphabet.contains(&c), "char outside alphabet: {c:?}");
        }
    }

    #[test]
    fn random_string_invalid_utf8_alphabet_errors() {
        // Alphabet bytes that aren't valid UTF-8 can't be decoded into chars.
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) = lua
            .load(r#"return rng.random_string(8, "\xff\xfe")"#)
            .eval()
            .unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.random_string:"));
    }

    #[test]
    fn random_string_zero_length() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.random_string(0)"#).eval().unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn random_string_empty_alphabet_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) = lua
            .load(r#"return rng.random_string(8, "")"#)
            .eval()
            .unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.random_string:"));
    }

    #[test]
    fn random_string_negative_length_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) =
            lua.load(r#"return rng.random_string(-1)"#).eval().unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.random_string:"));
    }

    // token_urlsafe ---------------------------------------------------------

    #[test]
    fn token_urlsafe_is_url_safe() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.token_urlsafe(32)"#).eval().unwrap();
        // Base64-url-safe without padding: only [A-Za-z0-9_-].
        for c in s.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "non url-safe char: {c}"
            );
        }
        assert!(!s.contains('='));
    }

    #[test]
    fn token_urlsafe_length_matches_base64_of_input() {
        // 32 bytes → ceil(32 * 4 / 3) = 43 chars without padding.
        let lua = engine();
        let s: String = lua.load(r#"return rng.token_urlsafe(32)"#).eval().unwrap();
        assert_eq!(s.len(), 43);
    }

    #[test]
    fn token_urlsafe_zero_is_empty() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.token_urlsafe(0)"#).eval().unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn token_urlsafe_negative_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) =
            lua.load(r#"return rng.token_urlsafe(-1)"#).eval().unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.token_urlsafe:"));
    }

    // UUIDs -----------------------------------------------------------------

    #[test]
    fn uuid_v4_canonical_form() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.uuid_v4()"#).eval().unwrap();
        assert_eq!(s.len(), 36);
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        // v4: third group starts with '4'.
        assert!(parts[2].starts_with('4'), "v4 marker missing: {s}");
    }

    #[test]
    fn uuid_v4_two_draws_differ() {
        let lua = engine();
        let (a, b): (String, String) = lua
            .load(r#"return rng.uuid_v4(), rng.uuid_v4()"#)
            .eval()
            .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_v7_canonical_form_and_version_marker() {
        let lua = engine();
        let s: String = lua.load(r#"return rng.uuid_v7()"#).eval().unwrap();
        assert_eq!(s.len(), 36);
        let parts: Vec<&str> = s.split('-').collect();
        assert!(parts[2].starts_with('7'), "v7 marker missing: {s}");
    }

    #[test]
    fn uuid_v7_is_time_ordered() {
        // v7 ids are unix-ms timestamp prefixed. Successive draws separated by
        // a millisecond must compare lexicographically increasing.
        let lua = engine();
        let a: String = lua.load(r#"return rng.uuid_v7()"#).eval().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b: String = lua.load(r#"return rng.uuid_v7()"#).eval().unwrap();
        assert!(a < b, "v7 ordering broken: {a} !< {b}");
    }

    #[test]
    fn uuid_parse_round_trips_format() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local u = rng.uuid_v4()
                local bytes, err = rng.uuid_parse(u)
                assert(err == nil)
                assert(#bytes == 16)
                local back = rng.uuid_format(bytes)
                return u .. "|" .. back
                "#,
            )
            .eval()
            .unwrap();
        let (orig, back) = s.split_once('|').unwrap();
        assert_eq!(orig, back);
    }

    #[test]
    fn uuid_parse_invalid_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) = lua
            .load(r#"return rng.uuid_parse("not-a-uuid")"#)
            .eval()
            .unwrap();
        assert!(v.is_none());
        assert!(e.unwrap().starts_with("random.uuid_parse:"));
    }

    #[test]
    fn uuid_format_wrong_length_errors() {
        let lua = engine();
        let (v, e): (Option<String>, Option<String>) = lua
            .load(r#"return rng.uuid_format("too short")"#)
            .eval()
            .unwrap();
        assert!(v.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("random.uuid_format:"));
        assert!(e.contains("expected 16 bytes"));
    }

    #[test]
    fn uuid_parse_accepts_uppercase_and_braces() {
        // `uuid::parse_str` is permissive on case and braces — call out the
        // contract so a future regression is caught.
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local bytes, err = rng.uuid_parse("550E8400-E29B-41D4-A716-446655440000")
                assert(err == nil, err)
                return rng.uuid_format(bytes)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "550e8400-e29b-41d4-a716-446655440000");

        // Braced (Microsoft GUID) form — `uuid::parse_str` strips the braces.
        let braced: String = lua
            .load(
                r#"
                local bytes, err = rng.uuid_parse("{550e8400-e29b-41d4-a716-446655440000}")
                assert(err == nil, err)
                return rng.uuid_format(bytes)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(braced, "550e8400-e29b-41d4-a716-446655440000");
    }
}

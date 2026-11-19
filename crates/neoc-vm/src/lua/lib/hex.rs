//! hex encode / decode — `lib:hex`.
//!
//! See docs/src/lib/reference/hex.md for the public surface contract.
//!
//! Mirrors the `hex` crate (the de-facto Rust standard named in the issue):
//! `encode` is lowercase, `encode_upper` is uppercase — separate functions
//! rather than a boolean flag, matching `lib:base64`'s per-variant naming and
//! sidestepping a positional-bool footgun. `decode` is case-insensitive and
//! skips ASCII whitespace (so hex dumps and space-grouped wire formats decode
//! without pre-cleaning), then strict: odd length or any non-hex character is
//! refused via `from_err`.

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:hex` surface through the facade. `encode` / `encode_upper`
/// are infallible [`crate::lua::facade::Module::func`]s.
///
/// `decode` is a *mixed* raise-plus-channel shape: a hex-decode failure goes to
/// the `(value, error)` channel via [`crate::lua::sandbox::from_err`], but a
/// `lua.create_string` failure on the decoded bytes raises an `mlua::Error`
/// directly (the bare `?`) — a mix `try_func` cannot model. It is registered
/// verbatim with [`crate::lua::facade::Module::set`] (keeping its
/// `#[instrument]` span and `hex.decode` channel prefix) and recorded with [`crate::lua::facade::Module::record_fn`]. The
/// `(value, error)` shape, the `hex.decode` error prefix, the raise on
/// `create_string`, and the `lua::lib::hex` spans are byte-identical to the
/// hand-written registration this replaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:hex")?;
    m.func(
        "encode",
        &[("input", "string")],
        "Encode `input` bytes to a lowercase hex string — two characters per byte, drawn from `0`–`9` and `a`–`f`.",
        |_, input: mlua::LuaString| Ok(hex::encode(input.as_bytes())),
    )?
    .func(
        "encode_upper",
        &[("input", "string")],
        "Encode `input` bytes to an uppercase hex string (`0`–`9` and `A`–`F`).",
        |_, input: mlua::LuaString| Ok(hex::encode_upper(input.as_bytes())),
    )?
    .set("decode", lua.create_function(decode)?)?;

    m.record_fn(
        "decode",
        &[("input", "string")],
        "Result<string, string>",
        "Decode a hex string back to raw bytes; case-insensitive and tolerant of ASCII whitespace, strict otherwise.",
    );

    Ok(m)
}

#[instrument(name = "hex.decode", target = "lua::lib::hex", skip_all)]
fn decode(lua: &Lua, input: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    // Drop ASCII whitespace before decoding so dumps like "de ad be ef" or
    // newline-wrapped blobs round-trip. The error position then refers to the
    // whitespace-stripped stream, but the offending character is still named —
    // good enough, and far less surprising than rejecting any space.
    let cleaned: Vec<u8> = input
        .as_bytes()
        .iter()
        .filter(|&&b| !b.is_ascii_whitespace())
        .copied()
        .collect();
    match hex::decode(&cleaned) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(e) => from_err(lua, "hex.decode", e),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
//
// Same three-tier shape as `lib:base64` — encode the same 2.25 KiB payload so
// iteration cost clears mlua call overhead and the deltas stay stable:
//   - library:      `hex::encode(bytes)` — pure hex crate.
//   - into_out_lua: identity Rust fn over the same `mlua::LuaString`. The bridge.
//   - through_lua:  `hex.encode(payload)` from a compiled chunk. End-to-end.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("hex");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    g.bench_function("library", |b| {
        b.iter(|| {
            let _ = hex::encode(black_box(payload.as_bytes()));
        });
    });

    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::LuaString| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::LuaString = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("lib:hex")).encode(p) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua", |b| {
        b.iter(|| {
            let _: mlua::LuaString = chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    g.finish();
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    /// Test-only engine. Binds `hex = require("lib:hex")` as a global to keep
    /// the inline test bodies short — these tests validate hex behaviour, not
    /// require's plumbing (which has its own coverage).
    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:hex")"#).eval().unwrap();
        lua.globals().set("hex", m).unwrap();
        lua
    }

    #[test]
    fn encode_lowercase() {
        let lua = engine();
        // 0xff 0xab — exercises the a–f range, lowercase only.
        let s: String = lua.load(r#"return hex.encode("\255\171")"#).eval().unwrap();
        assert_eq!(s, "ffab");
    }

    #[test]
    fn encode_upper_uppercase() {
        let lua = engine();
        let s: String = lua
            .load(r#"return hex.encode_upper("\255\171")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "FFAB");
    }

    #[test]
    fn encode_empty() {
        let lua = engine();
        let (lower, upper): (String, String) = lua
            .load(r#"return hex.encode(""), hex.encode_upper("")"#)
            .eval()
            .unwrap();
        assert_eq!(lower, "");
        assert_eq!(upper, "");
    }

    #[test]
    fn decode_known_value() {
        let lua = engine();
        let (s, err): (String, Option<String>) =
            lua.load(r#"return hex.decode("6869")"#).eval().unwrap();
        assert_eq!(s, "hi");
        assert!(err.is_none());
    }

    #[test]
    fn decode_is_case_insensitive() {
        let lua = engine();
        // Mixed case must decode to the same bytes as either pure case.
        let s: mlua::LuaString = lua
            .load(
                r#"
                local dec, err = hex.decode("fFaB")
                assert(err == nil)
                return dec
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[255u8, 171]);
    }

    #[test]
    fn decode_skips_whitespace() {
        let lua = engine();
        let s: mlua::LuaString = lua
            .load(
                r#"
                local dec, err = hex.decode("de ad\tbe\nef")
                assert(err == nil)
                return dec
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[0xde_u8, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn round_trip_binary() {
        let lua = engine();
        let s: mlua::LuaString = lua
            .load(
                r#"
                local raw = "\0\1\2\254\255"
                return hex.decode(hex.encode(raw))
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[0u8, 1, 2, 254, 255]);
    }

    #[test]
    fn decode_empty_is_empty_bytes() {
        let lua = engine();
        let (s, err): (String, Option<String>) =
            lua.load(r#"return hex.decode("")"#).eval().unwrap();
        assert_eq!(s, "");
        assert!(err.is_none());
    }

    #[test]
    fn decode_odd_length_errs() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) =
            lua.load(r#"return hex.decode("abc")"#).eval().unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("hex.decode:"));
    }

    #[test]
    fn decode_non_hex_errs() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) =
            lua.load(r#"return hex.decode("zz")"#).eval().unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("hex.decode:"));
    }
}

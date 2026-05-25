//! base64 encode / decode (standard and URL-safe alphabets).
//!
//! See docs/modules/base64.md for the public surface contract.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use mlua::{Lua, Table};

use crate::lua::facade::Module;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:base64` surface through the facade. Each of the four
/// alphabets exposes an infallible `encode*` and a fallible `decode*`. The
/// `(value, error)` shape, the `base64.decode*` error prefix, and the
/// `luau::lib::base64` span are byte-identical to the hand-written
/// registration this replaces.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:base64")?;
    m.func(
        "encode",
        &[("input", "string")],
        "Encode `input` using the standard alphabet (A–Z, a–z, 0–9, +, /) with `=` padding.",
        |_, s: mlua::String| Ok(STANDARD.encode(s.as_bytes())),
    )?
    .try_func(
        "decode",
        &[("input", "string")],
        "Decode a standard-alphabet base64 string back to raw bytes.",
        |lua, s: mlua::String| decode_with(lua, &STANDARD, &s.as_bytes()),
    )?
    .func(
        "encode_no_pad",
        &[("input", "string")],
        "Encode using the standard alphabet without `=` padding.",
        |_, s: mlua::String| Ok(STANDARD_NO_PAD.encode(s.as_bytes())),
    )?
    .try_func(
        "decode_no_pad",
        &[("input", "string")],
        "Decode standard-alphabet base64 that has no padding.",
        |lua, s: mlua::String| decode_with(lua, &STANDARD_NO_PAD, &s.as_bytes()),
    )?
    .func(
        "encode_url_safe",
        &[("input", "string")],
        "Encode using the URL-safe alphabet (`-` and `_` instead of `+` and `/`), with `=` padding.",
        |_, s: mlua::String| Ok(URL_SAFE.encode(s.as_bytes())),
    )?
    .try_func(
        "decode_url_safe",
        &[("input", "string")],
        "Decode URL-safe base64 with padding.",
        |lua, s: mlua::String| decode_with(lua, &URL_SAFE, &s.as_bytes()),
    )?
    .func(
        "encode_url_safe_no_pad",
        &[("input", "string")],
        "Encode using the URL-safe alphabet without padding.",
        |_, s: mlua::String| Ok(URL_SAFE_NO_PAD.encode(s.as_bytes())),
    )?
    .try_func(
        "decode_url_safe_no_pad",
        &[("input", "string")],
        "Decode URL-safe base64 without padding.",
        |lua, s: mlua::String| decode_with(lua, &URL_SAFE_NO_PAD, &s.as_bytes()),
    )?;
    Ok(m)
}

/// A decode failure rendered as a message whose `Display` matches the wrapped
/// error's. The facade prefixes it as `base64.<fn>: <e>`, so this preserves the
/// exact error string the original hand-written functions produced.
struct DecodeFail(String);

impl fmt::Display for DecodeFail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Decodes `bytes` with `engine` into a Lua byte string, mirroring the original
/// `lua.create_string(&decoded)` path so the decoded value stays a string (not
/// a numeric table). The base64 error flows to the facade's
/// `(nil, "base64.<fn>: <e>")` error channel unchanged.
fn decode_with(
    lua: &Lua,
    engine: &base64::engine::GeneralPurpose,
    bytes: &[u8],
) -> Result<mlua::String, DecodeFail> {
    let decoded = engine
        .decode(bytes)
        .map_err(|e| DecodeFail(e.to_string()))?;
    lua.create_string(&decoded)
        .map_err(|e| DecodeFail(e.to_string()))
}

// ---------------------------------------------------------------------------
// Benchmarks
//
// Three tiers, all encoding the same 2.25 KiB payload to bring iteration
// cost above mlua call overhead so the deltas are stable:
//   - library:       `STANDARD.encode(bytes)` — pure base64 crate.
//   - into_out_lua:  identity Rust fn registered with mlua, called with the
//                    same `mlua::String` shape. Isolates the bridge.
//   - through_lua:   `b64.encode(payload)` invoked from a compiled
//                    Lua chunk. End-to-end path users actually pay.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("base64");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    g.bench_function("library", |b| {
        b.iter(|| {
            let _ = STANDARD.encode(black_box(payload.as_bytes()));
        });
    });

    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("lib:base64")).encode(p) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua", |b| {
        b.iter(|| {
            let _: mlua::String = chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    g.finish();
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    /// Test-only engine. Binds `b64 = require("lib:base64")` as a global to
    /// keep the inline test bodies short — these tests validate base64
    /// behaviour, not require's plumbing (which has its own coverage).
    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:base64")"#).eval().unwrap();
        lua.globals().set("b64", m).unwrap();
        lua
    }

    #[test]
    fn encode_standard() {
        let lua = engine();
        let (s, err): (String, Option<String>) =
            lua.load(r#"return b64.encode("hello")"#).eval().unwrap();
        assert_eq!(s, "aGVsbG8=");
        assert!(err.is_none());
    }

    #[test]
    fn decode_standard() {
        let lua = engine();
        let (s, err): (String, Option<String>) =
            lua.load(r#"return b64.decode("aGVsbG8=")"#).eval().unwrap();
        assert_eq!(s, "hello");
        assert!(err.is_none());
    }

    #[test]
    fn round_trip_binary() {
        let lua = engine();
        let s: mlua::String = lua
            .load(
                r#"
                local raw = "\0\1\2\254\255"
                local enc = b64.encode(raw)
                local dec = b64.decode(enc)
                return dec
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[0u8, 1, 2, 254, 255]);
    }

    #[test]
    fn url_safe_alphabet() {
        let lua = engine();
        let (std_, url): (String, String) = lua
            .load(
                r#"
                local raw = "\251\255\254"
                local std_enc = b64.encode(raw)
                local url_enc = b64.encode_url_safe(raw)
                return std_enc, url_enc
            "#,
            )
            .eval()
            .unwrap();
        assert!(std_.contains('+') || std_.contains('/'));
        assert!(!url.contains('+') && !url.contains('/'));
        assert_eq!(std_.len(), url.len());
    }

    #[test]
    fn no_padding_variant() {
        let lua = engine();
        let (padded, bare): (String, String) = lua
            .load(
                r#"
                local raw = "hi"
                local p = b64.encode(raw)
                local b = b64.encode_no_pad(raw)
                return p, b
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(padded, "aGk=");
        assert_eq!(bare, "aGk");
    }

    #[test]
    fn decode_url_safe() {
        let lua = engine();
        let s: mlua::String = lua
            .load(
                r#"
                local raw = "\251\255\254"
                local url_enc = b64.encode_url_safe(raw)
                local dec, err = b64.decode_url_safe(url_enc)
                assert(err == nil)
                return dec
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[251u8, 255, 254]);
    }

    #[test]
    fn decode_invalid_returns_err() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return b64.decode("not*base64!")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let err = err.expect("error string expected");
        assert!(err.starts_with("base64.decode:"), "missing prefix: {err}");
    }

    #[test]
    fn decode_no_pad_with_padded_input_errs() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return b64.decode_no_pad("aGVsbG8=")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().starts_with("base64.decode_no_pad:"));
    }

    #[test]
    fn round_trip_url_safe_no_pad() {
        let lua = engine();
        let s: mlua::String = lua
            .load(
                r#"
                local raw = "\251\255\254"
                local enc = b64.encode_url_safe_no_pad(raw)
                assert(not enc:find("="))
                local dec, err = b64.decode_url_safe_no_pad(enc)
                assert(err == nil)
                return dec
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[251u8, 255, 254]);
    }
}

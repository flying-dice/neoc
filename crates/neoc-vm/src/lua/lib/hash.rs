//! Cryptographic and non-cryptographic hashing — `lib:hash`.
//!
//! Streaming `Hasher` UserData backed by `sha2`, `sha3`, `blake3`,
//! `xxhash-rust`, `md-5`, `sha1`. Flat one-shot helper `hash.hash(algo, bytes)`
//! layered on top.
//!
//! Algorithm names match Python's `hashlib` snake_case:
//! `sha256`, `sha384`, `sha512`, `sha3_256`, `sha3_512`, `blake3`, `xxh3`,
//! `xxh64`, `md5`, `sha1`. The latter two are present for interop only.
//!
//! ## Digest semantics
//!
//! `:digest()` is **idempotent** — matches Python `hashlib`. State is cloned
//! before finalize, so repeated calls return the same value and you can keep
//! `:update`-ing after a `:digest()`. Node/Java's consume-on-digest is the
//! footgun branch and we avoid it deliberately.
//!
//! Output is raw bytes per the issue spec. Hex/base64 conversion belongs in
//! `lib:hex` / `lib:base64`.
//!
//! ## xxhash byte order
//!
//! `xxh3` / `xxh64` produce a 64-bit integer; we serialise big-endian so the
//! byte representation matches what Python's `xxhash.xxh*.digest()` returns
//! and what hex dumps display.

use blake3::Hasher as Blake3Hasher;
use md5::{Digest as Md5DigestTrait, Md5};
use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use sha1::Sha1;
use sha2::{Digest as Sha2DigestTrait, Sha256, Sha384, Sha512};
use sha3::{Sha3_256, Sha3_512};
use tracing::instrument;
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

/// Canonical algorithm list. Order: cryptographic
/// first, non-cryptographic next, deprecated last. Used by `algorithms()` and
/// by tests that iterate every backend.
const ALGOS: &[&str] = &[
    "sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5", "sha1",
];

// ---------------------------------------------------------------------------
// State enum — one variant per backend. Picked over `Box<dyn DynDigest>`
// because `xxhash-rust` does not impl the `digest` traits at all and
// `blake3` only impls them behind a non-default feature. An explicit enum
// keeps the dispatch local and readable.
// ---------------------------------------------------------------------------

// Variants differ in size by ~2 KiB (blake3's SIMD state, xxh3's striped
// buffer). The enum lives inside `Hasher` UserData which is already heap-
// allocated by mlua, so per-variant boxing would only trade one indirection
// for two without changing the working set.
#[allow(clippy::large_enum_variant)]
enum State {
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Sha3_256(Sha3_256),
    Sha3_512(Sha3_512),
    Blake3(Blake3Hasher),
    Xxh3(Xxh3),
    Xxh64(Xxh64),
    Md5(Md5),
    Sha1(Sha1),
}

impl State {
    fn new(algo: &str) -> Result<Self, String> {
        match algo {
            "sha256" => Ok(Self::Sha256(Sha256::new())),
            "sha384" => Ok(Self::Sha384(Sha384::new())),
            "sha512" => Ok(Self::Sha512(Sha512::new())),
            "sha3_256" => Ok(Self::Sha3_256(Sha3_256::new())),
            "sha3_512" => Ok(Self::Sha3_512(Sha3_512::new())),
            "blake3" => Ok(Self::Blake3(Blake3Hasher::new())),
            "xxh3" => Ok(Self::Xxh3(Xxh3::new())),
            "xxh64" => Ok(Self::Xxh64(Xxh64::new(0))),
            "md5" => Ok(Self::Md5(Md5::new())),
            "sha1" => Ok(Self::Sha1(Sha1::new())),
            _ => Err(format!("unknown algorithm '{algo}'")),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(h) => Sha2DigestTrait::update(h, data),
            Self::Sha384(h) => Sha2DigestTrait::update(h, data),
            Self::Sha512(h) => Sha2DigestTrait::update(h, data),
            Self::Sha3_256(h) => sha3::Digest::update(h, data),
            Self::Sha3_512(h) => sha3::Digest::update(h, data),
            Self::Blake3(h) => {
                h.update(data);
            }
            Self::Xxh3(h) => h.update(data),
            Self::Xxh64(h) => h.update(data),
            Self::Md5(h) => Md5DigestTrait::update(h, data),
            Self::Sha1(h) => sha1::Digest::update(h, data),
        }
    }

    /// Idempotent digest — clones the backend state before finalising so the
    /// hasher remains usable. Matches Python `hashlib` semantics.
    fn digest(&self) -> Vec<u8> {
        match self {
            Self::Sha256(h) => h.clone().finalize().to_vec(),
            Self::Sha384(h) => h.clone().finalize().to_vec(),
            Self::Sha512(h) => h.clone().finalize().to_vec(),
            Self::Sha3_256(h) => h.clone().finalize().to_vec(),
            Self::Sha3_512(h) => h.clone().finalize().to_vec(),
            Self::Blake3(h) => h.finalize().as_bytes().to_vec(),
            Self::Xxh3(h) => h.digest().to_be_bytes().to_vec(),
            Self::Xxh64(h) => h.digest().to_be_bytes().to_vec(),
            Self::Md5(h) => h.clone().finalize().to_vec(),
            Self::Sha1(h) => h.clone().finalize().to_vec(),
        }
    }

    fn algo(&self) -> &'static str {
        match self {
            Self::Sha256(_) => "sha256",
            Self::Sha384(_) => "sha384",
            Self::Sha512(_) => "sha512",
            Self::Sha3_256(_) => "sha3_256",
            Self::Sha3_512(_) => "sha3_512",
            Self::Blake3(_) => "blake3",
            Self::Xxh3(_) => "xxh3",
            Self::Xxh64(_) => "xxh64",
            Self::Md5(_) => "md5",
            Self::Sha1(_) => "sha1",
        }
    }
}

// ---------------------------------------------------------------------------
// Hasher UserData
// ---------------------------------------------------------------------------

pub struct Hasher {
    state: State,
}

impl UserData for Hasher {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("update", |_, this, data: mlua::LuaString| {
            this.state.update(&data.as_bytes());
            Ok(())
        });

        methods.add_method("digest", |lua, this, _: ()| {
            let bytes = this.state.digest();
            lua.create_string(&bytes)
        });

        methods.add_method("algorithm", |_, this, _: ()| Ok(this.state.algo()));

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("Hasher({})", this.state.algo()))
        });
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:hash` surface through the facade.
///
/// `hash` is a *mixed* raise-plus-channel one-shot: an unknown-algorithm
/// failure goes to the `(value, error)` channel via
/// [`crate::lua::sandbox::err`], but a `lua.create_string` failure on the
/// digest bytes raises an `mlua::Error` directly (the bare `?`) — a mix
/// `try_func` cannot model — so it is set verbatim with
/// [`crate::lua::facade::Module::set`] (keeping its `hash.hash` span and channel
/// prefix) and recorded for the header with
/// [`crate::lua::facade::Module::record_fn`]. The `Hasher` constructor sub-table
/// and the `algorithms` lister are likewise set verbatim with
/// [`crate::lua::facade::Module::set`] because their hand-written registration
/// carried (respectively) a `hash.Hasher.new` span the facade's `type_` would
/// rename to `Hasher.new`, and no span at all — routing them through the facade
/// would change the trace shape. The `Hasher` userdata keeps its own
/// `impl UserData` and is recorded here for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:hash")?;

    let hasher_sub = lua.create_table()?;
    hasher_sub.set("new", lua.create_function(hasher_new)?)?;
    m.set("Hasher", hasher_sub)?;

    m.set("hash", lua.create_function(hash_one_shot)?)?;

    m.set(
        "algorithms",
        lua.create_function(|lua, _: ()| {
            let tbl = lua.create_table()?;
            for (i, a) in ALGOS.iter().enumerate() {
                tbl.set(i + 1, *a)?;
            }
            Ok(tbl)
        })?,
    )?;

    m.record_fn(
        "hash",
        &[("algo", "string"), ("data", "string")],
        "Result<string, string>",
        "One-shot hash of `data` under `algo`, returning the digest as raw bytes.",
    )
    .record_fn(
        "algorithms",
        &[],
        "LuaValue",
        "The list of supported algorithm names.",
    );

    m.userdata("Hasher", |h| {
        h.method(
            "update",
            &[("data", "string")],
            "()",
            "Feed `data` into the running digest.",
        )
        .method(
            "digest",
            &[],
            "string",
            "Return the current digest as raw bytes; idempotent and does not consume the hasher.",
        )
        .method(
            "algorithm",
            &[],
            "string",
            "Return the algorithm name this hasher was constructed with.",
        );
    });

    Ok(m)
}

#[instrument(name = "hash.Hasher.new", target = "lua::lib::hash", skip_all)]
fn hasher_new(lua: &Lua, algo: String) -> mlua::Result<(Value, Value)> {
    match State::new(&algo) {
        Ok(state) => ok(lua, Hasher { state }),
        Err(msg) => err(lua, format!("hash.Hasher.new: {msg}")),
    }
}

#[instrument(name = "hash.hash", target = "lua::lib::hash", skip_all)]
fn hash_one_shot(
    lua: &Lua,
    (algo, data): (String, mlua::LuaString),
) -> mlua::Result<(Value, Value)> {
    match State::new(&algo) {
        Ok(mut state) => {
            state.update(&data.as_bytes());
            ok(lua, lua.create_string(state.digest())?)
        }
        Err(msg) => err(lua, format!("hash.hash: {msg}")),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("hash");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    // library tier — pure backend, no mlua.
    for algo in ALGOS {
        let name = format!("library/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let mut s = State::new(algo).unwrap();
                s.update(black_box(payload.as_bytes()));
                let _ = s.digest();
            });
        });
    }

    // into_out_lua tier — identity marshalling, no hashing.
    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::LuaString| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::LuaString = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // through_lua tier — end-to-end via require("lib:hash").
    for algo in ALGOS {
        let chunk: mlua::Function = lua
            .load(format!(
                r#"return function(p) return (require("lib:hash")).hash("{algo}", p) end"#
            ))
            .eval()
            .expect("chunk");
        let name = format!("through_lua/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let _: mlua::LuaString = chunk.call(black_box(payload_lua.clone())).unwrap();
            });
        });
    }

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:hash")"#).eval().unwrap();
        lua.globals().set("hash", m).unwrap();
        lua
    }

    fn bytes_to_hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for byte in b {
            s.push_str(&format!("{byte:02x}"));
        }
        s
    }

    /// One known vector per algorithm — guards against silently picking up a
    /// different crate version that changes default seeds/parameters.
    #[test]
    fn one_shot_known_vectors() {
        let lua = engine();
        let cases: &[(&str, &str, &str)] = &[
            // SHA-2 / NIST test vectors for "abc".
            (
                "sha256",
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "sha384",
                "abc",
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
            ),
            (
                "sha512",
                "abc",
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            ),
            // SHA-3 / NIST test vectors for "abc".
            (
                "sha3_256",
                "abc",
                "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532",
            ),
            (
                "sha3_512",
                "abc",
                "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0",
            ),
            // BLAKE3 / reference vector for "abc" (first 32 bytes of XOF).
            (
                "blake3",
                "abc",
                "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
            ),
            // MD5 / RFC 1321 test value.
            ("md5", "abc", "900150983cd24fb0d6963f7d28e17f72"),
            // SHA-1 / FIPS 180-1 test value.
            ("sha1", "abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
        ];
        for (algo, input, expected_hex) in cases {
            let s: mlua::LuaString = lua
                .load(format!(r#"return hash.hash("{algo}", "{input}")"#))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert_eq!(
                bytes_to_hex(&s.as_bytes()),
                *expected_hex,
                "{algo} mismatch"
            );
        }
    }

    #[test]
    fn xxh3_known_vector() {
        // xxhash spec test vector: empty input → 0x2d06800538d394c2 (xxh3_64).
        let lua = engine();
        let s: mlua::LuaString = lua.load(r#"return hash.hash("xxh3", "")"#).eval().unwrap();
        assert_eq!(bytes_to_hex(&s.as_bytes()), "2d06800538d394c2");
    }

    #[test]
    fn xxh64_known_vector() {
        // xxhash spec test vector: empty input, seed 0 → 0xef46db3751d8e999.
        let lua = engine();
        let s: mlua::LuaString = lua.load(r#"return hash.hash("xxh64", "")"#).eval().unwrap();
        assert_eq!(bytes_to_hex(&s.as_bytes()), "ef46db3751d8e999");
    }

    #[test]
    fn streaming_matches_one_shot() {
        let lua = engine();
        for algo in [
            "sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5",
            "sha1",
        ] {
            let (one, streamed): (mlua::LuaString, mlua::LuaString) = lua
                .load(format!(
                    r#"
                    local one = hash.hash("{algo}", "hello world")
                    local h = hash.Hasher.new("{algo}")
                    h:update("hello ")
                    h:update("world")
                    return one, h:digest()
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert_eq!(
                bytes_to_hex(&one.as_bytes()),
                bytes_to_hex(&streamed.as_bytes()),
                "{algo}: streaming digest must match one-shot"
            );
        }
    }

    #[test]
    fn empty_input_produces_digest() {
        // Every algorithm must produce a digest for the empty string, not
        // error — content addressing needs `hash(empty)` to be a stable id.
        let lua = engine();
        for algo in [
            "sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5",
            "sha1",
        ] {
            let s: mlua::LuaString = lua
                .load(format!(r#"return hash.hash("{algo}", "")"#))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(
                !s.as_bytes().is_empty(),
                "{algo}: empty-input digest must be non-empty"
            );
        }
    }

    #[test]
    fn digest_is_idempotent() {
        let lua = engine();
        let (a, b): (mlua::LuaString, mlua::LuaString) = lua
            .load(
                r#"
                local h = hash.Hasher.new("sha256")
                h:update("hello")
                local first = h:digest()
                local second = h:digest()
                return first, second
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(bytes_to_hex(&a.as_bytes()), bytes_to_hex(&b.as_bytes()));
    }

    #[test]
    fn update_after_digest_extends_state() {
        // Python `hashlib` allows continuing to update after digest. We do too.
        let lua = engine();
        let (after_first, expected): (mlua::LuaString, mlua::LuaString) = lua
            .load(
                r#"
                local h = hash.Hasher.new("sha256")
                h:update("hello ")
                local _ = h:digest()        -- shouldn't freeze state
                h:update("world")
                return h:digest(), hash.hash("sha256", "hello world")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            bytes_to_hex(&after_first.as_bytes()),
            bytes_to_hex(&expected.as_bytes())
        );
    }

    #[test]
    fn binary_input_round_trip() {
        let lua = engine();
        let s: mlua::LuaString = lua
            .load(
                r#"
                local raw = "\0\1\2\254\255"
                return hash.hash("sha256", raw)
                "#,
            )
            .eval()
            .unwrap();
        // SHA-256 of bytes [0x00 0x01 0x02 0xfe 0xff] (verified vs sha256sum).
        assert_eq!(
            bytes_to_hex(&s.as_bytes()),
            "103597c5abb6113da596c18e9d1da69364eafe00a2bfaa8b12e53c44bd6b0429"
        );
    }

    #[test]
    fn unknown_algorithm_one_shot_errors() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) =
            lua.load(r#"return hash.hash("md4", "x")"#).eval().unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("hash.hash: "), "bad prefix: {e}");
        assert!(e.contains("unknown algorithm 'md4'"), "bad body: {e}");
    }

    #[test]
    fn unknown_algorithm_hasher_new_errors() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) =
            lua.load(r#"return hash.Hasher.new("md4")"#).eval().unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("hash.Hasher.new: "), "bad prefix: {e}");
    }

    #[test]
    fn algorithms_list_is_complete() {
        let lua = engine();
        let algos: Vec<String> = lua.load(r#"return hash.algorithms()"#).eval().unwrap();
        assert_eq!(
            algos,
            super::ALGOS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn tostring_includes_algorithm() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local h = hash.Hasher.new("blake3")
                return tostring(h)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "Hasher(blake3)");
    }

    #[test]
    fn algorithm_method_returns_name() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local h = hash.Hasher.new("sha3_512")
                return h:algorithm()
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "sha3_512");
    }

    #[test]
    fn large_input_streaming() {
        // 1 MiB through update in 4 KiB chunks must equal one-shot of the same.
        let lua = engine();
        let (streamed, oneshot): (mlua::LuaString, mlua::LuaString) = lua
            .load(
                r#"
                local chunk = string.rep("x", 4096)
                local h = hash.Hasher.new("sha256")
                for _ = 1, 256 do h:update(chunk) end
                local all = string.rep("x", 4096 * 256)
                return h:digest(), hash.hash("sha256", all)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            bytes_to_hex(&streamed.as_bytes()),
            bytes_to_hex(&oneshot.as_bytes())
        );
    }
}

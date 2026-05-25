//! Keyed-hash message authentication codes — `lib:hmac`.
//!
//! Streaming `Hmac` UserData backed by the RustCrypto `hmac` crate over the
//! SHA-2 digests from `lib:hash`'s `sha2` backend. Flat one-shot helper
//! `hmac.hmac(algo, key, bytes)` layered on top, plus a constant-time
//! `hmac.verify(a, b)` tag comparison.
//!
//! Algorithm names match `lib:hash`'s snake_case SHA-2 family: `sha256`,
//! `sha384`, `sha512`. Non-SHA2 underlying digests (SHA-1, MD5) are out of
//! scope per the issue — HMAC's security reduction assumes a modern PRF.
//!
//! ## Digest semantics
//!
//! `:digest()` is **idempotent** — it clones the MAC state before finalising,
//! so repeated calls return the same tag and `:update` still works afterwards.
//! Mirrors `lib:hash`; avoids the consume-on-finalize footgun.
//!
//! ## Keys
//!
//! HMAC accepts a key of any length — the construction hashes keys longer than
//! the block size and zero-pads shorter ones — so `new` never rejects a key.
//! Output is raw tag bytes per the issue spec; hex/base64 via `lib:hex` /
//! `lib:base64`.
//!
//! ## Constant-time verification
//!
//! `verify` compares two tags via `subtle::ConstantTimeEq` — the comparison
//! does not short-circuit on the first differing byte, closing the timing
//! side-channel that a plain `==` opens. The security-critical reason this
//! module exists; never compare tags with `==`. Length is not secret (it is
//! fixed by the algorithm), so a length mismatch returns `false` directly.
//!
//! Tracing spans `skip_all` deliberately: the key and message must never reach
//! the log.

use hmac::Mac;
use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use sha2::{Sha256, Sha384, Sha512};
use subtle::ConstantTimeEq;
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

// `hmac::Hmac` is fully qualified so the Lua-facing UserData below can take the
// unadorned `Hmac` name without colliding with the crate's generic type.
type HmacSha256 = hmac::Hmac<Sha256>;
type HmacSha384 = hmac::Hmac<Sha384>;
type HmacSha512 = hmac::Hmac<Sha512>;

/// Canonical algorithm list — the d.luau / docs order. Used by `algorithms()`
/// and by tests that iterate every backend.
const ALGOS: &[&str] = &["sha256", "sha384", "sha512"];

// ---------------------------------------------------------------------------
// State enum — one variant per SHA-2 backend. An explicit enum keeps dispatch
// local and readable, mirroring `lib:hash`. All three variants share the same
// `Mac` trait, so `update`/`finalize` need no per-variant trait disambiguation.
// ---------------------------------------------------------------------------

enum State {
    Sha256(HmacSha256),
    Sha384(HmacSha384),
    Sha512(HmacSha512),
}

impl State {
    // `new_from_slice` is fallible in the `Mac` trait for fixed-key MACs; for
    // HMAC it accepts any key length and never errors, so the `expect` is
    // unreachable. Message matches the RustCrypto docs' own example.
    fn new(algo: &str, key: &[u8]) -> Result<Self, String> {
        match algo {
            "sha256" => Ok(Self::Sha256(
                HmacSha256::new_from_slice(key).expect("HMAC accepts a key of any length"),
            )),
            "sha384" => Ok(Self::Sha384(
                HmacSha384::new_from_slice(key).expect("HMAC accepts a key of any length"),
            )),
            "sha512" => Ok(Self::Sha512(
                HmacSha512::new_from_slice(key).expect("HMAC accepts a key of any length"),
            )),
            _ => Err(format!("unknown algorithm '{algo}'")),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(m) => m.update(data),
            Self::Sha384(m) => m.update(data),
            Self::Sha512(m) => m.update(data),
        }
    }

    /// Idempotent — clones the MAC state before finalising so the instance
    /// stays usable. Matches `lib:hash`'s `:digest()` contract.
    fn digest(&self) -> Vec<u8> {
        // Every backend shares the same `Mac` finalize path; the match only
        // recovers the concrete type the enum erased. Unlike `lib:hash`, no
        // variant needs bespoke serialisation, so the body stays generic.
        fn finalize_to_vec<M: Mac + Clone>(mac: &M) -> Vec<u8> {
            mac.clone().finalize().into_bytes().to_vec()
        }
        match self {
            Self::Sha256(m) => finalize_to_vec(m),
            Self::Sha384(m) => finalize_to_vec(m),
            Self::Sha512(m) => finalize_to_vec(m),
        }
    }

    fn algo(&self) -> &'static str {
        match self {
            Self::Sha256(_) => "sha256",
            Self::Sha384(_) => "sha384",
            Self::Sha512(_) => "sha512",
        }
    }
}

/// Constant-time tag comparison. `subtle` compares every byte regardless of
/// where the first mismatch is; a length mismatch (not secret — fixed by the
/// algorithm) yields `false` without inspecting contents.
fn verify(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

// ---------------------------------------------------------------------------
// Hmac UserData
// ---------------------------------------------------------------------------

pub struct Hmac {
    state: State,
}

impl UserData for Hmac {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("update", |_, this, data: mlua::String| {
            this.state.update(&data.as_bytes());
            Ok(())
        });

        methods.add_method("digest", |lua, this, _: ()| {
            let bytes = this.state.digest();
            lua.create_string(&bytes)
        });

        methods.add_method("algorithm", |_, this, _: ()| Ok(this.state.algo()));

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("Hmac({})", this.state.algo()))
        });
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:hmac` surface through the facade.
///
/// Every free function is set verbatim with [`crate::lua::facade::Module::set`]
/// and recorded for the header with [`crate::lua::facade::Module::record_fn`]:
/// `hmac.Hmac.new` and `hmac.hmac` raise through [`crate::lua::sandbox::err`]
/// with a baked `"<stem>.<fn>: <e>"` prefix under their own `#[instrument]`
/// spans (the `hmac.Hmac.new` span the facade's `type_` would rename to
/// `Hmac.new`), and `verify` / `algorithms` carry no span at all — routing any
/// of them through the facade would change the trace shape. The `Hmac` userdata
/// keeps its own `impl UserData` and is recorded here for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:hmac")?;

    let hmac_sub = lua.create_table()?;
    hmac_sub.set("new", lua.create_function(hmac_new)?)?;
    m.set("Hmac", hmac_sub)?;

    m.set("hmac", lua.create_function(hmac_one_shot)?)?;

    m.set(
        "verify",
        lua.create_function(|_, (a, b): (mlua::String, mlua::String)| {
            Ok(verify(&a.as_bytes(), &b.as_bytes()))
        })?,
    )?;

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

    m.userdata("Hmac", |h| {
        h.method(
            "update",
            &[("data", "string")],
            "()",
            "Feed `data` into the running MAC.",
        )
        .method(
            "digest",
            &[],
            "string",
            "Return the current tag as raw bytes; idempotent and does not consume the MAC.",
        )
        .method(
            "algorithm",
            &[],
            "string",
            "Return the algorithm name this MAC was constructed with.",
        );
    });

    m.record_fn(
        "hmac",
        &[("algo", "string"), ("key", "string"), ("data", "string")],
        "Result<string, string>",
        "One-shot tag of `data` under `algo` and `key`, returning the tag as raw bytes.",
    )
    .record_fn(
        "verify",
        &[("a", "string"), ("b", "string")],
        "bool",
        "Compare two tags in constant time; returns `true` iff they are equal.",
    )
    .record_fn(
        "algorithms",
        &[],
        "LuaValue",
        "Return the list of supported algorithm names in canonical order.",
    );

    Ok(m)
}

#[instrument(name = "hmac.Hmac.new", target = "luau::lib::hmac", skip_all)]
fn hmac_new(lua: &Lua, (algo, key): (String, mlua::String)) -> mlua::Result<(Value, Value)> {
    match State::new(&algo, &key.as_bytes()) {
        Ok(state) => ok(lua, Hmac { state }),
        Err(msg) => err(lua, format!("hmac.Hmac.new: {msg}")),
    }
}

#[instrument(name = "hmac.hmac", target = "luau::lib::hmac", skip_all)]
fn hmac_one_shot(
    lua: &Lua,
    (algo, key, data): (String, mlua::String, mlua::String),
) -> mlua::Result<(Value, Value)> {
    match State::new(&algo, &key.as_bytes()) {
        Ok(mut state) => {
            state.update(&data.as_bytes());
            ok(lua, lua.create_string(state.digest())?)
        }
        Err(msg) => err(lua, format!("hmac.hmac: {msg}")),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let key = b"benchmark-hmac-key-0123456789abc";
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let key_lua = lua.create_string(key).expect("key");
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("hmac");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    // library tier — pure backend, no mlua.
    for algo in ALGOS {
        let name = format!("library/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let mut s = State::new(algo, black_box(key)).unwrap();
                s.update(black_box(payload.as_bytes()));
                let _ = s.digest();
            });
        });
    }

    // into_out_lua tier — identity marshalling, no MAC.
    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // through_lua tier — end-to-end via require("lib:hmac").
    for algo in ALGOS {
        let chunk: mlua::Function = lua
            .load(format!(
                r#"return function(k, p) return (require("lib:hmac")).hmac("{algo}", k, p) end"#
            ))
            .eval()
            .expect("chunk");
        let name = format!("through_lua/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let _: mlua::String = chunk
                    .call((black_box(key_lua.clone()), black_box(payload_lua.clone())))
                    .unwrap();
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
        let m: mlua::Table = lua.load(r#"return require("lib:hmac")"#).eval().unwrap();
        lua.globals().set("hmac", m).unwrap();
        lua
    }

    fn bytes_to_hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for byte in b {
            s.push_str(&format!("{byte:02x}"));
        }
        s
    }

    /// RFC 4231 test vectors — Test Case 2 (key "Jefe", ASCII message), one
    /// per supported digest. Guards against a crate bump silently changing the
    /// construction. Vectors are the canonical published values.
    #[test]
    fn one_shot_rfc4231_vectors() {
        let lua = engine();
        let cases: &[(&str, &str)] = &[
            (
                "sha256",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                "sha384",
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
            ),
            (
                "sha512",
                "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
            ),
        ];
        for (algo, expected_hex) in cases {
            let s: mlua::String = lua
                .load(format!(
                    r#"return hmac.hmac("{algo}", "Jefe", "what do ya want for nothing?")"#
                ))
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
    fn streaming_matches_one_shot() {
        let lua = engine();
        for algo in super::ALGOS {
            let (one, streamed): (mlua::String, mlua::String) = lua
                .load(format!(
                    r#"
                    local one = hmac.hmac("{algo}", "secret", "hello world")
                    local m = hmac.Hmac.new("{algo}", "secret")
                    m:update("hello ")
                    m:update("world")
                    return one, m:digest()
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert_eq!(
                bytes_to_hex(&one.as_bytes()),
                bytes_to_hex(&streamed.as_bytes()),
                "{algo}: streaming tag must match one-shot"
            );
        }
    }

    #[test]
    fn digest_is_idempotent() {
        let lua = engine();
        let (a, b): (mlua::String, mlua::String) = lua
            .load(
                r#"
                local m = hmac.Hmac.new("sha256", "k")
                m:update("hello")
                return m:digest(), m:digest()
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(bytes_to_hex(&a.as_bytes()), bytes_to_hex(&b.as_bytes()));
    }

    #[test]
    fn update_after_digest_extends_state() {
        let lua = engine();
        let (after, expected): (mlua::String, mlua::String) = lua
            .load(
                r#"
                local m = hmac.Hmac.new("sha256", "k")
                m:update("hello ")
                local _ = m:digest()        -- must not freeze state
                m:update("world")
                return m:digest(), hmac.hmac("sha256", "k", "hello world")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            bytes_to_hex(&after.as_bytes()),
            bytes_to_hex(&expected.as_bytes())
        );
    }

    #[test]
    fn key_changes_tag() {
        // Different keys over identical data must produce different tags —
        // the whole point of a keyed MAC.
        let lua = engine();
        let (a, b): (mlua::String, mlua::String) = lua
            .load(
                r#"
                return hmac.hmac("sha256", "key-a", "msg"),
                       hmac.hmac("sha256", "key-b", "msg")
                "#,
            )
            .eval()
            .unwrap();
        assert_ne!(bytes_to_hex(&a.as_bytes()), bytes_to_hex(&b.as_bytes()));
    }

    #[test]
    fn empty_key_and_message_produce_tag() {
        // HMAC is defined for an empty key and empty message; both must yield a
        // full-width tag, not an error.
        let lua = engine();
        let s: mlua::String = lua
            .load(r#"return hmac.hmac("sha256", "", "")"#)
            .eval()
            .unwrap();
        assert_eq!(s.as_bytes().len(), 32, "sha256 tag must be 32 bytes");
    }

    #[test]
    fn binary_key_and_data_round_trip() {
        let lua = engine();
        let (streamed, oneshot): (mlua::String, mlua::String) = lua
            .load(
                r#"
                local key = "\0\1\2\254\255"
                local data = "\255\0\128"
                local m = hmac.Hmac.new("sha512", key)
                m:update(data)
                return m:digest(), hmac.hmac("sha512", key, data)
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            bytes_to_hex(&streamed.as_bytes()),
            bytes_to_hex(&oneshot.as_bytes())
        );
    }

    #[test]
    fn verify_accepts_matching_tags() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local a = hmac.hmac("sha256", "k", "msg")
                local b = hmac.hmac("sha256", "k", "msg")
                return hmac.verify(a, b)
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn verify_rejects_differing_tags() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local a = hmac.hmac("sha256", "k", "msg")
                local b = hmac.hmac("sha256", "k", "tampered")
                return hmac.verify(a, b)
                "#,
            )
            .eval()
            .unwrap();
        assert!(!ok);
    }

    #[test]
    fn verify_rejects_length_mismatch() {
        // A truncated tag must never verify against a full-width one.
        let lua = engine();
        let ok: bool = lua
            .load(r#"return hmac.verify("abc", "abcd")"#)
            .eval()
            .unwrap();
        assert!(!ok);
    }

    #[test]
    fn verify_empty_tags_match() {
        let lua = engine();
        let ok: bool = lua.load(r#"return hmac.verify("", "")"#).eval().unwrap();
        assert!(ok);
    }

    #[test]
    fn unknown_algorithm_one_shot_errors() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return hmac.hmac("sha1", "k", "x")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("hmac.hmac: "), "bad prefix: {e}");
        assert!(e.contains("unknown algorithm 'sha1'"), "bad body: {e}");
    }

    #[test]
    fn unknown_algorithm_new_errors() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return hmac.Hmac.new("md5", "k")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("hmac.Hmac.new: "), "bad prefix: {e}");
    }

    #[test]
    fn algorithms_list_is_complete() {
        let lua = engine();
        let algos: Vec<String> = lua.load(r#"return hmac.algorithms()"#).eval().unwrap();
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
            .load(r#"return tostring(hmac.Hmac.new("sha512", "k"))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "Hmac(sha512)");
    }

    #[test]
    fn algorithm_method_returns_name() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (hmac.Hmac.new("sha384", "k")):algorithm()"#)
            .eval()
            .unwrap();
        assert_eq!(s, "sha384");
    }

    #[test]
    fn large_input_streaming() {
        // 1 MiB through update in 4 KiB chunks must equal the one-shot tag.
        let lua = engine();
        let (streamed, oneshot): (mlua::String, mlua::String) = lua
            .load(
                r#"
                local chunk = string.rep("x", 4096)
                local m = hmac.Hmac.new("sha256", "k")
                for _ = 1, 256 do m:update(chunk) end
                local all = string.rep("x", 4096 * 256)
                return m:digest(), hmac.hmac("sha256", "k", all)
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

//! Authenticated symmetric encryption — `lib:crypto`.
//!
//! Two free functions, `crypto.encrypt` and `crypto.decrypt`, backed by the
//! RustCrypto `aes-gcm` and `chacha20poly1305` crates. AEAD-only by design:
//! every supported algorithm produces a ciphertext that is also a MAC, and the
//! `decrypt` path returns an error rather than plaintext if the tag does not
//! verify. There is no surface for raw CBC/CTR — the "no footguns" rule in the
//! module's issue (#34) excludes unauthenticated modes from this module
//! entirely, not "ships disabled".
//!
//! Algorithm names are snake_case, matching `lib:hash` / `lib:hmac`:
//! `aes_128_gcm`, `aes_256_gcm`, `chacha20_poly1305`.
//!
//! ## Surface shape
//!
//! ```text
//! encrypt(algo, key, nonce, plaintext, aad?) -> (ciphertext, nil) | (nil, err)
//! decrypt(algo, key, nonce, ciphertext, aad?) -> (plaintext, nil) | (nil, err)
//! ```
//!
//! `algo` is first to match `hash.hash` / `hmac.hmac`. The remaining order
//! matches the Node / Go / Java / .NET / Rust consensus (`key, nonce, msg,
//! aad`). Output is raw bytes (the encrypted message *concatenated with the
//! 16-byte auth tag*), per the AEAD libraries we delegate to; hex / base64
//! conversion belongs in `lib:hex` / `lib:base64`.
//!
//! ## Nonce is explicit, never derived
//!
//! The caller passes the nonce as an argument. The module deliberately does
//! not generate it — GCM and ChaCha20-Poly1305 are catastrophic under nonce
//! reuse, and the only way to be sure scripts handle that is to make them
//! think about it. Use `(require("lib:random")).random_bytes(crypto.nonce_size(algo))`
//! for a fresh random nonce.
//!
//! ## Constant-time tag verification
//!
//! Both backends verify the Poly1305 / GCM tag in constant time inside the
//! crate; we surface only a generic `"decryption failed"` on tag mismatch so
//! the error string carries no oracle-shaped detail. Tracing spans `skip_all`
//! deliberately — keys, nonces, plaintexts must never reach the log.

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{
    Aes128Gcm, Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use chacha20poly1305::ChaCha20Poly1305;
use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

/// Canonical algorithm list — the d.luau / docs order. Used by `algorithms()`,
/// `key_size` / `nonce_size` / `tag_size`, and by tests that iterate every
/// backend.
const ALGOS: &[&str] = &["aes_128_gcm", "aes_256_gcm", "chacha20_poly1305"];

// All three AEADs in this module use a 96-bit nonce and a 128-bit tag — the
// values are listed by RFC 5288 (GCM) and RFC 8439 (ChaCha20-Poly1305). Encoded
// as constants so `key_size` / `nonce_size` / `tag_size` and the validation
// guards stay in one place.
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

fn key_len(algo: &str) -> Result<usize, String> {
    match algo {
        "aes_128_gcm" => Ok(16),
        "aes_256_gcm" => Ok(32),
        "chacha20_poly1305" => Ok(32),
        _ => Err(format!("unknown algorithm '{algo}'")),
    }
}

// ---------------------------------------------------------------------------
// Cipher enum — one variant per backend. The `aead::Aead` trait is the common
// shape: identical `encrypt` / `decrypt` signatures across all three, so the
// per-call dispatch stays a one-line match. The keyed cipher is built once per
// call; benches will tell us whether a `Cipher.new` UserData is worth adding,
// but the issue scope is the flat function surface.
// ---------------------------------------------------------------------------

// AES-256 (~1 KiB key schedule) vs AES-128 (~736 B) leave the enum size
// dominated by one variant. Boxing would trade one stack write per call for an
// allocation per call — for a per-call ephemeral type that lives only for the
// length of one `encrypt`/`decrypt`, the stack copy is the right side of the
// tradeoff. Matches `lib:hash`'s identical reasoning.
#[allow(clippy::large_enum_variant)]
enum Cipher {
    Aes128Gcm(Aes128Gcm),
    Aes256Gcm(Aes256Gcm),
    ChaCha20Poly1305(ChaCha20Poly1305),
}

impl Cipher {
    fn new(algo: &str, key: &[u8]) -> Result<Self, String> {
        let expected = key_len(algo)?;
        if key.len() != expected {
            return Err(format!(
                "expected {expected}-byte key for {algo}, got {}",
                key.len()
            ));
        }
        match algo {
            "aes_128_gcm" => Ok(Self::Aes128Gcm(Aes128Gcm::new(GenericArray::from_slice(
                key,
            )))),
            "aes_256_gcm" => Ok(Self::Aes256Gcm(Aes256Gcm::new(GenericArray::from_slice(
                key,
            )))),
            "chacha20_poly1305" => Ok(Self::ChaCha20Poly1305(ChaCha20Poly1305::new(
                GenericArray::from_slice(key),
            ))),
            // unreachable — `key_len` above would have errored already.
            _ => Err(format!("unknown algorithm '{algo}'")),
        }
    }

    fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        if nonce.len() != NONCE_LEN {
            return Err(format!(
                "expected {NONCE_LEN}-byte nonce, got {}",
                nonce.len()
            ));
        }
        let nonce = GenericArray::from_slice(nonce);
        let payload = Payload {
            msg: plaintext,
            aad,
        };
        // The Aead impl can fail only on internal buffer-allocation paths
        // (16 + plaintext.len() > usize::MAX). In practice unreachable from
        // the runtime, but we surface the error rather than panic.
        let out = match self {
            Self::Aes128Gcm(c) => c.encrypt(nonce, payload),
            Self::Aes256Gcm(c) => c.encrypt(nonce, payload),
            Self::ChaCha20Poly1305(c) => c.encrypt(nonce, payload),
        };
        out.map_err(|e| format!("encryption failed: {e}"))
    }

    fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, String> {
        if nonce.len() != NONCE_LEN {
            return Err(format!(
                "expected {NONCE_LEN}-byte nonce, got {}",
                nonce.len()
            ));
        }
        if ciphertext.len() < TAG_LEN {
            return Err(format!(
                "ciphertext shorter than {TAG_LEN}-byte tag (got {})",
                ciphertext.len()
            ));
        }
        let nonce = GenericArray::from_slice(nonce);
        let payload = Payload {
            msg: ciphertext,
            aad,
        };
        let out = match self {
            Self::Aes128Gcm(c) => c.decrypt(nonce, payload),
            Self::Aes256Gcm(c) => c.decrypt(nonce, payload),
            Self::ChaCha20Poly1305(c) => c.decrypt(nonce, payload),
        };
        // Generic message — leaking "tag mismatch" vs "AAD wrong" vs anything
        // else is the canonical AEAD oracle footgun. The aead crate already
        // collapses those internally; we collapse again to be sure.
        out.map_err(|_| "decryption failed".to_string())
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:crypto` surface through the facade.
///
/// `encrypt` and `decrypt` are a *mixed* raise-plus-channel shape: a validation
/// or auth failure flows to the `(value, error)` channel via
/// [`crate::lua::sandbox::err`], but a `lua.create_string` failure on the output
/// bytes raises an `mlua::Error` directly (the bare `?`) — a mix `try_func`
/// cannot model — so each is set verbatim with
/// [`crate::lua::facade::Module::set`] (keeping its `crypto.<fn>` span and
/// channel prefix) and recorded for the header with
/// [`crate::lua::facade::Module::record_fn`], exactly as `lib:base64`'s
/// `decode*` functions are. `algorithms` builds a Lua table — there is no
/// `NeocType` for `Vec`, and it carried no span in the hand-written form, so
/// routing it through `func` would add one — and is likewise set verbatim and
/// recorded as `LuaValue`, mirroring `lib:hmac`'s `algorithms`.
///
/// The size queries (`key_size` / `nonce_size` / `tag_size`) are pure
/// `(value, error)` with no raise, so they go through
/// [`crate::lua::facade::Module::try_func`]: the facade supplies the
/// `crypto.<fn>` span and the `"crypto.<fn>: "` error prefix the hand-written
/// form baked manually, leaving nothing to record separately.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:crypto")?;

    m.set("encrypt", lua.create_function(encrypt)?)?
        .record_fn(
            "encrypt",
            &[
                ("algo", "string"),
                ("key", "string"),
                ("nonce", "string"),
                ("plaintext", "string"),
                ("aad", "string?"),
            ],
            "Result<string, string>",
            "AEAD-encrypt `plaintext` under `algo`/`key`/`nonce`, binding optional `aad`. Returns ciphertext concatenated with the auth tag as raw bytes.",
        )
        .set("decrypt", lua.create_function(decrypt)?)?
        .record_fn(
            "decrypt",
            &[
                ("algo", "string"),
                ("key", "string"),
                ("nonce", "string"),
                ("ciphertext", "string"),
                ("aad", "string?"),
            ],
            "Result<string, string>",
            "AEAD-decrypt `ciphertext` (with the trailing tag) under `algo`/`key`/`nonce`, requiring `aad` to match the value bound at encrypt time.",
        );

    m.set(
        "algorithms",
        lua.create_function(|lua, _: ()| {
            let tbl = lua.create_table()?;
            for (i, a) in ALGOS.iter().enumerate() {
                tbl.set(i + 1, *a)?;
            }
            Ok(tbl)
        })?,
    )?
    .record_fn(
        "algorithms",
        &[],
        "LuaValue",
        "Return the list of supported algorithm names in canonical order.",
    );

    // Size queries validate the algorithm name through `key_len`'s table so an
    // unknown name reports the same `"unknown algorithm '<x>'"` text every
    // sister function uses. All three supported AEADs ship a 96-bit nonce and a
    // 128-bit tag; when (if) a 192-bit XChaCha variant lands, the nonce arm
    // grows a branch here.
    m.try_func(
        "key_size",
        &[("algo", "string")],
        "Return the required key length in bytes for `algo`.",
        |_, algo: String| key_len(&algo).map(|n| n as i64),
    )?
    .try_func(
        "nonce_size",
        &[("algo", "string")],
        "Return the required nonce length in bytes for `algo`.",
        |_, algo: String| key_len(&algo).map(|_| NONCE_LEN as i64),
    )?
    .try_func(
        "tag_size",
        &[("algo", "string")],
        "Return the auth-tag length appended to ciphertext, in bytes, for `algo`.",
        |_, algo: String| key_len(&algo).map(|_| TAG_LEN as i64),
    )?;

    Ok(m)
}

#[instrument(name = "crypto.encrypt", target = "luau::lib::crypto", skip_all)]
fn encrypt(
    lua: &Lua,
    (algo, key, nonce, plaintext, aad): (
        String,
        mlua::String,
        mlua::String,
        mlua::String,
        Option<mlua::String>,
    ),
) -> mlua::Result<(Value, Value)> {
    let cipher = match Cipher::new(&algo, &key.as_bytes()) {
        Ok(c) => c,
        Err(msg) => return err(lua, format!("crypto.encrypt: {msg}")),
    };
    // Bind the optional AAD bytes into a stable slice for the call. An empty
    // slice when omitted matches the AEAD contract: encrypt-with-no-aad must
    // be byte-identical to encrypt-with-empty-aad so the surface has one
    // story for header-binding callers.
    let aad_bytes = aad.as_ref().map(|s| s.as_bytes());
    let aad_slice: &[u8] = aad_bytes.as_deref().unwrap_or(&[]);
    match cipher.encrypt(&nonce.as_bytes(), &plaintext.as_bytes(), aad_slice) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(msg) => err(lua, format!("crypto.encrypt: {msg}")),
    }
}

#[instrument(name = "crypto.decrypt", target = "luau::lib::crypto", skip_all)]
fn decrypt(
    lua: &Lua,
    (algo, key, nonce, ciphertext, aad): (
        String,
        mlua::String,
        mlua::String,
        mlua::String,
        Option<mlua::String>,
    ),
) -> mlua::Result<(Value, Value)> {
    let cipher = match Cipher::new(&algo, &key.as_bytes()) {
        Ok(c) => c,
        Err(msg) => return err(lua, format!("crypto.decrypt: {msg}")),
    };
    let aad_bytes = aad.as_ref().map(|s| s.as_bytes());
    let aad_slice: &[u8] = aad_bytes.as_deref().unwrap_or(&[]);
    match cipher.decrypt(&nonce.as_bytes(), &ciphertext.as_bytes(), aad_slice) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(msg) => err(lua, format!("crypto.decrypt: {msg}")),
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
    let nonce_bytes = [0u8; NONCE_LEN];
    let aad = b"";
    let payload_lua = lua.create_string(&payload).expect("payload");
    let nonce_lua = lua.create_string(nonce_bytes).expect("nonce");

    let mut g = c.benchmark_group("crypto");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    // Per-algo key bytes — the 16 / 32 / 32 split matches the supported set.
    let keys: &[(&str, &[u8])] = &[
        ("aes_128_gcm", b"benchmark-aes-128-gcm-keyok"[..16].as_ref()),
        (
            "aes_256_gcm",
            b"benchmark-aes-256-gcm-key-0123456789abc"[..32].as_ref(),
        ),
        (
            "chacha20_poly1305",
            b"benchmark-chacha20-poly1305-key-aaaaaaa"[..32].as_ref(),
        ),
    ];

    // library tier — pure backend, no mlua.
    for (algo, key) in keys {
        let name = format!("library/encrypt/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let c = Cipher::new(algo, black_box(key)).unwrap();
                let _ = c
                    .encrypt(&nonce_bytes, black_box(payload.as_bytes()), aad)
                    .unwrap();
            });
        });
    }

    // into_out_lua tier — identity marshalling, no encryption.
    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    // through_lua tier — end-to-end via require("lib:crypto").
    for (algo, key) in keys {
        let key_lua = lua.create_string(key).expect("key");
        let chunk: mlua::Function = lua
            .load(format!(
                r#"return function(k, n, p) return (require("lib:crypto")).encrypt("{algo}", k, n, p) end"#
            ))
            .eval()
            .expect("chunk");
        let name = format!("through_lua/encrypt/{algo}");
        g.bench_function(name, |b| {
            b.iter(|| {
                let _: mlua::String = chunk
                    .call((
                        black_box(key_lua.clone()),
                        black_box(nonce_lua.clone()),
                        black_box(payload_lua.clone()),
                    ))
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
        let m: mlua::Table = lua.load(r#"return require("lib:crypto")"#).eval().unwrap();
        lua.globals().set("crypto", m).unwrap();
        lua
    }

    fn from_hex(s: &str) -> Vec<u8> {
        assert!(
            s.len().is_multiple_of(2),
            "hex string must have even length"
        );
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("bad hex digit"))
            .collect()
    }

    fn to_hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for byte in b {
            s.push_str(&format!("{byte:02x}"));
        }
        s
    }

    /// One known answer per algorithm — guards against a crate bump silently
    /// changing the construction. Vectors are the published canonical values.
    #[test]
    fn encrypt_known_vectors() {
        // NIST GCM test vectors (Case 4 from "AES-GCM Test Vectors") for
        // AES-128 and AES-256, and the RFC 8439 §2.8.2 vector for
        // ChaCha20-Poly1305. All three include AAD so the AAD-binding path
        // is exercised end-to-end.
        let cases: &[(&str, &str, &str, &str, &str, &str)] = &[
            (
                "aes_128_gcm",
                "feffe9928665731c6d6a8f9467308308",
                "cafebabefacedbaddecaf888",
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
                "feedfacedeadbeeffeedfacedeadbeefabaddad2",
                "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e0915bc94fbc3221a5db94fae95ae7121a47",
            ),
            (
                "aes_256_gcm",
                "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308",
                "cafebabefacedbaddecaf888",
                "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
                "feedfacedeadbeeffeedfacedeadbeefabaddad2",
                "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f66276fc6ece0f4e1768cddf8853bb2d551b",
            ),
            (
                "chacha20_poly1305",
                "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f",
                "070000004041424344454647",
                "4c616469657320616e642047656e746c656d656e206f662074686520636c617373206f66202739393a204966204920636f756c64206f6666657220796f75206f6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73637265656e20776f756c642062652069742e",
                "50515253c0c1c2c3c4c5c6c7",
                "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691",
            ),
        ];
        let lua = engine();
        for (algo, key_hex, nonce_hex, pt_hex, aad_hex, ct_hex) in cases {
            let key = from_hex(key_hex);
            let nonce = from_hex(nonce_hex);
            let pt = from_hex(pt_hex);
            let aad = from_hex(aad_hex);
            lua.globals()
                .set("key", lua.create_string(&key).unwrap())
                .unwrap();
            lua.globals()
                .set("nonce", lua.create_string(&nonce).unwrap())
                .unwrap();
            lua.globals()
                .set("pt", lua.create_string(&pt).unwrap())
                .unwrap();
            lua.globals()
                .set("aad", lua.create_string(&aad).unwrap())
                .unwrap();
            let s: mlua::String = lua
                .load(format!(
                    r#"return crypto.encrypt("{algo}", key, nonce, pt, aad)"#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert_eq!(to_hex(&s.as_bytes()), *ct_hex, "{algo} mismatch");
        }
    }

    #[test]
    fn round_trip_matches_plaintext() {
        let lua = engine();
        for algo in super::ALGOS {
            let (pt_in, pt_out, e): (mlua::String, Option<mlua::String>, Option<String>) = lua
                .load(format!(
                    r#"
                    local key = ("k"):rep(crypto.key_size("{algo}"))
                    local nonce = ("n"):rep(crypto.nonce_size("{algo}"))
                    local pt = "hello, world"
                    local ct, _ = crypto.encrypt("{algo}", key, nonce, pt)
                    local out, e = crypto.decrypt("{algo}", key, nonce, ct)
                    return pt, out, e
                    "#
                ))
                .eval()
                .unwrap_or_else(|err| panic!("{algo}: {err}"));
            assert!(e.is_none(), "{algo}: decrypt errored: {e:?}");
            let out = pt_out.unwrap_or_else(|| panic!("{algo}: nil plaintext"));
            assert_eq!(pt_in.as_bytes().to_vec(), out.as_bytes().to_vec());
        }
    }

    #[test]
    fn aad_mismatch_fails_decrypt() {
        // Encrypt with one AAD, decrypt with another — must fail. Catches a
        // regression where AAD was dropped on either path.
        let lua = engine();
        let (val, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                local key = ("k"):rep(32)
                local nonce = ("n"):rep(12)
                local ct = crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header-a")
                return crypto.decrypt("aes_256_gcm", key, nonce, ct, "header-b")
                "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(
            e.unwrap().starts_with("crypto.decrypt: decryption failed"),
            "AAD mismatch should fail with the generic decrypt error",
        );
    }

    #[test]
    fn tampered_ciphertext_fails_decrypt() {
        // Flip a byte in the ciphertext — the tag check must reject it.
        let lua = engine();
        let (val, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                local key = ("k"):rep(32)
                local nonce = ("n"):rep(12)
                local ct = crypto.encrypt("chacha20_poly1305", key, nonce, "msg")
                local tampered = string.char(bit32.bxor(string.byte(ct, 1), 1)) .. string.sub(ct, 2)
                return crypto.decrypt("chacha20_poly1305", key, nonce, tampered)
                "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().starts_with("crypto.decrypt: decryption failed"));
    }

    #[test]
    fn wrong_key_fails_decrypt() {
        let lua = engine();
        let (val, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                local key_a = ("a"):rep(32)
                local key_b = ("b"):rep(32)
                local nonce = ("n"):rep(12)
                local ct = crypto.encrypt("aes_256_gcm", key_a, nonce, "msg")
                return crypto.decrypt("aes_256_gcm", key_b, nonce, ct)
                "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().starts_with("crypto.decrypt: decryption failed"));
    }

    #[test]
    fn omitted_aad_matches_empty_aad() {
        // The contract: a script that "doesn't pass AAD" must decrypt a
        // ciphertext produced with an empty-string AAD, and vice versa.
        let lua = engine();
        let (a, b, c, d): (mlua::String, mlua::String, mlua::String, mlua::String) = lua
            .load(
                r#"
                local key = ("k"):rep(32)
                local nonce = ("n"):rep(12)
                local ct1 = crypto.encrypt("aes_256_gcm", key, nonce, "msg")
                local ct2 = crypto.encrypt("aes_256_gcm", key, nonce, "msg", "")
                local pt1 = crypto.decrypt("aes_256_gcm", key, nonce, ct2)
                local pt2 = crypto.decrypt("aes_256_gcm", key, nonce, ct1, "")
                return ct1, ct2, pt1, pt2
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a.as_bytes().to_vec(), b.as_bytes().to_vec());
        assert_eq!(c.as_bytes().to_vec(), b"msg".to_vec());
        assert_eq!(d.as_bytes().to_vec(), b"msg".to_vec());
    }

    #[test]
    fn wrong_key_length_errors() {
        let lua = engine();
        let (v, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                return crypto.encrypt("aes_256_gcm", "short", ("n"):rep(12), "msg")
                "#,
            )
            .eval()
            .unwrap();
        assert!(v.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("crypto.encrypt: "), "bad prefix: {e}");
        assert!(e.contains("expected 32-byte key"), "bad body: {e}");
    }

    #[test]
    fn wrong_nonce_length_errors() {
        let lua = engine();
        let (v, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                return crypto.encrypt("aes_256_gcm", ("k"):rep(32), "short", "msg")
                "#,
            )
            .eval()
            .unwrap();
        assert!(v.is_none());
        let e = e.unwrap();
        assert!(e.starts_with("crypto.encrypt: "), "bad prefix: {e}");
        assert!(e.contains("expected 12-byte nonce"), "bad body: {e}");
    }

    #[test]
    fn truncated_ciphertext_errors_decrypt() {
        // A ciphertext shorter than the tag length can never be valid; we
        // reject it before reaching the backend so the error names the real
        // cause instead of falling out as "decryption failed".
        let lua = engine();
        let (v, e): (Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                return crypto.decrypt("aes_256_gcm", ("k"):rep(32), ("n"):rep(12), "short")
                "#,
            )
            .eval()
            .unwrap();
        assert!(v.is_none());
        let e = e.unwrap();
        assert!(e.contains("ciphertext shorter than"), "bad body: {e}");
    }

    #[test]
    fn unknown_algorithm_errors() {
        let lua = engine();
        for fn_name in ["encrypt", "key_size", "nonce_size", "tag_size"] {
            let call = match fn_name {
                "encrypt" => r#"crypto.encrypt("aes_512_gcm", "", "", "")"#.to_string(),
                _ => format!(r#"crypto.{fn_name}("aes_512_gcm")"#),
            };
            let (v, e): (Option<mlua::Value>, Option<String>) = lua
                .load(format!("return {call}"))
                .eval()
                .unwrap_or_else(|err| panic!("{fn_name}: {err}"));
            assert!(matches!(v, None | Some(mlua::Value::Nil)));
            let e = e.unwrap_or_else(|| panic!("{fn_name}: no error"));
            assert!(
                e.contains("unknown algorithm 'aes_512_gcm'"),
                "{fn_name}: {e}"
            );
        }
    }

    #[test]
    fn empty_plaintext_round_trips() {
        // AEAD over an empty plaintext must produce a tag-only ciphertext
        // (16 bytes) and decrypt back to empty — a corner case scripts hit
        // when "the payload is just the header".
        let lua = engine();
        let (ct, pt, e): (mlua::String, Option<mlua::String>, Option<String>) = lua
            .load(
                r#"
                local key = ("k"):rep(32)
                local nonce = ("n"):rep(12)
                local ct = crypto.encrypt("aes_256_gcm", key, nonce, "")
                local out, e = crypto.decrypt("aes_256_gcm", key, nonce, ct)
                return ct, out, e
                "#,
            )
            .eval()
            .unwrap();
        assert!(e.is_none());
        assert_eq!(ct.as_bytes().len(), 16, "ct must be tag-only");
        assert_eq!(pt.unwrap().as_bytes().len(), 0);
    }

    #[test]
    fn sizes_match_canonical_values() {
        let lua = engine();
        let key_sizes: Vec<i64> = ALGOS
            .iter()
            .map(|a| {
                lua.load(format!(r#"return crypto.key_size("{a}")"#))
                    .eval()
                    .unwrap()
            })
            .collect();
        assert_eq!(key_sizes, vec![16, 32, 32]);

        for a in ALGOS {
            let n: i64 = lua
                .load(format!(r#"return crypto.nonce_size("{a}")"#))
                .eval()
                .unwrap();
            assert_eq!(n, 12);
            let t: i64 = lua
                .load(format!(r#"return crypto.tag_size("{a}")"#))
                .eval()
                .unwrap();
            assert_eq!(t, 16);
        }
    }

    #[test]
    fn algorithms_list_is_complete() {
        let lua = engine();
        let algos: Vec<String> = lua.load(r#"return crypto.algorithms()"#).eval().unwrap();
        assert_eq!(
            algos,
            super::ALGOS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    use super::ALGOS;
}

//! Asymmetric signing and verification — `lib:pkey`.
//!
//! Keypair-as-object surface over the RustCrypto `signature` ecosystem: one
//! backend crate per algorithm family, unified behind the `signature` 2.x
//! `Signer`/`Verifier` traits and the `pkcs8` / `spki` encoding layer. A
//! `KeyPair` holds a private key and signs; a `PublicKey` verifies. The public
//! key is extracted from the private one via `:public_key()`.
//!
//! ## Algorithms
//!
//! Ed25519 is the modern default and is listed first.
//!
//! | Name                   | Backend         | Signature                       |
//! |------------------------|-----------------|---------------------------------|
//! | `ed25519`              | `ed25519-dalek` | 64 bytes                        |
//! | `ecdsa-p256`           | `p256`          | 64-byte fixed `r‖s` (JOSE ES256)|
//! | `rsa-pss-sha256`       | `rsa`           | modulus-width (256 B at 2048)   |
//! | `rsa-pkcs1v15-sha256`  | `rsa`           | modulus-width (256 B at 2048)   |
//!
//! ECDSA and RSA sign over the SHA-256 digest of the message; Ed25519 signs the
//! message directly (its internal hash is SHA-512). Generated RSA keys are
//! 2048-bit.
//!
//! ## Signature encoding
//!
//! ECDSA signatures are the fixed 64-byte `r‖s` form (IEEE P1363 / JOSE
//! ES256), **not** ASN.1 DER — that is the encoding JWT ES256 and most
//! cross-runtime verifiers expect. Ed25519 is always 64 bytes; RSA is the
//! modulus width. All are raw bytes; hex/base64 via `lib:hex` / `lib:base64`.
//!
//! ## Serialisation
//!
//! Private keys serialise as PKCS#8, public keys as SPKI (`SubjectPublicKeyInfo`)
//! — the cross-runtime interchange formats Node, Go, Java and .NET all read.
//! `:to_pem()` emits with LF line endings; `:to_der()` emits the raw DER bytes.
//!
//! Loading requires the algorithm explicitly: `KeyPair.from_pem(algo, pem)`.
//! Ed25519 and ECDSA carry their identity in the key's OID, but a plain RSA
//! `rsaEncryption` key does not record whether it is meant for PSS or
//! PKCS#1 v1.5 — so the scheme cannot be inferred and the caller must state it.
//! A uniform `algo`-first signature across every algorithm keeps the rule
//! simple and rejects key/algorithm mismatches at load time.
//!
//! ## Verification
//!
//! `PublicKey:verify(message, signature)` returns a plain `bool`: `true` iff
//! the signature is valid. A malformed or wrong-length signature is simply not
//! valid and returns `false`, never an error — there is one question to answer
//! and one answer to act on, so a caller cannot accidentally treat a bad
//! signature as success by checking only an error channel.
//!
//! Ed25519 verification is **strict** (`verify_strict`): non-canonical and
//! malleable signatures are rejected, so a third party cannot mint a second
//! valid signature for an existing `(key, message)` — the signature bytes are
//! safe to use as a dedup / idempotency key. ECDSA and RSA verify the canonical
//! encodings described above.
//!
//! Key bytes never reach the log: signing/serialising methods carry no tracing
//! span, and the facade constructor spans are `skip_all`.
//!
//! ## Security
//!
//! RSA private-key operations (sign) via the `rsa` crate (0.9.x) carry
//! RUSTSEC-2023-0071 — the "Marvin" key-recovery timing side-channel. No fixed
//! upstream release exists; `rsa` is the only pure-Rust RSA backend (#35
//! sanctions RustCrypto), so this is a conscious trade-off, not an oversight.
//! `ed25519` and `ecdsa-p256` are unaffected — prefer them for new signers.
//! Advisory-gate + upstream tracking: #51.

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods};
use pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rand::rngs::OsRng;
use sha2::Sha256;
use signature::{RandomizedSigner, SignatureEncoding, Signer, Verifier};
use spki::{DecodePublicKey, EncodePublicKey};

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

/// Canonical algorithm list — the `.neoc` / docs order, Ed25519 first. Used by
/// `algorithms()` and by tests that iterate every backend.
const ALGOS: &[&str] = &[
    "ed25519",
    "ecdsa-p256",
    "rsa-pss-sha256",
    "rsa-pkcs1v15-sha256",
];

/// Bit length for generated RSA keys. 2048 is the floor for new keys today;
/// callers needing 3072/4096 can file a follow-up rather than carry a knob that
/// breaks the uniform `generate(algo)` shape.
const RSA_BITS: usize = 2048;

// ---------------------------------------------------------------------------
// Algorithm tag
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Algo {
    Ed25519,
    EcdsaP256,
    RsaPss,
    RsaPkcs1v15,
}

impl Algo {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "ed25519" => Ok(Self::Ed25519),
            "ecdsa-p256" => Ok(Self::EcdsaP256),
            "rsa-pss-sha256" => Ok(Self::RsaPss),
            "rsa-pkcs1v15-sha256" => Ok(Self::RsaPkcs1v15),
            _ => Err(format!("unknown algorithm '{s}'")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::EcdsaP256 => "ecdsa-p256",
            Self::RsaPss => "rsa-pss-sha256",
            Self::RsaPkcs1v15 => "rsa-pkcs1v15-sha256",
        }
    }
}

// ---------------------------------------------------------------------------
// Key state — one variant per backend. RSA stores the bare `RsaPrivateKey`
// (the type that serialises) plus the scheme in the variant; the scheme-bound
// signing key is built per call, which is cheap next to the signature itself.
// ---------------------------------------------------------------------------

enum Secret {
    Ed25519(ed25519_dalek::SigningKey),
    EcdsaP256(p256::ecdsa::SigningKey),
    RsaPss(rsa::RsaPrivateKey),
    RsaPkcs1v15(rsa::RsaPrivateKey),
}

enum Public {
    Ed25519(ed25519_dalek::VerifyingKey),
    EcdsaP256(p256::ecdsa::VerifyingKey),
    RsaPss(rsa::RsaPublicKey),
    RsaPkcs1v15(rsa::RsaPublicKey),
}

impl Secret {
    fn algo(&self) -> Algo {
        match self {
            Self::Ed25519(_) => Algo::Ed25519,
            Self::EcdsaP256(_) => Algo::EcdsaP256,
            Self::RsaPss(_) => Algo::RsaPss,
            Self::RsaPkcs1v15(_) => Algo::RsaPkcs1v15,
        }
    }
}

impl Public {
    fn algo(&self) -> Algo {
        match self {
            Self::Ed25519(_) => Algo::Ed25519,
            Self::EcdsaP256(_) => Algo::EcdsaP256,
            Self::RsaPss(_) => Algo::RsaPss,
            Self::RsaPkcs1v15(_) => Algo::RsaPkcs1v15,
        }
    }
}

// ---------------------------------------------------------------------------
// KeyPair
// ---------------------------------------------------------------------------

pub struct KeyPair {
    secret: Secret,
}

impl KeyPair {
    fn generate(algo: Algo) -> Result<Self, String> {
        let secret = match algo {
            Algo::Ed25519 => Secret::Ed25519(ed25519_dalek::SigningKey::generate(&mut OsRng)),
            Algo::EcdsaP256 => Secret::EcdsaP256(p256::ecdsa::SigningKey::random(&mut OsRng)),
            Algo::RsaPss => {
                Secret::RsaPss(rsa::RsaPrivateKey::new(&mut OsRng, RSA_BITS).map_err(stringify)?)
            }
            Algo::RsaPkcs1v15 => Secret::RsaPkcs1v15(
                rsa::RsaPrivateKey::new(&mut OsRng, RSA_BITS).map_err(stringify)?,
            ),
        };
        Ok(Self { secret })
    }

    fn from_pem(algo: Algo, pem: &str) -> Result<Self, String> {
        let secret = match algo {
            Algo::Ed25519 => {
                Secret::Ed25519(ed25519_dalek::SigningKey::from_pkcs8_pem(pem).map_err(stringify)?)
            }
            Algo::EcdsaP256 => {
                Secret::EcdsaP256(p256::ecdsa::SigningKey::from_pkcs8_pem(pem).map_err(stringify)?)
            }
            Algo::RsaPss => {
                Secret::RsaPss(rsa::RsaPrivateKey::from_pkcs8_pem(pem).map_err(stringify)?)
            }
            Algo::RsaPkcs1v15 => {
                Secret::RsaPkcs1v15(rsa::RsaPrivateKey::from_pkcs8_pem(pem).map_err(stringify)?)
            }
        };
        Ok(Self { secret })
    }

    fn from_der(algo: Algo, der: &[u8]) -> Result<Self, String> {
        let secret = match algo {
            Algo::Ed25519 => {
                Secret::Ed25519(ed25519_dalek::SigningKey::from_pkcs8_der(der).map_err(stringify)?)
            }
            Algo::EcdsaP256 => {
                Secret::EcdsaP256(p256::ecdsa::SigningKey::from_pkcs8_der(der).map_err(stringify)?)
            }
            Algo::RsaPss => {
                Secret::RsaPss(rsa::RsaPrivateKey::from_pkcs8_der(der).map_err(stringify)?)
            }
            Algo::RsaPkcs1v15 => {
                Secret::RsaPkcs1v15(rsa::RsaPrivateKey::from_pkcs8_der(der).map_err(stringify)?)
            }
        };
        Ok(Self { secret })
    }

    fn public(&self) -> PublicKey {
        let public = match &self.secret {
            Secret::Ed25519(k) => Public::Ed25519(k.verifying_key()),
            Secret::EcdsaP256(k) => Public::EcdsaP256(*k.verifying_key()),
            Secret::RsaPss(k) => Public::RsaPss(k.to_public_key()),
            Secret::RsaPkcs1v15(k) => Public::RsaPkcs1v15(k.to_public_key()),
        };
        PublicKey { public }
    }

    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        let sig = match &self.secret {
            Secret::Ed25519(k) => k.try_sign(msg).map_err(stringify)?.to_vec(),
            Secret::EcdsaP256(k) => {
                let sig: p256::ecdsa::Signature = k.try_sign(msg).map_err(stringify)?;
                sig.to_vec()
            }
            Secret::RsaPss(k) => rsa::pss::SigningKey::<Sha256>::new(k.clone())
                .try_sign_with_rng(&mut OsRng, msg)
                .map_err(stringify)?
                .to_vec(),
            Secret::RsaPkcs1v15(k) => rsa::pkcs1v15::SigningKey::<Sha256>::new(k.clone())
                .try_sign(msg)
                .map_err(stringify)?
                .to_vec(),
        };
        Ok(sig)
    }

    fn to_pem(&self) -> Result<String, String> {
        let pem = match &self.secret {
            Secret::Ed25519(k) => k.to_pkcs8_pem(LineEnding::LF),
            Secret::EcdsaP256(k) => k.to_pkcs8_pem(LineEnding::LF),
            Secret::RsaPss(k) | Secret::RsaPkcs1v15(k) => k.to_pkcs8_pem(LineEnding::LF),
        }
        .map_err(stringify)?;
        Ok(pem.to_string())
    }

    fn to_der(&self) -> Result<Vec<u8>, String> {
        let der = match &self.secret {
            Secret::Ed25519(k) => k.to_pkcs8_der(),
            Secret::EcdsaP256(k) => k.to_pkcs8_der(),
            Secret::RsaPss(k) | Secret::RsaPkcs1v15(k) => k.to_pkcs8_der(),
        }
        .map_err(stringify)?;
        Ok(der.as_bytes().to_vec())
    }
}

impl UserData for KeyPair {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("public_key", |_, this, _: ()| Ok(this.public()));

        methods.add_method("sign", |lua, this, msg: mlua::String| {
            match this.sign(&msg.as_bytes()) {
                Ok(sig) => ok(lua, lua.create_string(&sig)?),
                Err(e) => err(lua, format!("KeyPair.sign: {e}")),
            }
        });

        methods.add_method("algorithm", |_, this, _: ()| Ok(this.secret.algo().name()));

        methods.add_method("to_pem", |lua, this, _: ()| match this.to_pem() {
            Ok(pem) => ok(lua, lua.create_string(&pem)?),
            Err(e) => err(lua, format!("KeyPair.to_pem: {e}")),
        });

        methods.add_method("to_der", |lua, this, _: ()| match this.to_der() {
            Ok(der) => ok(lua, lua.create_string(&der)?),
            Err(e) => err(lua, format!("KeyPair.to_der: {e}")),
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("KeyPair({})", this.secret.algo().name()))
        });
    }
}

// ---------------------------------------------------------------------------
// PublicKey
// ---------------------------------------------------------------------------

pub struct PublicKey {
    public: Public,
}

impl PublicKey {
    fn from_pem(algo: Algo, pem: &str) -> Result<Self, String> {
        let public = match algo {
            Algo::Ed25519 => Public::Ed25519(
                ed25519_dalek::VerifyingKey::from_public_key_pem(pem).map_err(stringify)?,
            ),
            Algo::EcdsaP256 => Public::EcdsaP256(
                p256::ecdsa::VerifyingKey::from_public_key_pem(pem).map_err(stringify)?,
            ),
            Algo::RsaPss => {
                Public::RsaPss(rsa::RsaPublicKey::from_public_key_pem(pem).map_err(stringify)?)
            }
            Algo::RsaPkcs1v15 => {
                Public::RsaPkcs1v15(rsa::RsaPublicKey::from_public_key_pem(pem).map_err(stringify)?)
            }
        };
        Ok(Self { public })
    }

    fn from_der(algo: Algo, der: &[u8]) -> Result<Self, String> {
        let public = match algo {
            Algo::Ed25519 => Public::Ed25519(
                ed25519_dalek::VerifyingKey::from_public_key_der(der).map_err(stringify)?,
            ),
            Algo::EcdsaP256 => Public::EcdsaP256(
                p256::ecdsa::VerifyingKey::from_public_key_der(der).map_err(stringify)?,
            ),
            Algo::RsaPss => {
                Public::RsaPss(rsa::RsaPublicKey::from_public_key_der(der).map_err(stringify)?)
            }
            Algo::RsaPkcs1v15 => {
                Public::RsaPkcs1v15(rsa::RsaPublicKey::from_public_key_der(der).map_err(stringify)?)
            }
        };
        Ok(Self { public })
    }

    /// `true` iff `sig` is a valid signature over `msg`. Any decode failure
    /// (wrong length, malformed encoding) is not a valid signature and returns
    /// `false` rather than raising.
    fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        match &self.public {
            Public::Ed25519(k) => ed25519_dalek::Signature::try_from(sig)
                .map(|s| k.verify_strict(msg, &s).is_ok())
                .unwrap_or(false),
            Public::EcdsaP256(k) => p256::ecdsa::Signature::try_from(sig)
                .map(|s| k.verify(msg, &s).is_ok())
                .unwrap_or(false),
            Public::RsaPss(k) => {
                let vk = rsa::pss::VerifyingKey::<Sha256>::new(k.clone());
                rsa::pss::Signature::try_from(sig)
                    .map(|s| vk.verify(msg, &s).is_ok())
                    .unwrap_or(false)
            }
            Public::RsaPkcs1v15(k) => {
                let vk = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(k.clone());
                rsa::pkcs1v15::Signature::try_from(sig)
                    .map(|s| vk.verify(msg, &s).is_ok())
                    .unwrap_or(false)
            }
        }
    }

    fn to_pem(&self) -> Result<String, String> {
        match &self.public {
            Public::Ed25519(k) => k.to_public_key_pem(LineEnding::LF),
            Public::EcdsaP256(k) => k.to_public_key_pem(LineEnding::LF),
            Public::RsaPss(k) | Public::RsaPkcs1v15(k) => k.to_public_key_pem(LineEnding::LF),
        }
        .map_err(stringify)
    }

    fn to_der(&self) -> Result<Vec<u8>, String> {
        let der = match &self.public {
            Public::Ed25519(k) => k.to_public_key_der(),
            Public::EcdsaP256(k) => k.to_public_key_der(),
            Public::RsaPss(k) | Public::RsaPkcs1v15(k) => k.to_public_key_der(),
        }
        .map_err(stringify)?;
        Ok(der.as_bytes().to_vec())
    }
}

impl UserData for PublicKey {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "verify",
            |_, this, (msg, sig): (mlua::String, mlua::String)| {
                Ok(this.verify(&msg.as_bytes(), &sig.as_bytes()))
            },
        );

        methods.add_method("algorithm", |_, this, _: ()| Ok(this.public.algo().name()));

        methods.add_method("to_pem", |lua, this, _: ()| match this.to_pem() {
            Ok(pem) => ok(lua, lua.create_string(&pem)?),
            Err(e) => err(lua, format!("PublicKey.to_pem: {e}")),
        });

        methods.add_method("to_der", |lua, this, _: ()| match this.to_der() {
            Ok(der) => ok(lua, lua.create_string(&der)?),
            Err(e) => err(lua, format!("PublicKey.to_der: {e}")),
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(format!("PublicKey({})", this.public.algo().name()))
        });
    }
}

/// RustCrypto's backend error types carry no shared trait beyond `Display`;
/// flattening every one to its string keeps the `(value, error)` channel
/// uniform without leaking backend-specific structure into the Lua surface.
fn stringify<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:pkey` surface through the facade.
///
/// `KeyPair` and `PublicKey` are recorded as constructor-bearing types via
/// [`crate::lua::facade::Module::type_`]: each `try_constructor` registers the
/// function on the type's sub-table and opens a `skip_all` `"<Type>.<ctor>"`
/// span, and each `method` records the matching `impl UserData` method for the
/// header. `algorithms` is the one free function.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:pkey")?;

    m.type_("KeyPair", |t| {
        t.try_constructor(
            "generate",
            &[("algo", "string")],
            "Generate a fresh keypair for `algo` (one of `algorithms()`). RSA keys are 2048-bit.",
            |_, algo: String| KeyPair::generate(Algo::parse(&algo)?),
        )?
        .try_constructor(
            "from_pem",
            &[("algo", "string"), ("pem", "string")],
            "Load a private key from a PKCS#8 PEM string. `algo` is required: a plain RSA key does not record the signing scheme (PSS vs PKCS#1 v1.5).",
            |_, (algo, pem): (String, String)| KeyPair::from_pem(Algo::parse(&algo)?, &pem),
        )?
        .try_constructor(
            "from_der",
            &[("algo", "string"), ("der", "string")],
            "Load a private key from PKCS#8 DER bytes. `algo` is required, as for `from_pem`.",
            |_, (algo, der): (String, mlua::String)| {
                KeyPair::from_der(Algo::parse(&algo)?, &der.as_bytes())
            },
        )?
        .method("public_key", &[], "PublicKey", "Extract the public key.")
        .method(
            "sign",
            &[("message", "string")],
            "Result<string, string>",
            "Sign `message` with the private key, returning the raw signature bytes.",
        )
        .method(
            "algorithm",
            &[],
            "string",
            "Return the algorithm name this keypair was built with.",
        )
        .method(
            "to_pem",
            &[],
            "Result<string, string>",
            "Serialise the private key as a PKCS#8 PEM string (LF line endings).",
        )
        .method(
            "to_der",
            &[],
            "Result<string, string>",
            "Serialise the private key as PKCS#8 DER bytes.",
        );
        Ok(())
    })?;

    m.type_("PublicKey", |t| {
        t.try_constructor(
            "from_pem",
            &[("algo", "string"), ("pem", "string")],
            "Load a public key from an SPKI PEM string under `algo`.",
            |_, (algo, pem): (String, String)| PublicKey::from_pem(Algo::parse(&algo)?, &pem),
        )?
        .try_constructor(
            "from_der",
            &[("algo", "string"), ("der", "string")],
            "Load a public key from SPKI DER bytes under `algo`.",
            |_, (algo, der): (String, mlua::String)| {
                PublicKey::from_der(Algo::parse(&algo)?, &der.as_bytes())
            },
        )?
        .method(
            "verify",
            &[("message", "string"), ("signature", "string")],
            "bool",
            "Verify `signature` over `message`; returns `true` iff valid, `false` on any mismatch or malformed signature.",
        )
        .method(
            "algorithm",
            &[],
            "string",
            "Return the algorithm name this public key was built with.",
        )
        .method(
            "to_pem",
            &[],
            "Result<string, string>",
            "Serialise the public key as an SPKI PEM string (LF line endings).",
        )
        .method(
            "to_der",
            &[],
            "Result<string, string>",
            "Serialise the public key as SPKI DER bytes.",
        );
        Ok(())
    })?;

    m.func(
        "algorithms",
        &[],
        "Return the list of supported algorithm names in canonical order.",
        |lua, _: ()| {
            let tbl = lua.create_table()?;
            for (i, a) in ALGOS.iter().enumerate() {
                tbl.set(i + 1, *a)?;
            }
            Ok(tbl)
        },
    )?;

    Ok(m)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:pkey")"#).eval().unwrap();
        lua.globals().set("pkey", m).unwrap();
        lua
    }

    /// generate → sign → verify round-trips for every algorithm. The slow RSA
    /// keygen is paid once per algorithm here and the verify must succeed.
    #[test]
    fn sign_then_verify_round_trips_each_algo() {
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local sig = assert(kp:sign("attached message"))
                    return kp:public_key():verify("attached message", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(ok, "{algo}: signature must verify");
        }
    }

    #[test]
    fn verify_rejects_tampered_message() {
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local sig = assert(kp:sign("original"))
                    return kp:public_key():verify("tampered", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(!ok, "{algo}: tampered message must not verify");
        }
    }

    #[test]
    fn verify_rejects_other_keys_signature() {
        // A signature from one keypair must not verify under another's public
        // key — the whole point of asymmetric signing.
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local a = assert(pkey.KeyPair.generate("{algo}"))
                    local b = assert(pkey.KeyPair.generate("{algo}"))
                    local sig = assert(a:sign("msg"))
                    return b:public_key():verify("msg", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(!ok, "{algo}: foreign key must not verify");
        }
    }

    #[test]
    fn verify_rejects_garbage_signature() {
        // A malformed signature is `false`, never an error.
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    return kp:public_key():verify("msg", "not a real signature")
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(!ok, "{algo}: garbage signature must return false");
        }
    }

    #[test]
    fn private_pem_round_trips_and_signs() {
        // to_pem → from_pem yields a key that produces verifiable signatures.
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local pem = assert(kp:to_pem())
                    assert(pem:match("^%-%-%-%-%-BEGIN PRIVATE KEY%-%-%-%-%-"), "PKCS#8 header")
                    local loaded = assert(pkey.KeyPair.from_pem("{algo}", pem))
                    local sig = assert(loaded:sign("msg"))
                    return kp:public_key():verify("msg", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(ok, "{algo}: reloaded private key must sign verifiably");
        }
    }

    #[test]
    fn private_der_round_trips() {
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local der = assert(kp:to_der())
                    local loaded = assert(pkey.KeyPair.from_der("{algo}", der))
                    local sig = assert(loaded:sign("msg"))
                    return kp:public_key():verify("msg", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(ok, "{algo}: reloaded DER private key must sign verifiably");
        }
    }

    #[test]
    fn public_pem_round_trips_and_verifies() {
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local sig = assert(kp:sign("msg"))
                    local pem = assert(kp:public_key():to_pem())
                    assert(pem:match("^%-%-%-%-%-BEGIN PUBLIC KEY%-%-%-%-%-"), "SPKI header")
                    local pub = assert(pkey.PublicKey.from_pem("{algo}", pem))
                    return pub:verify("msg", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(ok, "{algo}: reloaded public key must verify");
        }
    }

    #[test]
    fn public_der_round_trips_and_verifies() {
        let lua = engine();
        for algo in super::ALGOS {
            let ok: bool = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    local sig = assert(kp:sign("msg"))
                    local der = assert(kp:public_key():to_der())
                    local pub = assert(pkey.PublicKey.from_der("{algo}", der))
                    return pub:verify("msg", sig)
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert!(ok, "{algo}: reloaded DER public key must verify");
        }
    }

    #[test]
    fn ed25519_signature_is_64_bytes() {
        let lua = engine();
        let len: usize = lua
            .load(
                r#"
                local kp = assert(pkey.KeyPair.generate("ed25519"))
                return #kp:sign("msg")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(len, 64, "Ed25519 signatures are 64 bytes");
    }

    #[test]
    fn ecdsa_signature_is_fixed_64_bytes() {
        // Fixed r‖s form (JOSE ES256), not variable-length DER.
        let lua = engine();
        let len: usize = lua
            .load(
                r#"
                local kp = assert(pkey.KeyPair.generate("ecdsa-p256"))
                return #kp:sign("msg")
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(len, 64, "ECDSA P-256 fixed signatures are 64 bytes");
    }

    #[test]
    fn empty_message_signs_and_verifies() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local kp = assert(pkey.KeyPair.generate("ed25519"))
                local sig = assert(kp:sign(""))
                return kp:public_key():verify("", sig)
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok, "empty message must sign and verify");
    }

    #[test]
    fn binary_message_round_trips() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local msg = "\0\1\2\254\255\128"
                local kp = assert(pkey.KeyPair.generate("ed25519"))
                local sig = assert(kp:sign(msg))
                return kp:public_key():verify(msg, sig)
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok, "binary message must sign and verify");
    }

    #[test]
    fn generate_unknown_algorithm_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Value>, Option<String>) = lua
            .load(r#"return pkey.KeyPair.generate("rsa-1024")"#)
            .eval()
            .unwrap();
        assert!(val.is_none() || matches!(val, Some(mlua::Value::Nil)));
        let e = e.unwrap();
        assert!(e.starts_with("KeyPair.generate: "), "bad prefix: {e}");
        assert!(e.contains("unknown algorithm 'rsa-1024'"), "bad body: {e}");
    }

    #[test]
    fn from_pem_with_wrong_algorithm_errors() {
        // An Ed25519 PKCS#8 key loaded as ECDSA must fail — the OID mismatch is
        // caught by the typed parser, not silently misinterpreted.
        let lua = engine();
        let (val, e): (Option<mlua::Value>, Option<String>) = lua
            .load(
                r#"
                local kp = assert(pkey.KeyPair.generate("ed25519"))
                local pem = assert(kp:to_pem())
                return pkey.KeyPair.from_pem("ecdsa-p256", pem)
                "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none() || matches!(val, Some(mlua::Value::Nil)));
        assert!(
            e.unwrap().starts_with("KeyPair.from_pem: "),
            "wrong-algo load must surface a prefixed error"
        );
    }

    #[test]
    fn from_pem_rejects_malformed_input() {
        let lua = engine();
        let (val, e): (Option<mlua::Value>, Option<String>) = lua
            .load(r#"return pkey.KeyPair.from_pem("ed25519", "not pem")"#)
            .eval()
            .unwrap();
        assert!(val.is_none() || matches!(val, Some(mlua::Value::Nil)));
        assert!(e.unwrap().starts_with("KeyPair.from_pem: "));
    }

    #[test]
    fn algorithm_method_reports_name() {
        let lua = engine();
        for algo in super::ALGOS {
            let (priv_name, pub_name): (String, String) = lua
                .load(format!(
                    r#"
                    local kp = assert(pkey.KeyPair.generate("{algo}"))
                    return kp:algorithm(), kp:public_key():algorithm()
                    "#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("{algo}: {e}"));
            assert_eq!(priv_name, *algo);
            assert_eq!(pub_name, *algo);
        }
    }

    #[test]
    fn tostring_includes_algorithm() {
        let lua = engine();
        let (kp, pk): (String, String) = lua
            .load(
                r#"
                local kp = assert(pkey.KeyPair.generate("ed25519"))
                return tostring(kp), tostring(kp:public_key())
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(kp, "KeyPair(ed25519)");
        assert_eq!(pk, "PublicKey(ed25519)");
    }

    #[test]
    fn algorithms_list_is_complete() {
        let lua = engine();
        let algos: Vec<String> = lua.load(r#"return pkey.algorithms()"#).eval().unwrap();
        assert_eq!(
            algos,
            super::ALGOS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }
}

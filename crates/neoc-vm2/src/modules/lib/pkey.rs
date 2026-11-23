//! Asymmetric signing and verification: a `KeyPair` holds a private key and signs, a `PublicKey` verifies.
//!
//! One backend crate per algorithm family (RustCrypto), unified behind the `signature` traits and the `pkcs8` /
//! `spki` encoding layer. Ed25519 is the modern default and is listed first.
//!
//! | Algorithm             | Backend         | Signature                         |
//! |-----------------------|-----------------|-----------------------------------|
//! | `ed25519`             | `ed25519-dalek` | 64 bytes                          |
//! | `ecdsa-p256`          | `p256`          | 64-byte fixed `r||s` (JOSE ES256) |
//! | `rsa-pss-sha256`      | `rsa`           | modulus width (256 bytes at 2048) |
//! | `rsa-pkcs1v15-sha256` | `rsa`           | modulus width (256 bytes at 2048) |
//!
//! ECDSA and RSA sign the SHA-256 digest of the message; Ed25519 signs the message itself. Generated RSA keys are
//! 2048-bit. ECDSA signatures are the fixed 64-byte `r||s` form (IEEE P1363, what JWT ES256 and most verifiers
//! expect), not ASN.1 DER. Private keys are PKCS#8, public keys SPKI, the interchange formats every runtime reads;
//! PEM has LF line endings.
//!
//! Loading names the algorithm: Ed25519 and ECDSA carry theirs in the key, but a plain RSA key does not say
//! whether it is for PSS or PKCS#1 v1.5, so the caller states it, and a mismatch is refused at load time.
//!
//! `verify` answers `true` or `false`, and a malformed signature is simply not valid: one question, one answer to
//! act on, so a bad signature cannot be mistaken for success by looking only at an error. Ed25519 verification is
//! strict (non-canonical, malleable signatures are rejected), so the signature bytes are safe as a dedup key.
//!
//! Key bytes never reach the log: nothing here is traced with its arguments. RSA private-key operations go through
//! `rsa` 0.10, the constant-time rewrite, so RUSTSEC-2023-0071 (the Marvin timing attack on 0.9.x) does not apply.
//!
//! Ported from neoc-vm's `lib:pkey`. What changed on purpose: the algorithm is a declared union (an unknown name
//! is a bad argument), `algorithm` is a field, and every failure is a typed error table.

use std::fmt::Display;

use p256::elliptic_curve::Generate;
use pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rand::rngs::SysRng;
use sha2::Sha256;
use signature::{RandomizedSigner, SignatureEncoding, Signer, Verifier};
use spki::{DecodePublicKey, EncodePublicKey};

use self::pkey::{Algorithm, PkeyError};

/// Bit length of generated RSA keys: 2048 is the floor for new keys today.
const RSA_BITS: usize = 2048;

fn invalid_key(e: impl Display) -> PkeyError {
    PkeyError::InvalidKey { reason: e.to_string() }
}

fn generate_failed(e: impl Display) -> PkeyError {
    PkeyError::GenerateFailed { reason: e.to_string() }
}

fn sign_failed(e: impl Display) -> PkeyError {
    PkeyError::SignFailed { reason: e.to_string() }
}

fn encode_failed(e: impl Display) -> PkeyError {
    PkeyError::EncodeFailed { reason: e.to_string() }
}

/// A private key, one variant per backend. RSA stores the bare `PrivateKey` (the type that serialises) plus the
/// scheme in the variant; the scheme-bound signing key is built per call, which is cheap next to the signature.
enum Secret {
    Ed25519(ed25519_dalek::SigningKey),
    EcdsaP256(p256::ecdsa::SigningKey),
    RsaPss(rsa::PrivateKey),
    RsaPkcs1v15(rsa::PrivateKey),
}

enum Public {
    Ed25519(ed25519_dalek::VerifyingKey),
    EcdsaP256(p256::ecdsa::VerifyingKey),
    RsaPss(rsa::PublicKey),
    RsaPkcs1v15(rsa::PublicKey),
}

impl Secret {
    fn algorithm(&self) -> Algorithm {
        match self {
            Self::Ed25519(_) => Algorithm::Ed25519,
            Self::EcdsaP256(_) => Algorithm::EcdsaP256,
            Self::RsaPss(_) => Algorithm::RsaPss,
            Self::RsaPkcs1v15(_) => Algorithm::RsaPkcs1v15,
        }
    }

    fn generate(algorithm: Algorithm) -> Result<Self, PkeyError> {
        Ok(match algorithm {
            Algorithm::Ed25519 => {
                Self::Ed25519(ed25519_dalek::SigningKey::try_generate_from_rng(&mut SysRng).map_err(generate_failed)?)
            }
            Algorithm::EcdsaP256 => {
                Self::EcdsaP256(p256::ecdsa::SigningKey::try_generate_from_rng(&mut SysRng).map_err(generate_failed)?)
            }
            Algorithm::RsaPss => {
                Self::RsaPss(rsa::PrivateKey::new(&mut SysRng, RSA_BITS).map_err(generate_failed)?)
            }
            Algorithm::RsaPkcs1v15 => {
                Self::RsaPkcs1v15(rsa::PrivateKey::new(&mut SysRng, RSA_BITS).map_err(generate_failed)?)
            }
        })
    }

    fn from_pem(algorithm: Algorithm, pem: &str) -> Result<Self, PkeyError> {
        Ok(match algorithm {
            Algorithm::Ed25519 => Self::Ed25519(ed25519_dalek::SigningKey::from_pkcs8_pem(pem).map_err(invalid_key)?),
            Algorithm::EcdsaP256 => Self::EcdsaP256(p256::ecdsa::SigningKey::from_pkcs8_pem(pem).map_err(invalid_key)?),
            Algorithm::RsaPss => Self::RsaPss(rsa::PrivateKey::from_pkcs8_pem(pem).map_err(invalid_key)?),
            Algorithm::RsaPkcs1v15 => Self::RsaPkcs1v15(rsa::PrivateKey::from_pkcs8_pem(pem).map_err(invalid_key)?),
        })
    }

    fn from_der(algorithm: Algorithm, der: &[u8]) -> Result<Self, PkeyError> {
        Ok(match algorithm {
            Algorithm::Ed25519 => Self::Ed25519(ed25519_dalek::SigningKey::from_pkcs8_der(der).map_err(invalid_key)?),
            Algorithm::EcdsaP256 => Self::EcdsaP256(p256::ecdsa::SigningKey::from_pkcs8_der(der).map_err(invalid_key)?),
            Algorithm::RsaPss => Self::RsaPss(rsa::PrivateKey::from_pkcs8_der(der).map_err(invalid_key)?),
            Algorithm::RsaPkcs1v15 => Self::RsaPkcs1v15(rsa::PrivateKey::from_pkcs8_der(der).map_err(invalid_key)?),
        })
    }

    fn public(&self) -> Public {
        match self {
            Self::Ed25519(k) => Public::Ed25519(k.verifying_key()),
            Self::EcdsaP256(k) => Public::EcdsaP256(*k.verifying_key()),
            Self::RsaPss(k) => Public::RsaPss(k.to_public_key()),
            Self::RsaPkcs1v15(k) => Public::RsaPkcs1v15(k.to_public_key()),
        }
    }

    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, PkeyError> {
        Ok(match self {
            Self::Ed25519(k) => k.try_sign(msg).map_err(sign_failed)?.to_vec(),
            Self::EcdsaP256(k) => {
                let sig: p256::ecdsa::Signature = k.try_sign(msg).map_err(sign_failed)?;
                sig.to_vec()
            }
            Self::RsaPss(k) => rsa::pss::SigningKey::<Sha256>::new(k.clone())
                .try_sign_with_rng(&mut SysRng, msg)
                .map_err(sign_failed)?
                .to_vec(),
            Self::RsaPkcs1v15(k) => {
                rsa::pkcs1v15::SigningKey::<Sha256>::new(k.clone()).try_sign(msg).map_err(sign_failed)?.to_vec()
            }
        })
    }

    fn to_pem(&self) -> Result<String, PkeyError> {
        let pem = match self {
            Self::Ed25519(k) => k.to_pkcs8_pem(LineEnding::LF),
            Self::EcdsaP256(k) => k.to_pkcs8_pem(LineEnding::LF),
            Self::RsaPss(k) | Self::RsaPkcs1v15(k) => k.to_pkcs8_pem(LineEnding::LF),
        }
        .map_err(encode_failed)?;
        Ok(pem.to_string())
    }

    fn to_der(&self) -> Result<Vec<u8>, PkeyError> {
        let der = match self {
            Self::Ed25519(k) => k.to_pkcs8_der(),
            Self::EcdsaP256(k) => k.to_pkcs8_der(),
            Self::RsaPss(k) | Self::RsaPkcs1v15(k) => k.to_pkcs8_der(),
        }
        .map_err(encode_failed)?;
        Ok(der.as_bytes().to_vec())
    }
}

impl Public {
    fn algorithm(&self) -> Algorithm {
        match self {
            Self::Ed25519(_) => Algorithm::Ed25519,
            Self::EcdsaP256(_) => Algorithm::EcdsaP256,
            Self::RsaPss(_) => Algorithm::RsaPss,
            Self::RsaPkcs1v15(_) => Algorithm::RsaPkcs1v15,
        }
    }

    fn from_pem(algorithm: Algorithm, pem: &str) -> Result<Self, PkeyError> {
        Ok(match algorithm {
            Algorithm::Ed25519 => {
                Self::Ed25519(ed25519_dalek::VerifyingKey::from_public_key_pem(pem).map_err(invalid_key)?)
            }
            Algorithm::EcdsaP256 => {
                Self::EcdsaP256(p256::ecdsa::VerifyingKey::from_public_key_pem(pem).map_err(invalid_key)?)
            }
            Algorithm::RsaPss => Self::RsaPss(rsa::PublicKey::from_public_key_pem(pem).map_err(invalid_key)?),
            Algorithm::RsaPkcs1v15 => {
                Self::RsaPkcs1v15(rsa::PublicKey::from_public_key_pem(pem).map_err(invalid_key)?)
            }
        })
    }

    fn from_der(algorithm: Algorithm, der: &[u8]) -> Result<Self, PkeyError> {
        Ok(match algorithm {
            Algorithm::Ed25519 => {
                Self::Ed25519(ed25519_dalek::VerifyingKey::from_public_key_der(der).map_err(invalid_key)?)
            }
            Algorithm::EcdsaP256 => {
                Self::EcdsaP256(p256::ecdsa::VerifyingKey::from_public_key_der(der).map_err(invalid_key)?)
            }
            Algorithm::RsaPss => Self::RsaPss(rsa::PublicKey::from_public_key_der(der).map_err(invalid_key)?),
            Algorithm::RsaPkcs1v15 => {
                Self::RsaPkcs1v15(rsa::PublicKey::from_public_key_der(der).map_err(invalid_key)?)
            }
        })
    }

    /// Whether `sig` is a valid signature over `msg`; a signature that does not even decode is not valid.
    fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        match self {
            Self::Ed25519(k) => {
                ed25519_dalek::Signature::try_from(sig).is_ok_and(|s| k.verify_strict(msg, &s).is_ok())
            }
            Self::EcdsaP256(k) => p256::ecdsa::Signature::try_from(sig).is_ok_and(|s| k.verify(msg, &s).is_ok()),
            Self::RsaPss(k) => {
                let key = rsa::pss::VerifyingKey::<Sha256>::new(k.clone());
                rsa::pss::Signature::try_from(sig).is_ok_and(|s| key.verify(msg, &s).is_ok())
            }
            Self::RsaPkcs1v15(k) => {
                let key = rsa::pkcs1v15::VerifyingKey::<Sha256>::new(k.clone());
                rsa::pkcs1v15::Signature::try_from(sig).is_ok_and(|s| key.verify(msg, &s).is_ok())
            }
        }
    }

    fn to_pem(&self) -> Result<String, PkeyError> {
        match self {
            Self::Ed25519(k) => k.to_public_key_pem(LineEnding::LF),
            Self::EcdsaP256(k) => k.to_public_key_pem(LineEnding::LF),
            Self::RsaPss(k) | Self::RsaPkcs1v15(k) => k.to_public_key_pem(LineEnding::LF),
        }
        .map_err(encode_failed)
    }

    fn to_der(&self) -> Result<Vec<u8>, PkeyError> {
        let der = match self {
            Self::Ed25519(k) => k.to_public_key_der(),
            Self::EcdsaP256(k) => k.to_public_key_der(),
            Self::RsaPss(k) | Self::RsaPkcs1v15(k) => k.to_public_key_der(),
        }
        .map_err(encode_failed)?;
        Ok(der.as_bytes().to_vec())
    }
}

#[mlua_ir::module("lib:pkey")]
pub mod pkey {
    use std::sync::Arc;

    use mlua_ir::{Bytes, LuaError, Object, Shape};
    use serde::{Deserialize, Serialize};

    use super::{Public, Secret};

    /// A signature scheme.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    pub enum Algorithm {
        /// Ed25519 (64-byte signatures). The modern default.
        #[serde(rename = "ed25519")]
        Ed25519,
        /// ECDSA over NIST P-256 with SHA-256, signatures as fixed 64-byte `r||s` (JOSE ES256).
        #[serde(rename = "ecdsa-p256")]
        EcdsaP256,
        /// RSA-PSS with SHA-256 (2048-bit generated keys).
        #[serde(rename = "rsa-pss-sha256")]
        RsaPss,
        /// RSA PKCS#1 v1.5 with SHA-256 (2048-bit generated keys).
        #[serde(rename = "rsa-pkcs1v15-sha256")]
        RsaPkcs1v15,
    }

    impl Algorithm {
        fn name(self) -> &'static str {
            match self {
                Self::Ed25519 => "ed25519",
                Self::EcdsaP256 => "ecdsa-p256",
                Self::RsaPss => "rsa-pss-sha256",
                Self::RsaPkcs1v15 => "rsa-pkcs1v15-sha256",
            }
        }
    }

    /// Why a key could not be made, loaded, used or written out.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum PkeyError {
        /// The PEM or DER is not a valid key of the algorithm named (malformed, or a key of another algorithm).
        #[error("invalid key: {reason}")]
        InvalidKey {
            /// What the decoder said.
            reason: String,
        },
        /// A key could not be generated.
        #[error("key generation failed: {reason}")]
        GenerateFailed {
            /// What the backend said.
            reason: String,
        },
        /// The backend refused to sign.
        #[error("signing failed: {reason}")]
        SignFailed {
            /// What the backend said.
            reason: String,
        },
        /// A key could not be written as PEM or DER.
        #[error("encoding failed: {reason}")]
        EncodeFailed {
            /// What the backend said.
            reason: String,
        },
    }

    /// A private key, which signs. Clones share one key.
    ///
    /// ```luau
    /// local pair = assert(pkey.KeyPair.generate("ed25519"))
    /// local signature = assert(pair:sign("message"))
    /// assert(pair:public_key():verify("message", signature))
    /// ```
    #[derive(Clone, Object)]
    pub struct KeyPair {
        secret: Arc<Secret>,
    }

    #[mlua_ir::object]
    impl KeyPair {
        /// A fresh key pair. RSA keys are 2048-bit.
        pub fn generate(algorithm: Algorithm) -> Result<KeyPair, PkeyError> {
            Ok(KeyPair { secret: Arc::new(Secret::generate(algorithm)?) })
        }

        /// Load a private key from a PKCS#8 PEM string. The algorithm is required: a plain RSA key does not
        /// record the scheme (PSS or PKCS#1 v1.5).
        pub fn from_pem(algorithm: Algorithm, pem: String) -> Result<KeyPair, PkeyError> {
            Ok(KeyPair { secret: Arc::new(Secret::from_pem(algorithm, &pem)?) })
        }

        /// Load a private key from PKCS#8 DER bytes. The algorithm is required, as for `from_pem`.
        pub fn from_der(algorithm: Algorithm, der: Bytes) -> Result<KeyPair, PkeyError> {
            Ok(KeyPair { secret: Arc::new(Secret::from_der(algorithm, &der)?) })
        }

        /// The public half.
        pub fn public_key(&self) -> PublicKey {
            PublicKey { public: Arc::new(self.secret.public()) }
        }

        /// Sign `message`; the raw signature bytes.
        pub fn sign(&self, message: Bytes) -> Result<Bytes, PkeyError> {
            self.secret.sign(&message).map(Bytes)
        }

        /// The private key as a PKCS#8 PEM string (LF line endings).
        pub fn to_pem(&self) -> Result<String, PkeyError> {
            self.secret.to_pem()
        }

        /// The private key as PKCS#8 DER bytes.
        pub fn to_der(&self) -> Result<Bytes, PkeyError> {
            self.secret.to_der().map(Bytes)
        }

        /// The algorithm this key is for.
        #[get]
        pub fn algorithm(&self) -> Algorithm {
            self.secret.algorithm()
        }

        /// `KeyPair(ed25519)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("KeyPair({})", self.secret.algorithm().name())
        }
    }

    /// A public key, which verifies. Clones share one key.
    #[derive(Clone, Object)]
    pub struct PublicKey {
        public: Arc<Public>,
    }

    #[mlua_ir::object]
    impl PublicKey {
        /// Load a public key from an SPKI PEM string.
        pub fn from_pem(algorithm: Algorithm, pem: String) -> Result<PublicKey, PkeyError> {
            Ok(PublicKey { public: Arc::new(Public::from_pem(algorithm, &pem)?) })
        }

        /// Load a public key from SPKI DER bytes.
        pub fn from_der(algorithm: Algorithm, der: Bytes) -> Result<PublicKey, PkeyError> {
            Ok(PublicKey { public: Arc::new(Public::from_der(algorithm, &der)?) })
        }

        /// Whether `signature` is a valid signature over `message`: `false` for a wrong message, another key's
        /// signature, or one that is malformed.
        pub fn verify(&self, message: Bytes, signature: Bytes) -> bool {
            self.public.verify(&message, &signature)
        }

        /// The key as an SPKI PEM string (LF line endings).
        pub fn to_pem(&self) -> Result<String, PkeyError> {
            self.public.to_pem()
        }

        /// The key as SPKI DER bytes.
        pub fn to_der(&self) -> Result<Bytes, PkeyError> {
            self.public.to_der().map(Bytes)
        }

        /// The algorithm this key is for.
        #[get]
        pub fn algorithm(&self) -> Algorithm {
            self.public.algorithm()
        }

        /// `PublicKey(ed25519)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("PublicKey({})", self.public.algorithm().name())
        }
    }

    /// Every supported algorithm, Ed25519 first.
    pub fn algorithms() -> Vec<Algorithm> {
        vec![Algorithm::Ed25519, Algorithm::EcdsaP256, Algorithm::RsaPss, Algorithm::RsaPkcs1v15]
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:pkey")"#).eval().unwrap();
        lua.globals().set("pkey", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    /// Everything a key pair of `algo` must do, in one script: RSA key generation is slow, so each algorithm pays
    /// for its keys once (this runs in a test per algorithm, in parallel).
    fn exercise(algo: &str) {
        let report: String = eval(&format!(
            r#"
            local algo = "{algo}"
            local function must(value, err)
                assert(value ~= nil, err and tostring(err))
                return value -- one value, so it can end an argument list
            end
            local pair = must(pkey.KeyPair.generate(algo))
            local public = pair:public_key()
            local problems = {{}}
            local function check(name, ok) if not ok then table.insert(problems, name) end end

            -- sign, verify, reject
            local sig = must(pair:sign("attached message"))
            check("verifies", public:verify("attached message", sig))
            check("tampered message", not public:verify("tampered", sig))
            check("garbage signature", not public:verify("attached message", "not a real signature"))
            check("empty signature", not public:verify("attached message", ""))
            local other = must(pkey.KeyPair.generate(algo))
            check("foreign key", not other:public_key():verify("attached message", sig))

            -- empty and binary messages
            check("empty message", public:verify("", must(pair:sign(""))))
            local binary = "\0\1\2\254\255\128"
            check("binary message", public:verify(binary, must(pair:sign(binary))))

            -- private key: PEM and DER
            local pem = must(pair:to_pem())
            check("pkcs8 header", pem:match("^%-%-%-%-%-BEGIN PRIVATE KEY%-%-%-%-%-") ~= nil)
            check("lf endings", not pem:find("\r"))
            check("reloaded pem signs", public:verify("msg", must(must(pkey.KeyPair.from_pem(algo, pem)):sign("msg"))))
            local der = must(pair:to_der())
            check("reloaded der signs", public:verify("msg", must(must(pkey.KeyPair.from_der(algo, der)):sign("msg"))))

            -- public key: PEM and DER
            local public_pem = must(public:to_pem())
            check("spki header", public_pem:match("^%-%-%-%-%-BEGIN PUBLIC KEY%-%-%-%-%-") ~= nil)
            check("reloaded public pem", must(pkey.PublicKey.from_pem(algo, public_pem)):verify("attached message", sig))
            check("reloaded public der", must(pkey.PublicKey.from_der(algo, must(public:to_der()))):verify("attached message", sig))

            -- names
            check("algorithm fields", pair.algorithm == algo and public.algorithm == algo)
            check("tostring", tostring(pair) == "KeyPair(" .. algo .. ")" and tostring(public) == "PublicKey(" .. algo .. ")")

            return table.concat(problems, ", ")
            "#
        ));
        assert!(report.is_empty(), "{algo}: failed: {report}");
    }

    #[test]
    fn ed25519_works() {
        exercise("ed25519");
    }

    #[test]
    fn ecdsa_p256_works() {
        exercise("ecdsa-p256");
    }

    #[test]
    fn rsa_pss_works() {
        exercise("rsa-pss-sha256");
    }

    #[test]
    fn rsa_pkcs1v15_works() {
        exercise("rsa-pkcs1v15-sha256");
    }

    #[test]
    fn ed25519_signature_is_64_bytes() {
        assert_eq!(eval::<i64>(r#"return #(assert(pkey.KeyPair.generate("ed25519")):sign("msg"))"#), 64);
    }

    #[test]
    fn ecdsa_signature_is_fixed_64_bytes() {
        // Fixed r||s (JOSE ES256), not variable-length DER.
        assert_eq!(eval::<i64>(r#"return #(assert(pkey.KeyPair.generate("ecdsa-p256")):sign("msg"))"#), 64);
    }

    #[test]
    fn an_unknown_algorithm_is_refused_with_its_place() {
        let err = engine().load(r#"return pkey.KeyPair.generate("rsa-1024")"#).eval::<mlua::Value>().unwrap_err();
        let err = err.to_string();
        assert!(err.contains("lib:pkey.KeyPair.generate: argument #1 'algorithm'"), "{err}");
        assert!(err.contains("rsa-1024"), "{err}");
    }

    #[test]
    fn from_pem_with_the_wrong_algorithm_is_a_typed_error() {
        // An Ed25519 PKCS#8 key loaded as ECDSA fails on the OID, not silently misread.
        let (kind, op, text): (String, String, String) = eval(
            r#"
            local pem = assert((assert(pkey.KeyPair.generate("ed25519")):to_pem()))
            local value, e = pkey.KeyPair.from_pem("ecdsa-p256", pem)
            assert(value == nil)
            return e.kind, e.op, tostring(e)
            "#,
        );
        assert_eq!((kind.as_str(), op.as_str()), ("invalid_key", "lib:pkey.KeyPair.from_pem"));
        assert!(text.starts_with("lib:pkey.KeyPair.from_pem: invalid_key:"), "{text}");
    }

    #[test]
    fn malformed_input_is_a_typed_error() {
        let (a, b, c, d): (String, String, String, String) = eval(
            r#"
            local _, pem = pkey.KeyPair.from_pem("ed25519", "not pem")
            local _, der = pkey.KeyPair.from_der("ed25519", "\1\2\3")
            local _, public_pem = pkey.PublicKey.from_pem("ed25519", "not pem")
            local _, public_der = pkey.PublicKey.from_der("rsa-pss-sha256", "\1\2\3")
            return pem.kind .. pem.op, der.kind .. der.op, public_pem.kind .. public_pem.op, public_der.kind .. public_der.op
            "#,
        );
        assert_eq!(a, "invalid_keylib:pkey.KeyPair.from_pem");
        assert_eq!(b, "invalid_keylib:pkey.KeyPair.from_der");
        assert_eq!(c, "invalid_keylib:pkey.PublicKey.from_pem");
        assert_eq!(d, "invalid_keylib:pkey.PublicKey.from_der");
    }

    #[test]
    fn algorithms_list_is_complete() {
        assert_eq!(
            eval::<Vec<String>>("return pkey.algorithms()"),
            ["ed25519", "ecdsa-p256", "rsa-pss-sha256", "rsa-pkcs1v15-sha256"]
        );
    }
}

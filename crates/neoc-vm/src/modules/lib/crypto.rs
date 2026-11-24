//! Authenticated symmetric encryption (AEAD): `encrypt` and `decrypt`, over `aes-gcm` and `chacha20poly1305`.
//!
//! AEAD only, by design: every algorithm here produces a ciphertext that is also a MAC, and `decrypt` returns an
//! error rather than plaintext when the tag does not verify. There is no raw CBC or CTR.
//!
//! The nonce is the caller's, never derived: GCM and ChaCha20-Poly1305 are catastrophic under nonce reuse, and
//! the way to be sure a script thinks about it is to make it pass one. `lib:random`'s `random_bytes(nonce_size(algorithm))`
//! makes a fresh one. The ciphertext is the encrypted message followed by the 16-byte tag.
//!
//! A failed tag check is one generic error (`decryption_failed`): which of the key, the nonce, the AAD or the
//! bytes was wrong is the oracle an attacker wants. Spans never see keys, nonces or plaintext.
//!
//! The algorithm is a declared union (an unknown name is a bad argument), `key_size`, `nonce_size` and `tag_size`
//! cannot fail, and every failure is a typed error table.

use aes_gcm::aead::{Aead, Nonce, Payload};
use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;

/// All three AEADs use a 96-bit nonce and a 128-bit tag (RFC 5288 for GCM, RFC 8439 for ChaCha20-Poly1305).
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// The three AEADs share one 96-bit nonce type; `Aes128Gcm` stands in for it.
type AeadNonce = Nonce<Aes128Gcm>;

/// A keyed cipher, one variant per backend. Built once per call; the `Aead` trait gives all three the same shape.
// AES-256's key schedule dominates the size; a per-call value on the stack beats an allocation per call.
#[allow(clippy::large_enum_variant)]
enum Cipher {
    Aes128Gcm(Aes128Gcm),
    Aes256Gcm(Aes256Gcm),
    ChaCha20Poly1305(ChaCha20Poly1305),
}

impl Cipher {
    fn new(algorithm: crypto::Algorithm, key: &[u8]) -> Result<Self, crypto::CryptoError> {
        let wrong = || crypto::CryptoError::InvalidKeyLength {
            expected: algorithm.key_len(),
            got: u32::try_from(key.len()).unwrap_or(u32::MAX),
        };
        match algorithm {
            crypto::Algorithm::Aes128Gcm => {
                Aes128Gcm::new_from_slice(key).map(Self::Aes128Gcm).map_err(|_| wrong())
            }
            crypto::Algorithm::Aes256Gcm => {
                Aes256Gcm::new_from_slice(key).map(Self::Aes256Gcm).map_err(|_| wrong())
            }
            crypto::Algorithm::Chacha20Poly1305 => {
                ChaCha20Poly1305::new_from_slice(key).map(Self::ChaCha20Poly1305).map_err(|_| wrong())
            }
        }
    }

    fn nonce(nonce: &[u8]) -> Result<AeadNonce, crypto::CryptoError> {
        AeadNonce::try_from(nonce).map_err(|_| crypto::CryptoError::InvalidNonceLength {
            expected: NONCE_LEN as u32,
            got: u32::try_from(nonce.len()).unwrap_or(u32::MAX),
        })
    }

    fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, crypto::CryptoError> {
        let nonce = Self::nonce(nonce)?;
        let payload = Payload { msg: plaintext, aad };
        // Fails only on internal buffer allocation (16 + len > usize::MAX): unreachable in practice, but an
        // error rather than a panic.
        let out = match self {
            Self::Aes128Gcm(c) => c.encrypt(&nonce, payload),
            Self::Aes256Gcm(c) => c.encrypt(&nonce, payload),
            Self::ChaCha20Poly1305(c) => c.encrypt(&nonce, payload),
        };
        out.map_err(|e| crypto::CryptoError::EncryptionFailed { reason: e.to_string() })
    }

    fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, crypto::CryptoError> {
        let nonce = Self::nonce(nonce)?;
        if ciphertext.len() < TAG_LEN {
            return Err(crypto::CryptoError::CiphertextTooShort {
                minimum: TAG_LEN as u32,
                got: u32::try_from(ciphertext.len()).unwrap_or(u32::MAX),
            });
        }
        let payload = Payload { msg: ciphertext, aad };
        let out = match self {
            Self::Aes128Gcm(c) => c.decrypt(&nonce, payload),
            Self::Aes256Gcm(c) => c.decrypt(&nonce, payload),
            Self::ChaCha20Poly1305(c) => c.decrypt(&nonce, payload),
        };
        // Generic on purpose: "tag mismatch" against "AAD wrong" is the canonical AEAD oracle.
        out.map_err(|_| crypto::CryptoError::DecryptionFailed)
    }
}

#[mlua_ir::module("lib:crypto")]
pub mod crypto {
    use mlua_ir::{Bytes, LuaError, Shape};
    use serde::{Deserialize, Serialize};

    use super::{Cipher, NONCE_LEN, TAG_LEN};

    /// An AEAD cipher.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Algorithm {
        /// AES-128 in Galois/Counter Mode (16-byte key).
        #[serde(rename = "aes_128_gcm")]
        Aes128Gcm,
        /// AES-256 in Galois/Counter Mode (32-byte key).
        #[serde(rename = "aes_256_gcm")]
        Aes256Gcm,
        /// ChaCha20-Poly1305, RFC 8439 (32-byte key).
        #[serde(rename = "chacha20_poly1305")]
        Chacha20Poly1305,
    }

    impl Algorithm {
        pub(super) fn key_len(self) -> u32 {
            match self {
                Self::Aes128Gcm => 16,
                Self::Aes256Gcm | Self::Chacha20Poly1305 => 32,
            }
        }
    }

    /// Why a message could not be encrypted or decrypted.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CryptoError {
        /// The key is not the length the algorithm takes (`key_size`).
        #[error("expected {expected}-byte key, got {got}")]
        InvalidKeyLength {
            /// The length the algorithm takes.
            expected: u32,
            /// The length given.
            got: u32,
        },
        /// The nonce is not 12 bytes (`nonce_size`).
        #[error("expected {expected}-byte nonce, got {got}")]
        InvalidNonceLength {
            /// The length every algorithm takes.
            expected: u32,
            /// The length given.
            got: u32,
        },
        /// The ciphertext is shorter than the authentication tag, so it cannot be one.
        #[error("ciphertext shorter than the {minimum}-byte tag (got {got})")]
        CiphertextTooShort {
            /// The tag length.
            minimum: u32,
            /// The length given.
            got: u32,
        },
        /// The tag did not verify: the key, the nonce, the AAD or the ciphertext is not the one used to encrypt.
        /// Which one is deliberately not said.
        #[error("decryption failed")]
        DecryptionFailed,
        /// The backend refused to encrypt (in practice, a message too long for memory).
        #[error("encryption failed: {reason}")]
        EncryptionFailed {
            /// What the backend said.
            reason: String,
        },
    }

    /// Encrypt `plaintext` and authenticate it together with the optional `aad` (data that is sent in the clear and
    /// must match at decrypt time). The result is the encrypted message followed by the 16-byte tag. Leaving `aad`
    /// out is the same as an empty one.
    ///
    /// ```luau
    /// local random = require("lib:random")
    /// local key = assert(random.random_bytes(crypto.key_size("aes_256_gcm")))
    /// local nonce = assert(random.random_bytes(crypto.nonce_size("aes_256_gcm")))
    /// local sealed = assert(crypto.encrypt("aes_256_gcm", key, nonce, "attack at dawn", "header"))
    /// assert(assert(crypto.decrypt("aes_256_gcm", key, nonce, sealed, "header")) == "attack at dawn")
    /// local _, err = crypto.decrypt("aes_256_gcm", key, nonce, sealed, "other header")
    /// assert(err ~= nil and err.kind == "decryption_failed")
    /// ```
    pub fn encrypt(
        algorithm: Algorithm,
        key: Bytes,
        nonce: Bytes,
        plaintext: Bytes,
        aad: Option<Bytes>,
    ) -> Result<Bytes, CryptoError> {
        let cipher = Cipher::new(algorithm, &key)?;
        let aad = aad.unwrap_or_default();
        cipher.encrypt(&nonce, &plaintext, &aad).map(Bytes)
    }

    /// Decrypt `ciphertext` (with its trailing tag), requiring `aad` to be the value bound when it was encrypted.
    pub fn decrypt(
        algorithm: Algorithm,
        key: Bytes,
        nonce: Bytes,
        ciphertext: Bytes,
        aad: Option<Bytes>,
    ) -> Result<Bytes, CryptoError> {
        let cipher = Cipher::new(algorithm, &key)?;
        let aad = aad.unwrap_or_default();
        cipher.decrypt(&nonce, &ciphertext, &aad).map(Bytes)
    }

    /// Every supported algorithm.
    pub fn algorithms() -> Vec<Algorithm> {
        vec![Algorithm::Aes128Gcm, Algorithm::Aes256Gcm, Algorithm::Chacha20Poly1305]
    }

    /// The key length `algorithm` takes, in bytes.
    pub fn key_size(algorithm: Algorithm) -> u32 {
        algorithm.key_len()
    }

    /// The nonce length `algorithm` takes, in bytes.
    pub fn nonce_size(algorithm: Algorithm) -> u32 {
        let _ = algorithm;
        NONCE_LEN as u32
    }

    /// The length of the authentication tag `encrypt` appends to the ciphertext, in bytes.
    pub fn tag_size(algorithm: Algorithm) -> u32 {
        let _ = algorithm;
        TAG_LEN as u32
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:crypto")"#).eval().unwrap();
        lua.globals().set("crypto", m).unwrap();
        let hex: mlua::Table = lua.load(r#"return require("lib:hex")"#).eval().unwrap();
        lua.globals().set("hex", hex).unwrap();
        // `hex.decode` returns (bytes, nil): one value, so it can sit in the middle of an argument list
        lua.load(r#"function unhex(text) return (hex.decode(text)) end"#).exec().unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    /// `(kind, op, tostring)` of the typed error `code` returns as its second value.
    fn failure(code: &str) -> (String, String, String) {
        let lua = engine();
        let (value, err): (mlua::Value, mlua::Table) = lua.load(code).eval().unwrap();
        assert!(value.is_nil(), "{code} succeeded");
        let text: String = lua.load("return tostring(...)").call(err.clone()).unwrap();
        (err.get("kind").unwrap(), err.get("op").unwrap(), text)
    }

    const ALGOS: [&str; 3] = ["aes_128_gcm", "aes_256_gcm", "chacha20_poly1305"];

    /// One known answer per algorithm, AAD included: guards against a crate update changing the construction.
    /// NIST GCM test case 4 for AES-128 and AES-256, RFC 8439 section 2.8.2 for ChaCha20-Poly1305.
    #[test]
    fn encrypt_known_vectors() {
        let cases: [(&str, &str, &str, &str, &str, &str); 3] = [
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
        for (algo, key, nonce, plaintext, aad, expected) in cases {
            let sealed: String = eval(&format!(
                r#"
                local sealed = assert(crypto.encrypt("{algo}", unhex("{key}"), unhex("{nonce}"),
                    unhex("{plaintext}"), unhex("{aad}")))
                return hex.encode(sealed)
                "#
            ));
            assert_eq!(sealed, expected, "{algo}");
            let opened: String = eval(&format!(
                r#"
                local opened = assert(crypto.decrypt("{algo}", unhex("{key}"), unhex("{nonce}"),
                    unhex("{expected}"), unhex("{aad}")))
                return hex.encode(opened)
                "#
            ));
            assert_eq!(opened, plaintext, "{algo}");
        }
    }

    #[test]
    fn round_trip_matches_plaintext() {
        for algo in ALGOS {
            let ok: bool = eval(&format!(
                r#"
                local key = ("k"):rep(crypto.key_size("{algo}"))
                local nonce = ("n"):rep(crypto.nonce_size("{algo}"))
                local ct = assert(crypto.encrypt("{algo}", key, nonce, "hello, world"))
                local out, err = crypto.decrypt("{algo}", key, nonce, ct)
                return err == nil and out == "hello, world"
                "#
            ));
            assert!(ok, "{algo}");
        }
    }

    #[test]
    fn aad_mismatch_fails_decrypt_with_the_generic_error() {
        let (kind, op, text) = failure(
            r#"
            local key, nonce = ("k"):rep(32), ("n"):rep(12)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", "header-a"))
            return crypto.decrypt("aes_256_gcm", key, nonce, ct, "header-b")
            "#,
        );
        assert_eq!((kind.as_str(), op.as_str()), ("decryption_failed", "lib:crypto.decrypt"));
        assert!(text.starts_with("lib:crypto.decrypt: decryption_failed:"), "{text}");
    }

    #[test]
    fn tampered_ciphertext_fails_decrypt() {
        let (kind, _, _) = failure(
            r#"
            local key, nonce = ("k"):rep(32), ("n"):rep(12)
            local ct = assert(crypto.encrypt("chacha20_poly1305", key, nonce, "msg"))
            local tampered = string.char(bit32.bxor(string.byte(ct, 1), 1)) .. string.sub(ct, 2)
            return crypto.decrypt("chacha20_poly1305", key, nonce, tampered)
            "#,
        );
        assert_eq!(kind, "decryption_failed");
    }

    #[test]
    fn wrong_key_fails_decrypt() {
        let (kind, _, _) = failure(
            r#"
            local nonce = ("n"):rep(12)
            local ct = assert(crypto.encrypt("aes_256_gcm", ("a"):rep(32), nonce, "msg"))
            return crypto.decrypt("aes_256_gcm", ("b"):rep(32), nonce, ct)
            "#,
        );
        assert_eq!(kind, "decryption_failed");
    }

    #[test]
    fn omitted_aad_matches_empty_aad() {
        let ok: bool = eval(
            r#"
            local key, nonce = ("k"):rep(32), ("n"):rep(12)
            local ct1 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg"))
            local ct2 = assert(crypto.encrypt("aes_256_gcm", key, nonce, "msg", ""))
            return ct1 == ct2
                and assert(crypto.decrypt("aes_256_gcm", key, nonce, ct2)) == "msg"
                and assert(crypto.decrypt("aes_256_gcm", key, nonce, ct1, "")) == "msg"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn wrong_key_length_is_a_typed_error() {
        let (kind, op, text) =
            failure(r#"return crypto.encrypt("aes_256_gcm", "short", ("n"):rep(12), "msg")"#);
        assert_eq!((kind.as_str(), op.as_str()), ("invalid_key_length", "lib:crypto.encrypt"));
        assert!(text.contains("expected 32-byte key, got 5"), "{text}");
        let (expected, got): (u32, u32) = eval(
            r#"local _, e = crypto.encrypt("aes_256_gcm", "short", ("n"):rep(12), "msg") return e.expected, e.got"#,
        );
        assert_eq!((expected, got), (32, 5));
    }

    #[test]
    fn wrong_nonce_length_is_a_typed_error() {
        let (kind, _, text) =
            failure(r#"return crypto.encrypt("aes_256_gcm", ("k"):rep(32), "short", "msg")"#);
        assert_eq!(kind, "invalid_nonce_length");
        assert!(text.contains("expected 12-byte nonce, got 5"), "{text}");
        let (kind, _, _) =
            failure(r#"return crypto.decrypt("aes_256_gcm", ("k"):rep(32), "short", ("c"):rep(20))"#);
        assert_eq!(kind, "invalid_nonce_length");
    }

    #[test]
    fn truncated_ciphertext_names_the_real_cause() {
        // Shorter than the tag can never be valid; it is rejected before the backend, so the error is not the
        // generic decryption_failed.
        let (kind, _, text) =
            failure(r#"return crypto.decrypt("aes_256_gcm", ("k"):rep(32), ("n"):rep(12), "short")"#);
        assert_eq!(kind, "ciphertext_too_short");
        assert!(text.contains("shorter than the 16-byte tag (got 5)"), "{text}");
    }

    #[test]
    fn an_unknown_algorithm_is_refused_with_its_place() {
        let lua = engine();
        for (code, place) in [
            (
                r#"return crypto.encrypt("aes_512_gcm", "", "", "")"#,
                "lib:crypto.encrypt: argument #1 'algorithm'",
            ),
            (r#"return crypto.key_size("aes_512_gcm")"#, "lib:crypto.key_size: argument #1 'algorithm'"),
            (r#"return crypto.nonce_size("aes_512_gcm")"#, "argument #1 'algorithm'"),
            (r#"return crypto.tag_size("aes_512_gcm")"#, "argument #1 'algorithm'"),
        ] {
            let err = lua.load(code).eval::<mlua::Value>().unwrap_err().to_string();
            assert!(err.contains(place) && err.contains("aes_512_gcm"), "{err}");
        }
    }

    #[test]
    fn empty_plaintext_round_trips_as_a_tag_only_ciphertext() {
        let (len, ok): (i64, bool) = eval(
            r#"
            local key, nonce = ("k"):rep(32), ("n"):rep(12)
            local ct = assert(crypto.encrypt("aes_256_gcm", key, nonce, ""))
            return #ct, crypto.decrypt("aes_256_gcm", key, nonce, ct) == ""
            "#,
        );
        assert_eq!(len, 16);
        assert!(ok);
    }

    #[test]
    fn sizes_match_canonical_values() {
        let (keys, nonces, tags): (Vec<u32>, Vec<u32>, Vec<u32>) = eval(
            r#"
            local keys, nonces, tags = {}, {}, {}
            for _, a in crypto.algorithms() do
                table.insert(keys, crypto.key_size(a))
                table.insert(nonces, crypto.nonce_size(a))
                table.insert(tags, crypto.tag_size(a))
            end
            return keys, nonces, tags
            "#,
        );
        assert_eq!(keys, vec![16, 32, 32]);
        assert_eq!(nonces, vec![12; 3]);
        assert_eq!(tags, vec![16; 3]);
    }

    #[test]
    fn algorithms_list_is_complete() {
        assert_eq!(eval::<Vec<String>>("return crypto.algorithms()"), ALGOS);
    }
}

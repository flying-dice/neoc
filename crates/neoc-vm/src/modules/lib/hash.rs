//! Cryptographic and non-cryptographic hashing: one-shot and streaming.
//!
//! Output is raw bytes; `lib:hex` and `lib:base64` turn it into text. `xxh3` and `xxh64` produce a 64-bit
//! integer, serialised big-endian, which is what other languages' `digest()` returns and what a hex dump shows.

use blake3::Hasher as Blake3Hasher;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};
use sha3::{Sha3_256, Sha3_512};
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

/// A hash function's running state, one variant per backend. An enum rather than `dyn DynDigest`, because
/// `xxhash-rust` does not implement the `digest` traits and `blake3` only behind a feature.
// Variants differ in size by about 2 KiB; the state lives behind one allocation in the userdata either way.
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
    fn new(algorithm: hash::Algorithm) -> Self {
        use hash::Algorithm as A;
        match algorithm {
            A::Sha256 => Self::Sha256(Sha256::new()),
            A::Sha384 => Self::Sha384(Sha384::new()),
            A::Sha512 => Self::Sha512(Sha512::new()),
            A::Sha3_256 => Self::Sha3_256(Sha3_256::new()),
            A::Sha3_512 => Self::Sha3_512(Sha3_512::new()),
            A::Blake3 => Self::Blake3(Blake3Hasher::new()),
            A::Xxh3 => Self::Xxh3(Xxh3::new()),
            A::Xxh64 => Self::Xxh64(Xxh64::new(0)),
            A::Md5 => Self::Md5(Md5::new()),
            A::Sha1 => Self::Sha1(Sha1::new()),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(h) => Digest::update(h, data),
            Self::Sha384(h) => Digest::update(h, data),
            Self::Sha512(h) => Digest::update(h, data),
            Self::Sha3_256(h) => Digest::update(h, data),
            Self::Sha3_512(h) => Digest::update(h, data),
            Self::Blake3(h) => {
                h.update(data);
            }
            Self::Xxh3(h) => h.update(data),
            Self::Xxh64(h) => h.update(data),
            Self::Md5(h) => Digest::update(h, data),
            Self::Sha1(h) => Digest::update(h, data),
        }
    }

    /// The digest so far; the state is cloned first, so the hasher stays usable.
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
}

#[mlua_ir::module("lib:hash")]
pub mod hash {
    use std::sync::Arc;

    use mlua_ir::{Bytes, Object, Shape};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};

    use super::State;

    /// A hash function.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Algorithm {
        /// SHA-256 (32 bytes).
        Sha256,
        /// SHA-384 (48 bytes).
        Sha384,
        /// SHA-512 (64 bytes).
        Sha512,
        /// SHA3-256 (32 bytes).
        #[serde(rename = "sha3_256")]
        Sha3_256,
        /// SHA3-512 (64 bytes).
        #[serde(rename = "sha3_512")]
        Sha3_512,
        /// BLAKE3 (32 bytes).
        Blake3,
        /// XXH3, 64-bit, not cryptographic (8 bytes).
        Xxh3,
        /// XXH64 with seed 0, not cryptographic (8 bytes).
        Xxh64,
        /// MD5 (16 bytes). Broken; for interop only.
        Md5,
        /// SHA-1 (20 bytes). Broken; for interop only.
        Sha1,
    }

    impl Algorithm {
        /// Every algorithm: cryptographic first, then non-cryptographic, then the broken ones.
        pub const ALL: [Algorithm; 10] = [
            Self::Sha256,
            Self::Sha384,
            Self::Sha512,
            Self::Sha3_256,
            Self::Sha3_512,
            Self::Blake3,
            Self::Xxh3,
            Self::Xxh64,
            Self::Md5,
            Self::Sha1,
        ];
    }

    /// A running hash. `digest` does not consume it: call it any time, and keep calling `update` after.
    /// Clones share one state.
    ///
    /// ```luau
    /// local h = hash.Hasher.new("sha256")
    /// h:update("hello "):update("world")
    /// assert(h:digest() == hash.hash("sha256", "hello world"))
    /// ```
    #[derive(Clone, Object)]
    pub struct Hasher {
        algorithm: Algorithm,
        state: Arc<Mutex<State>>,
    }

    #[mlua_ir::object]
    impl Hasher {
        /// Start an incremental hash.
        pub fn new(algorithm: Algorithm) -> Hasher {
            Hasher { algorithm, state: Arc::new(Mutex::new(State::new(algorithm))) }
        }

        /// Feed `data` into the digest; the hasher itself, to chain calls.
        pub fn update(&self, data: Bytes) -> Hasher {
            self.state.lock().update(&data);
            self.clone()
        }

        /// The digest of everything fed so far, as raw bytes.
        pub fn digest(&self) -> Bytes {
            Bytes(self.state.lock().digest())
        }

        /// The algorithm this hasher runs.
        #[get]
        pub fn algorithm(&self) -> Algorithm {
            self.algorithm
        }

        /// `Hasher(sha256)`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("Hasher({})", algorithm_name(self.algorithm))
        }
    }

    fn algorithm_name(algorithm: Algorithm) -> String {
        serde_json::to_value(algorithm).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
    }

    /// The digest of `data` under `algorithm`, as raw bytes.
    ///
    /// ```luau
    /// local hex = require("lib:hex")
    /// assert(hex.encode(hash.hash("md5", "abc")) == "900150983cd24fb0d6963f7d28e17f72")
    /// ```
    pub fn hash(algorithm: Algorithm, data: Bytes) -> Bytes {
        let mut state = State::new(algorithm);
        state.update(&data);
        Bytes(state.digest())
    }

    /// Every supported algorithm.
    pub fn algorithms() -> Vec<Algorithm> {
        Algorithm::ALL.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:hash")"#).eval().unwrap();
        lua.globals().set("hash", m).unwrap();
        lua
    }

    fn hex_of(code: &str) -> String {
        let lua = engine();
        let s: mlua::LuaString = lua.load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"));
        s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    const ALGOS: [&str; 10] = ["sha256", "sha384", "sha512", "sha3_256", "sha3_512", "blake3", "xxh3", "xxh64", "md5", "sha1"];

    /// One known vector per algorithm: guards against a crate update changing seeds or parameters.
    #[test]
    fn one_shot_known_vectors() {
        let cases: &[(&str, &str, &str)] = &[
            ("sha256", "abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
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
            ("sha3_256", "abc", "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"),
            (
                "sha3_512",
                "abc",
                "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0",
            ),
            ("blake3", "abc", "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"),
            ("md5", "abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("sha1", "abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
        ];
        for (algo, input, expected) in cases {
            assert_eq!(hex_of(&format!(r#"return hash.hash("{algo}", "{input}")"#)), *expected, "{algo}");
        }
    }

    #[test]
    fn xxh3_known_vector() {
        assert_eq!(hex_of(r#"return hash.hash("xxh3", "")"#), "2d06800538d394c2");
    }

    #[test]
    fn xxh64_known_vector() {
        assert_eq!(hex_of(r#"return hash.hash("xxh64", "")"#), "ef46db3751d8e999");
    }

    #[test]
    fn streaming_matches_one_shot() {
        for algo in ALGOS {
            let streamed = hex_of(&format!(
                r#"local h = hash.Hasher.new("{algo}") h:update("hello ") h:update("world") return h:digest()"#
            ));
            assert_eq!(streamed, hex_of(&format!(r#"return hash.hash("{algo}", "hello world")"#)), "{algo}");
        }
    }

    #[test]
    fn empty_input_produces_digest() {
        for algo in ALGOS {
            assert!(!hex_of(&format!(r#"return hash.hash("{algo}", "")"#)).is_empty(), "{algo}");
        }
    }

    #[test]
    fn digest_is_idempotent() {
        let lua = engine();
        let (a, b): (mlua::LuaString, mlua::LuaString) = lua
            .load(r#"local h = hash.Hasher.new("sha256") h:update("hello") return h:digest(), h:digest()"#)
            .eval()
            .unwrap();
        assert_eq!(&*a.as_bytes(), &*b.as_bytes());
    }

    #[test]
    fn update_after_digest_extends_state() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local h = hash.Hasher.new("sha256")
                h:update("hello ")
                local _ = h:digest() -- does not freeze the state
                h:update("world")
                return h:digest() == hash.hash("sha256", "hello world")
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn update_chains() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local h = hash.Hasher.new("blake3")
                h:update("a"):update("b")
                return h:digest() == hash.hash("blake3", "ab")
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn binary_input() {
        // SHA-256 of bytes [0x00 0x01 0x02 0xfe 0xff] (verified against sha256sum).
        assert_eq!(
            hex_of(r#"return hash.hash("sha256", "\0\1\2\254\255")"#),
            "103597c5abb6113da596c18e9d1da69364eafe00a2bfaa8b12e53c44bd6b0429"
        );
    }

    #[test]
    fn an_unknown_algorithm_is_refused_with_its_place() {
        let lua = engine();
        for code in [r#"return hash.hash("md4", "x")"#, r#"return hash.Hasher.new("md4")"#] {
            let err = lua.load(code).eval::<mlua::Value>().unwrap_err().to_string();
            assert!(err.contains("argument #1 'algorithm'"), "{err}");
            assert!(err.contains("md4"), "{err}");
        }
    }

    #[test]
    fn algorithms_list_is_complete() {
        let lua = engine();
        let algos: Vec<String> = lua.load("return hash.algorithms()").eval().unwrap();
        assert_eq!(algos, ALGOS);
    }

    #[test]
    fn tostring_and_algorithm_field() {
        let lua = engine();
        let (text, algo): (String, String) = lua
            .load(r#"local h = hash.Hasher.new("sha3_512") return tostring(h), h.algorithm"#)
            .eval()
            .unwrap();
        assert_eq!((text.as_str(), algo.as_str()), ("Hasher(sha3_512)", "sha3_512"));
    }

    #[test]
    fn large_input_streaming() {
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local h = hash.Hasher.new("sha256")
                for _ = 1, 256 do h:update(string.rep("x", 4096)) end
                return h:digest() == hash.hash("sha256", string.rep("x", 4096 * 256))
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }
}

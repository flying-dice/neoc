//! Keyed-hash message authentication codes over the SHA-2 digests, and the constant-time tag comparison
//! that is the reason this module exists: never compare tags with `==`.
//!
//! A key of any length is fine (HMAC hashes a long key and zero-pads a short one), so nothing here can fail.
//! SHA-1 and MD5 are out of scope: HMAC's security argument assumes a modern hash.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use ::hmac::{KeyInit, Mac};
use sha2::{Sha256, Sha384, Sha512};

/// A MAC's running state, one variant per digest.
enum State {
    Sha256(::hmac::Hmac<Sha256>),
    Sha384(::hmac::Hmac<Sha384>),
    Sha512(::hmac::Hmac<Sha512>),
}

impl State {
    fn new(algorithm: self::hmac::Algorithm, key: &[u8]) -> Self {
        // HMAC accepts a key of any length, so `new_from_slice` never fails.
        const ANY_KEY: &str = "HMAC accepts a key of any length";
        match algorithm {
            self::hmac::Algorithm::Sha256 => Self::Sha256(KeyInit::new_from_slice(key).expect(ANY_KEY)),
            self::hmac::Algorithm::Sha384 => Self::Sha384(KeyInit::new_from_slice(key).expect(ANY_KEY)),
            self::hmac::Algorithm::Sha512 => Self::Sha512(KeyInit::new_from_slice(key).expect(ANY_KEY)),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(m) => m.update(data),
            Self::Sha384(m) => m.update(data),
            Self::Sha512(m) => m.update(data),
        }
    }

    /// The tag so far; the state is cloned first, so the MAC stays usable.
    fn digest(&self) -> Vec<u8> {
        fn finalize<M: Mac + Clone>(mac: &M) -> Vec<u8> {
            mac.clone().finalize().into_bytes().to_vec()
        }
        match self {
            Self::Sha256(m) => finalize(m),
            Self::Sha384(m) => finalize(m),
            Self::Sha512(m) => finalize(m),
        }
    }
}

#[mlua_ir::module("lib:hmac")]
pub mod hmac {
    use std::sync::Arc;

    use mlua_ir::{Bytes, Object, Shape};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use subtle::ConstantTimeEq as _;

    use super::State;

    /// The digest a MAC is built on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Algorithm {
        /// HMAC-SHA-256 (32-byte tag).
        Sha256,
        /// HMAC-SHA-384 (48-byte tag).
        Sha384,
        /// HMAC-SHA-512 (64-byte tag).
        Sha512,
    }

    /// A running MAC. `digest` does not consume it: call it any time, and keep calling `update` after.
    /// Clones share one state. Spans never see the key or the data.
    ///
    /// ```luau
    /// local m = hmac.Hmac.new("sha256", "secret")
    /// m:update("hello "):update("world")
    /// assert(hmac.verify(m:digest(), hmac.hmac("sha256", "secret", "hello world")))
    /// ```
    #[derive(Clone, Object)]
    pub struct Hmac {
        algorithm: Algorithm,
        state: Arc<Mutex<State>>,
    }

    #[mlua_ir::object]
    impl Hmac {
        /// Start an incremental MAC under `key`.
        pub fn new(algorithm: Algorithm, key: Bytes) -> Hmac {
            Hmac { algorithm, state: Arc::new(Mutex::new(State::new(algorithm, &key))) }
        }

        /// Feed `data` into the MAC; the MAC itself, to chain calls.
        pub fn update(&self, data: Bytes) -> Hmac {
            self.state.lock().update(&data);
            self.clone()
        }

        /// The tag of everything fed so far, as raw bytes.
        pub fn digest(&self) -> Bytes {
            Bytes(self.state.lock().digest())
        }

        /// The algorithm this MAC runs.
        #[get]
        pub fn algorithm(&self) -> Algorithm {
            self.algorithm
        }

        /// `Hmac(sha256)`.
        #[to_string]
        pub fn display(&self) -> String {
            let name = serde_json::to_value(self.algorithm).ok().and_then(|v| v.as_str().map(str::to_owned));
            format!("Hmac({})", name.unwrap_or_default())
        }
    }

    /// The tag of `data` under `algorithm` and `key`, as raw bytes.
    ///
    /// ```luau
    /// local hex = require("lib:hex")
    /// local tag = hmac.hmac("sha256", "Jefe", "what do ya want for nothing?")
    /// assert(hex.encode(tag) == "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
    /// ```
    pub fn hmac(algorithm: Algorithm, key: Bytes, data: Bytes) -> Bytes {
        let mut state = State::new(algorithm, &key);
        state.update(&data);
        Bytes(state.digest())
    }

    /// Whether two tags are equal, compared in constant time: the time taken does not depend on where they differ.
    /// Tags of different lengths are not equal (a length is not secret).
    pub fn verify(a: Bytes, b: Bytes) -> bool {
        a.ct_eq(&b).into()
    }

    /// Every supported algorithm.
    pub fn algorithms() -> Vec<Algorithm> {
        vec![Algorithm::Sha256, Algorithm::Sha384, Algorithm::Sha512]
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:hmac")"#).eval().unwrap();
        lua.globals().set("hmac", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    fn hex_of(code: &str) -> String {
        let lua = engine();
        let s: mlua::LuaString = lua.load(code).eval().unwrap_or_else(|e| panic!("{code}: {e}"));
        s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    const ALGOS: [&str; 3] = ["sha256", "sha384", "sha512"];

    /// RFC 4231 test case 2 (key "Jefe"), one per digest.
    #[test]
    fn one_shot_rfc4231_vectors() {
        let cases = [
            ("sha256", "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"),
            (
                "sha384",
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
            ),
            (
                "sha512",
                "164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737",
            ),
        ];
        for (algo, expected) in cases {
            let code = format!(r#"return hmac.hmac("{algo}", "Jefe", "what do ya want for nothing?")"#);
            assert_eq!(hex_of(&code), expected, "{algo}");
        }
    }

    #[test]
    fn streaming_matches_one_shot() {
        for algo in ALGOS {
            let streamed = hex_of(&format!(
                r#"local m = hmac.Hmac.new("{algo}", "secret") m:update("hello ") m:update("world") return m:digest()"#
            ));
            assert_eq!(
                streamed,
                hex_of(&format!(r#"return hmac.hmac("{algo}", "secret", "hello world")"#)),
                "{algo}"
            );
        }
    }

    #[test]
    fn digest_is_idempotent() {
        assert!(eval::<bool>(
            r#"local m = hmac.Hmac.new("sha256", "k") m:update("hello") return m:digest() == m:digest()"#
        ));
    }

    #[test]
    fn update_after_digest_extends_state() {
        assert!(eval::<bool>(
            r#"
            local m = hmac.Hmac.new("sha256", "k")
            m:update("hello ")
            local _ = m:digest() -- does not freeze the state
            m:update("world")
            return m:digest() == hmac.hmac("sha256", "k", "hello world")
            "#
        ));
    }

    #[test]
    fn update_chains() {
        assert!(eval::<bool>(
            r#"
            local m = hmac.Hmac.new("sha256", "k")
            m:update("a"):update("b")
            return m:digest() == hmac.hmac("sha256", "k", "ab")
            "#
        ));
    }

    #[test]
    fn key_changes_tag() {
        assert!(eval::<bool>(
            r#"return hmac.hmac("sha256", "key-a", "msg") ~= hmac.hmac("sha256", "key-b", "msg")"#
        ));
    }

    #[test]
    fn empty_key_and_message_produce_a_full_width_tag() {
        assert_eq!(eval::<i64>(r#"return #hmac.hmac("sha256", "", "")"#), 32);
    }

    #[test]
    fn binary_key_and_data() {
        assert!(eval::<bool>(
            r#"
            local key, data = "\0\1\2\254\255", "\255\0\128"
            local m = hmac.Hmac.new("sha512", key)
            m:update(data)
            return m:digest() == hmac.hmac("sha512", key, data)
            "#
        ));
    }

    #[test]
    fn verify_accepts_matching_and_rejects_differing_tags() {
        let (same, differ, shorter, empty): (bool, bool, bool, bool) = eval(
            r#"
            local a = hmac.hmac("sha256", "k", "msg")
            return hmac.verify(a, hmac.hmac("sha256", "k", "msg")),
                hmac.verify(a, hmac.hmac("sha256", "k", "tampered")),
                hmac.verify("abc", "abcd"),
                hmac.verify("", "")
            "#,
        );
        assert!(same && !differ && !shorter && empty);
    }

    #[test]
    fn an_unsupported_algorithm_is_refused_with_its_place() {
        let lua = engine();
        for (code, place) in [
            (r#"return hmac.hmac("sha1", "k", "x")"#, "lib:hmac.hmac: argument #1 'algorithm'"),
            (r#"return hmac.Hmac.new("md5", "k")"#, "argument #1 'algorithm'"),
        ] {
            let err = lua.load(code).eval::<mlua::Value>().unwrap_err().to_string();
            assert!(err.contains(place), "{err}");
        }
    }

    #[test]
    fn algorithms_list_is_complete() {
        assert_eq!(eval::<Vec<String>>("return hmac.algorithms()"), ALGOS);
    }

    #[test]
    fn tostring_and_algorithm_field() {
        let (text, algo): (String, String) =
            eval(r#"local m = hmac.Hmac.new("sha512", "k") return tostring(m), m.algorithm"#);
        assert_eq!((text.as_str(), algo.as_str()), ("Hmac(sha512)", "sha512"));
    }

    #[test]
    fn large_input_streaming() {
        assert!(eval::<bool>(
            r#"
            local m = hmac.Hmac.new("sha256", "k")
            for _ = 1, 256 do m:update(string.rep("x", 4096)) end
            return m:digest() == hmac.hmac("sha256", "k", string.rep("x", 4096 * 256))
            "#
        ));
    }
}

//! base64 encode / decode (standard and URL-safe alphabets).

use ::base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

#[mlua_ir::module("lib:base64")]
pub mod base64 {
    use ::base64::Engine as _;
    use ::base64::engine::GeneralPurpose;
    use mlua_ir::{Bytes, LuaError};
    use serde::Serialize;

    use super::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    /// Why a string is not valid base64 in the alphabet and padding the function reads.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// A character that is not in the alphabet.
        #[error("invalid byte {byte} at offset {offset}")]
        InvalidByte {
            /// Where it is, counting from 0.
            offset: u32,
            /// Its value.
            byte: u8,
        },
        /// The number of characters cannot be base64 (one more than a multiple of 4, say).
        #[error("invalid input length {length}")]
        InvalidLength {
            /// The number of characters.
            length: u32,
        },
        /// The last character has bits set that the data does not use.
        #[error("invalid last symbol {byte} at offset {offset}")]
        InvalidLastSymbol {
            /// Where it is, counting from 0.
            offset: u32,
            /// Its value.
            byte: u8,
        },
        /// The `=` padding is missing, in excess, or where the alphabet has none.
        #[error("invalid padding")]
        InvalidPadding,
    }

    fn decode_with(engine: &GeneralPurpose, input: &[u8]) -> Result<Bytes, DecodeError> {
        use ::base64::DecodeError as E;
        let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        engine.decode(input).map(Bytes).map_err(|e| match e {
            E::InvalidByte(offset, byte) => DecodeError::InvalidByte { offset: wide(offset), byte },
            E::InvalidLength(length) => DecodeError::InvalidLength { length: wide(length) },
            E::InvalidLastSymbol { offset, symbol, .. } => {
                DecodeError::InvalidLastSymbol { offset: wide(offset), byte: symbol }
            }
            E::InvalidPadding => DecodeError::InvalidPadding,
        })
    }

    /// Encode `input` using the standard alphabet (A-Z, a-z, 0-9, +, /) with `=` padding.
    ///
    /// ```luau
    /// assert(base64.encode("hello") == "aGVsbG8=")
    /// ```
    pub fn encode(input: Bytes) -> String {
        STANDARD.encode(&*input)
    }

    /// Encode using the standard alphabet without `=` padding.
    pub fn encode_no_pad(input: Bytes) -> String {
        STANDARD_NO_PAD.encode(&*input)
    }

    /// Encode using the URL-safe alphabet (`-` and `_` instead of `+` and `/`), with `=` padding.
    pub fn encode_url_safe(input: Bytes) -> String {
        URL_SAFE.encode(&*input)
    }

    /// Encode using the URL-safe alphabet without padding.
    pub fn encode_url_safe_no_pad(input: Bytes) -> String {
        URL_SAFE_NO_PAD.encode(&*input)
    }

    /// Decode a standard-alphabet base64 string back to raw bytes.
    ///
    /// ```luau
    /// local data = assert(base64.decode("aGVsbG8="))
    /// assert(data == "hello")
    /// local _, err = base64.decode("not*base64!")
    /// assert(err ~= nil and err.kind == "invalid_byte")
    /// ```
    pub fn decode(input: Bytes) -> Result<Bytes, DecodeError> {
        decode_with(&STANDARD, &input)
    }

    /// Decode standard-alphabet base64 that has no padding.
    pub fn decode_no_pad(input: Bytes) -> Result<Bytes, DecodeError> {
        decode_with(&STANDARD_NO_PAD, &input)
    }

    /// Decode URL-safe base64 with padding.
    pub fn decode_url_safe(input: Bytes) -> Result<Bytes, DecodeError> {
        decode_with(&URL_SAFE, &input)
    }

    /// Decode URL-safe base64 without padding.
    pub fn decode_url_safe_no_pad(input: Bytes) -> Result<Bytes, DecodeError> {
        decode_with(&URL_SAFE_NO_PAD, &input)
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    /// Test-only engine. Binds `b64 = require("lib:base64")` as a global to
    /// keep the inline test bodies short — these tests validate base64
    /// behaviour, not require's plumbing (which has its own coverage).
    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:base64")"#).eval().unwrap();
        lua.globals().set("b64", m).unwrap();
        lua
    }

    #[test]
    fn encode_standard() {
        let lua = engine();
        let (s, err): (String, Option<String>) = lua.load(r#"return b64.encode("hello")"#).eval().unwrap();
        assert_eq!(s, "aGVsbG8=");
        assert!(err.is_none());
    }

    #[test]
    fn decode_standard() {
        let lua = engine();
        let (s, err): (String, Option<String>) = lua.load(r#"return b64.decode("aGVsbG8=")"#).eval().unwrap();
        assert_eq!(s, "hello");
        assert!(err.is_none());
    }

    #[test]
    fn round_trip_binary() {
        let lua = engine();
        let s: mlua::LuaString = lua
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
        let s: mlua::LuaString = lua
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

    /// The `(value, err)` a decode of `code` returns: `err` is a typed table, read as `(kind, op, tostring)`.
    fn failure(code: &str) -> (Option<String>, String, String, String) {
        let lua = engine();
        let (val, err): (Option<String>, mlua::Table) = lua.load(code).eval().unwrap();
        let text: String = lua.load("return tostring(...)").call(err.clone()).unwrap();
        (val, err.get("kind").unwrap(), err.get("op").unwrap(), text)
    }

    #[test]
    fn decode_invalid_returns_a_typed_error() {
        let (val, kind, op, text) = failure(r#"return b64.decode("not*base64!")"#);
        assert!(val.is_none());
        assert_eq!(kind, "invalid_byte");
        assert_eq!(op, "lib:base64.decode");
        assert!(text.starts_with("lib:base64.decode: invalid_byte:"), "{text}");
    }

    #[test]
    fn the_error_carries_where_it_is() {
        let lua = engine();
        let (offset, byte): (u32, u8) =
            lua.load(r#"local _, e = b64.decode("ab*d") return e.offset, e.byte"#).eval().unwrap();
        assert_eq!((offset, byte), (2, b'*'));
    }

    #[test]
    fn decode_no_pad_with_padded_input_errs() {
        let (val, _, op, _) = failure(r#"return b64.decode_no_pad("aGVsbG8=")"#);
        assert!(val.is_none());
        assert_eq!(op, "lib:base64.decode_no_pad");
    }

    #[test]
    fn a_non_string_argument_is_refused_with_its_place() {
        let lua = engine();
        let err = lua.load("return b64.encode(1)").eval::<mlua::Value>().unwrap_err().to_string();
        assert!(err.contains("lib:base64.encode: argument #1 'input': expected a string, got 1"), "{err}");
    }

    #[test]
    fn round_trip_url_safe_no_pad() {
        let lua = engine();
        let s: mlua::LuaString = lua
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

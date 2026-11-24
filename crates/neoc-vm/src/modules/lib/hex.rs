//! hex encode / decode.

#[mlua_ir::module("lib:hex")]
pub mod hex {
    use mlua_ir::{Bytes, LuaError};
    use serde::Serialize;

    /// Why a string is not valid hex.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// A character that is not `0`-`9`, `a`-`f` or `A`-`F`.
        #[error("invalid character {character:?} at offset {offset}")]
        InvalidCharacter {
            /// The character.
            character: String,
            /// Where it is in the input, counting from 0 (whitespace included).
            offset: u32,
        },
        /// The digits (whitespace aside) do not pair up into whole bytes.
        #[error("odd number of digits")]
        OddLength,
    }

    /// Encode `input` to lowercase hex: two characters per byte, from `0`-`9` and `a`-`f`.
    ///
    /// ```luau
    /// assert(hex.encode("\255\171") == "ffab")
    /// ```
    pub fn encode(input: Bytes) -> String {
        ::hex::encode(&*input)
    }

    /// Encode `input` to uppercase hex (`0`-`9` and `A`-`F`).
    pub fn encode_upper(input: Bytes) -> String {
        ::hex::encode_upper(&*input)
    }

    /// Decode hex back to raw bytes. Case-insensitive, and ASCII whitespace is skipped (so `"de ad be ef"` and
    /// wrapped dumps decode as they are); anything else that is not a hex digit is refused.
    ///
    /// ```luau
    /// assert(assert(hex.decode("de ad\tBE\nef")) == "\222\173\190\239")
    /// local _, err = hex.decode("abc")
    /// assert(err ~= nil and err.kind == "odd_length")
    /// ```
    pub fn decode(input: Bytes) -> Result<Bytes, DecodeError> {
        let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        // (offset in the input, byte) of everything that is not whitespace
        let kept: Vec<(usize, u8)> =
            input.iter().copied().enumerate().filter(|(_, b)| !b.is_ascii_whitespace()).collect();
        let digits: Vec<u8> = kept.iter().map(|&(_, b)| b).collect();
        ::hex::decode(&digits).map(Bytes).map_err(|e| match e {
            ::hex::FromHexError::InvalidHexCharacter { c, index } => DecodeError::InvalidCharacter {
                character: c.to_string(),
                offset: wide(kept.get(index).map_or(index, |&(at, _)| at)),
            },
            // `hex` raises this only for fixed-size targets, which `decode` to a Vec never is
            ::hex::FromHexError::OddLength | ::hex::FromHexError::InvalidStringLength => {
                DecodeError::OddLength
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:hex")"#).eval().unwrap();
        lua.globals().set("hex", m).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    fn bytes(code: &str) -> Vec<u8> {
        let lua = engine();
        let s: mlua::LuaString = lua.load(code).eval().unwrap();
        s.as_bytes().to_vec()
    }

    #[test]
    fn encode_lowercase() {
        assert_eq!(eval::<String>(r#"return hex.encode("\255\171")"#), "ffab");
    }

    #[test]
    fn encode_upper_uppercase() {
        assert_eq!(eval::<String>(r#"return hex.encode_upper("\255\171")"#), "FFAB");
    }

    #[test]
    fn encode_empty() {
        let (lower, upper): (String, String) = eval(r#"return hex.encode(""), hex.encode_upper("")"#);
        assert_eq!((lower.as_str(), upper.as_str()), ("", ""));
    }

    #[test]
    fn decode_known_value() {
        let (s, err): (String, Option<String>) = eval(r#"return hex.decode("6869")"#);
        assert_eq!(s, "hi");
        assert!(err.is_none());
    }

    #[test]
    fn decode_is_case_insensitive() {
        assert_eq!(bytes(r#"return (hex.decode("fFaB"))"#), &[255u8, 171]);
    }

    #[test]
    fn decode_skips_whitespace() {
        assert_eq!(bytes(r#"return (hex.decode("de ad\tbe\nef"))"#), &[0xde_u8, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn round_trip_binary() {
        assert_eq!(bytes(r#"return (hex.decode(hex.encode("\0\1\2\254\255")))"#), &[0u8, 1, 2, 254, 255]);
    }

    #[test]
    fn decode_empty_is_empty_bytes() {
        let (s, err): (String, Option<String>) = eval(r#"return hex.decode("")"#);
        assert_eq!(s, "");
        assert!(err.is_none());
    }

    #[test]
    fn decode_odd_length_is_a_typed_error() {
        let (val, kind, op, text): (Option<String>, String, String, String) =
            eval(r#"local v, e = hex.decode("abc") return v, e.kind, e.op, tostring(e)"#);
        assert!(val.is_none());
        assert_eq!((kind.as_str(), op.as_str()), ("odd_length", "lib:hex.decode"));
        assert!(text.starts_with("lib:hex.decode: odd_length:"), "{text}");
    }

    #[test]
    fn decode_non_hex_names_the_character_and_its_place() {
        // the offset counts the whitespace the decoder skipped
        let (kind, character, offset): (String, String, u32) =
            eval(r#"local _, e = hex.decode("ab  zz") return e.kind, e.character, e.offset"#);
        assert_eq!((kind.as_str(), character.as_str(), offset), ("invalid_character", "z", 4));
    }

    #[test]
    fn a_non_string_argument_is_refused_with_its_place() {
        let err = engine().load("return hex.encode(1)").eval::<mlua::Value>().unwrap_err().to_string();
        assert!(err.contains("lib:hex.encode: argument #1 'input': expected a string, got 1"), "{err}");
    }
}

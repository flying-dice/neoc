//! Compression and decompression of bytes: `lib:compress` (`flate2`, `zstd`, `brotli`).
//!
//! `encode(format, data, { level })` and `decode(format, data, { max_size })`, in memory. `decode` stops at
//! `max_size` bytes of output (256 MiB when not given), so a small input that expands without end (a
//! decompression bomb) is an error and not all the memory there is.
//!
//! The bytes `encode` writes are pinned by a test: a dependency upgrade that changes them is seen there first.
//!
//! The API's rules:
//! - The format is a string enum: one that does not exist is refused as an argument.
//! - `level` and `max_size` are whole numbers from 0 up: anything else is refused as an argument with its place. An
//!   option that does not exist is refused too.
//! - Failures are typed: `EncodeError` (`level_out_of_range`, `failed`) and `DecodeError` (`empty`, `max_size_zero`,
//!   `too_large`, `corrupt`).

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

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};

use compress::{DecodeError, EncodeError, Format};

/// What `decode` writes at most when `max_size` is not given.
const DEFAULT_MAX_SIZE: u64 = 256 * 1024 * 1024;

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Gzip => "gzip",
            Format::Zlib => "zlib",
            Format::Zstd => "zstd",
            Format::Brotli => "brotli",
        }
    }

    /// The least level, the greatest, and the one used when none is given.
    fn levels(self) -> (u32, u32, u32) {
        match self {
            Format::Gzip | Format::Zlib => (0, 9, 6),
            Format::Zstd => (1, 22, 3),
            Format::Brotli => (0, 11, 6),
        }
    }
}

fn encode(format: Format, data: &[u8], level: Option<u32>) -> Result<Vec<u8>, EncodeError> {
    let (min, max, default) = format.levels();
    let level = level.unwrap_or(default);
    if !(min..=max).contains(&level) {
        return Err(EncodeError::LevelOutOfRange { format, level, min, max });
    }
    let failed = |error: std::io::Error| EncodeError::Failed { reason: error.to_string() };
    match format {
        Format::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
            encoder.write_all(data).map_err(failed)?;
            encoder.finish().map_err(failed)
        }
        Format::Zlib => {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
            encoder.write_all(data).map_err(failed)?;
            encoder.finish().map_err(failed)
        }
        Format::Zstd => {
            // the level is 1 to 22: it fits
            let mut encoder =
                zstd::Encoder::new(Vec::new(), i32::try_from(level).unwrap_or(22)).map_err(failed)?;
            encoder.write_all(data).map_err(failed)?;
            encoder.finish().map_err(failed)
        }
        Format::Brotli => {
            let mut output = Vec::new();
            brotli::CompressorWriter::new(&mut output, 4096, level, 22).write_all(data).map_err(failed)?;
            Ok(output)
        }
    }
}

/// Everything `reader` gives, or `TooLarge` once it is more than `max_size` bytes.
fn read_with_limit(mut reader: impl Read, format: Format, max_size: u64) -> Result<Vec<u8>, DecodeError> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|error| DecodeError::Corrupt { format, reason: error.to_string() })?;
        if n == 0 {
            return Ok(output);
        }
        if (output.len() + n) as u64 > max_size {
            return Err(DecodeError::TooLarge { max_size });
        }
        output.extend_from_slice(&buffer[..n]);
    }
}

fn decode(format: Format, data: &[u8], max_size: Option<u64>) -> Result<Vec<u8>, DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::Empty);
    }
    let max_size = max_size.unwrap_or(DEFAULT_MAX_SIZE);
    if max_size == 0 {
        return Err(DecodeError::MaxSizeZero);
    }
    match format {
        Format::Gzip => read_with_limit(GzDecoder::new(data), format, max_size),
        Format::Zlib => read_with_limit(ZlibDecoder::new(data), format, max_size),
        Format::Zstd => {
            let decoder = zstd::Decoder::new(data)
                .map_err(|error| DecodeError::Corrupt { format, reason: error.to_string() })?;
            read_with_limit(decoder, format, max_size)
        }
        Format::Brotli => read_with_limit(brotli::Decompressor::new(data, 4096), format, max_size),
    }
}

#[mlua_ir::module("lib:compress")]
pub mod compress {
    use mlua_ir::{Bytes, LuaError, Options, Shape};
    use serde::{Deserialize, Serialize};

    /// A compression format.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Format {
        /// gzip (RFC 1952): levels 0 to 9, 6 when not given.
        Gzip,
        /// zlib (RFC 1950): levels 0 to 9, 6 when not given.
        Zlib,
        /// Zstandard: levels 1 to 22, 3 when not given.
        Zstd,
        /// Brotli: levels 0 to 11, 6 when not given.
        Brotli,
    }

    /// How `encode` compresses.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct EncodeOptions {
        /// How hard to compress: higher is smaller and slower. Its range and the level used when not given are
        /// the format's.
        pub level: Option<u32>,
    }

    /// How `decode` decompresses.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct DecodeOptions {
        /// The most bytes it writes; more is an error. When not given: 256 MiB.
        pub max_size: Option<u64>,
    }

    /// Why data was not compressed.
    #[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum EncodeError {
        /// The format has no such level.
        #[error("level out of range for {}: {level} (it is from {min} to {max})", .format.name())]
        LevelOutOfRange {
            /// The format asked for.
            format: Format,
            /// The level asked for.
            level: u32,
            /// The format's least level.
            min: u32,
            /// The format's greatest level.
            max: u32,
        },
        /// The encoder failed.
        #[error("{reason}")]
        Failed {
            /// What the encoder said.
            reason: String,
        },
    }

    /// Why data was not decompressed.
    #[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum DecodeError {
        /// There is no input: no format's stream is empty.
        #[error("empty input is not a compressed stream")]
        Empty,
        /// `max_size` is 0, which no output fits.
        #[error("max_size must be more than 0")]
        MaxSizeZero,
        /// The output is more than `max_size` bytes.
        #[error("output exceeds max_size ({max_size} bytes)")]
        TooLarge {
            /// The limit that was passed.
            max_size: u64,
        },
        /// The input is not the format's, or is damaged.
        #[error("not valid {}: {reason}", .format.name())]
        Corrupt {
            /// The format it was read as.
            format: Format,
            /// What the decoder said.
            reason: String,
        },
    }

    /// Compress `data`.
    ///
    /// ```luau
    /// local packed = assert(compress.encode("gzip", string.rep("abc", 1000), { level = 9 }))
    /// assert(#packed < 100)
    /// assert(compress.decode("gzip", packed) == string.rep("abc", 1000))
    /// ```
    pub fn encode(format: Format, data: Bytes, options: EncodeOptions) -> Result<Bytes, EncodeError> {
        super::encode(format, &data, options.level).map(Bytes)
    }

    /// Decompress `data`, which was compressed as `format`.
    ///
    /// ```luau
    /// local packed = assert(compress.encode("zstd", string.rep("a", 1000)))
    /// local _, err = compress.decode("zstd", packed, { max_size = 10 })
    /// assert(err ~= nil and err.kind == "too_large")
    /// ```
    pub fn decode(format: Format, data: Bytes, options: DecodeOptions) -> Result<Bytes, DecodeError> {
        super::decode(format, &data, options.max_size).map(Bytes)
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;
    use neoc_vm_core::make_engine;

    const FORMATS: [Format; 4] = [Format::Gzip, Format::Zlib, Format::Zstd, Format::Brotli];

    fn engine() -> Lua {
        let lua = make_engine().unwrap();
        let module: mlua::Table = lua.load(r#"return require("lib:compress")"#).eval().unwrap();
        lua.globals().set("compress", module).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine().load(code).eval().unwrap()
    }

    #[test]
    fn round_trip_each_format() {
        let lua = engine();
        for format in ["gzip", "zlib", "zstd", "brotli"] {
            let result: String = lua
                .load(format!(
                    r#"
                    local compressed = assert(compress.encode("{format}", "hello world"))
                    return compress.decode("{format}", compressed)
                    "#
                ))
                .eval()
                .unwrap();
            assert_eq!(result, "hello world", "{format}");
        }
    }

    #[test]
    fn round_trip_binary() {
        let s: mlua::BString = eval(
            r#"
            local raw = "\0\1\2\254\255"
            local compressed = assert(compress.encode("gzip", raw))
            return compress.decode("gzip", compressed)
        "#,
        );
        assert_eq!(s.as_slice(), [0u8, 1, 2, 254, 255]);
    }

    #[test]
    fn round_trip_large() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        for format in FORMATS {
            let packed = encode(format, &data, None).unwrap();
            assert!(packed.len() < data.len(), "{format:?}");
            assert_eq!(decode(format, &packed, None).unwrap(), data, "{format:?}");
        }
    }

    #[test]
    fn empty_input_encodes_to_something_and_back_to_nothing() {
        for format in FORMATS {
            let packed = encode(format, b"", None).unwrap();
            assert!(!packed.is_empty(), "{format:?}: an empty input still has a stream");
            assert_eq!(decode(format, &packed, None).unwrap(), b"", "{format:?}");
        }
    }

    #[test]
    fn decode_empty_is_an_error() {
        let lua = engine();
        for format in ["gzip", "zlib", "zstd", "brotli"] {
            let (value, kind, text): (mlua::Value, String, String) = lua
                .load(format!(
                    r#"
                    local v, e = compress.decode("{format}", "")
                    return v, e.kind, tostring(e)
                    "#
                ))
                .eval()
                .unwrap();
            assert!(value.is_nil(), "{format}");
            assert_eq!(kind, "empty", "{format}");
            assert_eq!(text, "lib:compress.decode: empty: empty input is not a compressed stream");
        }
    }

    #[test]
    fn levels_are_each_formats_own() {
        for (format, min, max) in
            [(Format::Gzip, 0, 9), (Format::Zlib, 0, 9), (Format::Zstd, 1, 22), (Format::Brotli, 0, 11)]
        {
            for level in [min, max] {
                let packed = encode(format, b"hello", Some(level)).unwrap();
                assert_eq!(decode(format, &packed, None).unwrap(), b"hello", "{format:?} level {level}");
            }
            assert_eq!(
                encode(format, b"hello", Some(max + 1)),
                Err(EncodeError::LevelOutOfRange { format, level: max + 1, min, max })
            );
        }
        assert_eq!(
            encode(Format::Zstd, b"hello", Some(0)),
            Err(EncodeError::LevelOutOfRange { format: Format::Zstd, level: 0, min: 1, max: 22 })
        );
    }

    #[test]
    fn level_out_of_range_is_a_typed_error() {
        let (value, kind, level, max, text): (mlua::Value, String, u32, u32, String) = eval(
            r#"
            local v, e = compress.encode("gzip", "hello", { level = 99 })
            return v, e.kind, e.level, e.max, tostring(e)
        "#,
        );
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), level, max), ("level_out_of_range", 99, 9));
        assert_eq!(
            text,
            "lib:compress.encode: level_out_of_range: level out of range for gzip: 99 (it is from 0 to 9)"
        );
    }

    #[test]
    fn an_option_or_a_format_that_is_not_one_is_refused() {
        let lua = engine();
        for (call, place) in [
            (r#"encode("gzip", "hello", { level = 6.7 })"#, "lib:compress.encode: argument #3 'options'"),
            (r#"encode("gzip", "hello", { level = "fast" })"#, "lib:compress.encode: argument #3 'options'"),
            (r#"encode("gzip", "hello", { level = -1 })"#, "lib:compress.encode: argument #3 'options'"),
            (r#"encode("gzip", "hello", { levle = 1 })"#, "lib:compress.encode: argument #3 'options'"),
            (r#"encode("lz4", "hello")"#, "lib:compress.encode: argument #1 'format'"),
            (r#"encode("GZIP", "hello")"#, "lib:compress.encode: argument #1 'format'"),
            (r#"encode("gzip", 5)"#, "lib:compress.encode: argument #2 'data'"),
            (r#"decode("lz4", "hello")"#, "lib:compress.decode: argument #1 'format'"),
            (r#"decode("gzip", "data", { max_size = "1MB" })"#, "lib:compress.decode: argument #3 'options'"),
            (r#"decode("gzip", "data", { max_size = 1.5 })"#, "lib:compress.decode: argument #3 'options'"),
            (r#"decode("gzip", "data", { max_size = -1 })"#, "lib:compress.decode: argument #3 'options'"),
        ] {
            let err = lua
                .load(format!("return compress.{call}"))
                .eval::<mlua::Value>()
                .expect_err(call)
                .to_string();
            assert!(err.contains(place), "{call}: {err}");
        }
    }

    #[test]
    fn decode_corrupt_data() {
        for format in FORMATS {
            let result = decode(format, b"not compressed data", None);
            assert!(
                matches!(result, Err(DecodeError::Corrupt { format: f, .. }) if f == format),
                "{format:?}: {result:?}"
            );
        }
        let (value, kind, format): (mlua::Value, String, String) = eval(
            r#"
            local v, e = compress.decode("gzip", "not compressed data")
            return v, e.kind, e.format
        "#,
        );
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), format.as_str()), ("corrupt", "gzip"));
    }

    #[test]
    fn cross_format_incompatible() {
        let packed = encode(Format::Gzip, b"hello", None).unwrap();
        assert!(matches!(decode(Format::Zlib, &packed, None), Err(DecodeError::Corrupt { .. })));
    }

    #[test]
    fn decode_max_size() {
        let (value, kind, max_size, text): (mlua::Value, String, u64, String) = eval(
            r#"
            local compressed = assert(compress.encode("gzip", string.rep("a", 1000)))
            local v, e = compress.decode("gzip", compressed, { max_size = 10 })
            return v, e.kind, e.max_size, tostring(e)
        "#,
        );
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), max_size), ("too_large", 10));
        assert_eq!(text, "lib:compress.decode: too_large: output exceeds max_size (10 bytes)");

        for format in FORMATS {
            let packed = encode(format, &[b'a'; 1000], None).unwrap();
            assert_eq!(
                decode(format, &packed, Some(999)),
                Err(DecodeError::TooLarge { max_size: 999 }),
                "{format:?}"
            );
            assert_eq!(
                decode(format, &packed, Some(1000)).unwrap().len(),
                1000,
                "{format:?}: the limit itself fits"
            );
        }
        assert_eq!(decode(Format::Gzip, b"data", Some(0)), Err(DecodeError::MaxSizeZero));
    }

    /// The encoders are pinned, not only round-tripped: an upgrade (flate2's backend, zstd, brotli) that changes
    /// the bytes `encode` writes is a deliberate, recorded change, so it fails here first.
    #[test]
    fn the_bytes_encode_writes_are_stable() {
        let data = "neoc byte-stability probe 0123456789\n".repeat(20);
        let pinned = [
            (
                Format::Gzip,
                1,
                "1f8b08000000000004ffedd2b90900211040d17cab980616bc8f721c994010159dc4eead4330ffd1e735ea197033fd8b13965a78c3981d098454da58e743fc5ef4165cace000a73b00a6e4020000",
            ),
            (
                Format::Gzip,
                6,
                "1f8b08000000000000ffedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde800a73b00a6e4020000",
            ),
            (
                Format::Gzip,
                9,
                "1f8b08000000000002ffedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde800a73b00a6e4020000",
            ),
            (
                Format::Zlib,
                1,
                "7801edd2b90900211040d17cab980616bc8f721c994010159dc4eead4330ffd1e735ea197033fd8b13965a78c3981d098454da58e743fc5ef4165cace0003d25ee49",
            ),
            (
                Format::Zlib,
                6,
                "789cedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde8003d25ee49",
            ),
            (
                Format::Zlib,
                9,
                "78daedcab90940210c00d0fe4f91053e781fe3184921888aa6717be710ec5fa39e0137d3bf3861a985378cd9914048a58d753ec4af3df4d0bde8003d25ee49",
            ),
            (
                Format::Zstd,
                1,
                "28b52ffd004875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504",
            ),
            (
                Format::Zstd,
                3,
                "28b52ffd005875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504",
            ),
            (
                Format::Zstd,
                19,
                "28b52ffd006875010054026e656f6320627974652d73746162696c6974792070726f626520303132333435363738390a0100e5852af504",
            ),
            (
                Format::Brotli,
                1,
                "8b71010080aaaaaaeaffae97931c2e0ab0ecb7a38a821c0440414541552e0a0770f12b4da4b8c106639e11359b64027e25fb74202fd60fe70a12381f62caa5b66ec6ef18",
            ),
            (
                Format::Brotli,
                5,
                "1be30200c4dc467d255d28039fb7d5c9552762b10eaab02c0d9bbe60ff514b9107e7ba1121a65c6aeb630a04",
            ),
            (
                Format::Brotli,
                11,
                "1be302f8c5e6469d9208850df46f1577559cb3580735583478838d58ecff411acf736e38582de5525b1f9300",
            ),
        ];
        for (format, level, expected) in pinned {
            let packed = encode(format, data.as_bytes(), Some(level)).unwrap();
            assert_eq!(hex::encode(packed), expected, "{format:?} level {level}");
        }
        // zlib's stored block (level 0), as miniz_oxide 0.9 writes it
        assert_eq!(hex::encode(encode(Format::Zlib, b"", Some(0)).unwrap()), "7801010000ffff00000001");
    }
}

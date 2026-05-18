//! Compression and decompression — `lib:compress`.
//!
//! See docs/src/rfcs/0004-lib-compress.md for the public surface contract.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::{err, from_err, ok};

const DEFAULT_MAX_SIZE: usize = 256 * 1024 * 1024;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("encode", lua.create_function(encode)?)?;
    t.set("decode", lua.create_function(decode)?)?;
    Ok(t)
}

#[instrument(name = "compress.encode", target = "luau::lib::compress", skip_all)]
fn encode(
    lua: &Lua,
    (format, data, opts): (String, mlua::String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let raw = data.as_bytes();
    let bytes: &[u8] = &raw;
    let level = match extract_integer_opt(opts.as_ref(), "level", "compress.encode") {
        Ok(l) => l,
        Err(msg) => return err(lua, msg),
    };
    match format.as_str() {
        "gzip" => encode_gzip(lua, bytes, level),
        "zlib" => encode_zlib(lua, bytes, level),
        "zstd" => encode_zstd(lua, bytes, level),
        "brotli" => encode_brotli(lua, bytes, level),
        _ => err(lua, format!("compress.encode: unknown format '{format}'")),
    }
}

#[instrument(name = "compress.decode", target = "luau::lib::compress", skip_all)]
fn decode(
    lua: &Lua,
    (format, data, opts): (String, mlua::String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let raw = data.as_bytes();
    let bytes: &[u8] = &raw;
    if bytes.is_empty() {
        return err(
            lua,
            "compress.decode: empty input is not a valid compressed stream",
        );
    }
    let max_size = match extract_integer_opt(opts.as_ref(), "max_size", "compress.decode") {
        Ok(Some(n)) if n <= 0 => return err(lua, "compress.decode: max_size must be > 0"),
        Ok(Some(n)) => n as usize,
        Ok(None) => DEFAULT_MAX_SIZE,
        Err(msg) => return err(lua, msg),
    };
    match format.as_str() {
        "gzip" => decode_gzip(lua, bytes, max_size),
        "zlib" => decode_zlib(lua, bytes, max_size),
        "zstd" => decode_zstd(lua, bytes, max_size),
        "brotli" => decode_brotli(lua, bytes, max_size),
        _ => err(lua, format!("compress.decode: unknown format '{format}'")),
    }
}

fn extract_integer_opt(
    opts: Option<&Table>,
    key: &str,
    prefix: &str,
) -> Result<Option<i64>, String> {
    let Some(t) = opts else { return Ok(None) };
    let v: Value = t.get(key).map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(None),
        Value::Integer(i) => Ok(Some(i as i64)),
        Value::Number(f) => {
            if !f.is_finite() || f.fract() != 0.0 {
                Err(format!("{prefix}: {key} must be an integer"))
            } else {
                Ok(Some(f as i64))
            }
        }
        _ => Err(format!("{prefix}: invalid {key} type")),
    }
}

fn validate_level(
    format: &str,
    level: Option<i64>,
    min: i64,
    max: i64,
    default: i64,
) -> Result<i64, String> {
    match level {
        None => Ok(default),
        Some(n) if n >= min && n <= max => Ok(n),
        Some(n) => Err(format!(
            "compress.encode: level out of range for {format}: {n}"
        )),
    }
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 * 1024 && bytes.is_multiple_of(1024 * 1024) {
        format!("{} MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 && bytes.is_multiple_of(1024) {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

fn read_with_limit<R: Read>(mut reader: R, max_size: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if output.len() + n > max_size {
            return Err(std::io::Error::other(format!(
                "output exceeds max_size ({})",
                format_size(max_size)
            )));
        }
        output.extend_from_slice(&buf[..n]);
    }
    Ok(output)
}

// --- Encode ---

fn encode_gzip(lua: &Lua, data: &[u8], level: Option<i64>) -> mlua::Result<(Value, Value)> {
    let level = match validate_level("gzip", level, 0, 9, 6) {
        Ok(l) => l,
        Err(msg) => return err(lua, msg),
    };
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level as u32));
    if let Err(e) = encoder.write_all(data) {
        return from_err(lua, "compress.encode", e);
    }
    match encoder.finish() {
        Ok(compressed) => ok(lua, lua.create_string(&compressed)?),
        Err(e) => from_err(lua, "compress.encode", e),
    }
}

fn encode_zlib(lua: &Lua, data: &[u8], level: Option<i64>) -> mlua::Result<(Value, Value)> {
    let level = match validate_level("zlib", level, 0, 9, 6) {
        Ok(l) => l,
        Err(msg) => return err(lua, msg),
    };
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level as u32));
    if let Err(e) = encoder.write_all(data) {
        return from_err(lua, "compress.encode", e);
    }
    match encoder.finish() {
        Ok(compressed) => ok(lua, lua.create_string(&compressed)?),
        Err(e) => from_err(lua, "compress.encode", e),
    }
}

fn encode_zstd(lua: &Lua, data: &[u8], level: Option<i64>) -> mlua::Result<(Value, Value)> {
    let level = match validate_level("zstd", level, 1, 22, 3) {
        Ok(l) => l,
        Err(msg) => return err(lua, msg),
    };
    let mut encoder = match zstd::Encoder::new(Vec::new(), level as i32) {
        Ok(e) => e,
        Err(e) => return from_err(lua, "compress.encode", e),
    };
    if let Err(e) = encoder.write_all(data) {
        return from_err(lua, "compress.encode", e);
    }
    match encoder.finish() {
        Ok(compressed) => ok(lua, lua.create_string(&compressed)?),
        Err(e) => from_err(lua, "compress.encode", e),
    }
}

fn encode_brotli(lua: &Lua, data: &[u8], level: Option<i64>) -> mlua::Result<(Value, Value)> {
    let level = match validate_level("brotli", level, 0, 11, 6) {
        Ok(l) => l,
        Err(msg) => return err(lua, msg),
    };
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 4096, level as u32, 22);
        if let Err(e) = writer.write_all(data) {
            return from_err(lua, "compress.encode", e);
        }
    }
    ok(lua, lua.create_string(&output)?)
}

// --- Decode ---

fn decode_gzip(lua: &Lua, data: &[u8], max_size: usize) -> mlua::Result<(Value, Value)> {
    let reader = GzDecoder::new(data);
    match read_with_limit(reader, max_size) {
        Ok(decompressed) => ok(lua, lua.create_string(&decompressed)?),
        Err(e) => from_err(lua, "compress.decode", e),
    }
}

fn decode_zlib(lua: &Lua, data: &[u8], max_size: usize) -> mlua::Result<(Value, Value)> {
    let reader = ZlibDecoder::new(data);
    match read_with_limit(reader, max_size) {
        Ok(decompressed) => ok(lua, lua.create_string(&decompressed)?),
        Err(e) => from_err(lua, "compress.decode", e),
    }
}

fn decode_zstd(lua: &Lua, data: &[u8], max_size: usize) -> mlua::Result<(Value, Value)> {
    let reader = match zstd::Decoder::new(data) {
        Ok(r) => r,
        Err(e) => return from_err(lua, "compress.decode", e),
    };
    match read_with_limit(reader, max_size) {
        Ok(decompressed) => ok(lua, lua.create_string(&decompressed)?),
        Err(e) => from_err(lua, "compress.decode", e),
    }
}

fn decode_brotli(lua: &Lua, data: &[u8], max_size: usize) -> mlua::Result<(Value, Value)> {
    let reader = brotli::Decompressor::new(data, 4096);
    match read_with_limit(reader, max_size) {
        Ok(decompressed) => ok(lua, lua.create_string(&decompressed)?),
        Err(e) => from_err(lua, "compress.decode", e),
    }
}

// --- Benchmarks ---

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("compress");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    for format in &["gzip", "zlib", "zstd", "brotli"] {
        let encode_chunk: mlua::Function = lua
            .load(format!(
                r#"return function(p) return (require("lib:compress")).encode("{format}", p) end"#
            ))
            .eval()
            .expect("chunk");
        g.bench_function(format!("{format}_encode"), |b| {
            b.iter(|| {
                let _: mlua::String = encode_chunk.call(black_box(payload_lua.clone())).unwrap();
            });
        });

        let compressed_lua: mlua::String =
            encode_chunk.call(payload_lua.clone()).expect("compress");
        let decode_chunk: mlua::Function = lua
            .load(format!(
                r#"return function(p) return (require("lib:compress")).decode("{format}", p) end"#
            ))
            .eval()
            .expect("chunk");
        g.bench_function(format!("{format}_decode"), |b| {
            b.iter(|| {
                let _: mlua::String = decode_chunk
                    .call(black_box(compressed_lua.clone()))
                    .unwrap();
            });
        });
    }

    g.finish();
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua
            .load(r#"return require("lib:compress")"#)
            .eval()
            .unwrap();
        lua.globals().set("compress", m).unwrap();
        lua
    }

    #[test]
    fn round_trip_gzip() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("gzip", "hello world")
                local decompressed = compress.decode("gzip", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn round_trip_zlib() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("zlib", "hello world")
                local decompressed = compress.decode("zlib", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn round_trip_zstd() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("zstd", "hello world")
                local decompressed = compress.decode("zstd", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn round_trip_brotli() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("brotli", "hello world")
                local decompressed = compress.decode("brotli", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn round_trip_binary() {
        let lua = engine();
        let s: mlua::String = lua
            .load(
                r#"
                local raw = "\0\1\2\254\255"
                local compressed = compress.encode("gzip", raw)
                local decompressed = compress.decode("gzip", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(&*s.as_bytes(), &[0u8, 1, 2, 254, 255]);
    }

    #[test]
    fn encode_empty_produces_output() {
        let lua = engine();
        for format in ["gzip", "zlib", "zstd", "brotli"] {
            let s: mlua::String = lua
                .load(format!(r#"return compress.encode("{format}", "")"#))
                .eval()
                .unwrap();
            assert!(
                !s.as_bytes().is_empty(),
                "{format}: empty input should produce non-empty compressed output"
            );
        }
    }

    #[test]
    fn decode_empty_errors() {
        let lua = engine();
        for format in ["gzip", "zlib", "zstd", "brotli"] {
            let (val, e): (Option<String>, Option<String>) = lua
                .load(format!(r#"return compress.decode("{format}", "")"#))
                .eval()
                .unwrap();
            assert!(val.is_none(), "{format}: decode empty should return nil");
            let e = e.unwrap_or_else(|| panic!("{format}: decode empty should return error"));
            assert!(
                e.starts_with("compress.decode:"),
                "{format}: bad prefix: {e}"
            );
        }
    }

    #[test]
    fn encode_with_level() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("gzip", "hello", { level = 9 })
                local decompressed = compress.decode("gzip", compressed)
                return decompressed
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn encode_level_out_of_range() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.encode("gzip", "hello", { level = 99 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("level out of range"), "got: {e}");
    }

    #[test]
    fn encode_level_non_integer() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.encode("gzip", "hello", { level = 6.7 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("level must be an integer"), "got: {e}");
    }

    #[test]
    fn encode_level_invalid_type() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.encode("gzip", "hello", { level = "fast" })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("invalid level type"), "got: {e}");
    }

    #[test]
    fn encode_unknown_format() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.encode("lz4", "hello")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("unknown format 'lz4'"), "got: {e}");
    }

    #[test]
    fn decode_unknown_format() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.decode("lz4", "hello")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("unknown format 'lz4'"), "got: {e}");
    }

    #[test]
    fn decode_corrupt_data() {
        let lua = engine();
        for format in ["gzip", "zlib", "zstd", "brotli"] {
            let (val, e): (Option<String>, Option<String>) = lua
                .load(format!(
                    r#"return compress.decode("{format}", "not compressed data")"#
                ))
                .eval()
                .unwrap();
            assert!(val.is_none(), "{format}");
            assert!(e.unwrap().starts_with("compress.decode:"), "{format}");
        }
    }

    #[test]
    fn decode_max_size_exceeded() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local big = string.rep("a", 1000)
                local compressed = compress.encode("gzip", big)
                return compress.decode("gzip", compressed, { max_size = 10 })
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("output exceeds max_size"), "got: {e}");
    }

    #[test]
    fn decode_max_size_invalid_type() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.decode("gzip", "data", { max_size = "1MB" })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("invalid max_size type"), "got: {e}");
    }

    #[test]
    fn decode_max_size_non_integer() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.decode("gzip", "data", { max_size = 1.5 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("max_size must be an integer"), "got: {e}");
    }

    #[test]
    fn decode_max_size_zero() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.decode("gzip", "data", { max_size = 0 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("max_size must be > 0"), "got: {e}");
    }

    #[test]
    fn decode_max_size_negative() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.decode("gzip", "data", { max_size = -1 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("max_size must be > 0"), "got: {e}");
    }

    #[test]
    fn cross_format_incompatible() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local compressed = compress.encode("gzip", "hello")
                return compress.decode("zlib", compressed)
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().starts_with("compress.decode:"));
    }

    #[test]
    fn zstd_level_range() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("zstd", "hello", { level = 1 })
                return compress.decode("zstd", compressed)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");

        let (val, e): (Option<String>, Option<String>) = lua
            .load(r#"return compress.encode("zstd", "hello", { level = 0 })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().contains("level out of range"));
    }

    #[test]
    fn brotli_level_range() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local compressed = compress.encode("brotli", "hello", { level = 11 })
                return compress.decode("brotli", compressed)
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(result, "hello");
    }
}

//! `lib:compile` — programmatic build API for single-file executables.
//!
//! Exposes `bundle(opts)` to Luau scripts, allowing a `build.luau` to package
//! a Luau source file into a standalone executable. Modelled after
//! [Bun.build](https://bun.sh/docs/bundler) — programmatic-first, no CLI.
//!
//! ## Usage
//!
//! ```lua
//! local compile = require("lib:compile")
//!
//! local ok, err = compile.bundle({
//!     entrypoint = "main.luau",
//!     output     = "my-app",
//! })
//! ```
//!
//! ## Trailer format (RFC-0002)
//!
//! ```text
//! [0..7]   magic   b"NEOC_BL" (7 bytes)
//! [7]      version u8 0x01
//! [8..16]  offset  u64 little-endian — byte offset of payload start
//! [16..20] length  u32 little-endian — byte length of payload
//! ```
//! Total: 20 bytes.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::ok;

// ---------------------------------------------------------------------------
// Trailer constants — shared between the bundler and the startup detector.
// ---------------------------------------------------------------------------

pub const TRAILER_MAGIC: &[u8; 7] = b"NEOC_BL";
pub const TRAILER_VERSION: u8 = 0x01;
pub const TRAILER_SIZE: u64 = 20;

// ---------------------------------------------------------------------------
// Startup detection — called from main.rs on every launch.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum TrailerError {
    UnknownVersion(u8),
    CorruptBounds {
        offset: u64,
        length: u32,
        file_len: u64,
    },
}

/// Core trailer-parsing logic, operating on an arbitrary path.
///
/// Returns:
/// - `Ok(Some(payload))` — valid trailer found; payload extracted.
/// - `Ok(None)` — no NEOC_BL magic; this is a plain binary.
/// - `Err(TrailerError)` — magic matched but the trailer is malformed.
pub fn detect_embedded_payload_from(exe: &Path) -> Result<Option<Vec<u8>>, TrailerError> {
    let mut f = match std::fs::File::open(exe) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };

    let file_len = match f.seek(SeekFrom::End(0)) {
        Ok(n) => n,
        Err(_) => return Ok(None),
    };
    if file_len < TRAILER_SIZE {
        return Ok(None);
    }
    if f.seek(SeekFrom::End(-(TRAILER_SIZE as i64))).is_err() {
        return Ok(None);
    }

    let mut trailer = [0u8; 20];
    if f.read_exact(&mut trailer).is_err() {
        return Ok(None);
    }

    if &trailer[0..7] != TRAILER_MAGIC {
        return Ok(None);
    }

    let version = trailer[7];
    if version != TRAILER_VERSION {
        return Err(TrailerError::UnknownVersion(version));
    }

    let offset = u64::from_le_bytes(trailer[8..16].try_into().unwrap());
    let length = u32::from_le_bytes(trailer[16..20].try_into().unwrap());

    // Checked arithmetic to avoid wrapping overflow on a malformed trailer.
    let end = (offset as u128) + (length as u128) + (TRAILER_SIZE as u128);
    if end > file_len as u128 {
        return Err(TrailerError::CorruptBounds {
            offset,
            length,
            file_len,
        });
    }

    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Ok(None);
    }
    let mut payload = vec![0u8; length as usize];
    if f.read_exact(&mut payload).is_err() {
        return Ok(None);
    }

    Ok(Some(payload))
}

// ---------------------------------------------------------------------------
// Bundling — the core operation exposed to Luau.
// ---------------------------------------------------------------------------

/// Append `payload` + trailer to a writer. `stub_len` is the byte length of
/// the stub already written (used as the payload offset in the trailer).
fn write_trailer(f: &mut impl Write, payload: &[u8], stub_len: u64) -> std::io::Result<()> {
    let length: u32 = payload.len().try_into().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "payload too large (>4 GiB)",
        )
    })?;

    f.write_all(payload)?;
    f.write_all(TRAILER_MAGIC)?;
    f.write_all(&[TRAILER_VERSION])?;
    f.write_all(&stub_len.to_le_bytes())?;
    f.write_all(&length.to_le_bytes())?;
    Ok(())
}

/// Bundle a Luau source file into a self-contained executable.
///
/// 1. Reads the source from `entrypoint`.
/// 2. Uses the current `neoc` binary as the stub (this function runs *inside*
///    the runtime, so `current_exe()` is always the correct stub).
/// 3. Copies stub → output, appends payload + 20-byte trailer.
/// 4. Sets executable permission on Unix.
fn bundle_impl(entrypoint: &Path, output: &Path) -> Result<(), String> {
    // Read source.
    let source = std::fs::read(entrypoint)
        .map_err(|e| format!("compile.bundle: cannot read {}: {e}", entrypoint.display()))?;

    // Resolve stub — the running neoc binary.
    let stub_path = std::env::current_exe()
        .map_err(|e| format!("compile.bundle: cannot resolve current executable: {e}"))?;

    let stub_bytes = std::fs::read(&stub_path).map_err(|e| {
        format!(
            "compile.bundle: cannot read stub {}: {e}",
            stub_path.display()
        )
    })?;

    let stub_len = stub_bytes.len() as u64;

    // Build full output in memory (atomic write — no half-written file on error).
    let mut out_bytes = stub_bytes;
    write_trailer(&mut out_bytes, &source, stub_len)
        .map_err(|e| format!("compile.bundle: trailer write failed: {e}"))?;

    // Write output.
    std::fs::write(output, &out_bytes)
        .map_err(|e| format!("compile.bundle: cannot write {}: {e}", output.display()))?;

    // Make executable on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(output)
            .map_err(|e| format!("compile.bundle: cannot read permissions: {e}"))?
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        std::fs::set_permissions(output, perms)
            .map_err(|e| format!("compile.bundle: cannot set executable bit: {e}"))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Luau module surface
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("bundle", lua.create_function(bundle)?)?;
    Ok(t)
}

/// `compile.bundle({ entrypoint = "main.luau", output = "my-app" })`
///
/// Returns `(true, nil)` on success, `(nil, err)` on failure.
#[instrument(name = "compile.bundle", target = "luau::lib::compile", skip_all)]
fn bundle(lua: &Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let entrypoint: String = opts.get::<String>("entrypoint").map_err(|_| {
        mlua::Error::external("compile.bundle: opts.entrypoint is required (string)")
    })?;
    let output: String = opts
        .get::<String>("output")
        .map_err(|_| mlua::Error::external("compile.bundle: opts.output is required (string)"))?;

    match bundle_impl(Path::new(&entrypoint), Path::new(&output)) {
        Ok(()) => ok(lua, true),
        // bundle_impl errors are already prefixed with "compile.bundle: ".
        Err(e) => crate::lua::sandbox::err(lua, e),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // --- trailer format ---

    fn write_bundled(stub: &[u8], payload: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        let offset = stub.len() as u64;
        let length = payload.len() as u32;

        f.write_all(stub).unwrap();
        f.write_all(payload).unwrap();
        f.write_all(TRAILER_MAGIC).unwrap();
        f.write_all(&[TRAILER_VERSION]).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        f.write_all(&length.to_le_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn trailer_roundtrip_basic() {
        let stub = b"fake-neoc-binary-stub";
        let payload = b"print('hello from compiled neoc')";
        let f = write_bundled(stub, payload);
        let result = detect_embedded_payload_from(f.path())
            .expect("no trailer error")
            .expect("should detect payload");
        assert_eq!(result, payload);
    }

    #[test]
    fn trailer_roundtrip_empty_payload() {
        let f = write_bundled(b"stub", b"");
        let result = detect_embedded_payload_from(f.path())
            .expect("no trailer error")
            .expect("should detect payload");
        assert!(result.is_empty());
    }

    #[test]
    fn trailer_roundtrip_large_payload() {
        let stub = vec![0u8; 1024 * 1024];
        let payload: Vec<u8> = (0..65536).map(|i: u32| (i % 251) as u8).collect();
        let f = write_bundled(&stub, &payload);
        let result = detect_embedded_payload_from(f.path())
            .expect("no trailer error")
            .expect("should detect payload");
        assert_eq!(result, payload);
    }

    #[test]
    fn no_trailer_returns_none() {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(b"just a plain binary, no trailer").unwrap();
        f.flush().unwrap();
        assert!(
            detect_embedded_payload_from(f.path())
                .expect("no trailer error")
                .is_none()
        );
    }

    #[test]
    fn wrong_magic_returns_none() {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(b"stub-data").unwrap();
        f.write_all(b"BADMAGIC").unwrap();
        f.write_all(&[TRAILER_VERSION]).unwrap();
        f.write_all(&9u64.to_le_bytes()).unwrap();
        f.write_all(&0u32.to_le_bytes()).unwrap();
        f.flush().unwrap();
        assert!(
            detect_embedded_payload_from(f.path())
                .expect("no trailer error")
                .is_none()
        );
    }

    #[test]
    fn unknown_version_returns_error() {
        let stub = b"fake-stub";
        let payload = b"irrelevant";
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        let offset = stub.len() as u64;
        let length = payload.len() as u32;
        f.write_all(stub).unwrap();
        f.write_all(payload).unwrap();
        f.write_all(TRAILER_MAGIC).unwrap();
        f.write_all(&[0xFF]).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        f.write_all(&length.to_le_bytes()).unwrap();
        f.flush().unwrap();

        match detect_embedded_payload_from(f.path()) {
            Err(TrailerError::UnknownVersion(0xFF)) => {}
            other => panic!("expected UnknownVersion(0xFF), got {other:?}"),
        }
    }

    #[test]
    fn corrupt_bounds_returns_error() {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(b"stub").unwrap();
        f.write_all(TRAILER_MAGIC).unwrap();
        f.write_all(&[TRAILER_VERSION]).unwrap();
        f.write_all(&9999u64.to_le_bytes()).unwrap();
        f.write_all(&100u32.to_le_bytes()).unwrap();
        f.flush().unwrap();

        match detect_embedded_payload_from(f.path()) {
            Err(TrailerError::CorruptBounds { .. }) => {}
            other => panic!("expected CorruptBounds, got {other:?}"),
        }
    }

    #[test]
    fn magic_bytes_are_correct() {
        assert_eq!(TRAILER_MAGIC, b"NEOC_BL");
        assert_eq!(TRAILER_MAGIC.len(), 7);
    }

    #[test]
    fn trailer_size_is_twenty() {
        assert_eq!(TRAILER_SIZE, 20);
    }

    #[test]
    fn version_byte_is_one() {
        assert_eq!(TRAILER_VERSION, 0x01);
    }

    // --- write_trailer field layout ---

    #[test]
    fn write_trailer_field_layout() {
        let stub_len: u64 = 42;
        let payload = b"hello";
        let mut buf: Vec<u8> = Vec::new();
        write_trailer(&mut buf, payload, stub_len).unwrap();

        assert_eq!(&buf[..5], b"hello");
        assert_eq!(&buf[5..12], TRAILER_MAGIC);
        assert_eq!(buf[12], TRAILER_VERSION);
        let offset = u64::from_le_bytes(buf[13..21].try_into().unwrap());
        assert_eq!(offset, stub_len);
        let length = u32::from_le_bytes(buf[21..25].try_into().unwrap());
        assert_eq!(length, payload.len() as u32);
        assert_eq!(buf.len(), 25);
    }

    // --- Luau API ---

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:compile")"#).eval().unwrap();
        lua.globals().set("compile", m).unwrap();
        lua
    }

    #[test]
    fn bundle_missing_entrypoint_returns_err() {
        let lua = engine();
        let (val, err): (Option<bool>, Option<String>) = lua
            .load(r#"return compile.bundle({ entrypoint = "/nonexistent.luau", output = "/tmp/nope" })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let err = err.expect("error string expected");
        assert!(err.contains("compile.bundle"), "missing prefix: {err}");
    }

    #[test]
    fn bundle_requires_entrypoint_field() {
        let lua = engine();
        let result: mlua::Result<(Value, Value)> = lua
            .load(r#"return compile.bundle({ output = "/tmp/nope" })"#)
            .eval();
        assert!(result.is_err());
    }

    #[test]
    fn bundle_requires_output_field() {
        let lua = engine();
        let result: mlua::Result<(Value, Value)> = lua
            .load(r#"return compile.bundle({ entrypoint = "main.luau" })"#)
            .eval();
        assert!(result.is_err());
    }
}

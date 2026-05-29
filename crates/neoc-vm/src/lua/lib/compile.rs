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

use crate::lua::facade::Module;
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

struct TrailerHeader {
    offset: u64,
    length: u32,
}

/// Parse the last 20 bytes of `bytes` as a NEOC_BL trailer.
///
/// Returns:
/// - `Ok(Some(header))` — magic + supported version matched.
/// - `Ok(None)` — no magic, or buffer too short.
/// - `Err(version)` — magic matched but the trailer version is unknown.
fn parse_trailer(bytes: &[u8]) -> Result<Option<TrailerHeader>, u8> {
    if bytes.len() < TRAILER_SIZE as usize {
        return Ok(None);
    }
    let t = bytes.len() - TRAILER_SIZE as usize;
    let trailer = &bytes[t..];
    if trailer[0..7] != *TRAILER_MAGIC {
        return Ok(None);
    }
    let version = trailer[7];
    if version != TRAILER_VERSION {
        return Err(version);
    }
    // `trailer` is exactly TRAILER_SIZE bytes here (the length check above plus
    // the caller's tail slice), so these fixed-width reads cannot truncate.
    let mut offset_bytes = [0u8; 8];
    offset_bytes.copy_from_slice(&trailer[8..16]);
    let mut length_bytes = [0u8; 4];
    length_bytes.copy_from_slice(&trailer[16..20]);
    let offset = u64::from_le_bytes(offset_bytes);
    let length = u32::from_le_bytes(length_bytes);
    Ok(Some(TrailerHeader { offset, length }))
}

/// Core trailer-parsing logic, operating on an arbitrary path.
///
/// Returns:
/// - `Ok(Some(payload))` — valid trailer found; payload extracted.
/// - `Ok(None)` — no NEOC_BL magic; this is a plain binary.
/// - `Err(TrailerError)` — magic matched but the trailer is malformed.
// TODO: clean-code - 0.7 - SRP: open+seek+read+validate magic/version+validate bounds+extract payload — split into read_trailer + extract_payload
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

    let mut trailer = [0u8; TRAILER_SIZE as usize];
    if f.read_exact(&mut trailer).is_err() {
        return Ok(None);
    }

    let TrailerHeader { offset, length } = match parse_trailer(&trailer) {
        Ok(Some(h)) => h,
        Ok(None) => return Ok(None),
        Err(v) => return Err(TrailerError::UnknownVersion(v)),
    };

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
// TODO: clean-code - 0.75 - SRP: read source + resolve stub + strip prior trailer + assemble + write + chmod — extract strip_existing_trailer and set_executable
fn bundle_impl(entrypoint: &Path, output: &Path) -> Result<(), String> {
    // Read source.
    let source = std::fs::read(entrypoint)
        .map_err(|e| format!("compile.bundle: cannot read {}: {e}", entrypoint.display()))?;

    // Resolve stub — the running neoc binary.
    let stub_path = std::env::current_exe()
        .map_err(|e| format!("compile.bundle: cannot resolve current executable: {e}"))?;

    let mut stub_bytes = std::fs::read(&stub_path).map_err(|e| {
        format!(
            "compile.bundle: cannot read stub {}: {e}",
            stub_path.display()
        )
    })?;

    // Strip any previously embedded payload so that re-bundling a bundled binary
    // does not inflate the output by the prior payload + trailer on each pass.
    if let Ok(Some(TrailerHeader { offset, .. })) = parse_trailer(&stub_bytes) {
        let t = stub_bytes.len() - TRAILER_SIZE as usize;
        if (offset as usize) < t {
            stub_bytes.truncate(offset as usize);
        }
    }

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
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:compile` surface through the facade.
///
/// `bundle` is set verbatim with [`crate::lua::facade::Module::set`]: it
/// carries its own `#[instrument(name = "compile.bundle")]` span and bakes its
/// own `"compile.bundle: "` error prefix in `bundle_impl` (so it routes through
/// `ok` / `err` rather than the facade's auto-prefix path). Its signature is
/// recorded for the header via [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:compile")?;

    m.set("bundle", lua.create_function(bundle)?)?;
    m.record_fn(
        "bundle",
        &[("opts", "LuaValue")],
        "Result<bool, string>",
        "Bundle a Luau source file into a self-contained executable. `opts` requires `entrypoint` and `output` string fields.",
    );

    Ok(m)
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
        // Use a 7-byte fake magic (same length as TRAILER_MAGIC) so the trailer
        // is structurally correct at 20 bytes and the reader inspects the right bytes.
        let stub = b"stub-data";
        let payload = b"";
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        let offset = stub.len() as u64;
        let length = payload.len() as u32;
        f.write_all(stub).unwrap();
        f.write_all(b"BADMAGX").unwrap(); // 7 bytes — wrong magic, correct length
        f.write_all(&[TRAILER_VERSION]).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        f.write_all(&length.to_le_bytes()).unwrap();
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

    // --- re-bundle idempotency ---

    #[test]
    fn rebundle_does_not_inflate_stub() {
        // Simulate: bundled_binary = stub + payload_a + trailer_a
        // Re-bundle with payload_b — output must be stub + payload_b + trailer_b,
        // not stub + payload_a + trailer_a + payload_b + trailer_b.
        use std::io::Write as _;

        let stub = b"fake-neoc-binary";
        let payload_a = b"first-script";
        let payload_b = b"second-script";

        // Build first bundle (mimics what write_bundled does).
        let mut first_bundle: Vec<u8> = Vec::new();
        first_bundle.write_all(stub).unwrap();
        let offset_a = stub.len() as u64;
        let length_a = payload_a.len() as u32;
        first_bundle.write_all(payload_a).unwrap();
        first_bundle.write_all(TRAILER_MAGIC).unwrap();
        first_bundle.write_all(&[TRAILER_VERSION]).unwrap();
        first_bundle.write_all(&offset_a.to_le_bytes()).unwrap();
        first_bundle.write_all(&length_a.to_le_bytes()).unwrap();

        // The stripping logic in bundle_impl, applied inline.
        let mut stub_bytes = first_bundle.clone();
        if stub_bytes.len() as u64 >= TRAILER_SIZE {
            let t = stub_bytes.len() - TRAILER_SIZE as usize;
            if stub_bytes[t..t + 7] == *TRAILER_MAGIC && stub_bytes[t + 7] == TRAILER_VERSION {
                let offset = u64::from_le_bytes(stub_bytes[t + 8..t + 16].try_into().unwrap());
                if (offset as usize) < t {
                    stub_bytes.truncate(offset as usize);
                }
            }
        }

        // After stripping, stub_bytes must equal the original stub only.
        assert_eq!(&stub_bytes[..], stub.as_slice());

        // Building the second bundle from the stripped stub gives the correct size.
        let stub_len = stub_bytes.len() as u64;
        let mut second_bundle = stub_bytes;
        write_trailer(&mut second_bundle, payload_b, stub_len).unwrap();
        let expected_size = stub.len() + payload_b.len() + TRAILER_SIZE as usize;
        assert_eq!(second_bundle.len(), expected_size);

        // And detection on the second bundle returns payload_b, not payload_a.
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(&second_bundle).unwrap();
        f.flush().unwrap();
        let detected = detect_embedded_payload_from(f.path())
            .expect("no error")
            .expect("payload present");
        assert_eq!(detected, payload_b.as_slice());
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

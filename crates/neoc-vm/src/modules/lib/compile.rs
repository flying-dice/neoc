//! Single-file executables: `lib:compile`.
//!
//! `bundle` packages a Lua source file into a standalone executable: a copy of the running `neoc` binary with the
//! source appended, which the binary finds and runs when it starts ([`detect_embedded_payload_from`]). Modelled on
//! [Bun.build](https://bun.sh/docs/bundler): called from a build script, not from a command line.
//!
//! ## Trailer format
//!
//! ```text
//! [0..7]   magic   b"NEOC_BL" (7 bytes)
//! [7]      version u8 0x01
//! [8..16]  offset  u64 little-endian: byte offset of payload start
//! [16..20] length  u32 little-endian: byte length of payload
//! ```
//! Total: 20 bytes, the last 20 of the file.
//!
//! The module reads and writes whatever paths it is given: an embedder that runs scripts it does not trust leaves
//! it out of what it offers (`modules::install_allowed`).
//!
//! The API's rules:
//! - A failure is a typed `BundleError` (`entrypoint_unreadable`, `stub_unreadable`, `too_large`, `output_unwritable`)
//!   with the path and the operating system's reason.
//! - A field that does not exist is refused with the other wrong arguments.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use compile::BundleError;

// ---------------------------------------------------------------------------
// Trailer constants: shared between the bundler and the startup detector.
// ---------------------------------------------------------------------------

pub const TRAILER_MAGIC: &[u8; 7] = b"NEOC_BL";
pub const TRAILER_VERSION: u8 = 0x01;
pub const TRAILER_SIZE: u64 = 20;
const TRAILER_LEN: usize = 20;

// ---------------------------------------------------------------------------
// Startup detection: called by the binary on every launch.
// ---------------------------------------------------------------------------

/// Why a file that ends in the trailer's magic has no payload to run.
#[derive(Debug, PartialEq)]
pub enum TrailerError {
    UnknownVersion(u8),
    CorruptBounds { offset: u64, length: u32, file_len: u64 },
}

#[derive(Clone, Copy)]
struct TrailerHeader {
    offset: u64,
    length: u32,
}

/// Parse the last 20 bytes of `bytes` as a trailer.
///
/// - `Ok(Some(header))`: the magic and a supported version.
/// - `Ok(None)`: no magic, or fewer than 20 bytes.
/// - `Err(version)`: the magic, with a version this build does not know.
fn parse_trailer(bytes: &[u8]) -> Result<Option<TrailerHeader>, u8> {
    let Some(start) = bytes.len().checked_sub(TRAILER_LEN) else {
        return Ok(None);
    };
    let trailer = &bytes[start..];
    if trailer[0..7] != *TRAILER_MAGIC {
        return Ok(None);
    }
    let version = trailer[7];
    if version != TRAILER_VERSION {
        return Err(version);
    }
    // `trailer` is exactly 20 bytes, so these fixed-width reads cannot truncate.
    let mut offset_bytes = [0u8; 8];
    offset_bytes.copy_from_slice(&trailer[8..16]);
    let mut length_bytes = [0u8; 4];
    length_bytes.copy_from_slice(&trailer[16..20]);
    Ok(Some(TrailerHeader {
        offset: u64::from_le_bytes(offset_bytes),
        length: u32::from_le_bytes(length_bytes),
    }))
}

/// The length of an open file and its last 20 bytes, or `None` if the file is shorter than a trailer or a seek or
/// read fails: all of which mean "no embedded payload", as for a plain binary.
fn read_trailer_bytes(f: &mut std::fs::File) -> Option<(u64, [u8; TRAILER_LEN])> {
    let file_len = f.seek(SeekFrom::End(0)).ok()?;
    let start = file_len.checked_sub(TRAILER_SIZE)?;
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut trailer = [0u8; TRAILER_LEN];
    f.read_exact(&mut trailer).ok()?;
    Some((file_len, trailer))
}

/// Check a trailer's bounds against the file's length, then read the payload out. A trailer whose offset and
/// length overrun the file is `CorruptBounds`; a seek or read that fails past that check is "no payload".
fn extract_payload(
    f: &mut std::fs::File,
    header: TrailerHeader,
    file_len: u64,
) -> Result<Option<Vec<u8>>, TrailerError> {
    let TrailerHeader { offset, length } = header;
    // widened: a malformed trailer must not wrap
    let end = u128::from(offset) + u128::from(length) + u128::from(TRAILER_SIZE);
    let (true, Ok(len)) = (end <= u128::from(file_len), usize::try_from(length)) else {
        return Err(TrailerError::CorruptBounds { offset, length, file_len });
    };
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Ok(None);
    }
    let mut payload = vec![0u8; len];
    if f.read_exact(&mut payload).is_err() {
        return Ok(None);
    }
    Ok(Some(payload))
}

/// Detect and read an embedded payload from the binary at `exe`.
///
/// - `Ok(Some(payload))`: a valid trailer, and the payload it points at.
/// - `Ok(None)`: no magic; this is a plain binary.
///
/// # Errors
/// The magic matched but the trailer is malformed.
pub fn detect_embedded_payload_from(exe: &Path) -> Result<Option<Vec<u8>>, TrailerError> {
    let Ok(mut f) = std::fs::File::open(exe) else {
        return Ok(None);
    };
    let Some((file_len, trailer)) = read_trailer_bytes(&mut f) else {
        return Ok(None);
    };
    let header = match parse_trailer(&trailer) {
        Ok(Some(header)) => header,
        Ok(None) => return Ok(None),
        Err(version) => return Err(TrailerError::UnknownVersion(version)),
    };
    extract_payload(&mut f, header, file_len)
}

// ---------------------------------------------------------------------------
// Bundling.
// ---------------------------------------------------------------------------

/// Append `payload` and its trailer to a writer. `stub_len` is the length of the stub already written: the
/// payload's offset.
fn write_trailer(f: &mut impl Write, payload: &[u8], stub_len: u64) -> Result<(), BundleError> {
    let length = u32::try_from(payload.len()).map_err(|_| BundleError::TooLarge)?;
    let written = (|| {
        f.write_all(payload)?;
        f.write_all(TRAILER_MAGIC)?;
        f.write_all(&[TRAILER_VERSION])?;
        f.write_all(&stub_len.to_le_bytes())?;
        f.write_all(&length.to_le_bytes())
    })();
    // the writer is memory: it does not fail
    written.map_err(|e| BundleError::OutputUnwritable { path: String::new(), reason: e.to_string() })
}

/// Cut a payload a stub already carries, so that bundling with a bundled binary does not stack a payload and a
/// trailer on every pass.
fn strip_existing_trailer(bytes: &mut Vec<u8>) {
    if let Ok(Some(TrailerHeader { offset, .. })) = parse_trailer(bytes) {
        let trailer_start = bytes.len() - TRAILER_LEN;
        if let Ok(offset) = usize::try_from(offset)
            && offset < trailer_start
        {
            bytes.truncate(offset);
        }
    }
}

/// Bundle the source at `entrypoint` with the binary at `stub` into `output`: the stub (without a payload it may
/// carry), the source, the trailer; executable on Unix. The output is built in memory and written at once, so a
/// failure leaves no half-written file.
fn bundle_with(stub: &Path, entrypoint: &Path, output: &Path) -> Result<(), BundleError> {
    let source = std::fs::read(entrypoint).map_err(|e| BundleError::EntrypointUnreadable {
        path: entrypoint.display().to_string(),
        reason: e.to_string(),
    })?;
    let mut out_bytes = std::fs::read(stub).map_err(|e| BundleError::StubUnreadable {
        reason: format!("cannot read stub {}: {e}", stub.display()),
    })?;
    strip_existing_trailer(&mut out_bytes);
    let stub_len = out_bytes.len() as u64;
    write_trailer(&mut out_bytes, &source, stub_len)?;

    let unwritable =
        |reason: String| BundleError::OutputUnwritable { path: output.display().to_string(), reason };
    std::fs::write(output, &out_bytes).map_err(|e| unwritable(e.to_string()))?;
    set_executable(output).map_err(|e| unwritable(format!("cannot set the executable bit: {e}")))
}

/// Sets the executable bits on `output`. Unix only; nothing elsewhere.
#[cfg(unix)]
fn set_executable(output: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(output)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(output, perms)
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn set_executable(_output: &Path) -> std::io::Result<()> {
    Ok(())
}

#[mlua_ir::module("lib:compile")]
pub mod compile {
    use std::path::Path;

    use mlua_ir::{LuaError, Shape};
    use serde::{Deserialize, Serialize};

    use super::bundle_with;

    /// Why an executable was not built.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum BundleError {
        /// The source file cannot be read: it is not there, or not this process's to read.
        #[error("cannot read {path}: {reason}")]
        EntrypointUnreadable {
            /// The entrypoint asked for.
            path: String,
            /// What the operating system said.
            reason: String,
        },
        /// The running binary, which every bundle starts with, cannot be found or read.
        #[error("{reason}")]
        StubUnreadable {
            /// What could not be done, and what the operating system said.
            reason: String,
        },
        /// The source is larger than a bundle can hold (4 GiB).
        #[error("payload too large (>4 GiB)")]
        TooLarge,
        /// The executable cannot be written, or cannot be made executable.
        #[error("cannot write {path}: {reason}")]
        OutputUnwritable {
            /// The output asked for.
            path: String,
            /// What the operating system said.
            reason: String,
        },
    }

    /// What to bundle, and where to.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct Bundle {
        /// The Lua source file the executable runs.
        pub entrypoint: String,
        /// The executable to write; a file already there is replaced.
        pub output: String,
    }

    /// Build a standalone executable: a copy of the running binary that runs the source at `entrypoint`.
    ///
    /// ```luau
    /// assert(compile.bundle({ entrypoint = "main.lua", output = "my-app" }))
    /// ```
    pub fn bundle(bundle: Bundle) -> Result<(), BundleError> {
        let stub = std::env::current_exe().map_err(|e| BundleError::StubUnreadable {
            reason: format!("cannot resolve current executable: {e}"),
        })?;
        bundle_with(&stub, Path::new(&bundle.entrypoint), Path::new(&bundle.output))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use mlua::{Lua, Value};

    use super::*;

    // --- trailer format ---

    /// `stub`, `payload` and a trailer with this magic, version, offset and length.
    fn file_with(
        stub: &[u8],
        payload: &[u8],
        magic: &[u8; 7],
        version: u8,
        offset: u64,
        length: u32,
    ) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(stub).unwrap();
        f.write_all(payload).unwrap();
        f.write_all(magic).unwrap();
        f.write_all(&[version]).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        f.write_all(&length.to_le_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn write_bundled(stub: &[u8], payload: &[u8]) -> tempfile::NamedTempFile {
        let length = u32::try_from(payload.len()).unwrap();
        file_with(stub, payload, TRAILER_MAGIC, TRAILER_VERSION, stub.len() as u64, length)
    }

    fn detected(f: &tempfile::NamedTempFile) -> Vec<u8> {
        detect_embedded_payload_from(f.path()).expect("no trailer error").expect("should detect payload")
    }

    #[test]
    fn trailer_roundtrip_basic() {
        let payload = b"print('hello from compiled neoc')";
        assert_eq!(detected(&write_bundled(b"fake-neoc-binary-stub", payload)), payload);
    }

    #[test]
    fn trailer_roundtrip_empty_payload() {
        assert!(detected(&write_bundled(b"stub", b"")).is_empty());
    }

    #[test]
    fn trailer_roundtrip_large_payload() {
        let stub = vec![0u8; 1024 * 1024];
        let payload: Vec<u8> = (0..65536u32).map(|i| u8::try_from(i % 251).unwrap()).collect();
        assert_eq!(detected(&write_bundled(&stub, &payload)), payload);
    }

    #[test]
    fn no_trailer_returns_none() {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(b"just a plain binary, no trailer").unwrap();
        f.flush().unwrap();
        assert!(detect_embedded_payload_from(f.path()).expect("no trailer error").is_none());
        // shorter than a trailer, and not there at all
        let mut short = tempfile::NamedTempFile::new().expect("tempfile");
        short.write_all(b"NEOC_BL").unwrap();
        short.flush().unwrap();
        assert!(detect_embedded_payload_from(short.path()).expect("no trailer error").is_none());
        let gone = short.path().with_extension("gone");
        assert!(detect_embedded_payload_from(&gone).expect("no trailer error").is_none());
    }

    #[test]
    fn wrong_magic_returns_none() {
        // seven bytes, as the magic is, so the reader inspects the right ones
        let f = file_with(b"stub-data", b"", b"BADMAGX", TRAILER_VERSION, 9, 0);
        assert!(detect_embedded_payload_from(f.path()).expect("no trailer error").is_none());
    }

    #[test]
    fn unknown_version_returns_error() {
        let f = file_with(b"fake-stub", b"irrelevant", TRAILER_MAGIC, 0xFF, 9, 10);
        assert_eq!(detect_embedded_payload_from(f.path()), Err(TrailerError::UnknownVersion(0xFF)));
    }

    #[test]
    fn corrupt_bounds_returns_error() {
        let f = file_with(b"stub", b"", TRAILER_MAGIC, TRAILER_VERSION, 9999, 100);
        assert_eq!(
            detect_embedded_payload_from(f.path()),
            Err(TrailerError::CorruptBounds { offset: 9999, length: 100, file_len: 24 })
        );
        // an offset and a length that would wrap
        let f = file_with(b"stub", b"", TRAILER_MAGIC, TRAILER_VERSION, u64::MAX, u32::MAX);
        assert!(matches!(detect_embedded_payload_from(f.path()), Err(TrailerError::CorruptBounds { .. })));
    }

    #[test]
    fn the_trailer_constants_are_the_format() {
        assert_eq!(TRAILER_MAGIC, b"NEOC_BL");
        assert_eq!(TRAILER_MAGIC.len(), 7);
        assert_eq!(TRAILER_SIZE, 20);
        assert_eq!(TRAILER_LEN as u64, TRAILER_SIZE);
        assert_eq!(TRAILER_VERSION, 0x01);
    }

    #[test]
    fn write_trailer_field_layout() {
        let stub_len: u64 = 42;
        let payload = b"hello";
        let mut buf: Vec<u8> = Vec::new();
        write_trailer(&mut buf, payload, stub_len).unwrap();

        assert_eq!(&buf[..5], b"hello");
        assert_eq!(&buf[5..12], TRAILER_MAGIC);
        assert_eq!(buf[12], TRAILER_VERSION);
        assert_eq!(u64::from_le_bytes(buf[13..21].try_into().unwrap()), stub_len);
        assert_eq!(u32::from_le_bytes(buf[21..25].try_into().unwrap()), 5);
        assert_eq!(buf.len(), 25);
    }

    // --- bundling ---

    /// A bundle is the stub, the source and the trailer, and the binary finds the source in it.
    #[test]
    fn a_bundle_is_the_stub_and_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let (stub, entry, output) =
            (dir.path().join("stub"), dir.path().join("main.lua"), dir.path().join("app"));
        std::fs::write(&stub, b"fake-neoc-binary").unwrap();
        std::fs::write(&entry, b"print('bundled')").unwrap();
        bundle_with(&stub, &entry, &output).unwrap();

        let written = std::fs::read(&output).unwrap();
        assert_eq!(written.len(), b"fake-neoc-binary".len() + b"print('bundled')".len() + TRAILER_LEN);
        assert!(written.starts_with(b"fake-neoc-binary"));
        assert_eq!(detect_embedded_payload_from(&output).unwrap().unwrap(), b"print('bundled')");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&output).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    /// Bundling with a binary that is itself a bundle replaces its payload: the output is the stub, the second
    /// source and one trailer, not both payloads stacked.
    #[test]
    fn rebundle_does_not_inflate_stub() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        std::fs::write(path("stub"), b"fake-neoc-binary").unwrap();
        std::fs::write(path("a.lua"), b"first-script").unwrap();
        std::fs::write(path("b.lua"), b"second-script").unwrap();
        bundle_with(&path("stub"), &path("a.lua"), &path("first")).unwrap();
        bundle_with(&path("first"), &path("b.lua"), &path("second")).unwrap();

        let second = std::fs::read(path("second")).unwrap();
        assert_eq!(second.len(), b"fake-neoc-binary".len() + b"second-script".len() + TRAILER_LEN);
        assert_eq!(detect_embedded_payload_from(&path("second")).unwrap().unwrap(), b"second-script");
        // and a trailer whose offset points past itself is left alone
        let mut odd = b"stub".to_vec();
        odd.extend_from_slice(TRAILER_MAGIC);
        odd.push(TRAILER_VERSION);
        odd.extend_from_slice(&9999u64.to_le_bytes());
        odd.extend_from_slice(&0u32.to_le_bytes());
        let before = odd.clone();
        strip_existing_trailer(&mut odd);
        assert_eq!(odd, before);
    }

    #[test]
    fn what_cannot_be_read_or_written_is_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        std::fs::write(path("stub"), b"stub").unwrap();
        std::fs::write(path("main.lua"), b"return 1").unwrap();
        let missing = bundle_with(&path("stub"), &path("nope.lua"), &path("out")).unwrap_err();
        assert!(
            matches!(&missing, BundleError::EntrypointUnreadable { path, .. } if path.ends_with("nope.lua"))
        );
        assert!(!path("out").exists(), "nothing is written when the bundle fails");
        let stub = bundle_with(&path("no-stub"), &path("main.lua"), &path("out")).unwrap_err();
        assert!(
            matches!(&stub, BundleError::StubUnreadable { reason } if reason.contains("no-stub")),
            "{stub:?}"
        );
        let output = bundle_with(&path("stub"), &path("main.lua"), &path("no-dir").join("out")).unwrap_err();
        assert!(
            matches!(&output, BundleError::OutputUnwritable { path, .. } if path.ends_with("out")),
            "{output:?}"
        );
    }

    // --- Lua API ---

    fn engine() -> Lua {
        let lua = neoc_vm_core::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:compile")"#).eval().unwrap();
        lua.globals().set("compile", m).unwrap();
        lua
    }

    #[test]
    fn bundle_missing_entrypoint_returns_err() {
        let lua = engine();
        let (value, kind, path, text): (Value, String, String, String) = lua
            .load(
                r#"local value, err = compile.bundle({ entrypoint = "/nonexistent.lua", output = "/tmp/nope" })
                   return value, err.kind, err.path, tostring(err)"#,
            )
            .eval()
            .unwrap();
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), path.as_str()), ("entrypoint_unreadable", "/nonexistent.lua"));
        assert!(
            text.starts_with("lib:compile.bundle: entrypoint_unreadable: cannot read /nonexistent.lua: "),
            "{text}"
        );
    }

    #[test]
    fn bundle_requires_its_fields_and_no_others() {
        let lua = engine();
        for bundle in [
            r#"{ output = "/tmp/nope" }"#,
            r#"{ entrypoint = "main.lua" }"#,
            r#"{ entrypoint = "main.lua", output = 5 }"#,
            r#"{ entrypoint = "main.lua", output = "out", minify = true }"#,
            r#""main.lua""#,
            "nil",
        ] {
            let err = lua.load(format!("return compile.bundle({bundle})")).eval::<Value>().expect_err(bundle);
            assert!(err.to_string().contains("lib:compile.bundle: argument #1 'bundle'"), "{bundle}: {err}");
        }
    }

    /// The module builds executables out of any file it is pointed at: an embedder leaves it out of what it
    /// offers, and then neither the script nor a module it can reach has it.
    #[test]
    fn an_embedder_can_leave_it_out() {
        let lua = neoc_vm_core::make_bare_engine().expect("engine");
        let allowed: Vec<&str> =
            neoc_vm_core::modules::names().filter(|name| *name != "lib:compile").collect();
        neoc_vm_core::modules::install_allowed(&lua, &allowed).expect("install");
        let err =
            lua.load(r#"return require("lib:compile")"#).eval::<Value>().expect_err("refused").to_string();
        assert!(err.contains(r#"unknown module "lib:compile""#), "{err}");
    }
}

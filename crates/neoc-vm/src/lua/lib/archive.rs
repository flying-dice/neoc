//! Tar and zip archive handling — `lib:archive`.
//!
//! Buffer variants (`*_bytes`) are the primary API: the dominant agent/pipeline
//! pattern is bytes in memory (a tarball off an HTTP body), not files on disk.
//! The file-oriented wrappers read/write the filesystem and otherwise share the
//! same core.
//!
//! Compression is integrated internally — a caller never `require`s
//! `lib:compress` to open a `.tar.gz`. On **create**, the codec is inferred from
//! the file extension (`.tar.gz`/`.tgz` → gzip, `.tar.zst` → zstd, bare `.tar`
//! and all in-memory tars → none) and `opts.compression` overrides it; a clash
//! between the explicit option and the extension is an error. On **read**
//! (extract/list) the codec is auto-detected from the stream's magic bytes, so
//! the same call opens a raw, gzipped, or zstd-compressed tar transparently.
//!
//! Security — extraction is safe by default. Every archive entry name is run
//! through [`sanitize_entry_name`] before a byte is written: traversal
//! (`../`), absolute paths (`/etc/...`, `C:\...`), null bytes, over-long names,
//! and Windows separators are rejected; a name that *normalises* back inside
//! the destination (`safe/../safe/x`) is accepted. Symlink/hardlink targets are
//! resolved relative to their own location and rejected if they escape.
//! Decompression bombs are capped by `opts.max_output_size` (default 256 MiB,
//! the same ceiling as `lib:compress`).
//!
//! Open questions from the issue, resolved here:
//! - `max_output_size` default → hardcoded 256 MiB, overridable. Safe by
//!   default so an agent that sets no limit is still protected.
//! - `opts.include` semantics → `globset` (the engine behind `lib:glob`), so
//!   patterns behave identically to `lib:glob`. `*` is not separator-aware
//!   (matches across `/`), matching `lib:glob`'s default.
//! - list metadata fields → `name`, `size`, `mode`, `timestamp`, `is_dir`, and
//!   `compression_ratio`. The ratio is per-entry `compressed/uncompressed` for
//!   zip (each zip entry is compressed independently); for tar it is `nil`,
//!   because a `.tar.gz` is one solid stream with no per-entry compressed size.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::Compression as GzLevel;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use globset::{Glob, GlobSet, GlobSetBuilder};
use mlua::{Lua, Table, Value};
use tracing::instrument;
use walkdir::WalkDir;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};

/// Total decompressed output ceiling, mirroring `lib:compress`'s default.
const DEFAULT_MAX_OUTPUT: usize = 256 * 1024 * 1024;
/// Reject pathological entry names (the issue's >4096-char rule).
const MAX_NAME_LEN: usize = 4096;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:archive` surface through the facade.
///
/// Every function builds its result with `lua.create_string` / `create_table`
/// (a raise via `?`) yet routes validation and IO failures through the
/// `(value, error)` channel — the same mixed shape as `lib:compress`. That mix
/// is outside `try_func`, so each function is registered verbatim with
/// [`Module::set`] (keeping its `archive.<fn>` span + error prefix) and
/// recorded with [`Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:archive")?;

    m.set("tar_create_bytes", lua.create_function(tar_create_bytes)?)?
        .set("tar_create", lua.create_function(tar_create)?)?
        .set("tar_extract", lua.create_function(tar_extract)?)?
        .set("tar_extract_bytes", lua.create_function(tar_extract_bytes)?)?
        .set("tar_list", lua.create_function(tar_list)?)?
        .set("tar_list_bytes", lua.create_function(tar_list_bytes)?)?
        .set("zip_create_bytes", lua.create_function(zip_create_bytes)?)?
        .set("zip_create", lua.create_function(zip_create)?)?
        .set("zip_extract", lua.create_function(zip_extract)?)?
        .set("zip_extract_bytes", lua.create_function(zip_extract_bytes)?)?
        .set("zip_list", lua.create_function(zip_list)?)?
        .set("zip_list_bytes", lua.create_function(zip_list_bytes)?)?;

    m.record_fn(
        "tar_create_bytes",
        &[("entries", "LuaValue"), ("opts", "LuaValue?")],
        "Result<string, string>",
        "Build a tar archive in memory from `{ path, archive_path? }` entries (read from disk) and return its bytes. `opts.compression` (`gzip`/`zstd`/`none`, default none) and `opts.strip_prefix` apply.",
    )
    .record_fn(
        "tar_create",
        &[("path", "string"), ("entries", "LuaValue"), ("opts", "LuaValue?")],
        "Result<bool, string>",
        "Write a tar archive to `path` from `{ path, archive_path? }` entries. Compression is inferred from the extension (`.tar.gz`/`.tgz`/`.tar.zst`); `opts.compression` overrides it and a clash with the extension is an error.",
    )
    .record_fn(
        "tar_extract",
        &[("path_or_bytes", "string"), ("dest", "string"), ("opts", "LuaValue?")],
        "Result<number, string>",
        "Extract a tar archive (file path or raw bytes; compression auto-detected) under `dest`, returning the entry count. `opts.include` filters by glob; `opts.max_output_size` caps output. Traversal-safe.",
    )
    .record_fn(
        "tar_extract_bytes",
        &[("bytes", "string"), ("name", "string"), ("opts", "LuaValue?")],
        "Result<string, string>",
        "Extract a single named entry from an in-memory tar archive and return its contents. `opts.max_output_size` caps output.",
    )
    .record_fn(
        "tar_list",
        &[("path_or_bytes", "string")],
        "Result<LuaValue, string>",
        "List a tar archive (file path or raw bytes), returning `{ name, size, mode, timestamp, is_dir, compression_ratio }` per entry. `compression_ratio` is nil for tar (one solid stream).",
    )
    .record_fn(
        "tar_list_bytes",
        &[("bytes", "string")],
        "Result<LuaValue, string>",
        "List an in-memory tar archive — same metadata as `tar_list`.",
    )
    .record_fn(
        "zip_create_bytes",
        &[("entries", "LuaValue")],
        "Result<string, string>",
        "Build a zip archive in memory from a `{ [name] = contents }` map and return its bytes.",
    )
    .record_fn(
        "zip_create",
        &[("path", "string"), ("entries", "LuaValue"), ("opts", "LuaValue?")],
        "Result<bool, string>",
        "Write a zip archive to `path` from a list of filesystem paths (files or directories). `opts.strip_prefix` applies.",
    )
    .record_fn(
        "zip_extract",
        &[("path_or_bytes", "string"), ("dest", "string"), ("opts", "LuaValue?")],
        "Result<number, string>",
        "Extract a zip archive (file path or raw bytes) under `dest`, returning the entry count. `opts.include` filters by glob; `opts.max_output_size` caps output. Traversal-safe.",
    )
    .record_fn(
        "zip_extract_bytes",
        &[("bytes", "string"), ("name", "string"), ("opts", "LuaValue?")],
        "Result<string, string>",
        "Extract a single named entry from an in-memory zip archive and return its contents. `opts.max_output_size` caps output.",
    )
    .record_fn(
        "zip_list",
        &[("path_or_bytes", "string")],
        "Result<LuaValue, string>",
        "List a zip archive (file path or raw bytes), returning `{ name, size, mode, timestamp, is_dir, compression_ratio }` per entry.",
    )
    .record_fn(
        "zip_list_bytes",
        &[("bytes", "string")],
        "Result<LuaValue, string>",
        "List an in-memory zip archive — same metadata as `zip_list`.",
    );

    Ok(m)
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Comp {
    None,
    Gzip,
    Zstd,
}

impl Comp {
    /// Parses an explicit `opts.compression` label.
    fn from_label(s: &str) -> Result<Self, String> {
        match s {
            "none" => Ok(Self::None),
            "gzip" => Ok(Self::Gzip),
            "zstd" => Ok(Self::Zstd),
            other => Err(format!(
                "unknown compression {other:?} (expected 'gzip', 'zstd', or 'none')"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Zstd => "zstd",
        }
    }
}

/// Infers the codec from a tar archive's file extension.
fn comp_from_ext(path: &str) -> Comp {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Comp::Gzip
    } else if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        Comp::Zstd
    } else {
        Comp::None
    }
}

/// Detects the codec of a stream from its leading magic bytes, so reads open
/// raw / gzip / zstd tars transparently.
fn comp_from_magic(bytes: &[u8]) -> Comp {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        Comp::Gzip
    } else if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Comp::Zstd
    } else {
        Comp::None
    }
}

/// Reconciles the inferred-from-extension codec with an explicit
/// `opts.compression`: the explicit value wins, but a clash with a
/// compression-bearing extension is an error (the issue's hard requirement).
fn resolve_create_comp(path: &str, explicit: Option<&str>) -> Result<Comp, String> {
    let inferred = comp_from_ext(path);
    let Some(label) = explicit else {
        return Ok(inferred);
    };
    let chosen = Comp::from_label(label)?;
    if inferred != Comp::None && inferred != chosen {
        return Err(format!(
            "compression {:?} conflicts with the extension of {path:?} ({})",
            chosen.label(),
            inferred.label()
        ));
    }
    Ok(chosen)
}

// ---------------------------------------------------------------------------
// Path safety — the extraction guarantee
// ---------------------------------------------------------------------------

/// True for a Windows drive prefix (`C:`), which `std::path` only recognises as
/// absolute when running on Windows — so we detect it explicitly to reject
/// `C:\...` even on a Unix host.
fn has_drive_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

/// Folds a path's components onto `stack`, lexically resolving `.`/`..`.
///
/// `..` pops; an underflow (popping past the start) means the path escapes its
/// root and is rejected. Root and prefix components are absolute and rejected.
/// The stack carries no `..` on return, so the join can never climb out.
fn push_components(stack: &mut Vec<OsString>, path: &Path) -> Result<(), String> {
    for comp in path.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => {
                return Err("absolute paths are not allowed".to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err("path escapes the destination directory".to_string());
                }
            }
            Component::Normal(c) => stack.push(c.to_os_string()),
        }
    }
    Ok(())
}

/// Validates one archive entry name and returns the safe **relative** path to
/// join under the destination, or an error describing the rejection.
fn sanitize_entry_name(raw: &str) -> Result<PathBuf, String> {
    if raw.as_bytes().contains(&0) {
        return Err(format!("entry name contains a null byte: {raw:?}"));
    }
    if raw.len() > MAX_NAME_LEN {
        return Err(format!(
            "entry name exceeds {MAX_NAME_LEN} bytes ({} bytes)",
            raw.len()
        ));
    }
    if raw == "." || raw == ".." {
        return Err(format!("entry name {raw:?} is not a valid path"));
    }
    if has_drive_prefix(raw) {
        return Err(format!("entry name {raw:?} is an absolute (drive) path"));
    }
    // Normalise Windows separators so `a\b\..\c` is analysed as `a/b/../c`.
    let normalised = raw.replace('\\', "/");
    let mut stack: Vec<OsString> = Vec::new();
    push_components(&mut stack, Path::new(&normalised))?;
    if stack.is_empty() {
        return Err(format!(
            "entry name {raw:?} resolves to the destination root"
        ));
    }
    Ok(stack.iter().collect())
}

/// Validates a symlink/hardlink target against escape. `link_rel` is the
/// already-sanitised location of the link inside the destination; the target is
/// resolved relative to the link's parent, exactly as the OS would resolve it.
fn check_link_target(link_rel: &Path, target: &str) -> Result<(), String> {
    if target.as_bytes().contains(&0) {
        return Err(format!("link target contains a null byte: {target:?}"));
    }
    if has_drive_prefix(target) {
        return Err(format!(
            "link target {target:?} is an absolute (drive) path"
        ));
    }
    let normalised = target.replace('\\', "/");
    let mut stack: Vec<OsString> = Vec::new();
    // Seed with the link's parent directory; the parent is sanitised, so it
    // carries no `..` and underflow can only come from the target itself.
    if let Some(parent) = link_rel.parent() {
        push_components(&mut stack, parent)?;
    }
    push_components(&mut stack, Path::new(&normalised))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Decompression-bomb cap
// ---------------------------------------------------------------------------

/// A `Read` adapter that errors once more than `remaining` bytes are produced,
/// bounding decompressed output regardless of how small the compressed input
/// is.
struct LimitReader<R> {
    inner: R,
    remaining: usize,
    /// The configured ceiling, retained so the overrun error cites the limit
    /// actually in force rather than the default (`remaining` is spent to 0).
    limit: usize,
}

impl<R: Read> LimitReader<R> {
    fn new(inner: R, limit: usize) -> Self {
        Self {
            inner,
            remaining: limit,
            limit,
        }
    }
}

impl<R: Read> Read for LimitReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            // At the limit: a further non-empty read means the stream overruns.
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other(format!(
                    "decompressed output exceeds max_output_size ({} bytes)",
                    self.limit
                ))),
            };
        }
        let cap = buf.len().min(self.remaining);
        let n = self.inner.read(&mut buf[..cap])?;
        self.remaining -= n;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------

fn opt_string(opts: Option<&Table>, key: &str, prefix: &str) -> Result<Option<String>, String> {
    let Some(t) = opts else { return Ok(None) };
    let v: Value = t.get(key).map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(None),
        Value::String(s) => Ok(Some(s.to_string_lossy())),
        _ => Err(format!("{prefix}: {key} must be a string")),
    }
}

/// Reads `opts.max_output_size`, defaulting to [`DEFAULT_MAX_OUTPUT`]. Must be a
/// positive integer.
fn opt_max_output(opts: Option<&Table>, prefix: &str) -> Result<usize, String> {
    let Some(t) = opts else {
        return Ok(DEFAULT_MAX_OUTPUT);
    };
    let v: Value = t
        .get("max_output_size")
        .map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(DEFAULT_MAX_OUTPUT),
        Value::Integer(n) if n > 0 => Ok(n as usize),
        Value::Integer(_) => Err(format!("{prefix}: max_output_size must be > 0")),
        Value::Number(f) if f.is_finite() && f.fract() == 0.0 && f > 0.0 => Ok(f as usize),
        Value::Number(_) => Err(format!(
            "{prefix}: max_output_size must be a positive integer"
        )),
        _ => Err(format!("{prefix}: max_output_size must be a number")),
    }
}

/// Builds the optional `opts.include` glob filter. An absent or empty list means
/// no filtering (every entry is extracted).
fn build_include(opts: Option<&Table>, prefix: &str) -> Result<Option<GlobSet>, String> {
    let Some(t) = opts else { return Ok(None) };
    let v: Value = t.get("include").map_err(|e| format!("{prefix}: {e}"))?;
    let arr = match v {
        Value::Nil => return Ok(None),
        Value::Table(arr) => arr,
        _ => {
            return Err(format!(
                "{prefix}: include must be a table of glob patterns"
            ));
        }
    };
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for pat in arr.sequence_values::<String>() {
        let pat = pat.map_err(|e| format!("{prefix}: include: {e}"))?;
        let glob = Glob::new(&pat).map_err(|e| format!("{prefix}: include: {e}"))?;
        builder.add(glob);
        any = true;
    }
    if !any {
        return Ok(None);
    }
    let set = builder
        .build()
        .map_err(|e| format!("{prefix}: include: {e}"))?;
    Ok(Some(set))
}

/// Resolves the bare `path_or_bytes` argument: a valid UTF-8 string naming an
/// existing file is read from disk; anything else (non-UTF-8 archive bytes, or a
/// path that is not a file) is taken as the archive bytes verbatim.
fn resolve_source(arg: &mlua::LuaString) -> Result<Vec<u8>, String> {
    let bytes = arg.as_bytes();
    if let Ok(s) = std::str::from_utf8(&bytes) {
        let p = Path::new(s);
        if p.is_file() {
            return fs::read(p).map_err(|e| format!("{s}: {e}"));
        }
    }
    Ok(bytes.to_vec())
}

// ---------------------------------------------------------------------------
// Entry metadata
// ---------------------------------------------------------------------------

struct EntryMeta {
    name: String,
    size: u64,
    mode: u32,
    timestamp: i64,
    is_dir: bool,
    /// Per-entry `compressed/uncompressed`; `None` where undefined (all tar).
    ratio: Option<f64>,
}

fn entry_table(lua: &Lua, meta: &EntryMeta) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("name", lua.create_string(&meta.name)?)?;
    t.set("size", meta.size)?;
    t.set("mode", meta.mode)?;
    t.set("timestamp", meta.timestamp)?;
    t.set("is_dir", meta.is_dir)?;
    match meta.ratio {
        Some(r) => t.set("compression_ratio", r)?,
        None => t.set("compression_ratio", Value::Nil)?,
    }
    Ok(t)
}

fn entries_to_table(lua: &Lua, metas: &[EntryMeta]) -> mlua::Result<Table> {
    let out = lua.create_table()?;
    for (i, meta) in metas.iter().enumerate() {
        out.set(i + 1, entry_table(lua, meta)?)?;
    }
    Ok(out)
}

/// Howard Hinnant's days-from-civil, used to turn a zip DOS timestamp into unix
/// seconds without pulling a date crate in for one conversion.
fn civil_to_unix(year: i64, month: u32, day: u32, h: u32, mi: u32, s: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(s)
}

// ---------------------------------------------------------------------------
// Pack-source collection (shared by tar + zip create)
// ---------------------------------------------------------------------------

/// Expands one pack source into `(disk_file, archive_name)` pairs: a file maps
/// to itself; a directory is walked and each file mapped under `base_name`.
/// `strip_prefix`, when set, is removed from the front of every archive name.
fn expand_source(
    disk: &Path,
    base_name: &str,
    strip: Option<&str>,
    out: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    let meta = fs::symlink_metadata(disk).map_err(|e| format!("{}: {e}", disk.display()))?;
    if meta.is_file() {
        out.push((disk.to_path_buf(), strip_name(base_name, strip)));
        return Ok(());
    }
    if meta.is_dir() {
        let base = base_name.trim_end_matches('/');
        for entry in WalkDir::new(disk).follow_links(false) {
            let entry = entry.map_err(|e| format!("{}: {e}", disk.display()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(disk)
                .map_err(|e| format!("{}: {e}", disk.display()))?;
            let rel = rel.to_string_lossy().replace('\\', "/");
            let name = if base.is_empty() {
                rel
            } else {
                format!("{base}/{rel}")
            };
            out.push((entry.path().to_path_buf(), strip_name(&name, strip)));
        }
        return Ok(());
    }
    Err(format!(
        "{}: not a regular file or directory",
        disk.display()
    ))
}

/// Removes `strip` from the front of an archive name, then any leftover leading
/// separator, so `strip_prefix = "src/"` turns `src/main.rs` into `main.rs`.
fn strip_name(name: &str, strip: Option<&str>) -> String {
    let stripped = match strip {
        Some(prefix) => name.strip_prefix(prefix).unwrap_or(name),
        None => name,
    };
    stripped.trim_start_matches('/').to_string()
}

// ---------------------------------------------------------------------------
// Extraction helpers (shared by tar + zip)
// ---------------------------------------------------------------------------

/// Applies the optional include filter, then validates the entry name. Returns
/// `None` when the entry is filtered out, `Some(Ok(rel))` for a safe relative
/// path, or `Some(Err)` when the name is unsafe. Tar and zip extraction both go
/// through here so the filter-then-sanitise step — the extraction security
/// boundary — has a single implementation.
fn accept_entry(raw: &str, include: Option<&GlobSet>) -> Option<Result<PathBuf, String>> {
    if include.is_some_and(|set| !set.is_match(raw.replace('\\', "/"))) {
        return None;
    }
    Some(sanitize_entry_name(raw))
}

/// Writes one regular-file entry under `dest` at the sanitised relative path,
/// creating parent directories first.
fn write_file_entry(dest: &Path, rel: &Path, content: &[u8]) -> Result<(), String> {
    let target = dest.join(rel);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(&target, content).map_err(|e| format!("{}: {e}", target.display()))
}

// ---------------------------------------------------------------------------
// Tar core
// ---------------------------------------------------------------------------

/// Reads tar `{ path, archive_path? }` entry specs into pack sources.
fn parse_tar_entries(
    entries: &Table,
    strip: Option<&str>,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    for entry in entries.sequence_values::<Table>() {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path: String = entry
            .get("path")
            .map_err(|_| "each entry needs a string `path`".to_string())?;
        let archive_path: Option<String> = match entry.get::<Value>("archive_path") {
            Ok(Value::Nil) => None,
            Ok(Value::String(s)) => Some(s.to_string_lossy()),
            Ok(_) => return Err("archive_path must be a string".to_string()),
            Err(e) => return Err(format!("archive_path: {e}")),
        };
        let base = archive_path.unwrap_or_else(|| path.clone());
        expand_source(Path::new(&path), &base, strip, &mut out)?;
    }
    Ok(out)
}

fn append_tar<W: Write>(
    builder: &mut tar::Builder<W>,
    sources: &[(PathBuf, String)],
) -> Result<(), String> {
    for (disk, name) in sources {
        builder
            .append_path_with_name(disk, name)
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

/// Builds a tar archive (optionally compressed) from pack sources, in memory.
fn build_tar(sources: &[(PathBuf, String)], comp: Comp) -> Result<Vec<u8>, String> {
    match comp {
        Comp::None => {
            let mut b = tar::Builder::new(Vec::new());
            append_tar(&mut b, sources)?;
            b.into_inner().map_err(|e| e.to_string())
        }
        Comp::Gzip => {
            let mut b = tar::Builder::new(GzEncoder::new(Vec::new(), GzLevel::default()));
            append_tar(&mut b, sources)?;
            let enc = b.into_inner().map_err(|e| e.to_string())?;
            enc.finish().map_err(|e| e.to_string())
        }
        Comp::Zstd => {
            let enc = zstd::Encoder::new(Vec::new(), 3).map_err(|e| e.to_string())?;
            let mut b = tar::Builder::new(enc);
            append_tar(&mut b, sources)?;
            let enc = b.into_inner().map_err(|e| e.to_string())?;
            enc.finish().map_err(|e| e.to_string())
        }
    }
}

/// Opens a (possibly compressed) tar stream for reading, capping decompressed
/// output at `cap`.
fn tar_archive(bytes: &[u8], cap: usize) -> Result<tar::Archive<Box<dyn Read + '_>>, String> {
    let reader: Box<dyn Read + '_> = match comp_from_magic(bytes) {
        Comp::None => Box::new(LimitReader::new(bytes, cap)),
        Comp::Gzip => Box::new(LimitReader::new(GzDecoder::new(bytes), cap)),
        Comp::Zstd => Box::new(LimitReader::new(
            zstd::Decoder::new(bytes).map_err(|e| e.to_string())?,
            cap,
        )),
    };
    Ok(tar::Archive::new(reader))
}

fn tar_list_entries(bytes: &[u8]) -> Result<Vec<EntryMeta>, String> {
    let mut archive = tar_archive(bytes, DEFAULT_MAX_OUTPUT)?;
    let mut out = Vec::new();
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let header = entry.header();
        let name = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        out.push(EntryMeta {
            name,
            size: entry.size(),
            mode: header.mode().unwrap_or(0),
            timestamp: header.mtime().map(|m| m as i64).unwrap_or(0),
            is_dir: header.entry_type().is_dir(),
            ratio: None,
        });
    }
    Ok(out)
}

fn tar_extract_all(
    bytes: &[u8],
    dest: &Path,
    include: Option<&GlobSet>,
    cap: usize,
) -> Result<usize, String> {
    let mut archive = tar_archive(bytes, cap)?;
    let mut count = 0usize;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let raw = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let Some(rel) = accept_entry(&raw, include) else {
            continue;
        };
        let rel = rel?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            let target = dest.join(&rel);
            fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        } else if entry_type.is_symlink() || entry_type.is_hard_link() {
            let link = entry
                .link_name()
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{raw}: link entry has no target"))?;
            let link = link.to_string_lossy();
            check_link_target(&rel, &link)?;
            write_link(&dest.join(&rel), &link, entry_type.is_symlink())?;
        } else {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            write_file_entry(dest, &rel, &buf)?;
        }
        count += 1;
    }
    Ok(count)
}

fn tar_extract_named(bytes: &[u8], name: &str, cap: usize) -> Result<Vec<u8>, String> {
    let want = name.replace('\\', "/");
    let mut archive = tar_archive(bytes, cap)?;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let entry_name = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if entry_name == want {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            return Ok(buf);
        }
    }
    Err(format!("entry {name:?} not found in archive"))
}

// ---------------------------------------------------------------------------
// Zip core
// ---------------------------------------------------------------------------

fn zip_options(mode: Option<u32>) -> zip::write::SimpleFileOptions {
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    match mode {
        Some(m) => opts.unix_permissions(m),
        None => opts,
    }
}

/// Builds a zip from an in-memory `{ [name] = contents }` map.
fn build_zip_bytes(entries: &Table) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for pair in entries.pairs::<String, mlua::LuaString>() {
        let (name, content) = pair.map_err(|e| format!("entry: {e}"))?;
        writer
            .start_file(&name, zip_options(Some(0o644)))
            .map_err(|e| format!("{name}: {e}"))?;
        writer
            .write_all(&content.as_bytes())
            .map_err(|e| format!("{name}: {e}"))?;
    }
    let cursor = writer.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

/// Builds a zip on `writer` from a list of filesystem paths (files/dirs).
fn build_zip_files<W: Write + io::Seek>(
    writer: W,
    entries: &Table,
    strip: Option<&str>,
) -> Result<(), String> {
    let mut sources = Vec::new();
    for path in entries.sequence_values::<String>() {
        let path = path.map_err(|e| format!("entry: {e}"))?;
        expand_source(Path::new(&path), &path, strip, &mut sources)?;
    }
    let mut zw = zip::ZipWriter::new(writer);
    for (disk, name) in &sources {
        let mode = fs::metadata(disk).ok().map(|m| unix_mode(&m));
        zw.start_file(name, zip_options(mode))
            .map_err(|e| format!("{name}: {e}"))?;
        let data = fs::read(disk).map_err(|e| format!("{}: {e}", disk.display()))?;
        zw.write_all(&data).map_err(|e| format!("{name}: {e}"))?;
    }
    zw.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(unix)]
fn unix_mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

#[cfg(not(unix))]
fn unix_mode(_meta: &fs::Metadata) -> u32 {
    0o644
}

fn zip_mtime<R: Read + ?Sized>(file: &zip::read::ZipFile<'_, R>) -> i64 {
    let Some(dt) = file.last_modified() else {
        return 0;
    };
    civil_to_unix(
        i64::from(dt.year()),
        u32::from(dt.month()),
        u32::from(dt.day()),
        u32::from(dt.hour()),
        u32::from(dt.minute()),
        u32::from(dt.second()),
    )
}

fn zip_ratio(size: u64, compressed: u64) -> Option<f64> {
    if size == 0 {
        None
    } else {
        Some(compressed as f64 / size as f64)
    }
}

fn zip_list_entries(bytes: &[u8]) -> Result<Vec<EntryMeta>, String> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        out.push(EntryMeta {
            name: file.name().map_err(|e| e.to_string())?.into_owned(),
            size: file.size(),
            mode: file.unix_mode().unwrap_or(0),
            timestamp: zip_mtime(&file),
            is_dir: file.is_dir(),
            ratio: zip_ratio(file.size(), file.compressed_size()),
        });
    }
    Ok(out)
}

fn zip_extract_all(
    bytes: &[u8],
    dest: &Path,
    include: Option<&GlobSet>,
    cap: usize,
) -> Result<usize, String> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut remaining = cap;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let raw = file.name().map_err(|e| e.to_string())?.into_owned();
        let Some(rel) = accept_entry(&raw, include) else {
            continue;
        };
        let rel = rel?;
        if file.is_dir() {
            let target = dest.join(&rel);
            fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        } else {
            let mut buf = Vec::new();
            LimitReader::new(&mut file, remaining)
                .read_to_end(&mut buf)
                .map_err(|e| e.to_string())?;
            remaining -= buf.len();
            write_file_entry(dest, &rel, &buf)?;
        }
        count += 1;
    }
    Ok(count)
}

fn zip_extract_named(bytes: &[u8], name: &str, cap: usize) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut file = archive
        .by_name(name)
        .map_err(|_| format!("entry {name:?} not found in archive"))?;
    let mut buf = Vec::new();
    LimitReader::new(&mut file, cap)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Creates a symlink (or hard link) at `target` pointing at `link`. Symlink
/// creation is unix-only; on other platforms it is an explicit error rather
/// than a silent skip.
#[cfg(unix)]
fn write_link(target: &Path, link: &str, symlink: bool) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if symlink {
        std::os::unix::fs::symlink(link, target).map_err(|e| format!("{}: {e}", target.display()))
    } else {
        let src = target.parent().unwrap_or(Path::new(".")).join(link);
        fs::hard_link(&src, target).map_err(|e| format!("{}: {e}", target.display()))
    }
}

#[cfg(not(unix))]
fn write_link(_target: &Path, _link: &str, _symlink: bool) -> Result<(), String> {
    Err("link entries are only supported on unix".to_string())
}

// ---------------------------------------------------------------------------
// Lua-facing functions
// ---------------------------------------------------------------------------

#[instrument(
    name = "archive.tar_create_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn tar_create_bytes(
    lua: &Lua,
    (entries, opts): (Table, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_create_bytes";
    let strip = match opt_string(opts.as_ref(), "strip_prefix", prefix) {
        Ok(s) => s,
        Err(msg) => return err(lua, msg),
    };
    let comp = match opt_string(opts.as_ref(), "compression", prefix) {
        Ok(Some(label)) => match Comp::from_label(&label) {
            Ok(c) => c,
            Err(msg) => return err(lua, format!("{prefix}: {msg}")),
        },
        Ok(None) => Comp::None,
        Err(msg) => return err(lua, msg),
    };
    let sources = match parse_tar_entries(&entries, strip.as_deref()) {
        Ok(s) => s,
        Err(msg) => return err(lua, format!("{prefix}: {msg}")),
    };
    match build_tar(&sources, comp) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(name = "archive.tar_create", target = "lua::lib::archive", skip_all)]
fn tar_create(
    lua: &Lua,
    (path, entries, opts): (String, Table, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_create";
    let strip = match opt_string(opts.as_ref(), "strip_prefix", prefix) {
        Ok(s) => s,
        Err(msg) => return err(lua, msg),
    };
    let explicit = match opt_string(opts.as_ref(), "compression", prefix) {
        Ok(s) => s,
        Err(msg) => return err(lua, msg),
    };
    let comp = match resolve_create_comp(&path, explicit.as_deref()) {
        Ok(c) => c,
        Err(msg) => return err(lua, format!("{prefix}: {msg}")),
    };
    let sources = match parse_tar_entries(&entries, strip.as_deref()) {
        Ok(s) => s,
        Err(msg) => return err(lua, format!("{prefix}: {msg}")),
    };
    let bytes = match build_tar(&sources, comp) {
        Ok(b) => b,
        Err(msg) => return from_err(lua, prefix, msg),
    };
    match fs::write(&path, &bytes) {
        Ok(()) => ok(lua, true),
        Err(e) => from_err(lua, prefix, format!("{path}: {e}")),
    }
}

#[instrument(name = "archive.tar_extract", target = "lua::lib::archive", skip_all)]
fn tar_extract(
    lua: &Lua,
    (source, dest, opts): (mlua::LuaString, String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_extract";
    let cap = match opt_max_output(opts.as_ref(), prefix) {
        Ok(c) => c,
        Err(msg) => return err(lua, msg),
    };
    let include = match build_include(opts.as_ref(), prefix) {
        Ok(i) => i,
        Err(msg) => return err(lua, msg),
    };
    let bytes = match resolve_source(&source) {
        Ok(b) => b,
        Err(msg) => return from_err(lua, prefix, msg),
    };
    match tar_extract_all(&bytes, Path::new(&dest), include.as_ref(), cap) {
        Ok(n) => ok(lua, n as i64),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(
    name = "archive.tar_extract_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn tar_extract_bytes(
    lua: &Lua,
    (bytes, name, opts): (mlua::LuaString, String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_extract_bytes";
    let cap = match opt_max_output(opts.as_ref(), prefix) {
        Ok(c) => c,
        Err(msg) => return err(lua, msg),
    };
    match tar_extract_named(&bytes.as_bytes(), &name, cap) {
        Ok(content) => ok(lua, lua.create_string(&content)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(name = "archive.tar_list", target = "lua::lib::archive", skip_all)]
fn tar_list(lua: &Lua, source: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_list";
    let bytes = match resolve_source(&source) {
        Ok(b) => b,
        Err(msg) => return from_err(lua, prefix, msg),
    };
    match tar_list_entries(&bytes) {
        Ok(metas) => ok(lua, entries_to_table(lua, &metas)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(
    name = "archive.tar_list_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn tar_list_bytes(lua: &Lua, bytes: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.tar_list_bytes";
    match tar_list_entries(&bytes.as_bytes()) {
        Ok(metas) => ok(lua, entries_to_table(lua, &metas)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(
    name = "archive.zip_create_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn zip_create_bytes(lua: &Lua, entries: Table) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_create_bytes";
    match build_zip_bytes(&entries) {
        Ok(bytes) => ok(lua, lua.create_string(&bytes)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(name = "archive.zip_create", target = "lua::lib::archive", skip_all)]
fn zip_create(
    lua: &Lua,
    (path, entries, opts): (String, Table, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_create";
    let strip = match opt_string(opts.as_ref(), "strip_prefix", prefix) {
        Ok(s) => s,
        Err(msg) => return err(lua, msg),
    };
    let file = match fs::File::create(&path) {
        Ok(f) => f,
        Err(e) => return from_err(lua, prefix, format!("{path}: {e}")),
    };
    match build_zip_files(file, &entries, strip.as_deref()) {
        Ok(()) => ok(lua, true),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(name = "archive.zip_extract", target = "lua::lib::archive", skip_all)]
fn zip_extract(
    lua: &Lua,
    (source, dest, opts): (mlua::LuaString, String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_extract";
    let cap = match opt_max_output(opts.as_ref(), prefix) {
        Ok(c) => c,
        Err(msg) => return err(lua, msg),
    };
    let include = match build_include(opts.as_ref(), prefix) {
        Ok(i) => i,
        Err(msg) => return err(lua, msg),
    };
    let bytes = match resolve_source(&source) {
        Ok(b) => b,
        Err(msg) => return from_err(lua, prefix, msg),
    };
    match zip_extract_all(&bytes, Path::new(&dest), include.as_ref(), cap) {
        Ok(n) => ok(lua, n as i64),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(
    name = "archive.zip_extract_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn zip_extract_bytes(
    lua: &Lua,
    (bytes, name, opts): (mlua::LuaString, String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_extract_bytes";
    let cap = match opt_max_output(opts.as_ref(), prefix) {
        Ok(c) => c,
        Err(msg) => return err(lua, msg),
    };
    match zip_extract_named(&bytes.as_bytes(), &name, cap) {
        Ok(content) => ok(lua, lua.create_string(&content)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(name = "archive.zip_list", target = "lua::lib::archive", skip_all)]
fn zip_list(lua: &Lua, source: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_list";
    let bytes = match resolve_source(&source) {
        Ok(b) => b,
        Err(msg) => return from_err(lua, prefix, msg),
    };
    match zip_list_entries(&bytes) {
        Ok(metas) => ok(lua, entries_to_table(lua, &metas)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[instrument(
    name = "archive.zip_list_bytes",
    target = "lua::lib::archive",
    skip_all
)]
fn zip_list_bytes(lua: &Lua, bytes: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    let prefix = "archive.zip_list_bytes";
    match zip_list_entries(&bytes.as_bytes()) {
        Ok(metas) => ok(lua, entries_to_table(lua, &metas)?),
        Err(msg) => from_err(lua, prefix, msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- sanitize_entry_name: the extraction security matrix (host-independent) ---

    #[test]
    fn accepts_plain_relative_paths() {
        assert_eq!(
            sanitize_entry_name("src/main.rs").unwrap(),
            PathBuf::from("src/main.rs")
        );
    }

    #[test]
    fn accepts_paths_that_normalise_within_dest() {
        // The issue's explicit accept case.
        assert_eq!(
            sanitize_entry_name("safe/../safe/file.txt").unwrap(),
            PathBuf::from("safe/file.txt")
        );
    }

    #[test]
    fn rejects_parent_traversal() {
        assert!(sanitize_entry_name("../../etc/passwd").is_err());
        assert!(sanitize_entry_name("a/../../b").is_err());
    }

    #[test]
    fn rejects_absolute_paths() {
        assert!(sanitize_entry_name("/etc/passwd").is_err());
    }

    #[test]
    fn rejects_windows_drive_and_backslash_escape() {
        assert!(sanitize_entry_name("C:\\Windows\\System32").is_err());
        // Backslash normalises to '/', so this is a parent escape, not a name.
        assert!(sanitize_entry_name("..\\..\\secret").is_err());
    }

    #[test]
    fn rejects_null_byte() {
        assert!(sanitize_entry_name("a\0b").is_err());
    }

    #[test]
    fn rejects_dot_and_dotdot_names() {
        assert!(sanitize_entry_name(".").is_err());
        assert!(sanitize_entry_name("..").is_err());
    }

    #[test]
    fn rejects_overlong_names() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        assert!(sanitize_entry_name(&long).is_err());
    }

    #[test]
    fn backslash_separator_normalises_to_safe_path() {
        assert_eq!(
            sanitize_entry_name("a\\b\\c.txt").unwrap(),
            PathBuf::from("a/b/c.txt")
        );
    }

    // --- check_link_target ---

    #[test]
    fn link_target_within_dest_is_ok() {
        // dest/sub/link -> ../sibling resolves to dest/sibling (inside).
        assert!(check_link_target(Path::new("sub/link"), "../sibling").is_ok());
    }

    #[test]
    fn link_target_escaping_dest_is_rejected() {
        // dest/link -> ../../etc/passwd escapes.
        assert!(check_link_target(Path::new("link"), "../../etc/passwd").is_err());
        assert!(check_link_target(Path::new("link"), "/etc/passwd").is_err());
    }

    // --- compression resolution ---

    #[test]
    fn infers_compression_from_extension() {
        assert!(comp_from_ext("a.tar.gz") == Comp::Gzip);
        assert!(comp_from_ext("a.tgz") == Comp::Gzip);
        assert!(comp_from_ext("a.tar.zst") == Comp::Zstd);
        assert!(comp_from_ext("a.tar") == Comp::None);
    }

    #[test]
    fn explicit_compression_clashing_with_extension_errors() {
        assert!(resolve_create_comp("a.tar.gz", Some("zstd")).is_err());
        // Matching is fine.
        assert!(resolve_create_comp("a.tar.gz", Some("gzip")).is_ok());
        // No compression-bearing extension: explicit wins freely.
        assert!(resolve_create_comp("a.tar", Some("gzip")).unwrap() == Comp::Gzip);
    }

    #[test]
    fn magic_detection_round_trips() {
        let sources: Vec<(PathBuf, String)> = Vec::new();
        let gz = build_tar(&sources, Comp::Gzip).unwrap();
        assert!(comp_from_magic(&gz) == Comp::Gzip);
        let zst = build_tar(&sources, Comp::Zstd).unwrap();
        assert!(comp_from_magic(&zst) == Comp::Zstd);
        let raw = build_tar(&sources, Comp::None).unwrap();
        assert!(comp_from_magic(&raw) == Comp::None);
    }

    // --- link confinement through extraction (closes the symlink/hardlink branch) ---

    /// A raw single-entry tar whose only member is a link of `ty`, named `name`
    /// and pointing at `target`. Drives a crafted escaping link through real
    /// extraction rather than testing `check_link_target` in isolation.
    fn tar_with_link(name: &str, target: &str, ty: tar::EntryType) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(ty);
        header.set_size(0);
        header.set_mode(0o777);
        builder
            .append_link(&mut header, name, target)
            .expect("craft link entry");
        builder.into_inner().expect("finish tar")
    }

    #[test]
    fn extraction_rejects_escaping_symlink_entry() {
        // Confinement must fire through tar_extract_all — before a byte is
        // written — not only in the isolated check_link_target unit.
        let bytes = tar_with_link("link", "../../etc/passwd", tar::EntryType::symlink());
        let err = tar_extract_all(&bytes, Path::new("dest"), None, DEFAULT_MAX_OUTPUT)
            .expect_err("escaping symlink must be rejected");
        assert!(
            err.contains("escapes the destination"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn extraction_rejects_escaping_hardlink_entry() {
        // Same gate covers the is_hard_link() half of the branch.
        let bytes = tar_with_link("link", "../../etc/passwd", tar::EntryType::hard_link());
        let err = tar_extract_all(&bytes, Path::new("dest"), None, DEFAULT_MAX_OUTPUT)
            .expect_err("escaping hardlink must be rejected");
        assert!(
            err.contains("escapes the destination"),
            "unexpected error: {err}"
        );
    }

    // --- decompression-bomb cap ---

    #[test]
    fn limit_reader_error_cites_configured_cap() {
        // Overrun message reports the limit in force, not the 256 MiB default.
        let data = [0u8; 64];
        let mut reader = LimitReader::new(&data[..], 8);
        let mut sink = Vec::new();
        let err = reader
            .read_to_end(&mut sink)
            .expect_err("reading past the cap must error");
        assert!(
            err.to_string().contains("8 bytes"),
            "unexpected error: {err}"
        );
    }
}

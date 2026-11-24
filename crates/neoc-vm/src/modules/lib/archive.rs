//! Tar and zip archives: `lib:archive` (the `tar` and `zip` crates).
//!
//! An archive is a value: `Tar.open(path)` or `Tar.from_bytes(bytes)` (and the same for `Zip`), then `list()`,
//! `extract(dest)` and `read(name)`. `create` builds one in memory and `create_file` writes one to disk.
//!
//! Compression of a tar is part of it: no script needs `lib:compress` to open a `.tar.gz`. On reading, the codec is
//! found from the first bytes (raw, gzip or zstd). `create` compresses as `options.compression` says (not at all
//! when not given); `create_file` goes by the file's extension (`.tar.gz`/`.tgz` is gzip, `.tar.zst`/`.tzst` is
//! zstd), and an option that says otherwise than the extension is an error.
//!
//! Extraction is safe by default. Every entry name goes through [`sanitize_entry_name`] before a byte is written:
//! `..` out of the destination, an absolute path (`/etc/...`, `C:\...`), a null byte and a name longer than 4096
//! bytes are refused; a name that comes back inside the destination (`safe/../safe/x`) is taken. A symlink's or a
//! hard link's target is resolved from where the link is and refused if it leaves the destination. What an archive
//! may expand to is capped by `max_output_size` (256 MiB when not given, as `lib:compress`).
//!
//! `include` patterns are `lib:glob`'s (`*` crosses `/`). An entry's `compression_ratio` is its compressed size
//! over its size for a zip, whose entries are compressed one by one, and `nil` for a tar, which is one stream.
//!
//! The API's rules:
//! - An archive is an object: `Tar.open(path)` / `Tar.from_bytes(bytes)` with `:list()`, `:extract(dest, options?)` and
//!   `:read(name, options?)`. Whether an archive is a file or bytes is said by the constructor, never guessed from the
//!   argument. `Zip` is the same.
//! - `Tar.create(entries, options?)` and `Zip.create(entries)` give the archive's bytes; `Tar.create_file(path,
//!   entries, options?)` and `Zip.create_file(path, paths, options?)` write a file. `Zip.create` writes its entries in
//!   the order of their names.
//! - `compression` is a string enum, `max_output_size` a whole number from 0 up, `include` a list of strings, an entry
//!   `{ path, archive_path? }`: anything else, or an option that does not exist, is refused as an argument.
//! - Every failure is the typed `ArchiveError`.

use std::cell::Cell;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use flate2::Compression as GzLevel;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use globset::{GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use archive::{ArchiveError, Compression, Entry, TarEntry};

use crate::modules::std::osstr::OsPath;

/// What an archive may expand to when `max_output_size` is not given: `lib:compress`'s limit.
const DEFAULT_MAX_OUTPUT: u64 = 256 * 1024 * 1024;
/// The longest entry name taken.
const MAX_NAME_LEN: usize = 4096;

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

/// The codec a tar file's extension names.
fn compression_of_extension(path: &Path) -> Compression {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Compression::Gzip
    } else if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
        Compression::Zstd
    } else {
        Compression::None
    }
}

/// The codec a stream starts with, so a read opens a raw, gzip or zstd tar alike.
fn compression_of_magic(bytes: &[u8]) -> Compression {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        Compression::Gzip
    } else if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Compression::Zstd
    } else {
        Compression::None
    }
}

/// The codec to write `path` with: the option when given, else the extension's. An option that is not what a
/// compressed extension says is an error.
fn resolve_create_compression(
    path: &Path,
    explicit: Option<Compression>,
) -> Result<Compression, ArchiveError> {
    let extension = compression_of_extension(path);
    let Some(compression) = explicit else {
        return Ok(extension);
    };
    if extension != Compression::None && extension != compression {
        return Err(ArchiveError::CompressionMismatch {
            path: OsPath(path.to_owned()),
            compression,
            extension,
        });
    }
    Ok(compression)
}

// ---------------------------------------------------------------------------
// Path safety: what extraction guarantees
// ---------------------------------------------------------------------------

/// Whether `s` starts with a Windows drive (`C:`). `std::path` knows it as absolute on Windows only, and `C:\...` is
/// refused on every host.
fn has_drive_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

/// Put `path`'s components on `stack`, resolving `.` and `..` by the text alone. A `..` with nothing to pop leaves
/// the root and is refused, and so is anything absolute. The stack holds no `..` after, so joining it cannot climb
/// out.
fn push_components(stack: &mut Vec<OsString>, path: &Path) -> Result<(), String> {
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err("absolute paths are not allowed".to_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.pop().is_none() {
                    return Err("path escapes the destination directory".to_owned());
                }
            }
            Component::Normal(part) => stack.push(part.to_os_string()),
        }
    }
    Ok(())
}

/// The safe relative path of an entry name, to join under the destination; or why the name is refused.
fn sanitize_entry_name(raw: &str) -> Result<PathBuf, String> {
    if raw.as_bytes().contains(&0) {
        return Err("the name contains a null byte".to_owned());
    }
    if raw.len() > MAX_NAME_LEN {
        return Err(format!("the name exceeds {MAX_NAME_LEN} bytes ({} bytes)", raw.len()));
    }
    if raw == "." || raw == ".." {
        return Err("the name is not a path".to_owned());
    }
    if has_drive_prefix(raw) {
        return Err("the name is an absolute (drive) path".to_owned());
    }
    // Windows separators too, so `a\b\..\c` is read as `a/b/../c`
    let normalised = raw.replace('\\', "/");
    let mut stack: Vec<OsString> = Vec::new();
    push_components(&mut stack, Path::new(&normalised))?;
    if stack.is_empty() {
        return Err("the name resolves to the destination itself".to_owned());
    }
    Ok(stack.iter().collect())
}

/// Whether a symlink's or hard link's target stays in the destination. `link_rel` is where the link is, already
/// sanitised; the target is resolved from the link's directory, as the operating system would.
fn check_link_target(link_rel: &Path, target: &str) -> Result<(), String> {
    if target.as_bytes().contains(&0) {
        return Err("the target contains a null byte".to_owned());
    }
    if has_drive_prefix(target) {
        return Err("the target is an absolute (drive) path".to_owned());
    }
    let normalised = target.replace('\\', "/");
    let mut stack: Vec<OsString> = Vec::new();
    // the link's directory is sanitised and holds no `..`: only the target can go too far up
    if let Some(parent) = link_rel.parent() {
        push_components(&mut stack, parent)?;
    }
    push_components(&mut stack, Path::new(&normalised))
}

// ---------------------------------------------------------------------------
// The cap on what an archive expands to
// ---------------------------------------------------------------------------

/// How much an archive may still expand to. Shared by the readers of one call, and it remembers having been
/// passed: the libraries hand a reader's failure on as their own error, and this is how the call knows which it was.
#[derive(Clone)]
struct Cap {
    limit: u64,
    remaining: Rc<Cell<u64>>,
    passed: Rc<Cell<bool>>,
}

impl Cap {
    fn new(max_output_size: Option<u64>) -> Result<Self, ArchiveError> {
        let limit = max_output_size.unwrap_or(DEFAULT_MAX_OUTPUT);
        if limit == 0 {
            return Err(ArchiveError::MaxOutputSizeZero);
        }
        Ok(Self { limit, remaining: Rc::new(Cell::new(limit)), passed: Rc::new(Cell::new(false)) })
    }

    /// A reader of `inner` that fails once more than what remains has been read.
    fn reader<R: Read>(&self, inner: R) -> LimitReader<R> {
        LimitReader { inner, cap: self.clone() }
    }

    /// What a failed read means: the cap was passed, or the archive is not readable.
    fn error(&self, error: impl std::fmt::Display) -> ArchiveError {
        if self.passed.get() {
            ArchiveError::TooLarge { max_output_size: self.limit }
        } else {
            ArchiveError::Corrupt { reason: error.to_string() }
        }
    }
}

/// A `Read` that fails once its [`Cap`] is spent, however small the compressed input is.
struct LimitReader<R> {
    inner: R,
    cap: Cap,
}

impl<R: Read> Read for LimitReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.cap.remaining.get();
        if remaining == 0 {
            // at the limit: one more byte means the stream goes past it
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => {
                    self.cap.passed.set(true);
                    Err(io::Error::other(format!(
                        "output exceeds max_output_size ({} bytes)",
                        self.cap.limit
                    )))
                }
            };
        }
        let allowed = buf.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..allowed])?;
        self.cap.remaining.set(remaining - n as u64);
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// What both formats share
// ---------------------------------------------------------------------------

fn io_error(path: &Path, error: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Io { path: OsPath(path.to_owned()), reason: error.to_string() }
}

fn corrupt(error: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Corrupt { reason: error.to_string() }
}

/// The `include` patterns as one set; `None` (every entry) when there are none.
fn build_include(patterns: Option<&[String]>) -> Result<Option<GlobSet>, ArchiveError> {
    let Some(patterns) = patterns.filter(|patterns| !patterns.is_empty()) else {
        return Ok(None);
    };
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let invalid = |reason: String| ArchiveError::InvalidPattern { pattern: pattern.clone(), reason };
        builder.add(super::glob::compile(pattern).map_err(invalid)?);
    }
    builder.build().map(Some).map_err(|error| ArchiveError::InvalidPattern {
        pattern: patterns[0].clone(),
        reason: error.to_string(),
    })
}

/// Howard Hinnant's days-from-civil: a zip's DOS date and time as Unix seconds.
fn civil_to_unix(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
}

/// The files `disk` stands for, each with its name in the archive: a file is itself, a directory is every file
/// under it, named under `base_name`. `strip` is taken off the front of every name.
fn expand_source(
    disk: &Path,
    base_name: &str,
    strip: Option<&str>,
    out: &mut Vec<(PathBuf, String)>,
) -> Result<(), ArchiveError> {
    let meta = fs::symlink_metadata(disk).map_err(|error| io_error(disk, error))?;
    if meta.is_file() {
        out.push((disk.to_path_buf(), strip_name(base_name, strip)));
        return Ok(());
    }
    if !meta.is_dir() {
        return Err(ArchiveError::NotAFileOrDirectory { path: OsPath(disk.to_owned()) });
    }
    let base = base_name.trim_end_matches('/');
    for entry in WalkDir::new(disk).follow_links(false) {
        let entry = entry.map_err(|error| io_error(disk, error))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(disk).map_err(|error| io_error(disk, error))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        let name = if base.is_empty() { rel } else { format!("{base}/{rel}") };
        out.push((entry.path().to_path_buf(), strip_name(&name, strip)));
    }
    Ok(())
}

/// `name` without `strip` at its front, and without the separator that leaves: `src/` turns `src/main.rs` into
/// `main.rs`.
fn strip_name(name: &str, strip: Option<&str>) -> String {
    let stripped = strip.and_then(|prefix| name.strip_prefix(prefix)).unwrap_or(name);
    stripped.trim_start_matches('/').to_owned()
}

/// The entry's safe relative path, or `None` when `include` leaves it out. Tar and zip extraction both come
/// through here: the filter, then the check of the name, is in one place.
fn accept_entry(raw: &str, include: Option<&GlobSet>) -> Option<Result<PathBuf, ArchiveError>> {
    if include.is_some_and(|set| !set.is_match(raw.replace('\\', "/"))) {
        return None;
    }
    Some(
        sanitize_entry_name(raw).map_err(|reason| ArchiveError::UnsafeEntry { name: raw.to_owned(), reason }),
    )
}

/// Write a file entry under `dest` at its sanitised path, making the directories above it.
fn write_file_entry(dest: &Path, rel: &Path, content: &[u8]) -> Result<(), ArchiveError> {
    let target = dest.join(rel);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    fs::write(&target, content).map_err(|error| io_error(&target, error))
}

/// Make a symlink (or a hard link) at `target` to `link`. Unix only: elsewhere it is an error, not a link quietly
/// left out.
#[cfg(unix)]
fn write_link(target: &Path, link: &str, symlink: bool, _name: &str) -> Result<(), ArchiveError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    if symlink {
        std::os::unix::fs::symlink(link, target).map_err(|error| io_error(target, error))
    } else {
        let source = target.parent().unwrap_or(Path::new(".")).join(link);
        fs::hard_link(&source, target).map_err(|error| io_error(target, error))
    }
}

#[cfg(not(unix))]
fn write_link(_target: &Path, _link: &str, _symlink: bool, name: &str) -> Result<(), ArchiveError> {
    Err(ArchiveError::Unsupported {
        name: name.to_owned(),
        reason: "link entries are only extracted on Unix".to_owned(),
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, ArchiveError> {
    fs::read(path).map_err(|error| io_error(path, error))
}

// ---------------------------------------------------------------------------
// Tar
// ---------------------------------------------------------------------------

/// The files the entries stand for, each with its name in the archive.
fn tar_sources(entries: &[TarEntry], strip: Option<&str>) -> Result<Vec<(PathBuf, String)>, ArchiveError> {
    let mut out = Vec::new();
    for entry in entries {
        let path = entry.path.as_ref();
        let base = entry.archive_path.clone().unwrap_or_else(|| path.to_string_lossy().into_owned());
        expand_source(path, &base, strip, &mut out)?;
    }
    Ok(out)
}

fn append_tar<W: Write>(
    builder: &mut tar::Builder<W>,
    sources: &[(PathBuf, String)],
) -> Result<(), ArchiveError> {
    for (disk, name) in sources {
        // the writer refuses a name that would leave the destination (`..`), besides what reading `disk` can fail on
        builder
            .append_path_with_name(disk, name)
            .map_err(|error| ArchiveError::InvalidName { name: name.clone(), reason: error.to_string() })?;
    }
    Ok(())
}

/// A tar of `sources`, compressed as asked, in memory.
fn build_tar(sources: &[(PathBuf, String)], compression: Compression) -> Result<Vec<u8>, ArchiveError> {
    let failed = |error: io::Error| ArchiveError::WriteFailed { reason: error.to_string() };
    match compression {
        Compression::None => {
            let mut builder = tar::Builder::new(Vec::new());
            append_tar(&mut builder, sources)?;
            builder.into_inner().map_err(failed)
        }
        Compression::Gzip => {
            let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), GzLevel::default()));
            append_tar(&mut builder, sources)?;
            builder.into_inner().map_err(failed)?.finish().map_err(failed)
        }
        Compression::Zstd => {
            let mut builder = tar::Builder::new(zstd::Encoder::new(Vec::new(), 3).map_err(failed)?);
            append_tar(&mut builder, sources)?;
            builder.into_inner().map_err(failed)?.finish().map_err(failed)
        }
    }
}

/// A tar stream, decompressed as its first bytes say, that expands to no more than `cap`.
fn tar_archive<'a>(bytes: &'a [u8], cap: &Cap) -> Result<tar::Archive<Box<dyn Read + 'a>>, ArchiveError> {
    let reader: Box<dyn Read + 'a> = match compression_of_magic(bytes) {
        Compression::None => Box::new(cap.reader(bytes)),
        Compression::Gzip => Box::new(cap.reader(GzDecoder::new(bytes))),
        Compression::Zstd => Box::new(cap.reader(zstd::Decoder::new(bytes).map_err(corrupt)?)),
    };
    Ok(tar::Archive::new(reader))
}

fn tar_list(bytes: &[u8]) -> Result<Vec<Entry>, ArchiveError> {
    let cap = Cap::new(None)?;
    let mut archive = tar_archive(bytes, &cap)?;
    let mut out = Vec::new();
    for entry in archive.entries().map_err(|error| cap.error(error))? {
        let entry = entry.map_err(|error| cap.error(error))?;
        let header = entry.header();
        out.push(Entry {
            name: entry.path().map_err(|error| cap.error(error))?.to_string_lossy().into_owned(),
            size: entry.size(),
            mode: header.mode().unwrap_or(0),
            timestamp: header.mtime().map_or(0, |mtime| i64::try_from(mtime).unwrap_or(i64::MAX)),
            is_dir: header.entry_type().is_dir(),
            compression_ratio: None,
        });
    }
    Ok(out)
}

fn tar_extract(
    bytes: &[u8],
    dest: &Path,
    include: Option<&GlobSet>,
    cap: &Cap,
) -> Result<usize, ArchiveError> {
    let mut archive = tar_archive(bytes, cap)?;
    let mut count = 0;
    for entry in archive.entries().map_err(|error| cap.error(error))? {
        let mut entry = entry.map_err(|error| cap.error(error))?;
        let raw = entry.path().map_err(|error| cap.error(error))?.to_string_lossy().into_owned();
        let Some(rel) = accept_entry(&raw, include) else {
            continue;
        };
        let rel = rel?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            let target = dest.join(&rel);
            fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
        } else if entry_type.is_symlink() || entry_type.is_hard_link() {
            let link = entry
                .link_name()
                .map_err(|error| cap.error(error))?
                .ok_or_else(|| corrupt(format!("{raw}: the link entry has no target")))?;
            let link = link.to_string_lossy();
            check_link_target(&rel, &link).map_err(|reason| ArchiveError::UnsafeLink {
                name: raw.clone(),
                target: link.clone().into_owned(),
                reason,
            })?;
            write_link(&dest.join(&rel), &link, entry_type.is_symlink(), &raw)?;
        } else {
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|error| cap.error(error))?;
            write_file_entry(dest, &rel, &content)?;
        }
        count += 1;
    }
    Ok(count)
}

fn tar_read(bytes: &[u8], name: &str, cap: &Cap) -> Result<Vec<u8>, ArchiveError> {
    let want = name.replace('\\', "/");
    let mut archive = tar_archive(bytes, cap)?;
    for entry in archive.entries().map_err(|error| cap.error(error))? {
        let mut entry = entry.map_err(|error| cap.error(error))?;
        let entry_name = entry.path().map_err(|error| cap.error(error))?.to_string_lossy().replace('\\', "/");
        if entry_name == want {
            let mut content = Vec::new();
            entry.read_to_end(&mut content).map_err(|error| cap.error(error))?;
            return Ok(content);
        }
    }
    Err(ArchiveError::NotFound { name: name.to_owned() })
}

// ---------------------------------------------------------------------------
// Zip
// ---------------------------------------------------------------------------

fn zip_options(mode: Option<u32>) -> zip::write::SimpleFileOptions {
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    match mode {
        Some(mode) => options.unix_permissions(mode),
        None => options,
    }
}

fn zip_failed(name: &str) -> impl Fn(zip::result::ZipError) -> ArchiveError + '_ {
    move |error| ArchiveError::InvalidName { name: name.to_owned(), reason: error.to_string() }
}

fn write_failed(error: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::WriteFailed { reason: error.to_string() }
}

/// A zip of `entries` (name and content), in memory.
fn build_zip_bytes<'a>(
    entries: impl IntoIterator<Item = (&'a String, &'a [u8])>,
) -> Result<Vec<u8>, ArchiveError> {
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for (name, content) in entries {
        writer.start_file(name, zip_options(Some(0o644))).map_err(zip_failed(name))?;
        writer.write_all(content).map_err(write_failed)?;
    }
    Ok(writer.finish().map_err(write_failed)?.into_inner())
}

/// A zip of the files and directories at `paths`, written to `writer`.
fn build_zip_files<W: Write + io::Seek>(
    writer: W,
    paths: &[OsPath],
    strip: Option<&str>,
) -> Result<(), ArchiveError> {
    let mut sources = Vec::new();
    for path in paths {
        expand_source(path.as_ref(), &path.0.to_string_lossy(), strip, &mut sources)?;
    }
    let mut writer = zip::ZipWriter::new(writer);
    for (disk, name) in &sources {
        let mode = fs::metadata(disk).ok().map(|meta| unix_mode(&meta));
        writer.start_file(name, zip_options(mode)).map_err(zip_failed(name))?;
        writer.write_all(&read_file(disk)?).map_err(write_failed)?;
    }
    writer.finish().map_err(write_failed)?;
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
    file.last_modified().map_or(0, |modified| {
        civil_to_unix(
            i64::from(modified.year()),
            u32::from(modified.month()),
            u32::from(modified.day()),
            u32::from(modified.hour()),
            u32::from(modified.minute()),
            u32::from(modified.second()),
        )
    })
}

fn zip_open(bytes: &[u8]) -> Result<zip::ZipArchive<io::Cursor<&[u8]>>, ArchiveError> {
    zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(corrupt)
}

fn zip_list(bytes: &[u8]) -> Result<Vec<Entry>, ArchiveError> {
    let mut archive = zip_open(bytes)?;
    let mut out = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(corrupt)?;
        #[allow(clippy::cast_precision_loss)] // a ratio: the last bits of a size past 2^53 do not show in it
        let compression_ratio =
            (file.size() != 0).then(|| file.compressed_size() as f64 / file.size() as f64);
        out.push(Entry {
            name: file.name().map_err(corrupt)?.into_owned(),
            size: file.size(),
            mode: file.unix_mode().unwrap_or(0),
            timestamp: zip_mtime(&file),
            is_dir: file.is_dir(),
            compression_ratio,
        });
    }
    Ok(out)
}

fn zip_extract(
    bytes: &[u8],
    dest: &Path,
    include: Option<&GlobSet>,
    cap: &Cap,
) -> Result<usize, ArchiveError> {
    let mut archive = zip_open(bytes)?;
    let mut count = 0;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(corrupt)?;
        let raw = file.name().map_err(corrupt)?.into_owned();
        let Some(rel) = accept_entry(&raw, include) else {
            continue;
        };
        let rel = rel?;
        if file.is_dir() {
            let target = dest.join(&rel);
            fs::create_dir_all(&target).map_err(|error| io_error(&target, error))?;
        } else {
            let mut content = Vec::new();
            cap.reader(&mut file).read_to_end(&mut content).map_err(|error| cap.error(error))?;
            write_file_entry(dest, &rel, &content)?;
        }
        count += 1;
    }
    Ok(count)
}

fn zip_read(bytes: &[u8], name: &str, cap: &Cap) -> Result<Vec<u8>, ArchiveError> {
    let mut archive = zip_open(bytes)?;
    let mut file = archive.by_name(name).map_err(|_| ArchiveError::NotFound { name: name.to_owned() })?;
    let mut content = Vec::new();
    cap.reader(&mut file).read_to_end(&mut content).map_err(|error| cap.error(error))?;
    Ok(content)
}

#[mlua_ir::module("lib:archive")]
pub mod archive {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Arc;

    use mlua_ir::{Bytes, LuaError, Object, Options, Shape};
    use serde::{Deserialize, Serialize};

    use super::{
        Cap, build_include, build_tar, build_zip_bytes, build_zip_files, io_error, read_file,
        resolve_create_compression, tar_sources,
    };
    use crate::modules::std::osstr::OsPath;

    /// How a tar is compressed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Compression {
        /// Not at all: a plain `.tar`.
        None,
        /// gzip: `.tar.gz`, `.tgz`.
        Gzip,
        /// Zstandard: `.tar.zst`, `.tzst`.
        Zstd,
    }

    impl Compression {
        fn name(self) -> &'static str {
            match self {
                Compression::None => "none",
                Compression::Gzip => "gzip",
                Compression::Zstd => "zstd",
            }
        }
    }

    /// A file or a directory to put in a tar.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct TarEntry {
        /// Where it is on disk. A directory is every file under it.
        pub path: OsPath,
        /// Its name in the archive (a directory's files are named under it). When not given: `path`.
        pub archive_path: Option<String>,
    }

    /// How a tar is made.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct TarOptions {
        /// How to compress it. When not given: not at all for `create`, and as the file's extension says for
        /// `create_file`.
        pub compression: Option<Compression>,
        /// What to take off the front of every name in the archive: `src/` turns `src/main.rs` into `main.rs`.
        pub strip_prefix: Option<String>,
    }

    /// How a zip is made from files.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct ZipOptions {
        /// What to take off the front of every name in the archive.
        pub strip_prefix: Option<String>,
    }

    /// How an archive is extracted.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct ExtractOptions {
        /// Extract only the entries whose name matches one of these `lib:glob` patterns. When not given, or empty:
        /// every entry.
        pub include: Option<Vec<String>>,
        /// The most bytes the archive may expand to; more is an error. When not given: 256 MiB.
        pub max_output_size: Option<u64>,
    }

    /// How one entry is read.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct ReadOptions {
        /// The most bytes that may be decompressed to reach and read the entry; more is an error. When not given:
        /// 256 MiB.
        pub max_output_size: Option<u64>,
    }

    /// An entry of an archive.
    #[derive(Debug, Clone, PartialEq, Serialize, Shape)]
    pub struct Entry {
        /// Its name in the archive, with `/` between its parts.
        pub name: String,
        /// Its size in bytes, uncompressed.
        pub size: u64,
        /// Its Unix permission bits; 0 when the archive has none.
        pub mode: u32,
        /// When it was last modified, in seconds since the Unix epoch; 0 when the archive does not say.
        pub timestamp: i64,
        /// Whether it is a directory.
        pub is_dir: bool,
        /// Its compressed size over its size, for a zip entry that is not empty. `nil` for a tar, which is
        /// compressed as one stream.
        pub compression_ratio: Option<f64>,
    }

    /// Why an archive could not be made, read or extracted.
    #[derive(Debug, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ArchiveError {
        /// A file or a directory on disk could not be read or written.
        #[error("{}: {reason}", .path.0.display())]
        Io {
            /// The path that failed.
            path: OsPath,
            /// What the operating system said.
            reason: String,
        },
        /// Something to archive is neither a file nor a directory.
        #[error("{}: not a regular file or directory", .path.0.display())]
        NotAFileOrDirectory {
            /// The path given.
            path: OsPath,
        },
        /// An entry cannot be written under this name (`..`, an absolute path), or its file could not be added.
        #[error("{name}: {reason}")]
        InvalidName {
            /// The name in the archive.
            name: String,
            /// What the archive writer said.
            reason: String,
        },
        /// The archive could not be written.
        #[error("{reason}")]
        WriteFailed {
            /// What the archive writer said.
            reason: String,
        },
        /// `compression` is not what the file's extension says.
        #[error("compression {:?} conflicts with the extension of {:?} ({})", .compression.name(), .path.0.display(), .extension.name())]
        CompressionMismatch {
            /// The file to write.
            path: OsPath,
            /// The option given.
            compression: Compression,
            /// What the extension says.
            extension: Compression,
        },
        /// An `include` pattern is not a glob.
        #[error("invalid include pattern {pattern:?}: {reason}")]
        InvalidPattern {
            /// The pattern as given.
            pattern: String,
            /// What is wrong with it.
            reason: String,
        },
        /// `max_output_size` is 0, which nothing fits.
        #[error("max_output_size must be more than 0")]
        MaxOutputSizeZero,
        /// The bytes are not an archive of this format, or are damaged.
        #[error("{reason}")]
        Corrupt {
            /// What the archive reader said.
            reason: String,
        },
        /// An entry's name would be written outside the destination, or is not a path. Nothing more is extracted.
        #[error("entry {name:?}: {reason}")]
        UnsafeEntry {
            /// The name in the archive.
            name: String,
            /// Why it is refused.
            reason: String,
        },
        /// A symlink or a hard link points outside the destination. Nothing more is extracted.
        #[error("link {name:?} to {target:?}: {reason}")]
        UnsafeLink {
            /// The link's name in the archive.
            name: String,
            /// Where it points.
            target: String,
            /// Why it is refused.
            reason: String,
        },
        /// The archive expands to more than `max_output_size`.
        #[error("output exceeds max_output_size ({max_output_size} bytes)")]
        TooLarge {
            /// The limit that was passed.
            max_output_size: u64,
        },
        /// The archive has no entry of this name.
        #[error("entry {name:?} not found in archive")]
        NotFound {
            /// The name asked for.
            name: String,
        },
        /// An entry is of a kind this platform cannot extract.
        #[error("{name}: {reason}")]
        Unsupported {
            /// The name in the archive.
            name: String,
            /// What is not supported.
            reason: String,
        },
    }

    /// A tar archive, compressed or not. `Tar.create` and `Tar.create_file` make one.
    #[derive(Debug, Clone, Object)]
    pub struct Tar {
        bytes: Arc<Vec<u8>>,
    }

    #[mlua_ir::object]
    impl Tar {
        /// The tar in the file at `path`: plain, gzip or zstd, as its first bytes say.
        pub fn open(path: OsPath) -> Result<Tar, ArchiveError> {
            Ok(Tar { bytes: Arc::new(read_file(path.as_ref())?) })
        }

        /// The tar that `bytes` are: plain, gzip or zstd, as its first bytes say. It is read when it is used.
        ///
        /// ```luau
        /// local tar = archive.Tar.from_bytes(response_body)
        /// for _, entry in assert(tar:list()) do
        ///     print(entry.name, entry.size)
        /// end
        /// ```
        pub fn from_bytes(bytes: Bytes) -> Tar {
            Tar { bytes: Arc::new(bytes.0) }
        }

        /// A tar of the files and directories `entries` name, as bytes.
        ///
        /// ```luau
        /// local bytes = assert(archive.Tar.create({
        ///     { path = "src", archive_path = "app/src" },
        ///     { path = "README.md" },
        /// }, { compression = "gzip" }))
        /// ```
        pub fn create(entries: Vec<TarEntry>, options: TarOptions) -> Result<Bytes, ArchiveError> {
            let sources = tar_sources(&entries, options.strip_prefix.as_deref())?;
            build_tar(&sources, options.compression.unwrap_or(Compression::None)).map(Bytes)
        }

        /// Write a tar of the files and directories `entries` name to `path`, compressed as its extension says
        /// (`.tar.gz`, `.tgz`, `.tar.zst`, `.tzst`; not at all otherwise).
        pub fn create_file(
            path: OsPath,
            entries: Vec<TarEntry>,
            options: TarOptions,
        ) -> Result<(), ArchiveError> {
            let compression = resolve_create_compression(path.as_ref(), options.compression)?;
            let sources = tar_sources(&entries, options.strip_prefix.as_deref())?;
            let bytes = build_tar(&sources, compression)?;
            fs::write(&path, bytes).map_err(|error| io_error(path.as_ref(), error))
        }

        /// The entries, in the archive's order.
        pub fn list(&self) -> Result<Vec<Entry>, ArchiveError> {
            super::tar_list(&self.bytes)
        }

        /// Write the entries under the directory `dest`; how many were written. An entry that would land outside
        /// `dest` stops the extraction with an error.
        ///
        /// ```luau
        /// local tar = assert(archive.Tar.open("bundle.tar.gz"))
        /// local count = assert(tar:extract("out", { include = { "*.lua" } }))
        /// ```
        pub fn extract(&self, dest: OsPath, options: ExtractOptions) -> Result<usize, ArchiveError> {
            let cap = Cap::new(options.max_output_size)?;
            let include = build_include(options.include.as_deref())?;
            super::tar_extract(&self.bytes, dest.as_ref(), include.as_ref(), &cap)
        }

        /// The content of the entry named `name`.
        pub fn read(&self, name: String, options: ReadOptions) -> Result<Bytes, ArchiveError> {
            let cap = Cap::new(options.max_output_size)?;
            super::tar_read(&self.bytes, &name, &cap).map(Bytes)
        }
    }

    /// A zip archive. `Zip.create` and `Zip.create_file` make one.
    #[derive(Debug, Clone, Object)]
    pub struct Zip {
        bytes: Arc<Vec<u8>>,
    }

    #[mlua_ir::object]
    impl Zip {
        /// The zip in the file at `path`.
        pub fn open(path: OsPath) -> Result<Zip, ArchiveError> {
            Ok(Zip { bytes: Arc::new(read_file(path.as_ref())?) })
        }

        /// The zip that `bytes` are. It is read when it is used.
        pub fn from_bytes(bytes: Bytes) -> Zip {
            Zip { bytes: Arc::new(bytes.0) }
        }

        /// A zip of `entries`, each a name and its content, as bytes. The entries are written in the order of
        /// their names.
        ///
        /// ```luau
        /// local bytes = assert(archive.Zip.create({ ["a.txt"] = "alpha", ["dir/b.txt"] = "beta" }))
        /// local zip = archive.Zip.from_bytes(bytes)
        /// assert(zip:read("dir/b.txt") == "beta")
        /// ```
        pub fn create(entries: BTreeMap<String, Bytes>) -> Result<Bytes, ArchiveError> {
            build_zip_bytes(entries.iter().map(|(name, content)| (name, &content.0[..]))).map(Bytes)
        }

        /// Write a zip of the files and directories at `paths` to `path`. Each is named in the archive as it is
        /// given, a directory's files under it.
        pub fn create_file(
            path: OsPath,
            paths: Vec<OsPath>,
            options: ZipOptions,
        ) -> Result<(), ArchiveError> {
            let file = fs::File::create(&path).map_err(|error| io_error(path.as_ref(), error))?;
            build_zip_files(file, &paths, options.strip_prefix.as_deref())
        }

        /// The entries, in the archive's order.
        pub fn list(&self) -> Result<Vec<Entry>, ArchiveError> {
            super::zip_list(&self.bytes)
        }

        /// Write the entries under the directory `dest`; how many were written. An entry that would land outside
        /// `dest` stops the extraction with an error.
        pub fn extract(&self, dest: OsPath, options: ExtractOptions) -> Result<usize, ArchiveError> {
            let cap = Cap::new(options.max_output_size)?;
            let include = build_include(options.include.as_deref())?;
            super::zip_extract(&self.bytes, dest.as_ref(), include.as_ref(), &cap)
        }

        /// The content of the entry named `name`.
        pub fn read(&self, name: String, options: ReadOptions) -> Result<Bytes, ArchiveError> {
            let cap = Cap::new(options.max_output_size)?;
            super::zip_read(&self.bytes, &name, &cap).map(Bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;
    use crate::make_engine;

    // --- sanitize_entry_name: what extraction refuses, whatever the host ---

    #[test]
    fn accepts_plain_relative_paths() {
        assert_eq!(sanitize_entry_name("src/main.rs").unwrap(), PathBuf::from("src/main.rs"));
    }

    #[test]
    fn accepts_paths_that_normalise_within_dest() {
        assert_eq!(sanitize_entry_name("safe/../safe/file.txt").unwrap(), PathBuf::from("safe/file.txt"));
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
        // a backslash is a separator, so this is `..` out of the destination and not a name
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
        assert!(sanitize_entry_name("a/..").is_err(), "the destination itself");
    }

    #[test]
    fn rejects_overlong_names() {
        assert!(sanitize_entry_name(&"a".repeat(MAX_NAME_LEN + 1)).is_err());
        assert!(sanitize_entry_name(&"a".repeat(MAX_NAME_LEN)).is_ok());
    }

    #[test]
    fn backslash_separator_normalises_to_safe_path() {
        assert_eq!(sanitize_entry_name("a\\b\\c.txt").unwrap(), PathBuf::from("a/b/c.txt"));
    }

    // --- check_link_target ---

    #[test]
    fn link_target_within_dest_is_ok() {
        // dest/sub/link -> ../sibling is dest/sibling
        assert!(check_link_target(Path::new("sub/link"), "../sibling").is_ok());
    }

    #[test]
    fn link_target_escaping_dest_is_rejected() {
        assert!(check_link_target(Path::new("link"), "../../etc/passwd").is_err());
        assert!(check_link_target(Path::new("link"), "/etc/passwd").is_err());
        assert!(check_link_target(Path::new("link"), "C:\\Windows").is_err());
        assert!(check_link_target(Path::new("link"), "a\0b").is_err());
    }

    // --- compression ---

    #[test]
    fn infers_compression_from_extension() {
        let of = |path: &str| compression_of_extension(Path::new(path));
        assert_eq!(of("a.tar.gz"), Compression::Gzip);
        assert_eq!(of("a.TGZ"), Compression::Gzip);
        assert_eq!(of("a.tar.zst"), Compression::Zstd);
        assert_eq!(of("a.tzst"), Compression::Zstd);
        assert_eq!(of("a.tar"), Compression::None);
    }

    #[test]
    fn explicit_compression_clashing_with_extension_errors() {
        let resolve = |path: &str, explicit| resolve_create_compression(Path::new(path), explicit);
        assert!(matches!(
            resolve("a.tar.gz", Some(Compression::Zstd)),
            Err(ArchiveError::CompressionMismatch {
                compression: Compression::Zstd,
                extension: Compression::Gzip,
                ..
            })
        ));
        assert_eq!(resolve("a.tar.gz", Some(Compression::Gzip)), Ok(Compression::Gzip));
        assert_eq!(resolve("a.tar.gz", None), Ok(Compression::Gzip));
        // no compressed extension: the option decides
        assert_eq!(resolve("a.tar", Some(Compression::Gzip)), Ok(Compression::Gzip));
        assert_eq!(resolve("a.tar", None), Ok(Compression::None));
    }

    #[test]
    fn magic_detection_round_trips() {
        for compression in [Compression::Gzip, Compression::Zstd, Compression::None] {
            let bytes = build_tar(&[], compression).unwrap();
            assert_eq!(compression_of_magic(&bytes), compression);
            assert_eq!(tar_list(&bytes).unwrap(), []);
        }
    }

    // --- links, through extraction itself ---

    /// A tar whose one entry is a link of type `ty` named `name` to `target`.
    fn tar_with_link(name: &str, target: &str, ty: tar::EntryType) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(ty);
        header.set_size(0);
        header.set_mode(0o777);
        builder.append_link(&mut header, name, target).expect("a link entry");
        builder.into_inner().expect("a tar")
    }

    #[test]
    fn extraction_rejects_escaping_link_entries() {
        // the refusal comes from extraction, before anything is written, for a symlink and for a hard link
        for ty in [tar::EntryType::symlink(), tar::EntryType::hard_link()] {
            let bytes = tar_with_link("link", "../../etc/passwd", ty);
            let dest = tempfile::tempdir().unwrap();
            let error =
                tar_extract(&bytes, dest.path(), None, &Cap::new(None).unwrap()).expect_err("a link out");
            assert_eq!(
                error,
                ArchiveError::UnsafeLink {
                    name: "link".into(),
                    target: "../../etc/passwd".into(),
                    reason: "path escapes the destination directory".into(),
                }
            );
            assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0, "nothing was written");
        }
    }

    // --- the cap ---

    #[test]
    fn the_cap_names_its_limit_and_lets_the_limit_itself_through() {
        let data = [0u8; 64];
        let cap = Cap::new(Some(8)).unwrap();
        let error = cap.reader(&data[..]).read_to_end(&mut Vec::new()).expect_err("past the cap");
        assert!(error.to_string().contains("8 bytes"), "{error}");
        assert_eq!(cap.error(error), ArchiveError::TooLarge { max_output_size: 8 });

        let cap = Cap::new(Some(64)).unwrap();
        let mut all = Vec::new();
        cap.reader(&data[..]).read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 64);
        assert_eq!(cap.error("damaged"), ArchiveError::Corrupt { reason: "damaged".into() });
        assert_eq!(Cap::new(Some(0)).err(), Some(ArchiveError::MaxOutputSizeZero));
    }

    #[test]
    fn a_zip_date_is_unix_seconds() {
        assert_eq!(civil_to_unix(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(civil_to_unix(1980, 1, 1, 0, 0, 0), 315_532_800);
        assert_eq!(civil_to_unix(2026, 5, 19, 1, 30, 45), 1_779_154_245);
    }

    // --- through Lua ---

    /// An engine, and a directory holding `main.lua`, `Cargo.toml` and `sub/deep.txt`; the script's `root` is its
    /// path with `/` separators.
    fn scratch() -> (Lua, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.lua"), "print('hi')").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/deep.txt"), "deep").unwrap();
        let lua = make_engine().unwrap();
        lua.globals().set("root", dir.path().to_string_lossy().replace('\\', "/")).unwrap();
        lua.load(r#"archive = require("lib:archive") fs = require("std:fs")"#).exec().unwrap();
        (lua, dir)
    }

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, code: &str) -> T {
        lua.load(code).eval().unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn tar_round_trips_name_size_and_content_under_each_compression() {
        let (lua, _dir) = scratch();
        for compression in ["none", "gzip", "zstd"] {
            let (count, name, size, ratio, content): (usize, String, u64, mlua::Value, String) = eval(
                &lua,
                &format!(
                    r#"
                    local bytes = assert(archive.Tar.create(
                        {{ {{ path = root .. "/main.lua", archive_path = "app/main.lua" }} }},
                        {{ compression = "{compression}" }}))
                    local tar = archive.Tar.from_bytes(bytes)
                    local entries = assert(tar:list())
                    return #entries, entries[1].name, entries[1].size, entries[1].compression_ratio,
                        (assert(tar:read("app/main.lua")))
                    "#
                ),
            );
            assert_eq!((count, name.as_str(), size), (1, "app/main.lua", 11), "{compression}");
            assert!(ratio.is_nil(), "a tar is one stream: no ratio by entry");
            assert_eq!(content, "print('hi')");
        }
    }

    #[test]
    fn tar_keeps_binary_content() {
        let (lua, dir) = scratch();
        fs::write(dir.path().join("blob.bin"), [0u8, 1, 2, 254, 255, 127, 128]).unwrap();
        let content: mlua::BString = eval(
            &lua,
            r#"
            local bytes = assert(archive.Tar.create({ { path = root .. "/blob.bin", archive_path = "blob.bin" } },
                { compression = "gzip" }))
            return (assert(archive.Tar.from_bytes(bytes):read("blob.bin")))
            "#,
        );
        assert_eq!(content.as_slice(), [0u8, 1, 2, 254, 255, 127, 128]);
    }

    #[test]
    fn a_directory_is_every_file_under_it_and_strip_prefix_shortens_names() {
        let (lua, _dir) = scratch();
        let (mut names, stripped): (Vec<String>, Vec<String>) = eval(
            &lua,
            r#"
            local function names(bytes)
                local out = {}
                for _, entry in assert(archive.Tar.from_bytes(bytes):list()) do
                    table.insert(out, entry.name)
                end
                table.sort(out)
                return out
            end
            local whole = assert(archive.Tar.create({ { path = root, archive_path = "app" } }))
            local stripped = assert(archive.Tar.create({ { path = root .. "/main.lua", archive_path = "src/main.lua" } },
                { strip_prefix = "src/" }))
            return names(whole), names(stripped)
            "#,
        );
        names.sort();
        assert_eq!(names, ["app/Cargo.toml", "app/main.lua", "app/sub/deep.txt"]);
        assert_eq!(stripped, ["main.lua"]);
    }

    #[test]
    fn tar_file_is_compressed_as_its_extension_says_and_extracts() {
        let (lua, dir) = scratch();
        let (written, count, content, deep): (bool, usize, String, String) = eval(
            &lua,
            r#"
            local out = root .. "/bundle.tar.gz"
            local written = assert(archive.Tar.create_file(out, {
                { path = root .. "/main.lua", archive_path = "main.lua" },
                { path = root .. "/sub", archive_path = "sub" },
            }))
            local tar = assert(archive.Tar.open(out))
            local count = assert(tar:extract(root .. "/out"))
            return written, count, fs.read(root .. "/out/main.lua"), fs.read(root .. "/out/sub/deep.txt")
            "#,
        );
        assert!(written);
        assert_eq!((count, content.as_str(), deep.as_str()), (2, "print('hi')", "deep"));
        let bytes = fs::read(dir.path().join("bundle.tar.gz")).unwrap();
        assert_eq!(compression_of_magic(&bytes), Compression::Gzip);
    }

    #[test]
    fn a_compression_that_is_not_the_extensions_is_a_typed_error() {
        let (lua, dir) = scratch();
        let (value, kind, compression, extension, text): (mlua::Value, String, String, String, String) = eval(
            &lua,
            r#"
            local v, e = archive.Tar.create_file(root .. "/x.tar.gz", { { path = root .. "/main.lua" } },
                { compression = "zstd" })
            return v, e.kind, e.compression, e.extension, tostring(e)
            "#,
        );
        assert!(value.is_nil());
        assert_eq!(
            (kind.as_str(), compression.as_str(), extension.as_str()),
            ("compression_mismatch", "zstd", "gzip")
        );
        assert!(text.starts_with(
            "lib:archive.Tar.create_file: compression_mismatch: compression \"zstd\" conflicts"
        ));
        assert!(!dir.path().join("x.tar.gz").exists(), "nothing was written");
    }

    #[test]
    fn include_extracts_only_what_matches() {
        let (lua, dir) = scratch();
        let (count, content, kind): (usize, String, String) = eval(
            &lua,
            r#"
            local bytes = assert(archive.Tar.create({
                { path = root .. "/main.lua", archive_path = "main.lua" },
                { path = root .. "/Cargo.toml", archive_path = "Cargo.toml" },
            }))
            local tar = archive.Tar.from_bytes(bytes)
            local count = assert(tar:extract(root .. "/out", { include = { "*.lua" } }))
            local _, bad = tar:extract(root .. "/out", { include = { "[" } })
            return count, fs.read(root .. "/out/main.lua"), bad.kind
            "#,
        );
        assert_eq!((count, content.as_str(), kind.as_str()), (1, "print('hi')", "invalid_pattern"));
        assert!(!dir.path().join("out/Cargo.toml").exists());
    }

    #[test]
    fn the_tar_writer_refuses_a_name_that_leaves_the_destination() {
        let (lua, _dir) = scratch();
        let (value, kind, name): (mlua::Value, String, String) = eval(
            &lua,
            r#"
            local v, e = archive.Tar.create({ { path = root .. "/main.lua", archive_path = "../escape.lua" } })
            return v, e.kind, e.name
            "#,
        );
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), name.as_str()), ("invalid_name", "../escape.lua"));
    }

    #[test]
    fn a_zip_entry_that_leaves_the_destination_is_refused_and_one_that_comes_back_is_taken() {
        let (lua, dir) = scratch();
        let (value, kind, name, reason, count, content): (
            mlua::Value,
            String,
            String,
            String,
            usize,
            String,
        ) = eval(
            &lua,
            r#"
            local evil = archive.Zip.from_bytes((assert(archive.Zip.create({ ["../../evil.txt"] = "pwned" }))))
            local v, e = evil:extract(root .. "/out")
            local safe = archive.Zip.from_bytes((assert(archive.Zip.create({ ["safe/../safe/ok.txt"] = "fine" }))))
            local count = assert(safe:extract(root .. "/out"))
            return v, e.kind, e.name, e.reason, count, fs.read(root .. "/out/safe/ok.txt")
            "#,
        );
        assert!(value.is_nil());
        assert_eq!((kind.as_str(), name.as_str()), ("unsafe_entry", "../../evil.txt"));
        assert_eq!(reason, "path escapes the destination directory");
        assert_eq!((count, content.as_str()), (1, "fine"));
        assert!(!dir.path().parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn max_output_size_caps_what_an_archive_expands_to() {
        let (lua, dir) = scratch();
        fs::write(dir.path().join("big.txt"), vec![b'a'; 200_000]).unwrap();
        let kinds: Vec<String> = eval(
            &lua,
            r#"
            local big = string.rep("a", 200000)
            local zip = archive.Zip.from_bytes((assert(archive.Zip.create({ ["big.txt"] = big }))))
            local tar = archive.Tar.from_bytes((assert(archive.Tar.create(
                { { path = root .. "/big.txt", archive_path = "big.txt" } }, { compression = "gzip" }))))
            local kinds = {}
            for _, a in { zip, tar } do
                local _, extract = a:extract(root .. "/out", { max_output_size = 1024 })
                local _, read = a:read("big.txt", { max_output_size = 1024 })
                local _, zero = a:read("big.txt", { max_output_size = 0 })
                assert(extract.max_output_size == 1024)
                assert(#assert(a:read("big.txt", { max_output_size = 300000 })) == 200000)
                table.insert(kinds, extract.kind)
                table.insert(kinds, read.kind)
                table.insert(kinds, zero.kind)
            end
            return kinds
            "#,
        );
        assert_eq!(
            kinds,
            [
                "too_large",
                "too_large",
                "max_output_size_zero",
                "too_large",
                "too_large",
                "max_output_size_zero"
            ]
        );
    }

    #[test]
    fn zip_round_trips_with_a_ratio_for_each_entry() {
        let (lua, _dir) = scratch();
        let (names, size, small_ratio, content, kind, name): (
            Vec<String>,
            u64,
            bool,
            String,
            String,
            String,
        ) = eval(
            &lua,
            r#"
            local zip = archive.Zip.from_bytes((assert(archive.Zip.create({
                ["b.txt"] = "small", ["a.txt"] = string.rep("x", 1000), ["empty"] = "",
            }))))
            local entries = assert(zip:list())
            local names = {}
            for _, entry in entries do
                table.insert(names, entry.name)
            end
            assert(entries[3].compression_ratio == nil, "an empty entry has no ratio")
            local _, missing = zip:read("nope.txt")
            return names, entries[1].size, entries[1].compression_ratio < 1, (assert(zip:read("a.txt"))),
                missing.kind, missing.name
            "#,
        );
        assert_eq!(names, ["a.txt", "b.txt", "empty"], "in the order of their names");
        assert_eq!(size, 1000);
        assert!(small_ratio, "a thousand x compress well");
        assert_eq!(content, "x".repeat(1000));
        assert_eq!((kind.as_str(), name.as_str()), ("not_found", "nope.txt"));
    }

    #[test]
    fn zip_file_from_paths_extracts_and_reads_back() {
        let (lua, _dir) = scratch();
        let (written, count, names, content): (bool, usize, Vec<String>, String) = eval(
            &lua,
            r#"
            local out = root .. "/out.zip"
            local written = assert(archive.Zip.create_file(out, { root .. "/main.lua", root .. "/sub" },
                { strip_prefix = root }))
            local zip = assert(archive.Zip.open(out))
            local names = {}
            for _, entry in assert(zip:list()) do
                table.insert(names, entry.name)
            end
            table.sort(names)
            local count = assert(zip:extract(root .. "/unzipped"))
            return written, count, names, fs.read(root .. "/unzipped/sub/deep.txt")
            "#,
        );
        assert!(written);
        assert_eq!(count, 2);
        assert_eq!(names, ["main.lua", "sub/deep.txt"]);
        assert_eq!(content, "deep");
    }

    #[test]
    fn what_is_not_an_archive_or_not_there_is_a_typed_error() {
        let (lua, _dir) = scratch();
        let kinds: Vec<String> = eval(
            &lua,
            r#"
            local kinds = {}
            local function kind(value, err)
                assert(value == nil)
                table.insert(kinds, err.kind)
            end
            kind(archive.Tar.open(root .. "/nope.tar"))
            kind(archive.Zip.open(root .. "/nope.zip"))
            kind(archive.Zip.from_bytes("not a zip"):list())
            kind(archive.Tar.from_bytes("not a tar, and long enough to look like a header that is not one"):list())
            kind(archive.Tar.from_bytes("\31\139 not gzip"):list())
            kind(archive.Tar.from_bytes(""):read("x"))
            kind(archive.Tar.create({ { path = root .. "/nope" } }))
            kind(archive.Zip.create_file(root .. "/no/such/dir/out.zip", {}))
            return kinds
            "#,
        );
        assert_eq!(kinds, ["io", "io", "corrupt", "corrupt", "corrupt", "not_found", "io", "io"]);
    }

    #[test]
    fn what_is_not_an_option_or_an_entry_is_refused_as_an_argument() {
        let (lua, _dir) = scratch();
        for (call, place) in [
            (
                r#"archive.Tar.create({}, { compression = "lz4" })"#,
                "lib:archive.Tar.create: argument #2 'options'",
            ),
            (
                r#"archive.Tar.create({}, { compresion = "gzip" })"#,
                "lib:archive.Tar.create: argument #2 'options'",
            ),
            (
                r#"archive.Tar.create({ { archive_path = "x" } })"#,
                "lib:archive.Tar.create: argument #1 'entries'",
            ),
            (
                r#"archive.Tar.create({ { path = "x", archive_path = 5 } })"#,
                "lib:archive.Tar.create: argument #1 'entries'",
            ),
            (r#"archive.Tar.create({ "main.lua" })"#, "lib:archive.Tar.create: argument #1 'entries'"),
            (r#"archive.Zip.create({ "content" })"#, "lib:archive.Zip.create: argument #1 'entries'"),
            (r#"archive.Zip.create({ a = 5 })"#, "lib:archive.Zip.create: argument #1 'entries'"),
            (
                r#"archive.Tar.from_bytes(""):extract(root, { max_output_size = -1 })"#,
                "lib:archive.Tar:extract: argument #2 'options'",
            ),
            (
                r#"archive.Tar.from_bytes(""):extract(root, { max_output_size = 1.5 })"#,
                "lib:archive.Tar:extract: argument #2 'options'",
            ),
            (
                r#"archive.Tar.from_bytes(""):extract(root, { include = "*.lua" })"#,
                "lib:archive.Tar:extract: argument #2 'options'",
            ),
            (
                r#"archive.Zip.from_bytes(""):read("x", { include = {} })"#,
                "lib:archive.Zip:read: argument #2 'options'",
            ),
            (r"archive.Tar.from_bytes({})", "lib:archive.Tar.from_bytes: argument #1 'bytes'"),
        ] {
            let error = lua.load(format!("return {call}")).eval::<mlua::Value>().expect_err(call).to_string();
            assert!(error.contains(place), "{call}: {error}");
        }
    }
}

//! `lib:csv` reader path — streaming parse, record→Lua conversion, and the
//! verbatim-source error formatting. The writer half lives in `super::writer`;
//! the shared options and the public `lib:csv` surface in `super` (`csv.rs`).

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Read};
use std::sync::Arc;

use mlua::{Function, IntoLua, Lua, UserData, UserDataMethods, Value};
use parking_lot::Mutex;

use crate::lua::sandbox::{err, ok};
use crate::lua::std::io::ExclusiveBorrow;

use super::{ColumnType, MAX_RAW_LINE_BYTES, ParseOpts, RAW_CAPTURE_CAP};

/// Adapter implementing `Read` over an `Arc<Mutex<Box<dyn BufRead + Send>>>` —
/// the shape `ReaderHandle` exposes. Locks per read; never holds the guard
/// across calls.
pub(super) struct LockingReader {
    pub(super) inner: Arc<Mutex<Box<dyn BufRead + Send>>>,
}

impl Read for LockingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.lock().read(buf)
    }
}

// ---------------------------------------------------------------------------
// RawCapture — bounded rolling capture of source bytes
// ---------------------------------------------------------------------------

/// Rolling buffer of the most recent source bytes, keyed by their global byte
/// offset in the underlying stream. The error-formatting path slices this to
/// reproduce the offending line byte-for-byte.
pub(super) struct RawCapture {
    buf: VecDeque<u8>,
    base: u64,
    cap: usize,
}

impl RawCapture {
    fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(cap.min(8 * 1024)),
            base: 0,
            cap,
        }
    }

    fn append(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes.iter().copied());
        while self.buf.len() > self.cap {
            self.buf.pop_front();
            self.base += 1;
        }
    }

    /// Return bytes covering `[start, end)` if the range is fully captured.
    /// Returns `None` if either endpoint has been evicted or the range is invalid.
    fn slice(&self, start: u64, end: u64) -> Option<Vec<u8>> {
        if end < start {
            return None;
        }
        let buf_end = self.base + self.buf.len() as u64;
        if start < self.base || end > buf_end {
            return None;
        }
        let s = (start - self.base) as usize;
        let e = (end - self.base) as usize;
        Some(self.buf.range(s..e).copied().collect())
    }
}

/// `Read` adapter that mirrors every byte read into a `RawCapture`.
struct CapturingReader<R: Read> {
    inner: R,
    capture: Arc<Mutex<RawCapture>>,
}

impl<R: Read> Read for CapturingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.capture.lock().append(&buf[..n]);
        }
        Ok(n)
    }
}

pub(super) fn new_capture() -> Arc<Mutex<RawCapture>> {
    Arc::new(Mutex::new(RawCapture::new(RAW_CAPTURE_CAP)))
}

pub(super) fn wrap_capturing(reader: BoxedReader, capture: Arc<Mutex<RawCapture>>) -> BoxedReader {
    Box::new(CapturingReader {
        inner: reader,
        capture,
    })
}

// ---------------------------------------------------------------------------
// CsvReader UserData
// ---------------------------------------------------------------------------

pub(super) type BoxedReader = Box<dyn Read + Send>;

pub(super) struct CsvReaderState {
    reader: csv::Reader<BoxedReader>,
    opts: ParseOpts,
    capture: Arc<Mutex<RawCapture>>,
    headers_cache: Option<Result<Vec<String>, String>>,
    finished: bool,
    _borrow: Option<ExclusiveBorrow>,
}

impl CsvReaderState {
    fn ensure_headers(&mut self, prefix: &str) -> Result<Option<Vec<String>>, String> {
        if !self.opts.headers {
            return Ok(None);
        }
        if self.headers_cache.is_none() {
            let res = validate_headers(&mut self.reader, Some(&self.capture), &self.opts, prefix);
            self.headers_cache = Some(res);
        }
        match self.headers_cache.as_ref().unwrap() {
            Ok(v) => Ok(Some(v.clone())),
            Err(e) => Err(e.clone()),
        }
    }
}

pub(super) struct CsvReader {
    pub(super) state: Arc<Mutex<CsvReaderState>>,
}

#[neoc_macros::lua_userdata]
impl UserData for CsvReader {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Return the cached header row (parsing it if not yet read). Errors if the reader was not created with `headers = true`.
        #[lua("Result<LuaValue, string>")]
        methods.add_method("headers", |lua, this, _: ()| {
            let mut state = this.state.lock();
            if !state.opts.headers {
                return err(
                    lua,
                    "csv.reader: 'headers' option must be true to call :headers()",
                );
            }
            match state.ensure_headers("csv.reader") {
                Ok(Some(v)) => {
                    let t = lua.create_table()?;
                    for (i, h) in v.iter().enumerate() {
                        t.set(i + 1, lua.create_string(h.as_bytes())?)?;
                    }
                    ok(lua, t)
                }
                Ok(None) => ok(lua, Value::Nil),
                Err(msg) => err(lua, msg),
            }
        });

        /// Return a Lua iterator yielding the next row on each call, `nil` at EOF. Iterator errors raise.
        #[lua("LuaValue")]
        methods.add_method("rows", |lua, this, _: ()| {
            create_rows_iter(lua, this.state.clone(), "csv.reader")
        });
    }
}

// ---------------------------------------------------------------------------
// Iterator factory — shared by csv.rows and reader:rows
// ---------------------------------------------------------------------------

pub(super) fn create_rows_iter(
    lua: &Lua,
    state: Arc<Mutex<CsvReaderState>>,
    prefix: &'static str,
) -> mlua::Result<Function> {
    lua.create_function(move |lua, _: ()| -> mlua::Result<Value> {
        let mut s = state.lock();
        if s.finished {
            return Ok(Value::Nil);
        }
        // Lazy header parse + duplicate detection on first row.
        let headers = match s.ensure_headers(prefix) {
            Ok(h) => h,
            Err(msg) => {
                s.finished = true;
                return Err(mlua::Error::external(msg));
            }
        };
        let mut record = csv::ByteRecord::new();
        let read_res = s.reader.read_byte_record(&mut record);
        let read = match read_res {
            Ok(b) => b,
            Err(e) => {
                let end = s.reader.position().byte();
                let msg = csv_error_message(
                    prefix,
                    &e,
                    s.opts.delimiter,
                    s.opts.quote,
                    Some(&record),
                    Some(&s.capture),
                    Some(end),
                );
                s.finished = true;
                return Err(mlua::Error::external(msg));
            }
        };
        if !read {
            s.finished = true;
            return Ok(Value::Nil);
        }
        let line = record.position().map(|p| p.line()).unwrap_or(0);
        let row = match record_to_lua(
            lua,
            &record,
            headers.as_deref(),
            &s.opts.types,
            line,
            prefix,
        ) {
            Ok(v) => v,
            Err(msg) => {
                s.finished = true;
                return Err(mlua::Error::external(msg));
            }
        };
        Ok(row)
    })
}

pub(super) fn record_to_lua(
    lua: &Lua,
    record: &csv::ByteRecord,
    headers: Option<&[String]>,
    types: &HashMap<String, ColumnType>,
    line: u64,
    prefix: &str,
) -> Result<Value, String> {
    let t = lua.create_table().map_err(|e| format!("{prefix}: {e}"))?;
    match headers {
        Some(hs) => {
            // Length parity is enforced upstream by `ReaderBuilder::flexible(false)` —
            // any mismatch surfaces as `csv::ErrorKind::UnequalLengths` before reaching
            // here. The mapping is many→one (each header to a record field), so we
            // tolerate `record.len() < hs.len()` defensively via `record.get`.
            for (i, h) in hs.iter().enumerate() {
                let raw_field = record.get(i).unwrap_or(b"");
                let value = match types.get(h) {
                    Some(ty) => coerce_field(lua, raw_field, *ty, h, line, prefix)?,
                    None => string_field(lua, raw_field, prefix)?,
                };
                t.set(h.as_str(), value)
                    .map_err(|e| format!("{prefix}: {e}"))?;
            }
        }
        None => {
            for (i, field) in record.iter().enumerate() {
                let v = string_field(lua, field, prefix)?;
                t.set(i as i64 + 1, v)
                    .map_err(|e| format!("{prefix}: {e}"))?;
            }
        }
    }
    t.into_lua(lua).map_err(|e| format!("{prefix}: {e}"))
}

fn string_field(lua: &Lua, bytes: &[u8], prefix: &str) -> Result<Value, String> {
    lua.create_string(bytes)
        .map(Value::String)
        .map_err(|e| format!("{prefix}: {e}"))
}

fn coerce_field(
    _lua: &Lua,
    raw: &[u8],
    ty: ColumnType,
    column: &str,
    line: u64,
    prefix: &str,
) -> Result<Value, String> {
    if raw.is_empty() {
        return Ok(Value::Nil);
    }
    let s = std::str::from_utf8(raw)
        .map_err(|e| format!("{prefix}: line {line}: column \"{column}\" invalid utf-8: {e}"))?;
    match ty {
        ColumnType::Number => s.parse::<f64>().map(Value::Number).map_err(|_| {
            format!(
                "{prefix}: line {line}: column \"{column}\" cannot coerce \"{}\" to number",
                truncate_for_error(s, MAX_RAW_LINE_BYTES)
            )
        }),
        ColumnType::Boolean => match s.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(Value::Boolean(true)),
            "false" | "0" => Ok(Value::Boolean(false)),
            _ => Err(format!(
                "{prefix}: line {line}: column \"{column}\" cannot coerce \"{}\" to boolean",
                truncate_for_error(s, MAX_RAW_LINE_BYTES)
            )),
        },
    }
}

pub(super) fn build_csv_reader<R: Read>(r: R, opts: &ParseOpts) -> csv::Reader<R> {
    let mut b = csv::ReaderBuilder::new();
    b.has_headers(opts.headers);
    b.delimiter(opts.delimiter);
    b.quote(opts.quote);
    b.escape(Some(opts.escape));
    b.flexible(false);
    if let Some(c) = opts.comment {
        b.comment(Some(c));
    }
    b.from_reader(r)
}

pub(super) fn new_reader_state(
    reader: csv::Reader<BoxedReader>,
    opts: ParseOpts,
    capture: Arc<Mutex<RawCapture>>,
    borrow: Option<ExclusiveBorrow>,
) -> Arc<Mutex<CsvReaderState>> {
    Arc::new(Mutex::new(CsvReaderState {
        reader,
        opts,
        capture,
        headers_cache: None,
        finished: false,
        _borrow: borrow,
    }))
}

/// Read the header record, format duplicate-header and parse errors with the
/// `module.function` prefix. Shared by `parse_fn` and `CsvReaderState`.
pub(super) fn validate_headers<R: Read>(
    reader: &mut csv::Reader<R>,
    capture: Option<&Arc<Mutex<RawCapture>>>,
    opts: &ParseOpts,
    prefix: &str,
) -> Result<Vec<String>, String> {
    let header_res = reader.headers().cloned();
    let end = reader.position().byte();
    let rec = match header_res {
        Ok(r) => r,
        Err(e) => {
            return Err(csv_error_message(
                prefix,
                &e,
                opts.delimiter,
                opts.quote,
                None,
                capture,
                Some(end),
            ));
        }
    };
    let fields: Vec<String> = rec.iter().map(|s| s.to_string()).collect();
    if let Some(dup) = find_duplicate(&fields) {
        let refs: Vec<&str> = rec.iter().collect();
        let raw = raw_line_text(
            Some(0),
            Some(end),
            capture,
            opts.delimiter,
            opts.quote,
            &refs,
        );
        return Err(format!(
            "{prefix}: line 1: duplicate header \"{dup}\": \"{}\"",
            truncate_for_error(&raw, MAX_RAW_LINE_BYTES)
        ));
    }
    Ok(fields)
}

pub(super) fn csv_error_message(
    prefix: &str,
    e: &csv::Error,
    delimiter: u8,
    quote: u8,
    record: Option<&csv::ByteRecord>,
    capture: Option<&Arc<Mutex<RawCapture>>>,
    reader_byte: Option<u64>,
) -> String {
    use csv::ErrorKind::*;
    match e.kind() {
        UnequalLengths {
            pos,
            expected_len,
            len,
        } => {
            let line = pos.as_ref().map(|p| p.line()).unwrap_or(0);
            let raw = error_line(record, capture, reader_byte, delimiter, quote);
            format!(
                "{prefix}: line {line}: found {len} fields, expected {expected_len}: \"{}\"",
                truncate_for_error(&raw, MAX_RAW_LINE_BYTES)
            )
        }
        Utf8 { pos, err } => {
            let line = pos.as_ref().map(|p| p.line()).unwrap_or(0);
            format!("{prefix}: line {line}: invalid utf-8: {err}")
        }
        Io(io_err) => format!("{prefix}: {io_err}"),
        _ => format!("{prefix}: {e}"),
    }
}

/// Best raw-byte line for the failing record. Tries the capture first; on miss,
/// reconstructs from the record's parsed fields (lossy on non-UTF-8 bytes).
fn error_line(
    record: Option<&csv::ByteRecord>,
    capture: Option<&Arc<Mutex<RawCapture>>>,
    reader_byte: Option<u64>,
    delimiter: u8,
    quote: u8,
) -> String {
    if let Some(rec) = record {
        let start = rec.position().map(|p| p.byte());
        let owned: Vec<String> = rec
            .iter()
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
        raw_line_text(start, reader_byte, capture, delimiter, quote, &refs)
    } else {
        String::new()
    }
}

/// Slice `[start, end)` out of the capture and strip the trailing record
/// terminator; on miss fall back to reconstructed line from `fallback_fields`.
fn raw_line_text(
    start: Option<u64>,
    end: Option<u64>,
    capture: Option<&Arc<Mutex<RawCapture>>>,
    delimiter: u8,
    quote: u8,
    fallback_fields: &[&str],
) -> String {
    if let (Some(cap), Some(s), Some(e)) = (capture, start, end)
        && let Some(bytes) = cap.lock().slice(s, e)
    {
        let trimmed = strip_terminator(&bytes);
        return String::from_utf8_lossy(trimmed).into_owned();
    }
    reconstruct_line(fallback_fields, delimiter, quote)
}

fn strip_terminator(bytes: &[u8]) -> &[u8] {
    let mut b = bytes;
    // The csv crate places `Position::byte()` after the first terminator byte.
    // For CRLF that lands on the `\n`, so the slice starts with the prior
    // record's `\n` — strip it.
    if b.first() == Some(&b'\n') {
        b = &b[1..];
    }
    if b.last() == Some(&b'\n') {
        b = &b[..b.len() - 1];
    }
    if b.last() == Some(&b'\r') {
        b = &b[..b.len() - 1];
    }
    b
}

fn find_duplicate(headers: &[String]) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    for h in headers {
        if !seen.insert(h.as_str()) {
            return Some(h.clone());
        }
    }
    None
}

fn reconstruct_line(fields: &[&str], delimiter: u8, quote: u8) -> String {
    let delim = delimiter as char;
    let q = quote as char;
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(delim);
        }
        let needs_quote =
            f.contains(delim) || f.contains(q) || f.contains('\n') || f.contains('\r');
        if needs_quote {
            out.push(q);
            for c in f.chars() {
                if c == q {
                    out.push(q);
                }
                out.push(c);
            }
            out.push(q);
        } else {
            out.push_str(f);
        }
    }
    out
}

fn truncate_for_error(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push('…');
    out
}

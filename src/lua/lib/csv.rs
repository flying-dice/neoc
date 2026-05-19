//! CSV parsing and writing — `lib:csv`.
//!
//! See docs/src/rfcs/0010-lib-csv.md for the public surface contract.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Cursor, Read};
use std::sync::Arc;

use mlua::{Function, IntoLua, Lua, Table, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tracing::instrument;

use crate::lua::sandbox::{err, from_err, ok};
use crate::lua::std::io::{ExclusiveBorrow, ReaderHandle};

const MAX_RAW_LINE_BYTES: usize = 80;

/// Source-byte capture buffer cap. Comfortably above any realistic CSV record;
/// the underlying `csv` crate buffers in 8 KiB chunks, so this guarantees the
/// failing record's bytes are still in the capture when the error is raised.
const RAW_CAPTURE_CAP: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Module entry
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("parse", lua.create_function(parse_fn)?)?;
    t.set("rows", lua.create_function(rows_fn)?)?;
    t.set("reader", lua.create_function(reader_fn)?)?;
    t.set("stringify", lua.create_function(stringify_fn)?)?;
    t.set("writer", lua.create_function(writer_fn)?)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnType {
    Number,
    Boolean,
}

#[derive(Clone)]
struct ParseOpts {
    delimiter: u8,
    headers: bool,
    quote: u8,
    escape: u8,
    comment: Option<u8>,
    types: HashMap<String, ColumnType>,
}

impl Default for ParseOpts {
    fn default() -> Self {
        Self {
            delimiter: b',',
            headers: false,
            quote: b'"',
            escape: b'"',
            comment: None,
            types: HashMap::new(),
        }
    }
}

#[derive(Clone)]
struct WriteOpts {
    delimiter: u8,
    quote: u8,
    headers: Vec<String>,
}

fn parse_parse_opts(opts: Option<&Table>, prefix: &str) -> Result<ParseOpts, String> {
    let mut out = ParseOpts::default();
    let Some(t) = opts else { return Ok(out) };

    if let Some(b) = single_byte_opt(t, "delimiter", prefix)? {
        out.delimiter = b;
    }
    if let Some(b) = single_byte_opt(t, "quote", prefix)? {
        out.quote = b;
        out.escape = b;
    }
    if let Some(b) = single_byte_opt(t, "escape", prefix)? {
        out.escape = b;
    }
    if let Some(b) = single_byte_opt(t, "comment", prefix)? {
        out.comment = Some(b);
    }
    match t
        .get::<Value>("headers")
        .map_err(|e| format!("{prefix}: invalid 'headers': {e}"))?
    {
        Value::Nil => {}
        Value::Boolean(b) => out.headers = b,
        _ => return Err(format!("{prefix}: 'headers' must be a boolean")),
    }
    let types_val: Value = t
        .get("types")
        .map_err(|e| format!("{prefix}: invalid 'types': {e}"))?;
    match types_val {
        Value::Nil => {}
        Value::Table(tt) => {
            out.types = parse_types_map(&tt, prefix)?;
        }
        _ => return Err(format!("{prefix}: 'types' must be a table")),
    }
    if !out.types.is_empty() && !out.headers {
        return Err(format!("{prefix}: 'types' requires 'headers = true'"));
    }
    Ok(out)
}

fn parse_write_opts(opts: &Table, prefix: &str) -> Result<WriteOpts, String> {
    let mut delimiter = b',';
    let mut quote = b'"';
    if let Some(b) = single_byte_opt(opts, "delimiter", prefix)? {
        delimiter = b;
    }
    if let Some(b) = single_byte_opt(opts, "quote", prefix)? {
        quote = b;
    }
    let headers_val: Value = opts
        .get("headers")
        .map_err(|e| format!("{prefix}: invalid 'headers': {e}"))?;
    let headers = match headers_val {
        Value::Table(t) => {
            let mut v = Vec::new();
            for entry in t.sequence_values::<Value>() {
                match entry.map_err(|e| format!("{prefix}: invalid 'headers' entry: {e}"))? {
                    Value::String(s) => v.push(
                        s.to_str()
                            .map(|c| c.to_string())
                            .map_err(|e| format!("{prefix}: 'headers' entry not utf-8: {e}"))?,
                    ),
                    _ => return Err(format!("{prefix}: 'headers' must be a list of strings")),
                }
            }
            v
        }
        Value::Nil => return Err(format!("{prefix}: 'headers' is required")),
        _ => return Err(format!("{prefix}: 'headers' must be a list of strings")),
    };
    if headers.is_empty() {
        return Err(format!(
            "{prefix}: 'headers' must contain at least one column"
        ));
    }
    Ok(WriteOpts {
        delimiter,
        quote,
        headers,
    })
}

fn single_byte_opt(t: &Table, key: &str, prefix: &str) -> Result<Option<u8>, String> {
    let v: Value = t
        .get(key)
        .map_err(|e| format!("{prefix}: invalid '{key}': {e}"))?;
    match v {
        Value::Nil => Ok(None),
        Value::String(s) => {
            let bytes = s.as_bytes();
            if bytes.len() != 1 {
                Err(format!("{prefix}: '{key}' must be a single byte"))
            } else {
                Ok(Some(bytes[0]))
            }
        }
        _ => Err(format!("{prefix}: '{key}' must be a single-byte string")),
    }
}

fn parse_types_map(t: &Table, prefix: &str) -> Result<HashMap<String, ColumnType>, String> {
    let mut out = HashMap::new();
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, v) = pair.map_err(|e| format!("{prefix}: invalid 'types' entry: {e}"))?;
        let key = match k {
            Value::String(s) => s
                .to_str()
                .map(|c| c.to_string())
                .map_err(|e| format!("{prefix}: 'types' key not utf-8: {e}"))?,
            _ => return Err(format!("{prefix}: 'types' keys must be strings")),
        };
        let ty = match v {
            Value::String(s) => match &*s.as_bytes() {
                b"number" => ColumnType::Number,
                b"boolean" => ColumnType::Boolean,
                _ => {
                    return Err(format!(
                        "{prefix}: types[\"{key}\"]: unsupported type, expected \"number\" or \"boolean\""
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "{prefix}: types[\"{key}\"]: value must be a string"
                ));
            }
        };
        out.insert(key, ty);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// LockingReader — Read adapter over an Arc<Mutex<...>> handle
// ---------------------------------------------------------------------------

/// Adapter implementing `Read` over an `Arc<Mutex<Box<dyn BufRead + Send>>>` —
/// the shape `ReaderHandle` exposes. Locks per read; never holds the guard
/// across calls.
struct LockingReader {
    inner: Arc<Mutex<Box<dyn BufRead + Send>>>,
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
struct RawCapture {
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

fn new_capture() -> Arc<Mutex<RawCapture>> {
    Arc::new(Mutex::new(RawCapture::new(RAW_CAPTURE_CAP)))
}

fn wrap_capturing(reader: BoxedReader, capture: Arc<Mutex<RawCapture>>) -> BoxedReader {
    Box::new(CapturingReader {
        inner: reader,
        capture,
    })
}

// ---------------------------------------------------------------------------
// CsvReader UserData
// ---------------------------------------------------------------------------

type BoxedReader = Box<dyn Read + Send>;

struct CsvReaderState {
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

struct CsvReader {
    state: Arc<Mutex<CsvReaderState>>,
}

impl UserData for CsvReader {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
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

        methods.add_method("rows", |lua, this, _: ()| {
            create_rows_iter(lua, this.state.clone(), "csv.reader")
        });
    }
}

// ---------------------------------------------------------------------------
// Iterator factory — shared by csv.rows and reader:rows
// ---------------------------------------------------------------------------

fn create_rows_iter(
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

// ---------------------------------------------------------------------------
// Top-level functions
// ---------------------------------------------------------------------------

#[instrument(name = "csv.parse", target = "luau::lib::csv", skip_all)]
fn parse_fn(
    lua: &Lua,
    (data, opts): (mlua::String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let prefix = "csv.parse";
    let parse_opts = match parse_parse_opts(opts.as_ref(), prefix) {
        Ok(o) => o,
        Err(msg) => return err(lua, msg),
    };
    let bytes = data.as_bytes().to_vec();
    let capture = new_capture();
    let source = wrap_capturing(Box::new(Cursor::new(bytes)) as BoxedReader, capture.clone());
    let mut reader = build_csv_reader(source, &parse_opts);
    let headers = if parse_opts.headers {
        match validate_headers(&mut reader, Some(&capture), &parse_opts, prefix) {
            Ok(v) => Some(v),
            Err(msg) => return err(lua, msg),
        }
    } else {
        None
    };
    let out = lua.create_table()?;
    let mut idx: i64 = 1;
    let mut record = csv::ByteRecord::new();
    loop {
        match reader.read_byte_record(&mut record) {
            Ok(false) => break,
            Ok(true) => {
                let line = record.position().map(|p| p.line()).unwrap_or(0);
                let v = match record_to_lua(
                    lua,
                    &record,
                    headers.as_deref(),
                    &parse_opts.types,
                    line,
                    prefix,
                ) {
                    Ok(v) => v,
                    Err(msg) => return err(lua, msg),
                };
                out.set(idx, v)?;
                idx += 1;
            }
            Err(e) => {
                let end = reader.position().byte();
                return err(
                    lua,
                    csv_error_message(
                        prefix,
                        &e,
                        parse_opts.delimiter,
                        parse_opts.quote,
                        Some(&record),
                        Some(&capture),
                        Some(end),
                    ),
                );
            }
        }
    }
    ok(lua, out)
}

#[instrument(name = "csv.rows", target = "luau::lib::csv", skip_all)]
fn rows_fn(lua: &Lua, (data, opts): (mlua::String, Option<Table>)) -> mlua::Result<Function> {
    let parse_opts = parse_parse_opts(opts.as_ref(), "csv.rows").map_err(mlua::Error::external)?;
    let bytes = data.as_bytes().to_vec();
    let capture = new_capture();
    let source = wrap_capturing(Box::new(Cursor::new(bytes)) as BoxedReader, capture.clone());
    let reader = build_csv_reader(source, &parse_opts);
    let state = new_reader_state(reader, parse_opts, capture, None);
    create_rows_iter(lua, state, "csv.rows")
}

#[instrument(name = "csv.reader", target = "luau::lib::csv", skip_all)]
fn reader_fn(
    lua: &Lua,
    (handle_val, opts): (Value, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let parse_opts = match parse_parse_opts(opts.as_ref(), "csv.reader") {
        Ok(o) => o,
        Err(msg) => return err(lua, msg),
    };
    let handle_ud = match handle_val {
        Value::UserData(ud) => ud,
        _ => return err(lua, "csv.reader: argument must be a ReaderHandle"),
    };
    let rh = match handle_ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return err(lua, "csv.reader: argument must be a ReaderHandle"),
    };
    let borrow = match rh.try_borrow_exclusive() {
        Some(b) => b,
        None => {
            return err(
                lua,
                "csv.reader: handle is already in use by another reader",
            );
        }
    };
    let locking = LockingReader {
        inner: rh.inner.clone(),
    };
    drop(rh);
    let capture = new_capture();
    let source = wrap_capturing(Box::new(locking) as BoxedReader, capture.clone());
    let reader = build_csv_reader(source, &parse_opts);
    let state = new_reader_state(reader, parse_opts, capture, Some(borrow));
    ok(lua, CsvReader { state })
}

#[instrument(name = "csv.stringify", target = "luau::lib::csv", skip_all)]
fn stringify_fn(lua: &Lua, (records, opts): (Table, Table)) -> mlua::Result<(Value, Value)> {
    let prefix = "csv.stringify";
    let write_opts = match parse_write_opts(&opts, prefix) {
        Ok(o) => o,
        Err(msg) => return err(lua, msg),
    };
    let buf: Vec<u8> = Vec::new();
    let mut writer = build_csv_writer(buf, &write_opts);
    if let Err(e) = writer.write_record(&write_opts.headers) {
        return from_err(lua, prefix, e);
    }
    for entry in records.sequence_values::<Value>() {
        let v = entry?;
        let row = match v {
            Value::Table(t) => t,
            _ => return err(lua, format!("{prefix}: records must be tables")),
        };
        let fields = match collect_keyed_row(lua, &row, &write_opts.headers, prefix) {
            Ok(v) => v,
            Err(msg) => return err(lua, msg),
        };
        if let Err(e) = writer.write_record(&fields) {
            return from_err(lua, prefix, e);
        }
    }
    let bytes = match writer.into_inner() {
        Ok(b) => b,
        Err(e) => return from_err(lua, prefix, e.error()),
    };
    match String::from_utf8(bytes) {
        Ok(s) => ok(lua, lua.create_string(s.as_bytes())?),
        Err(e) => from_err(lua, prefix, e),
    }
}

#[instrument(name = "csv.writer", target = "luau::lib::csv", skip_all)]
fn writer_fn(lua: &Lua, opts: Table) -> mlua::Result<(Value, Value)> {
    let prefix = "csv.writer";
    let write_opts = match parse_write_opts(&opts, prefix) {
        Ok(o) => o,
        Err(msg) => return err(lua, msg),
    };
    let buf: Vec<u8> = Vec::new();
    let mut writer = build_csv_writer(buf, &write_opts);
    if let Err(e) = writer.write_record(&write_opts.headers) {
        return from_err(lua, prefix, e);
    }
    let state = CsvWriterState {
        writer,
        headers: write_opts.headers,
        finished: false,
    };
    ok(
        lua,
        CsvWriter {
            state: Arc::new(Mutex::new(state)),
        },
    )
}

// ---------------------------------------------------------------------------
// CsvWriter UserData
// ---------------------------------------------------------------------------

struct CsvWriterState {
    writer: csv::Writer<Vec<u8>>,
    headers: Vec<String>,
    finished: bool,
}

struct CsvWriter {
    state: Arc<Mutex<CsvWriterState>>,
}

impl UserData for CsvWriter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("row", |lua, this, values: Table| {
            let mut s = this.state.lock();
            if s.finished {
                return err(lua, "csv.writer: already finished");
            }
            let shape = match detect_shape(&values, "csv.writer") {
                Ok(sh) => sh,
                Err(msg) => return err(lua, msg),
            };
            let fields = match shape {
                Shape::Array(len) => {
                    if len != s.headers.len() {
                        return err(
                            lua,
                            format!(
                                "csv.writer: row has {} values, expected {} (headers length)",
                                len,
                                s.headers.len()
                            ),
                        );
                    }
                    let mut v = Vec::with_capacity(len);
                    for i in 1..=len {
                        let cell: Value = values.get(i as i64)?;
                        match value_to_field(&cell, "csv.writer") {
                            Ok(f) => v.push(f),
                            Err(msg) => return err(lua, msg),
                        }
                    }
                    v
                }
                Shape::Keyed => match collect_keyed_row(lua, &values, &s.headers, "csv.writer") {
                    Ok(v) => v,
                    Err(msg) => return err(lua, msg),
                },
            };
            if let Err(e) = s.writer.write_record(&fields) {
                return from_err(lua, "csv.writer", e);
            }
            ok(lua, true)
        });

        methods.add_method("finish", |lua, this, _: ()| {
            let mut s = this.state.lock();
            if s.finished {
                return err(lua, "csv.writer: already finished");
            }
            s.finished = true;
            if let Err(e) = s.writer.flush() {
                return from_err(lua, "csv.writer", e);
            }
            // Swap out the writer to take ownership of the buffer.
            let placeholder = csv::WriterBuilder::new().from_writer(Vec::new());
            let writer = std::mem::replace(&mut s.writer, placeholder);
            let bytes = match writer.into_inner() {
                Ok(b) => b,
                Err(e) => return from_err(lua, "csv.writer", e.error()),
            };
            match String::from_utf8(bytes) {
                Ok(out) => ok(lua, lua.create_string(out.as_bytes())?),
                Err(e) => from_err(lua, "csv.writer", e),
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Shape detection — array vs keyed
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Shape {
    Array(usize),
    Keyed,
}

fn detect_shape(t: &Table, prefix: &str) -> Result<Shape, String> {
    let mut max_int: i64 = 0;
    let mut int_count = 0usize;
    let mut string_count = 0usize;
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, _v) = pair.map_err(|e| format!("{prefix}: invalid row table: {e}"))?;
        match k {
            Value::Integer(i) if i >= 1 => {
                int_count += 1;
                let i64v = i as i64;
                if i64v > max_int {
                    max_int = i64v;
                }
            }
            Value::String(_) => {
                string_count += 1;
            }
            _ => {
                return Err(format!(
                    "{prefix}: row keys must be positive integers or strings"
                ));
            }
        }
    }
    if int_count > 0 && string_count > 0 {
        return Err(format!(
            "{prefix}: row values must be array or keyed table, not both"
        ));
    }
    if int_count > 0 {
        if int_count as i64 != max_int {
            return Err(format!(
                "{prefix}: row array has gaps (max index {max_int}, count {int_count})"
            ));
        }
        return Ok(Shape::Array(int_count));
    }
    Ok(Shape::Keyed)
}

fn collect_keyed_row(
    _lua: &Lua,
    t: &Table,
    headers: &[String],
    prefix: &str,
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(headers.len());
    for h in headers {
        let v: Value = t
            .get::<Value>(h.as_str())
            .map_err(|e| format!("{prefix}: lookup '{h}': {e}"))?;
        if matches!(v, Value::Nil) {
            out.push(String::new());
        } else {
            out.push(value_to_field(&v, prefix)?);
        }
    }
    Ok(out)
}

fn value_to_field(v: &Value, prefix: &str) -> Result<String, String> {
    match v {
        Value::Nil => Ok(String::new()),
        Value::String(s) => s
            .to_str()
            .map(|c| c.to_string())
            .map_err(|e| format!("{prefix}: field is not utf-8: {e}")),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(format_number(*n)),
        Value::Boolean(b) => Ok(if *b { "true".into() } else { "false".into() }),
        _ => Err(format!(
            "{prefix}: field must be string/number/boolean/nil, got {}",
            v.type_name()
        )),
    }
}

fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e16 {
        format!("{:.0}", n)
    } else {
        n.to_string()
    }
}

// ---------------------------------------------------------------------------
// Record conversion
// ---------------------------------------------------------------------------

fn record_to_lua(
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

// ---------------------------------------------------------------------------
// Reader / writer builders
// ---------------------------------------------------------------------------

fn build_csv_reader<R: Read>(r: R, opts: &ParseOpts) -> csv::Reader<R> {
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

fn build_csv_writer<W: std::io::Write>(w: W, opts: &WriteOpts) -> csv::Writer<W> {
    let mut b = csv::WriterBuilder::new();
    b.delimiter(opts.delimiter);
    b.quote(opts.quote);
    b.flexible(false);
    b.from_writer(w)
}

fn new_reader_state(
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
fn validate_headers<R: Read>(
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
            None,
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

// ---------------------------------------------------------------------------
// Error formatting + raw-line helpers
//
// The offending-line text in parse errors reproduces the source bytes verbatim
// by slicing them out of a rolling `RawCapture` keyed on global byte offset.
// If the bytes have aged out of the capture (cap exceeded) we fall back to a
// best-effort reconstruction from the parsed fields.
// ---------------------------------------------------------------------------

fn csv_error_message(
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
        raw_line_text(
            Some(rec),
            start,
            reader_byte,
            capture,
            delimiter,
            quote,
            &refs,
        )
    } else {
        String::new()
    }
}

/// Slice `[start, end)` out of the capture and strip the trailing record
/// terminator; on miss fall back to reconstructed line from `fallback_fields`.
fn raw_line_text(
    _record: Option<&csv::ByteRecord>,
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

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::{Throughput, black_box};

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let mut payload = String::from("name,age,city\n");
    for i in 0..200 {
        payload.push_str(&format!("user{i},30,Sheffield\n"));
    }
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("csv");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    let parse_chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("lib:csv")).parse(p, { headers = true }) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("parse_headers", |b| {
        b.iter(|| {
            let _: mlua::Table = parse_chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let rows_chunk: mlua::Function = lua
        .load(
            r#"return function(p)
                local n = 0
                for _ in (require("lib:csv")).rows(p, { headers = true }) do n = n + 1 end
                return n
            end"#,
        )
        .eval()
        .expect("chunk");
    g.bench_function("rows_iter", |b| {
        b.iter(|| {
            let _: i64 = rows_chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:csv")"#).eval().unwrap();
        lua.globals().set("csv", m).unwrap();
        lua
    }

    #[test]
    fn parse_rows_no_headers() {
        let lua = engine();
        let result: Vec<Vec<String>> = lua
            .load(
                r#"
                local rows = csv.parse("a,b,c\n1,2,3\n4,5,6")
                return rows
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            result,
            vec![
                vec!["a", "b", "c"],
                vec!["1", "2", "3"],
                vec!["4", "5", "6"]
            ]
        );
    }

    #[test]
    fn parse_with_headers() {
        let lua = engine();
        let (name, age): (String, String) = lua
            .load(
                r#"
                local rows = csv.parse("name,age\nAlice,30\nBob,25", { headers = true })
                return rows[1].name, rows[2].age
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(age, "25");
    }

    #[test]
    fn parse_with_types_number() {
        let lua = engine();
        let (n, t): (f64, String) = lua
            .load(
                r#"
                local rows = csv.parse("duration,passed\n1.23,true", {
                    headers = true,
                    types = { duration = "number" },
                })
                return rows[1].duration, type(rows[1].duration)
            "#,
            )
            .eval()
            .unwrap();
        assert!((n - 1.23).abs() < 1e-9);
        assert_eq!(t, "number");
    }

    #[test]
    fn parse_types_without_headers_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(
                r#"
                return csv.parse("a,b\n1,2", { types = { a = "number" } })
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("'types' requires 'headers = true'"), "got: {e}");
    }

    #[test]
    fn parse_duplicate_headers_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse("name,age,name\nAlice,30,bad", { headers = true })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("duplicate header"), "got: {e}");
        assert!(e.contains("\"name\""), "got: {e}");
    }

    #[test]
    fn parse_unequal_lengths_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse("a,b\n1,2\n3", { headers = true })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("expected 2"), "got: {e}");
    }

    #[test]
    fn parse_error_preserves_source_quoting() {
        let lua = engine();
        // The first field is quoted in source even though it's a single char.
        // Reconstruction would unquote it; the raw capture must preserve it.
        let (_, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse('a,b,c\n"x",y,z,extra\n')"#)
            .eval()
            .unwrap();
        let e = e.unwrap();
        assert!(
            e.ends_with(r#": ""x",y,z,extra""#),
            "expected source-quoted snippet, got: {e}"
        );
    }

    #[test]
    fn parse_error_preserves_escaped_quotes() {
        let lua = engine();
        let (_, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse('a,b\n"he said ""hi""",1,extra')"#)
            .eval()
            .unwrap();
        let e = e.unwrap();
        assert!(
            e.contains(r#""he said ""hi""",1,extra"#),
            "expected escaped quotes preserved, got: {e}"
        );
    }

    #[test]
    fn parse_error_truncates_long_line() {
        let lua = engine();
        // Construct an offending row well over 80 bytes — must end in `…`.
        let script = format!(
            r#"return csv.parse('a,b\n{},extra')"#,
            "x".repeat(120) + ",y"
        );
        let (_, e): (Option<mlua::Table>, Option<String>) = lua.load(&script).eval().unwrap();
        let e = e.unwrap();
        assert!(e.ends_with("…\""), "expected trailing ellipsis, got: {e}");
    }

    #[test]
    fn rows_error_preserves_source_quoting() {
        let lua = engine();
        let err: mlua::Result<i64> = lua
            .load(
                r#"
                for _ in csv.rows('a,b\n"x",y,extra\n') do end
                return 0
            "#,
            )
            .eval();
        let msg = format!("{}", err.unwrap_err());
        assert!(
            msg.contains(r#""x",y,extra"#),
            "expected raw source bytes, got: {msg}"
        );
    }

    #[test]
    fn reader_error_preserves_source_quoting() {
        let lua = engine();
        let err: mlua::Result<i64> = lua
            .load(
                r#"
                local io = require("std:io")
                local h = io.from_string('a,b\n"x",y,extra\n')
                local r = csv.reader(h)
                for _ in r:rows() do end
                return 0
            "#,
            )
            .eval();
        let msg = format!("{}", err.unwrap_err());
        assert!(
            msg.contains(r#""x",y,extra"#),
            "expected raw source bytes from reader, got: {msg}"
        );
    }

    #[test]
    fn parse_empty_field_untyped_is_empty_string() {
        let lua = engine();
        let s: String = lua
            .load(r#"local r = csv.parse("a,,c") return r[1][2]"#)
            .eval()
            .unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn parse_empty_field_typed_is_nil() {
        let lua = engine();
        let is_nil: bool = lua
            .load(
                r#"
                local r = csv.parse("name,age\nAlice,", {
                    headers = true,
                    types = { age = "number" },
                })
                return r[1].age == nil
            "#,
            )
            .eval()
            .unwrap();
        assert!(is_nil);
    }

    #[test]
    fn parse_type_coercion_failure_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(
                r#"
                return csv.parse("dur\nabc", {
                    headers = true,
                    types = { dur = "number" },
                })
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("cannot coerce"), "got: {e}");
    }

    #[test]
    fn parse_tsv_via_delimiter() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local r = csv.parse("a\tb\nx\ty", { delimiter = "\t", headers = true })
                return r[1].b
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "y");
    }

    #[test]
    fn parse_multibyte_delimiter_errors() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse("a,b", { delimiter = ",," })"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("must be a single byte"), "got: {e}");
    }

    #[test]
    fn parse_handles_quoted_field_with_newline() {
        let lua = engine();
        let s: String = lua
            .load(r#"local r = csv.parse("a,b\n\"line\n1\",\"x\"") return r[2][1]"#)
            .eval()
            .unwrap();
        assert_eq!(s, "line\n1");
    }

    #[test]
    fn rows_iterator_traverses_all_records() {
        let lua = engine();
        let n: i64 = lua
            .load(
                r#"
                local n = 0
                for _ in csv.rows("a,b\n1,2\n3,4\n5,6", { headers = true }) do
                    n = n + 1
                end
                return n
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn rows_iterator_raises_on_error() {
        let lua = engine();
        let err: mlua::Result<i64> = lua
            .load(
                r#"
                local n = 0
                for _ in csv.rows("a,b\n1,2\n3", { headers = true }) do
                    n = n + 1
                end
                return n
            "#,
            )
            .eval();
        assert!(err.is_err(), "expected raise, got: {err:?}");
    }

    #[test]
    fn reader_from_string_handle_streams_records() {
        let lua = engine();
        let n: i64 = lua
            .load(
                r#"
                local io = require("std:io")
                local h = io.from_string("a,b\n1,2\n3,4\n5,6")
                local r = csv.reader(h, { headers = true })
                local count = 0
                for _ in r:rows() do count = count + 1 end
                return count
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn reader_headers_cached() {
        let lua = engine();
        let s: String = lua
            .load(
                r#"
                local io = require("std:io")
                local h = io.from_string("name,age\nAlice,30")
                local r = csv.reader(h, { headers = true })
                local first = r:headers()
                local second = r:headers()
                return first[1] .. ":" .. second[1]
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(s, "name:name");
    }

    #[test]
    fn reader_headers_requires_headers_opt() {
        let lua = engine();
        let (val, e): (Option<mlua::Table>, Option<String>) = lua
            .load(
                r#"
                local io = require("std:io")
                local h = io.from_string("a,b")
                local r = csv.reader(h)
                return r:headers()
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().contains("'headers' option must be true"));
    }

    #[test]
    fn reader_exclusive_borrow_rejects_second_reader() {
        let lua = engine();
        let (val, e): (Option<mlua::AnyUserData>, Option<String>) = lua
            .load(
                r#"
                local io = require("std:io")
                local h = io.from_string("a,b\n1,2")
                local _r1 = csv.reader(h)
                return csv.reader(h)
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("already in use"), "got: {e}");
    }

    #[test]
    fn reader_argument_must_be_handle() {
        let lua = engine();
        let res: mlua::Result<(Option<mlua::AnyUserData>, Option<String>)> =
            lua.load(r#"return csv.reader("not a handle")"#).eval();
        assert!(res.is_err() || res.unwrap().1.is_some());
    }

    #[test]
    fn stringify_writes_headers_and_rows() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return csv.stringify({
                    { name = "Alice", age = "30" },
                    { name = "Bob",   age = "25" },
                }, { headers = { "name", "age" } })
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "name,age\nAlice,30\nBob,25\n");
    }

    #[test]
    fn stringify_missing_key_emits_empty() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                return csv.stringify(
                    {{ name = "Alice" }},
                    { headers = { "name", "age" } }
                )
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "name,age\nAlice,\n");
    }

    #[test]
    fn stringify_requires_headers() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) =
            lua.load(r#"return csv.stringify({}, {})"#).eval().unwrap();
        assert!(val.is_none());
        assert!(e.unwrap().contains("'headers' is required"));
    }

    #[test]
    fn writer_accepts_array_row() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                local w = csv.writer({ headers = { "test", "status" } })
                w:row({ "login", "PASS" })
                w:row({ "signup", "FAIL" })
                return w:finish()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "test,status\nlogin,PASS\nsignup,FAIL\n");
    }

    #[test]
    fn writer_accepts_keyed_row() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                local w = csv.writer({ headers = { "test", "status" } })
                w:row({ test = "login", status = "PASS" })
                return w:finish()
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "test,status\nlogin,PASS\n");
    }

    #[test]
    fn writer_array_length_mismatch_errors() {
        let lua = engine();
        let (val, e): (Option<bool>, Option<String>) = lua
            .load(
                r#"
                local w = csv.writer({ headers = { "a", "b", "c" } })
                return w:row({ "x", "y" })
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("expected 3"), "got: {e}");
    }

    #[test]
    fn writer_mixed_keys_errors() {
        let lua = engine();
        let (val, e): (Option<bool>, Option<String>) = lua
            .load(
                r#"
                local w = csv.writer({ headers = { "a", "b" } })
                return w:row({ "x", b = "y" })
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("array or keyed table, not both"), "got: {e}");
    }

    #[test]
    fn writer_finish_twice_errors() {
        let lua = engine();
        let (val, e): (Option<String>, Option<String>) = lua
            .load(
                r#"
                local w = csv.writer({ headers = { "a" } })
                w:finish()
                return w:finish()
            "#,
            )
            .eval()
            .unwrap();
        assert!(val.is_none());
        let e = e.unwrap();
        assert!(e.contains("already finished"), "got: {e}");
    }

    #[test]
    fn round_trip_parse_stringify() {
        let lua = engine();
        let out: String = lua
            .load(
                r#"
                local input = "name,city\nAlice,Sheffield\nBob,Leeds\n"
                local records = csv.parse(input, { headers = true })
                return csv.stringify(records, { headers = { "name", "city" } })
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(out, "name,city\nAlice,Sheffield\nBob,Leeds\n");
    }
}

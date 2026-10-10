//! CSV parsing and writing — `lib:csv`.
//!
//! See docs/src/rfcs/0010-lib-csv.md for the public surface contract.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use mlua::{Function, Lua, Table, Value};
use parking_lot::Mutex;
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};
use crate::lua::std::io::ReaderHandle;

mod reader;
mod writer;

use reader::{
    BoxedReader, CsvReader, LockingReader, build_csv_reader, create_rows_iter, csv_error_message,
    new_capture, new_reader_state, record_to_lua, validate_headers, wrap_capturing,
};
use writer::{CsvWriter, CsvWriterState, build_csv_writer, collect_keyed_row};
const MAX_RAW_LINE_BYTES: usize = 80;

/// Source-byte capture buffer cap. Comfortably above any realistic CSV record;
/// the underlying `csv` crate buffers in 8 KiB chunks, so this guarantees the
/// failing record's bytes are still in the capture when the error is raised.
const RAW_CAPTURE_CAP: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Module entry
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:csv` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// each carries its own `#[instrument]` span; the fallible ones build their
/// `(value, error)` returns directly through `err` / `from_err` / `ok` with a
/// baked `"csv.<fn>: "` prefix, and `rows` returns a bare iterator `Function`
/// that raises `mlua::Error` (the Lua iterator protocol cannot carry a
/// `(nil, err)` tuple) — none fit the facade's `func` / `try_func` model. The
/// `CsvReader` / `CsvWriter` userdata keep their own `impl UserData` and are
/// recorded here for the header via [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:csv")?;

    m.set("parse", lua.create_function(parse_fn)?)?;
    m.record_fn(
        "parse",
        &[("data", "string"), ("opts", "LuaValue?")],
        "Result<LuaValue, string>",
        "Parse a CSV string into an array of rows. With `headers = true` rows are keyed by header name; typed columns coerce to number/boolean/nil.",
    );

    m.set("rows", lua.create_function(rows_fn)?)?;
    m.record_fn(
        "rows",
        &[("data", "string"), ("opts", "LuaValue?")],
        "LuaValue",
        "Return a Lua iterator over rows in an in-memory CSV string. Iterator errors raise.",
    );

    m.set("reader", lua.create_function(reader_fn)?)?;
    m.record_fn(
        "reader",
        &[("handle", "LuaValue"), ("opts", "LuaValue?")],
        "Result<CsvReader, string>",
        "Construct a streaming reader over a `ReaderHandle`, taking an exclusive borrow on the handle for its lifetime.",
    );

    m.set("stringify", lua.create_function(stringify_fn)?)?;
    m.record_fn(
        "stringify",
        &[("records", "LuaValue"), ("opts", "LuaValue")],
        "Result<string, string>",
        "Serialise an array of records to a CSV string. `opts.headers` is required and controls both the header row and field order.",
    );

    m.set("writer", lua.create_function(writer_fn)?)?;
    m.record_fn(
        "writer",
        &[("opts", "LuaValue")],
        "Result<CsvWriter, string>",
        "Construct an incremental writer that buffers in memory until `:finish()`. `opts.headers` is required and emitted as the first row.",
    );

    m.userdata("CsvReader", |r| {
        r.method(
            "headers",
            &[],
            "Result<LuaValue, string>",
            "Return the cached header row (parsing it if not yet read). Errors if the reader was not created with `headers = true`.",
        )
        .method(
            "rows",
            &[],
            "LuaValue",
            "Return a Lua iterator yielding the next row on each call, `nil` at EOF. Iterator errors raise.",
        );
    });

    m.userdata("CsvWriter", |w| {
        w.method(
            "row",
            &[("values", "LuaValue")],
            "Result<bool, string>",
            "Append a row. `values` is either a positional array (length must match `headers`) or a keyed table looked up by header name.",
        )
        .method(
            "finish",
            &[],
            "Result<string, string>",
            "Flush and return the accumulated CSV string. Subsequent calls error.",
        );
    });

    Ok(m)
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
// Top-level functions
// ---------------------------------------------------------------------------

#[instrument(name = "csv.parse", target = "lua::lib::csv", skip_all)]
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

#[instrument(name = "csv.rows", target = "lua::lib::csv", skip_all)]
fn rows_fn(lua: &Lua, (data, opts): (mlua::String, Option<Table>)) -> mlua::Result<Function> {
    let parse_opts = parse_parse_opts(opts.as_ref(), "csv.rows").map_err(mlua::Error::external)?;
    let bytes = data.as_bytes().to_vec();
    let capture = new_capture();
    let source = wrap_capturing(Box::new(Cursor::new(bytes)) as BoxedReader, capture.clone());
    let reader = build_csv_reader(source, &parse_opts);
    let state = new_reader_state(reader, parse_opts, capture, None);
    create_rows_iter(lua, state, "csv.rows")
}

#[instrument(name = "csv.reader", target = "lua::lib::csv", skip_all)]
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

#[instrument(name = "csv.stringify", target = "lua::lib::csv", skip_all)]
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

#[instrument(name = "csv.writer", target = "lua::lib::csv", skip_all)]
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
    fn reader_borrow_released_when_reader_is_dropped() {
        let lua = engine();
        lua.load(
            r#"
            local io = require("std:io")
            handle = io.from_string("a\n1")
            do
                local _r = csv.reader(handle)
            end
            "#,
        )
        .exec()
        .unwrap();
        // Collect outside the chunk so no live stack slot keeps the reader alive.
        lua.gc_collect().unwrap();
        lua.gc_collect().unwrap();
        let reusable: bool = lua
            .load("local r, e = csv.reader(handle); return r ~= nil and e == nil")
            .eval()
            .unwrap();
        assert!(
            reusable,
            "handle borrow must be released after the reader is collected"
        );
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

    // Given:  Windows-style CRLF-terminated input with a field-count overflow on record 2
    // When:   csv.parse raises UnequalLengths
    // Then:   the snippet is the raw record bytes, no leading \n carried over from the CRLF
    #[test]
    fn parse_error_strips_leading_lf_from_crlf_input() {
        let lua = engine();
        let (_, e): (Option<mlua::Table>, Option<String>) = lua
            .load(r#"return csv.parse('a,b\r\n"x",y,extra\r\n')"#)
            .eval()
            .unwrap();
        let e = e.unwrap();
        assert!(
            e.ends_with(r#": ""x",y,extra""#),
            "expected snippet with no leading \\n, got: {e}"
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

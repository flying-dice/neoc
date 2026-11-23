//! `lib:csv` writer path — the `CsvWriter` userdata, row-shape detection, and
//! field serialisation. Shared options live in `super` (`csv.rs`).

use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataMethods, Value};
use parking_lot::Mutex;

use crate::lua::sandbox::{err, from_err, ok};

use super::WriteOpts;

pub(super) struct CsvWriterState {
    pub(super) writer: csv::Writer<Vec<u8>>,
    pub(super) headers: Vec<String>,
    pub(super) finished: bool,
}

pub(super) struct CsvWriter {
    pub(super) state: Arc<Mutex<CsvWriterState>>,
}

#[neoc_macros::lua_userdata]
impl UserData for CsvWriter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Append a row. `values` is either a positional array (length must match `headers`) or a keyed table looked up by header name.
        #[lua("Result<bool, string>")]
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

        /// Flush and return the accumulated CSV string. Subsequent calls error.
        #[lua("Result<string, string>")]
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

pub(super) fn build_csv_writer<W: std::io::Write>(w: W, opts: &WriteOpts) -> csv::Writer<W> {
    let mut b = csv::WriterBuilder::new();
    b.delimiter(opts.delimiter);
    b.quote(opts.quote);
    b.flexible(false);
    b.from_writer(w)
}

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
                let i64v = i;
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

pub(super) fn collect_keyed_row(
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

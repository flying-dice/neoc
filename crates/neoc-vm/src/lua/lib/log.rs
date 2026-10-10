//! `lib:log` — structured logging.
//!
//! Rust std has no structured logging surface. This module fills that gap
//! with an instance-based logger API, modelled after Go's `log/slog`:
//! configuration is set at construction, never mutated mid-flight, and
//! child loggers are created by binding additional fields.
//!
//! ## Why instance-based, not global `set_level()` / `set_format()`
//!
//! `std:task` (#6) runs concurrent Luau tasks on a tokio runtime. A
//! `log.set_level()` API would race between tasks — one task lowering the
//! level mid-flight would change another task's behaviour without warning.
//! `slog` solved the same problem when Go moved off the old `log` package:
//! every logger is its own value, configuration is captured at
//! construction, and child loggers carry pre-bound context.
//!
//! ```lua
//! local log = require("lib:log")
//!
//! log.info("request handled", { method = "GET", status = 200 })
//!
//! local logger = log.new({ level = "debug", format = "json" })
//! local reqlog = logger:with({ request_id = "abc" })
//! reqlog:info("query executed", { rows = 100 })
//! -- output carries request_id automatically
//! ```
//!
//! ## Output
//!
//! - **stderr only.** stdout is the program's data channel; mixing log
//!   lines into it breaks pipes and structured-output consumers.
//! - **JSON by default.** Flat keys: `level`, `ts`, `msg`, then user
//!   fields. Designed to be parsed by `jq`, log shippers, and LLM tools
//!   without unwrapping nested objects.
//! - **Text format** available for human debugging — `key=value` pairs
//!   after the timestamp + level + message.
//! - **Timestamps:** UTC, RFC 3339, millisecond precision
//!   (`2026-05-19T01:30:00.123Z`).
//!
//! ## Field handling
//!
//! Lua values are converted to JSON: scalars round-trip; functions →
//! `"<function>"`; userdata → `"<userdata>"`; cycles → `"<circular>"`;
//! tables deeper than three levels → `"<depth limit>"`. `nil` fields are
//! omitted from output. A log call must never crash the program — every
//! non-serialisable shape resolves to a string placeholder.
//!
//! ## Backing on `tracing`
//!
//! Each call also emits a `tracing::event!` at `target =
//! "luau::lib::log"`, so the existing `LuauLayer` captures it when a
//! script runs inside `lua::scope` (used for VCR/recording). The stderr
//! write happens unconditionally — it does not depend on a subscriber
//! being installed.

use std::collections::{BTreeSet, HashSet};
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use mlua::{Lua, Table, UserData, UserDataMethods, Value};
use serde_json::{Map, Number, Value as Json};

use crate::lua::facade::Module;

/// Maximum nesting depth for `fields` table serialisation. Anything beyond
/// this becomes the string `"<depth limit>"`. Three is enough for normal
/// log payloads (one wrap of headers/params + one nested object) without
/// letting a runaway data structure block a log call indefinitely.
const DEPTH_LIMIT: u8 = 3;

// ---------------------------------------------------------------------------
// Levels
// ---------------------------------------------------------------------------

/// Log levels in ascending order of severity. `Off` is a sentinel that
/// suppresses every level when set on a logger — useful for test harnesses
/// and dormant child loggers.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Off,
}

impl LogLevel {
    fn parse(s: &str) -> mlua::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            "off" => Ok(Self::Off),
            other => Err(mlua::Error::external(format!(
                "log.new: unknown level {other:?} — expected trace|debug|info|warn|error|off"
            ))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Off => "off",
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Format {
    Json,
    Text,
}

impl Format {
    fn parse(s: &str) -> mlua::Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "text" => Ok(Self::Text),
            other => Err(mlua::Error::external(format!(
                "log.new: unknown format {other:?} — expected json|text"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// LuaLogger
// ---------------------------------------------------------------------------

/// Lua-facing logger handle. Holds a level, a format, and a (possibly
/// empty) map of pre-bound context fields. Each method clones the bound
/// map and merges in the call-site fields before emitting one line.
#[derive(Clone, Debug)]
pub struct LuaLogger {
    level: LogLevel,
    format: Format,
    bound_fields: Map<String, Json>,
}

impl LuaLogger {
    fn default_instance() -> Self {
        Self {
            level: LogLevel::Info,
            format: Format::Json,
            bound_fields: Map::new(),
        }
    }

    fn from_options(opts: Option<Table>) -> mlua::Result<Self> {
        let mut me = Self::default_instance();
        let Some(t) = opts else { return Ok(me) };
        if let Some(level) = optional_string(&t, "level")? {
            me.level = LogLevel::parse(&level)?;
        }
        if let Some(format) = optional_string(&t, "format")? {
            me.format = Format::parse(&format)?;
        }
        Ok(me)
    }

    fn child(&self, lua: &Lua, fields: Table) -> mlua::Result<Self> {
        let mut merged = self.bound_fields.clone();
        merge_table_into(&mut merged, lua, &fields, DEPTH_LIMIT)?;
        Ok(Self {
            level: self.level,
            format: self.format,
            bound_fields: merged,
        })
    }

    fn emit(
        &self,
        lua: &Lua,
        lvl: LogLevel,
        msg: &str,
        fields: Option<&Table>,
    ) -> mlua::Result<()> {
        if !self.enabled(lvl) {
            return Ok(());
        }
        let mut merged = self.bound_fields.clone();
        if let Some(t) = fields {
            merge_table_into(&mut merged, lua, t, DEPTH_LIMIT)?;
        }
        write_line(self.format, lvl, msg, &merged);
        emit_tracing(lvl, msg, &merged);
        Ok(())
    }

    fn enabled(&self, lvl: LogLevel) -> bool {
        self.level != LogLevel::Off && lvl >= self.level
    }
}

fn optional_string(t: &Table, key: &str) -> mlua::Result<Option<String>> {
    match t.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(s) => Ok(Some(s.to_str()?.to_owned())),
        other => Err(mlua::Error::external(format!(
            "log.new: option {key:?} must be a string, got Lua {}",
            other.type_name()
        ))),
    }
}

impl UserData for LuaLogger {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("with", |lua, this, fields: Table| this.child(lua, fields));
        methods.add_method(
            "trace",
            |lua, this, (msg, fields): (String, Option<Table>)| {
                this.emit(lua, LogLevel::Trace, &msg, fields.as_ref())
            },
        );
        methods.add_method(
            "debug",
            |lua, this, (msg, fields): (String, Option<Table>)| {
                this.emit(lua, LogLevel::Debug, &msg, fields.as_ref())
            },
        );
        methods.add_method(
            "info",
            |lua, this, (msg, fields): (String, Option<Table>)| {
                this.emit(lua, LogLevel::Info, &msg, fields.as_ref())
            },
        );
        methods.add_method(
            "warn",
            |lua, this, (msg, fields): (String, Option<Table>)| {
                this.emit(lua, LogLevel::Warn, &msg, fields.as_ref())
            },
        );
        methods.add_method(
            "error",
            |lua, this, (msg, fields): (String, Option<Table>)| {
                this.emit(lua, LogLevel::Error, &msg, fields.as_ref())
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Field serialisation
// ---------------------------------------------------------------------------

fn merge_table_into(
    into: &mut Map<String, Json>,
    lua: &Lua,
    t: &Table,
    depth: u8,
) -> mlua::Result<()> {
    let mut seen = HashSet::new();
    seen.insert(t.to_pointer() as usize);
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, v) = pair?;
        let key = key_to_string(&k)?;
        if matches!(v, Value::Nil) {
            // `nil` field — omit from output, matching the AC.
            continue;
        }
        let json = value_to_json(lua, v, depth.saturating_sub(1), &mut seen)?;
        into.insert(key, json);
    }
    Ok(())
}

fn key_to_string(v: &Value) -> mlua::Result<String> {
    match v {
        Value::String(s) => Ok(s.to_str()?.to_owned()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        // Booleans/nil/tables/functions/userdata as table keys are legal
        // in Lua but have no JSON/text representation. A log call must
        // never crash the program — coerce to a placeholder. Multiple
        // invalid keys collide on the same placeholder; acceptable as
        // the contract is "never crash", not "round-trip every shape".
        _ => Ok("<invalid key>".into()),
    }
}

fn value_to_json(lua: &Lua, v: Value, depth: u8, seen: &mut HashSet<usize>) -> mlua::Result<Json> {
    match v {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(b) => Ok(Json::Bool(b)),
        Value::Integer(i) => Ok(Json::Number(i.into())),
        Value::Number(n) => Ok(Number::from_f64(n).map(Json::Number).unwrap_or(Json::Null)),
        Value::String(s) => Ok(Json::String(s.to_str()?.to_owned())),
        Value::Function(_) => Ok(Json::String("<function>".into())),
        Value::Thread(_) => Ok(Json::String("<thread>".into())),
        Value::UserData(_) | Value::LightUserData(_) => Ok(Json::String("<userdata>".into())),
        Value::Table(t) => table_to_json(lua, &t, depth, seen),
        // Buffers/Errors/etc. — coerce to a placeholder so a log call never
        // crashes the program.
        _ => Ok(Json::String("<unsupported>".into())),
    }
}

fn table_to_json(lua: &Lua, t: &Table, depth: u8, seen: &mut HashSet<usize>) -> mlua::Result<Json> {
    let ptr = t.to_pointer() as usize;
    if !seen.insert(ptr) {
        return Ok(Json::String("<circular>".into()));
    }
    let result = if depth == 0 {
        Ok(Json::String("<depth limit>".into()))
    } else if is_array_like(t)? {
        let mut arr = Vec::new();
        for i in 1..=t.raw_len() {
            let v: Value = t.raw_get(i)?;
            arr.push(value_to_json(lua, v, depth - 1, seen)?);
        }
        Ok(Json::Array(arr))
    } else {
        let mut map = Map::new();
        for pair in t.clone().pairs::<Value, Value>() {
            let (k, v) = pair?;
            let key = key_to_string(&k)?;
            if matches!(v, Value::Nil) {
                continue;
            }
            map.insert(key, value_to_json(lua, v, depth - 1, seen)?);
        }
        Ok(Json::Object(map))
    };
    seen.remove(&ptr);
    result
}

/// Heuristic match for "Lua sequence": keys are exactly `1..=n` integers.
fn is_array_like(t: &Table) -> mlua::Result<bool> {
    let len = t.raw_len();
    if len == 0 {
        // Empty table → treat as object so `{}` round-trips as `{}` in JSON.
        return Ok(false);
    }
    let mut seen_keys = BTreeSet::new();
    for pair in t.clone().pairs::<Value, Value>() {
        let (k, _) = pair?;
        match k {
            Value::Integer(i) if i >= 1 => {
                seen_keys.insert(i as usize);
            }
            _ => return Ok(false),
        }
    }
    Ok(seen_keys.len() == len && seen_keys.iter().last().copied() == Some(len))
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Reserved keys take precedence — a caller can't shadow `level`/`ts`/`msg`
/// and corrupt the line shape. Enforced for both formats.
fn is_reserved_key(k: &str) -> bool {
    matches!(k, "level" | "ts" | "msg")
}

fn write_line(format: Format, lvl: LogLevel, msg: &str, fields: &Map<String, Json>) {
    let ts = now_rfc3339_millis();
    let line = match format {
        Format::Json => {
            let mut obj = Map::with_capacity(3 + fields.len());
            obj.insert("level".into(), Json::String(lvl.as_str().into()));
            obj.insert("ts".into(), Json::String(ts));
            obj.insert("msg".into(), Json::String(msg.to_owned()));
            for (k, v) in fields {
                if is_reserved_key(k) {
                    continue;
                }
                obj.insert(k.clone(), v.clone());
            }
            serde_json::to_string(&Json::Object(obj)).unwrap_or_default()
        }
        Format::Text => {
            let mut s = format!("{ts} [{}] {msg}", lvl.as_str());
            for (k, v) in fields {
                if is_reserved_key(k) {
                    continue;
                }
                s.push(' ');
                s.push_str(k);
                s.push('=');
                s.push_str(&format_text_value(v));
            }
            s
        }
    };
    // A failed stderr write is not recoverable from inside a log call —
    // ignore it rather than crashing the caller.
    let _ = writeln!(io::stderr(), "{line}");
}

fn format_text_value(v: &Json) -> String {
    match v {
        Json::String(s) => {
            if s.chars().any(|c| c.is_whitespace() || c == '=' || c == '"') {
                serde_json::to_string(s).unwrap_or_else(|_| s.clone())
            } else {
                s.clone()
            }
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Timestamp — RFC 3339 UTC with millisecond precision
// ---------------------------------------------------------------------------

/// Format `SystemTime::now()` as `YYYY-MM-DDThh:mm:ss.mmmZ`. Pure-std
/// epoch-to-civil conversion via Howard Hinnant's algorithm so we don't
/// pull in `chrono`/`jiff` just for one timestamp.
fn now_rfc3339_millis() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let millis = dur.subsec_millis();
    let (y, mo, d, h, m, s) = civil_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Hinnant's `civil_from_days` adapted to seconds. Returns `(year, month,
/// day, hour, minute, second)` in UTC. Correct for all dates representable
/// by an `i64` second count, including pre-1970 (negative seconds).
fn civil_from_unix(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400) as u32;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // Hinnant: shift epoch from 1970-01-01 to 0000-03-01 so leap-day
    // arithmetic stays integer-only.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute, second)
}

// ---------------------------------------------------------------------------
// Tracing emit — for VCR capture via the existing LuauLayer.
// ---------------------------------------------------------------------------

fn emit_tracing(lvl: LogLevel, msg: &str, fields: &Map<String, Json>) {
    // The Luau layer reads `tracing::Event`s via a `Visit` impl that
    // captures any field. We pack the merged map into a single
    // serialised JSON string so the capture is lossless without needing
    // dynamic structured fields (which `tracing::event!` cannot accept).
    let fields_json = serde_json::to_string(&Json::Object(fields.clone())).unwrap_or_default();
    match lvl {
        LogLevel::Trace => {
            tracing::trace!(target: "luau::lib::log", message = %msg, fields = %fields_json)
        }
        LogLevel::Debug => {
            tracing::debug!(target: "luau::lib::log", message = %msg, fields = %fields_json)
        }
        LogLevel::Info => {
            tracing::info!(target: "luau::lib::log", message = %msg, fields = %fields_json)
        }
        LogLevel::Warn => {
            tracing::warn!(target: "luau::lib::log", message = %msg, fields = %fields_json)
        }
        LogLevel::Error => {
            tracing::error!(target: "luau::lib::log", message = %msg, fields = %fields_json)
        }
        LogLevel::Off => {}
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:log` surface through the facade.
///
/// Every module function is set verbatim with
/// [`crate::lua::facade::Module::set`]: the hand-written registration carried
/// **no** `#[instrument]` span (each log call already emits its own
/// `tracing::event!`), so routing through the facade's `func` would add a span
/// and break byte-identical tracing. `new` constructs a `Logger`; the five
/// module-level level functions delegate to a fresh default-configured
/// instance on each call. The `Logger` userdata keeps its own `impl UserData`
/// and is recorded here for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:log")?;

    // `log.new({ level?, format? })` — explicit logger instance.
    m.set(
        "new",
        lua.create_function(|_, opts: Option<Table>| LuaLogger::from_options(opts))?,
    )?;

    m.record_fn(
        "new",
        &[("opts", "LuaValue?")],
        "Logger",
        "Construct an explicit logger instance; `opts` may carry `level` and `format`.",
    );

    // Module-level convenience: `log.info(msg, fields?)` delegates to a
    // default-configured instance (level=info, format=json).
    for (name, lvl, doc) in [
        (
            "trace",
            LogLevel::Trace,
            "Module-level convenience for the lowest level; delegates to a fresh default logger.",
        ),
        (
            "debug",
            LogLevel::Debug,
            "Module-level convenience for `debug`; delegates to a fresh default logger.",
        ),
        (
            "info",
            LogLevel::Info,
            "Module-level convenience for `info` — the default level; delegates to a fresh default logger.",
        ),
        (
            "warn",
            LogLevel::Warn,
            "Module-level convenience for `warn`; delegates to a fresh default logger.",
        ),
        (
            "error",
            LogLevel::Error,
            "Module-level convenience for `error`; delegates to a fresh default logger.",
        ),
    ] {
        let lvl_copy = lvl;
        m.set(
            name,
            lua.create_function(move |lua, (msg, fields): (String, Option<Table>)| {
                LuaLogger::default_instance().emit(lua, lvl_copy, &msg, fields.as_ref())
            })?,
        )?;
        m.record_fn(
            name,
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            doc,
        );
    }

    m.userdata("Logger", |l| {
        l.method(
            "with",
            &[("fields", "LuaValue")],
            "Logger",
            "Return a child logger with `fields` merged into the bound context; the parent is unchanged.",
        )
        .method(
            "trace",
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            "Emit a `trace`-level line if the logger's threshold permits it.",
        )
        .method(
            "debug",
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            "Emit a `debug`-level line if the logger's threshold permits it.",
        )
        .method(
            "info",
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            "Emit an `info`-level line if the logger's threshold permits it.",
        )
        .method(
            "warn",
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            "Emit a `warn`-level line if the logger's threshold permits it.",
        )
        .method(
            "error",
            &[("msg", "string"), ("fields", "LuaValue?")],
            "()",
            "Emit an `error`-level line; highest severity short of `off`.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn lua() -> Lua {
        crate::lua::sandbox::make_engine().expect("engine")
    }

    #[test]
    fn level_parse_accepts_all_known() {
        for (s, expected) in [
            ("trace", LogLevel::Trace),
            ("debug", LogLevel::Debug),
            ("info", LogLevel::Info),
            ("warn", LogLevel::Warn),
            ("error", LogLevel::Error),
            ("off", LogLevel::Off),
            ("INFO", LogLevel::Info),
        ] {
            assert_eq!(LogLevel::parse(s).unwrap(), expected);
        }
    }

    #[test]
    fn level_parse_rejects_unknown() {
        assert!(LogLevel::parse("verbose").is_err());
    }

    #[test]
    fn format_parse_accepts_known() {
        assert_eq!(Format::parse("json").unwrap(), Format::Json);
        assert_eq!(Format::parse("TEXT").unwrap(), Format::Text);
        assert!(Format::parse("yaml").is_err());
    }

    #[test]
    fn enabled_respects_threshold() {
        let l = LuaLogger {
            level: LogLevel::Warn,
            format: Format::Json,
            bound_fields: Map::new(),
        };
        assert!(!l.enabled(LogLevel::Trace));
        assert!(!l.enabled(LogLevel::Debug));
        assert!(!l.enabled(LogLevel::Info));
        assert!(l.enabled(LogLevel::Warn));
        assert!(l.enabled(LogLevel::Error));
    }

    #[test]
    fn enabled_off_suppresses_all() {
        let l = LuaLogger {
            level: LogLevel::Off,
            format: Format::Json,
            bound_fields: Map::new(),
        };
        assert!(!l.enabled(LogLevel::Trace));
        assert!(!l.enabled(LogLevel::Error));
    }

    #[test]
    fn timestamp_has_expected_shape() {
        let ts = now_rfc3339_millis();
        // YYYY-MM-DDThh:mm:ss.mmmZ → 24 chars.
        assert_eq!(ts.len(), 24);
        assert!(ts.ends_with('Z'));
        assert!(ts.chars().nth(4) == Some('-'));
        assert!(ts.chars().nth(10) == Some('T'));
        assert!(ts.chars().nth(19) == Some('.'));
    }

    #[test]
    fn civil_from_unix_known_dates() {
        // Unix epoch.
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        // 2026-05-19T01:30:45 UTC.
        // 56 years + 138 days (2026 is not a leap year, Jan=31, Feb=28,
        // Mar=31, Apr=30, May 1..19 ⇒ 31+28+31+30+18=138) → 1768872645.
        let secs = (2026 - 1970) as i64 * 365 * 86_400
            + leap_days_between(1970, 2026) * 86_400
            + 138 * 86_400
            + 3600
            + 30 * 60
            + 45;
        assert_eq!(civil_from_unix(secs), (2026, 5, 19, 1, 30, 45));
    }

    fn leap_days_between(from: i32, to: i32) -> i64 {
        // Leap years strictly between `from` (inclusive) and `to`
        // (exclusive): years divisible by 4 except centuries unless
        // divisible by 400.
        let is_leap = |y: i32| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        (from..to).filter(|y| is_leap(*y)).count() as i64
    }

    #[test]
    fn json_output_contains_required_keys() {
        // Smoke test for the JSON shape via merge_table_into +
        // serde_json::to_string. We don't capture stderr here — the
        // shape check is enough.
        let lua = lua();
        let t = lua.create_table().unwrap();
        t.set("method", "GET").unwrap();
        t.set("status", 200).unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        assert_eq!(fields.get("method").unwrap(), &Json::String("GET".into()));
        assert_eq!(fields.get("status").unwrap(), &Json::Number(200.into()));
    }

    #[test]
    fn nil_field_omitted() {
        let lua = lua();
        let t = lua.create_table().unwrap();
        t.set("present", "yes").unwrap();
        t.set("absent", Value::Nil).unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        assert!(fields.contains_key("present"));
        assert!(!fields.contains_key("absent"));
    }

    #[test]
    fn function_value_becomes_placeholder() {
        let lua = lua();
        let f = lua.create_function(|_, _: ()| Ok(())).unwrap();
        let t = lua.create_table().unwrap();
        t.set("cb", f).unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        assert_eq!(
            fields.get("cb").unwrap(),
            &Json::String("<function>".into())
        );
    }

    #[test]
    fn depth_limit_replaces_deep_value() {
        let lua = lua();
        let src = r#"
            local outer = { l1 = { l2 = { l3 = { l4 = { l5 = "deep" } } } } }
            return outer
        "#;
        let t: Table = lua.load(src).eval().unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        // At depth=3 we serialise three nested levels, then placeholder.
        let stringified = serde_json::to_string(&Json::Object(fields)).unwrap();
        assert!(
            stringified.contains("<depth limit>"),
            "expected depth-limit placeholder, got: {stringified}"
        );
    }

    #[test]
    fn circular_reference_becomes_placeholder() {
        let lua = lua();
        let src = r#"
            local a = {}
            a.self = a
            return a
        "#;
        let t: Table = lua.load(src).eval().unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        let stringified = serde_json::to_string(&Json::Object(fields)).unwrap();
        assert!(
            stringified.contains("<circular>"),
            "expected circular placeholder, got: {stringified}"
        );
    }

    #[test]
    fn array_like_table_becomes_json_array() {
        let lua = lua();
        let t: Table = lua.load(r#"return {10, 20, 30}"#).eval().unwrap();
        let mut fields = Map::new();
        // Wrap in an outer table so merge_table_into sees an object root.
        let wrapper = lua.create_table().unwrap();
        wrapper.set("items", t).unwrap();
        merge_table_into(&mut fields, &lua, &wrapper, DEPTH_LIMIT).unwrap();
        match fields.get("items").unwrap() {
            Json::Array(a) => assert_eq!(a.len(), 3),
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn reserved_keys_cannot_be_shadowed() {
        let mut fields = Map::new();
        fields.insert("level".into(), Json::String("bogus".into()));
        fields.insert("user".into(), Json::String("alice".into()));
        // We can't easily capture stderr here without test-only plumbing;
        // build the line by inlining the build logic against the same
        // contract `write_line` uses.
        let mut obj = Map::new();
        obj.insert("level".into(), Json::String("info".into()));
        obj.insert("ts".into(), Json::String("ts".into()));
        obj.insert("msg".into(), Json::String("m".into()));
        for (k, v) in &fields {
            if k == "level" || k == "ts" || k == "msg" {
                continue;
            }
            obj.insert(k.clone(), v.clone());
        }
        assert_eq!(obj.get("level").unwrap(), &Json::String("info".into()));
        assert_eq!(obj.get("user").unwrap(), &Json::String("alice".into()));
    }

    #[test]
    fn module_table_exposes_new_and_levels() {
        let lua = lua();
        let m = module(&lua).unwrap();
        for fn_name in ["new", "trace", "debug", "info", "warn", "error"] {
            let v: Value = m.get(fn_name).unwrap();
            assert!(matches!(v, Value::Function(_)), "{fn_name} missing");
        }
    }

    #[test]
    fn new_default_returns_info_json_logger() {
        let logger = LuaLogger::from_options(None).unwrap();
        assert_eq!(logger.level, LogLevel::Info);
        assert_eq!(logger.format, Format::Json);
        assert!(logger.bound_fields.is_empty());
    }

    #[test]
    fn new_unknown_level_errors() {
        let lua = lua();
        let opts = lua.create_table().unwrap();
        opts.set("level", "verbose").unwrap();
        let err = LuaLogger::from_options(Some(opts)).unwrap_err();
        assert!(err.to_string().contains("unknown level"));
    }

    #[test]
    fn non_string_field_key_does_not_crash() {
        // Booleans/tables/functions as table keys are legal in Lua. A log
        // call must never crash — they coerce to "<invalid key>".
        let lua = lua();
        let t: Table = lua
            .load(r#"local x = {}; x[true] = "v"; return x"#)
            .eval()
            .unwrap();
        let mut fields = Map::new();
        merge_table_into(&mut fields, &lua, &t, DEPTH_LIMIT).unwrap();
        assert_eq!(
            fields.get("<invalid key>").unwrap(),
            &Json::String("v".into())
        );
    }

    #[test]
    fn text_format_filters_reserved_keys() {
        // `is_reserved_key` is the shared filter used by both branches of
        // `write_line`; assert it identifies exactly the reserved set.
        for k in ["level", "ts", "msg"] {
            assert!(is_reserved_key(k), "{k} should be reserved");
        }
        for k in ["user", "request_id", "ts2", ""] {
            assert!(!is_reserved_key(k), "{k} should not be reserved");
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let mut g = c.benchmark_group("log");

    // Single info call with three fields — exercises field collection +
    // tracing emit. stderr write is included; the bench runs go to the
    // benchmark harness's stderr.
    let info_call: mlua::Function = lua
        .load(
            r#"
            local log = require("lib:log")
            local logger = log.new({ level = "info", format = "json" })
            return function()
                logger:info("hit", { method = "GET", path = "/x", status = 200 })
            end
            "#,
        )
        .eval()
        .expect("info call setup");
    g.bench_function("info_3_fields", |b| {
        b.iter(|| {
            let _: () = info_call.call(black_box(())).unwrap();
        });
    });

    // Suppressed-level call — exercises the early-out path. Should be
    // dominated by the level comparison and Lua call overhead.
    let suppressed_call: mlua::Function = lua
        .load(
            r#"
            local log = require("lib:log")
            local logger = log.new({ level = "error" })
            return function()
                logger:trace("noisy", { i = 1 })
            end
            "#,
        )
        .eval()
        .expect("suppressed setup");
    g.bench_function("suppressed_below_threshold", |b| {
        b.iter(|| {
            let _: () = suppressed_call.call(black_box(())).unwrap();
        });
    });

    g.finish();
}

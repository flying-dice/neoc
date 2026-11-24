//! Structured logging: `lib:log`, after Go's `log/slog`.
//!
//! A logger is a value: its level and format are set when it is made and never change, and `logger:with(fields)`
//! gives a child that carries those fields on every line. There is no global `set_level`: with concurrent tasks,
//! one task changing it would change what another logs. The module's own `info`, `warn`, ... write through a
//! default logger (level `info`, JSON).
//!
//! Output:
//! - To stderr, never stdout (stdout is the program's data). An embedder may take every line instead: see
//!   [`set_log_sink`].
//! - JSON by default, flat: `level`, `ts`, `msg`, then the fields. `text` is `ts [level] msg key=value ...` for
//!   reading by eye.
//! - `ts` is UTC, RFC 3339, to the millisecond (`2026-05-19T01:30:00.123Z`).
//! - A field named `level`, `ts` or `msg` is left out: it cannot replace the line's own.
//!
//! A log call never fails on what a field holds: a function is `"<function>"`, userdata `"<userdata>"`, a table that
//! holds itself `"<circular>"`, a table more than three levels deep `"<depth limit>"`.
//!
//! Each line is also a `tracing` event at target `lua::lib::log` (not when a sink is installed: the sink is where
//! the embedder routes it).
//!
//! The API's rules:
//! - `level` and `format` are string enums, exactly as written (`"info"`, not `"INFO"`), and an option that is not one
//!   is refused as an argument with its place.
//! - Fields are a table with string keys: a key that is not a string is refused as an argument.
//! - A string that is not UTF-8 is logged with U+FFFD in place of the bad bytes.
//! - The fields table itself is not part of cycle detection: a field that holds the fields table is written one level
//!   deep before `"<circular>"`.

use std::collections::{BTreeSet, HashSet};
use std::io::Write;

use mlua::{Table, Value};
use serde_json::{Map, Number, Value as Json};

use log::{Format, Level};

/// How deep a field's tables are written; deeper is `"<depth limit>"`. Three covers a usual payload (a wrap of
/// headers or parameters and one object in it) and keeps a runaway structure from holding a log call up.
const DEPTH_LIMIT: u8 = 3;

pub use neoc_vm_core::log_sink::{LogFn, set_log_sink};

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
            Level::Off => "off",
        }
    }
}

/// A field's value as JSON. What JSON has no word for is a placeholder: a log call does not fail on its fields.
fn value_to_json(value: Value, depth: u8, seen: &mut HashSet<usize>) -> Json {
    match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(b),
        Value::Integer(i) => Json::Number(i.into()),
        Value::Number(n) => Number::from_f64(n).map_or(Json::Null, Json::Number),
        Value::String(s) => Json::String(s.to_string_lossy()),
        Value::Function(_) => Json::String("<function>".into()),
        Value::Thread(_) => Json::String("<thread>".into()),
        Value::UserData(_) | Value::LightUserData(_) => Json::String("<userdata>".into()),
        Value::Table(table) => table_to_json(&table, depth, seen),
        // buffers, vectors, errors
        _ => Json::String("<unsupported>".into()),
    }
}

fn key_to_string(key: &Value) -> String {
    match key {
        Value::String(s) => s.to_string_lossy(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        // A boolean, a table or a function is a legal key in Lua and has no text. Several of them land on the
        // same placeholder, which is fine: the promise is not to fail, not to keep every shape.
        _ => "<invalid key>".into(),
    }
}

fn table_to_json(table: &Table, depth: u8, seen: &mut HashSet<usize>) -> Json {
    let pointer = table.to_pointer() as usize;
    if !seen.insert(pointer) {
        return Json::String("<circular>".into());
    }
    let json = if depth == 0 {
        Json::String("<depth limit>".into())
    } else if is_array_like(table) {
        let items = (1..=table.raw_len())
            .map(|i| value_to_json(table.raw_get(i).unwrap_or(Value::Nil), depth - 1, seen))
            .collect();
        Json::Array(items)
    } else {
        let mut map = Map::new();
        for (key, value) in table.pairs::<Value, Value>().flatten() {
            if !value.is_nil() {
                map.insert(key_to_string(&key), value_to_json(value, depth - 1, seen));
            }
        }
        Json::Object(map)
    };
    seen.remove(&pointer);
    json
}

/// Whether the keys are exactly `1..=n`. An empty table is an object, so `{}` is written `{}`.
fn is_array_like(table: &Table) -> bool {
    let len = table.raw_len();
    if len == 0 {
        return false;
    }
    let mut keys = BTreeSet::new();
    for (key, _) in table.pairs::<Value, Value>().flatten() {
        match key {
            Value::Integer(i) if i >= 1 => {
                keys.insert(usize::try_from(i).unwrap_or(usize::MAX));
            }
            _ => return false,
        }
    }
    keys.len() == len && keys.last().copied() == Some(len)
}

/// A field with one of these names is left out, in both formats: it would replace the line's own.
fn is_reserved_key(key: &str) -> bool {
    matches!(key, "level" | "ts" | "msg")
}

/// The time now in UTC, RFC 3339, to the millisecond: `2026-05-19T01:30:00.123Z`.
fn now_rfc3339_millis() -> String {
    format!("{:.3}", jiff::Timestamp::now())
}

fn format_line(format: Format, ts: &str, level: Level, message: &str, fields: &Map<String, Json>) -> String {
    let fields = fields.iter().filter(|(key, _)| !is_reserved_key(key));
    match format {
        Format::Json => {
            let mut line = Map::new();
            line.insert("level".into(), Json::String(level.as_str().into()));
            line.insert("ts".into(), Json::String(ts.to_owned()));
            line.insert("msg".into(), Json::String(message.to_owned()));
            for (key, value) in fields {
                line.insert(key.clone(), value.clone());
            }
            Json::Object(line).to_string()
        }
        Format::Text => {
            let mut line = format!("{ts} [{}] {message}", level.as_str());
            for (key, value) in fields {
                line.push(' ');
                line.push_str(key);
                line.push('=');
                line.push_str(&format_text_value(value));
            }
            line
        }
    }
}

/// A value after `key=`: a string is bare unless it has a space, a `=` or a quote, which need its JSON form.
fn format_text_value(value: &Json) -> String {
    match value {
        Json::String(s) if !s.chars().any(|c| c.is_whitespace() || c == '=' || c == '"') => s.clone(),
        other => other.to_string(),
    }
}

/// The line as a `tracing` event. The fields travel as one JSON string: an event's fields are fixed where it is
/// written, and these are the script's.
fn emit_tracing(level: Level, message: &str, fields: &Map<String, Json>) {
    let fields = Json::Object(fields.clone()).to_string();
    match level {
        Level::Trace => tracing::trace!(target: "lua::lib::log", message = %message, fields = %fields),
        Level::Debug => tracing::debug!(target: "lua::lib::log", message = %message, fields = %fields),
        Level::Info => tracing::info!(target: "lua::lib::log", message = %message, fields = %fields),
        Level::Warn => tracing::warn!(target: "lua::lib::log", message = %message, fields = %fields),
        Level::Error => tracing::error!(target: "lua::lib::log", message = %message, fields = %fields),
        Level::Off => {}
    }
}

#[mlua_ir::module("lib:log")]
pub mod log {
    use std::collections::{BTreeMap, HashSet};
    use std::io;

    use mlua::Lua;
    use mlua_ir::{Live, Object, Options, Shape};
    use neoc_vm_core::log_sink::log_sink;
    use serde::{Deserialize, Serialize};
    use serde_json::{Map, Value as Json};

    use super::{DEPTH_LIMIT, Write, emit_tracing, format_line, now_rfc3339_millis, value_to_json};

    mlua_ir::unknown_reason!(
        /// Why a field's value has no declared shape.
        FieldValue = "whatever a script logs under a field: any value, and what is not plain data is written as a placeholder"
    );

    /// The fields of a line, by name.
    pub type Fields = BTreeMap<String, Live<FieldValue>>;

    /// How serious a line is, least first. A logger writes the lines at its level and above.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Level {
        /// The finest detail.
        Trace,
        /// Detail for whoever debugs it.
        Debug,
        /// Normal operation.
        Info,
        /// Something to look at.
        Warn,
        /// Something failed.
        Error,
        /// Nothing is written: a logger at this level is silent.
        Off,
    }

    /// How a line is written.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Format {
        /// One flat JSON object: `level`, `ts`, `msg`, then the fields.
        Json,
        /// `ts [level] msg key=value ...`, for reading by eye.
        Text,
    }

    /// How a logger is made.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct LoggerOptions {
        /// The least serious level it writes. When not given: `info`.
        pub level: Option<Level>,
        /// How it writes a line. When not given: `json`.
        pub format: Option<Format>,
    }

    /// A logger: a level, a format, and the fields every one of its lines carries.
    #[derive(Debug, Clone, Object)]
    pub struct Logger {
        pub(super) level: Level,
        pub(super) format: Format,
        pub(super) bound: Map<String, Json>,
    }

    impl Default for Logger {
        fn default() -> Self {
            Self { level: Level::Info, format: Format::Json, bound: Map::new() }
        }
    }

    impl Logger {
        pub(super) fn enabled(&self, level: Level) -> bool {
            self.level != Level::Off && level >= self.level
        }

        /// This logger's fields with `fields` over them.
        fn merged(&self, fields: Option<Fields>) -> Map<String, Json> {
            let mut merged = self.bound.clone();
            for (key, value) in fields.unwrap_or_default() {
                // a `nil` field is not written
                if !value.value.is_nil() {
                    merged.insert(key, value_to_json(value.value, DEPTH_LIMIT - 1, &mut HashSet::new()));
                }
            }
            merged
        }

        fn emit(&self, lua: &Lua, level: Level, message: &str, fields: Option<Fields>) {
            if !self.enabled(level) {
                return;
            }
            let fields = self.merged(fields);
            let line = format_line(self.format, &now_rfc3339_millis(), level, message, &fields);
            // An embedder's sink takes the line instead of stderr, and the tracing event stands down: the sink is
            // where the embedder routes it.
            if let Some(sink) = log_sink(lua) {
                sink(level.as_str(), &line);
            } else {
                // a failed write to stderr is nothing a log call can mend
                let _ = writeln!(io::stderr(), "{line}");
                emit_tracing(level, message, &fields);
            }
        }
    }

    #[mlua_ir::object]
    impl Logger {
        /// A child logger: this one's level, format and fields, with `fields` over them. This logger is not
        /// changed.
        ///
        /// ```luau
        /// local request = log.new({ level = "debug" }):with({ request_id = "abc" })
        /// request:info("query executed", { rows = 100 }) -- the line carries request_id
        /// ```
        pub fn with(&self, fields: Fields) -> Logger {
            Logger { level: self.level, format: self.format, bound: self.merged(Some(fields)) }
        }

        /// Write a `trace` line, when this logger's level lets it.
        pub fn trace(&self, lua: &Lua, message: String, fields: Option<Fields>) {
            self.emit(lua, Level::Trace, &message, fields);
        }

        /// Write a `debug` line, when this logger's level lets it.
        pub fn debug(&self, lua: &Lua, message: String, fields: Option<Fields>) {
            self.emit(lua, Level::Debug, &message, fields);
        }

        /// Write an `info` line, when this logger's level lets it.
        pub fn info(&self, lua: &Lua, message: String, fields: Option<Fields>) {
            self.emit(lua, Level::Info, &message, fields);
        }

        /// Write a `warn` line, when this logger's level lets it.
        pub fn warn(&self, lua: &Lua, message: String, fields: Option<Fields>) {
            self.emit(lua, Level::Warn, &message, fields);
        }

        /// Write an `error` line, unless this logger is `off`.
        pub fn error(&self, lua: &Lua, message: String, fields: Option<Fields>) {
            self.emit(lua, Level::Error, &message, fields);
        }
    }

    /// A logger of its own level and format.
    ///
    /// ```luau
    /// local logger = log.new({ level = "debug", format = "text" })
    /// logger:debug("cache warmed", { entries = 12 })
    /// ```
    pub fn new(options: LoggerOptions) -> Logger {
        Logger {
            level: options.level.unwrap_or(Level::Info),
            format: options.format.unwrap_or(Format::Json),
            bound: Map::new(),
        }
    }

    /// Write a `trace` line through the default logger, whose level is `info`: nothing is written.
    pub fn trace(lua: &Lua, message: String, fields: Option<Fields>) {
        Logger::default().emit(lua, Level::Trace, &message, fields);
    }

    /// Write a `debug` line through the default logger, whose level is `info`: nothing is written.
    pub fn debug(lua: &Lua, message: String, fields: Option<Fields>) {
        Logger::default().emit(lua, Level::Debug, &message, fields);
    }

    /// Write an `info` line through the default logger (JSON, to stderr).
    ///
    /// ```luau
    /// log.info("request handled", { method = "GET", status = 200 })
    /// ```
    pub fn info(lua: &Lua, message: String, fields: Option<Fields>) {
        Logger::default().emit(lua, Level::Info, &message, fields);
    }

    /// Write a `warn` line through the default logger.
    pub fn warn(lua: &Lua, message: String, fields: Option<Fields>) {
        Logger::default().emit(lua, Level::Warn, &message, fields);
    }

    /// Write an `error` line through the default logger.
    pub fn error(lua: &Lua, message: String, fields: Option<Fields>) {
        Logger::default().emit(lua, Level::Error, &message, fields);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mlua::Lua;

    use super::log::Logger;
    use super::*;
    use neoc_vm_core::make_engine;

    /// The lines a sink was given: each level and line.
    type Lines = Arc<Mutex<Vec<(String, String)>>>;

    /// An engine whose `lib:log` lines are kept, and the lines.
    fn capturing() -> (Lua, Lines) {
        let lua = make_engine().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = lines.clone();
        set_log_sink(
            &lua,
            Arc::new(move |level, line| kept.lock().unwrap().push((level.to_owned(), line.to_owned()))),
        );
        (lua, lines)
    }

    /// The lines `code` logs, each JSON line parsed.
    fn logged(code: &str) -> Vec<(String, Json)> {
        let (lua, lines) = capturing();
        lua.load(format!("local log = require(\"lib:log\")\n{code}")).exec().unwrap();
        let lines = lines.lock().unwrap();
        lines.iter().map(|(level, line)| (level.clone(), serde_json::from_str(line).expect(line))).collect()
    }

    /// The fields of the one line `code` logs.
    fn fields(code: &str) -> Json {
        let mut lines = logged(code);
        assert_eq!(lines.len(), 1, "{lines:?}");
        lines.remove(0).1
    }

    fn logger(level: Level) -> Logger {
        Logger { level, format: Format::Json, bound: Map::new() }
    }

    #[test]
    fn enabled_respects_threshold() {
        let warn = logger(Level::Warn);
        assert!(!warn.enabled(Level::Trace));
        assert!(!warn.enabled(Level::Debug));
        assert!(!warn.enabled(Level::Info));
        assert!(warn.enabled(Level::Warn));
        assert!(warn.enabled(Level::Error));
    }

    #[test]
    fn enabled_off_suppresses_all() {
        let off = logger(Level::Off);
        assert!(!off.enabled(Level::Trace));
        assert!(!off.enabled(Level::Error));
    }

    #[test]
    fn timestamp_has_expected_shape() {
        // YYYY-MM-DDThh:mm:ss.mmmZ
        let ts = now_rfc3339_millis();
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.chars().nth(4), Some('-'));
        assert_eq!(ts.chars().nth(10), Some('T'));
        assert_eq!(ts.chars().nth(19), Some('.'));
    }

    #[test]
    fn a_json_line_has_level_ts_msg_then_the_fields() {
        let lines = logged(r#"log.info("request handled", { method = "GET", status = 200 })"#);
        assert_eq!(lines.len(), 1);
        let (level, line) = &lines[0];
        assert_eq!(level, "info");
        assert_eq!(line["level"], "info");
        assert_eq!(line["msg"], "request handled");
        assert_eq!(line["method"], "GET");
        assert_eq!(line["status"], 200);
        assert_eq!(line["ts"].as_str().unwrap().len(), 24);
    }

    #[test]
    fn the_default_logger_writes_info_and_above() {
        let levels: Vec<String> = logged(
            r#"
            log.trace("a") log.debug("b") log.info("c") log.warn("d") log.error("e")
        "#,
        )
        .into_iter()
        .map(|(level, _)| level)
        .collect();
        assert_eq!(levels, ["info", "warn", "error"]);
    }

    #[test]
    fn a_logger_writes_at_its_level_and_above() {
        let messages = |level: &str| -> Vec<String> {
            logged(&format!(
                r#"
                local logger = log.new({{ level = "{level}" }})
                logger:trace("trace") logger:debug("debug") logger:info("info") logger:warn("warn")
                logger:error("error")
            "#
            ))
            .into_iter()
            .map(|(_, line)| line["msg"].as_str().unwrap().to_owned())
            .collect()
        };
        assert_eq!(messages("trace"), ["trace", "debug", "info", "warn", "error"]);
        assert_eq!(messages("warn"), ["warn", "error"]);
        assert!(messages("off").is_empty());
    }

    #[test]
    fn new_default_is_an_info_json_logger() {
        let lines = logged(r#"local logger = log.new() logger:debug("hidden") logger:info("shown")"#);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1["msg"], "shown");
    }

    #[test]
    fn the_text_format_is_key_value_pairs() {
        let (lua, lines) = capturing();
        lua.load(
            r#"
            local logger = require("lib:log").new({ format = "text" })
            logger:warn("disk nearly full", { free = 12, path = "/var/log", note = "a b", eq = "x=y", ok = true })
        "#,
        )
        .exec()
        .unwrap();
        let lines = lines.lock().unwrap();
        let (level, line) = &lines[0];
        assert_eq!(level, "warn");
        let (ts, rest) = line.split_once(' ').unwrap();
        assert_eq!(ts.len(), 24);
        assert_eq!(rest, r#"[warn] disk nearly full eq="x=y" free=12 note="a b" ok=true path=/var/log"#);
    }

    #[test]
    fn with_binds_fields_and_leaves_the_parent_unchanged() {
        let lines = logged(
            r#"
            local parent = log.new():with({ service = "api" })
            local child = parent:with({ request_id = "abc", service = "worker" })
            child:info("child", { rows = 100 })
            parent:info("parent")
        "#,
        );
        let (child, parent) = (&lines[0].1, &lines[1].1);
        assert_eq!(child["request_id"], "abc");
        assert_eq!(child["service"], "worker", "the child's field is over the parent's");
        assert_eq!(child["rows"], 100);
        assert_eq!(parent["service"], "api");
        assert!(parent.get("request_id").is_none());
    }

    #[test]
    fn a_call_field_is_over_a_bound_one() {
        let line = fields(r#"log.new():with({ a = 1, b = 2 }):info("m", { b = 3 })"#);
        assert_eq!((line["a"].as_i64(), line["b"].as_i64()), (Some(1), Some(3)));
    }

    #[test]
    fn nil_field_omitted() {
        let line = fields(r#"log.info("m", { present = "yes", absent = nil })"#);
        assert_eq!(line["present"], "yes");
        assert!(line.get("absent").is_none());
    }

    #[test]
    fn what_json_has_no_word_for_becomes_a_placeholder() {
        let line = fields(
            r#"
            log.info("m", {
                cb = function() end,
                co = coroutine.create(function() end),
                object = log.new(),
                nan = 0 / 0,
                bytes = "\255",
            })
        "#,
        );
        assert_eq!(line["cb"], "<function>");
        assert_eq!(line["co"], "<thread>");
        assert_eq!(line["object"], "<userdata>");
        assert_eq!(line["nan"], Json::Null);
        assert_eq!(line["bytes"], "\u{fffd}");
    }

    #[test]
    fn depth_limit_replaces_deep_value() {
        let line = fields(r#"log.info("m", { l1 = { l2 = { l3 = { l4 = { l5 = "deep" } } } } })"#);
        assert_eq!(line["l1"]["l2"]["l3"], "<depth limit>");
        let line = fields(r#"log.info("m", { user = { id = 1, name = "alice" } })"#);
        assert_eq!(line["user"]["name"], "alice");
    }

    #[test]
    fn circular_reference_becomes_placeholder() {
        let line = fields(
            r#"
            local a = {}
            a.self = a
            log.info("m", { node = a })
        "#,
        );
        assert_eq!(line["node"]["self"], "<circular>");
    }

    #[test]
    fn array_like_table_becomes_json_array() {
        let line = fields(
            r#"log.info("m", { items = { 10, 20, 30 }, empty = {}, sparse = { [1] = "a", [3] = "c" } })"#,
        );
        assert_eq!(line["items"], serde_json::json!([10, 20, 30]));
        assert_eq!(line["empty"], serde_json::json!({}));
        assert_eq!(line["sparse"], serde_json::json!({ "1": "a", "3": "c" }));
    }

    #[test]
    fn a_key_that_is_not_text_in_a_nested_table_does_not_fail() {
        let line = fields(r#"log.info("m", { nested = { [true] = "v", name = "n" } })"#);
        assert_eq!(line["nested"]["<invalid key>"], "v");
        assert_eq!(line["nested"]["name"], "n");
    }

    #[test]
    fn reserved_keys_cannot_be_shadowed() {
        let line =
            fields(r#"log.info("real", { level = "bogus", ts = "never", msg = "fake", user = "alice" })"#);
        assert_eq!(line["level"], "info");
        assert_eq!(line["msg"], "real");
        assert_ne!(line["ts"], "never");
        assert_eq!(line["user"], "alice");

        let mut bound = Map::new();
        bound.insert("msg".into(), Json::String("fake".into()));
        bound.insert("user".into(), Json::String("alice".into()));
        assert_eq!(format_line(Format::Text, "TS", Level::Info, "real", &bound), "TS [info] real user=alice");
        for key in ["level", "ts", "msg"] {
            assert!(is_reserved_key(key), "{key} is reserved");
        }
        for key in ["user", "request_id", "ts2", ""] {
            assert!(!is_reserved_key(key), "{key} is not reserved");
        }
    }

    #[test]
    fn module_table_exposes_new_and_levels() {
        let lua = make_engine().unwrap();
        let module: Table = lua.load(r#"return require("lib:log")"#).eval().unwrap();
        for name in ["new", "trace", "debug", "info", "warn", "error"] {
            assert!(matches!(module.get::<Value>(name).unwrap(), Value::Function(_)), "{name} missing");
        }
    }

    #[test]
    fn an_option_that_is_not_one_is_refused() {
        let lua = make_engine().unwrap();
        for options in [
            r#"{ level = "verbose" }"#,
            r#"{ level = "INFO" }"#,
            r#"{ format = "yaml" }"#,
            "{ level = 5 }",
            r#"{ levle = "info" }"#,
        ] {
            let err = lua
                .load(format!(r#"return require("lib:log").new({options})"#))
                .eval::<Value>()
                .expect_err(options)
                .to_string();
            assert!(err.contains("lib:log.new: argument #1 'options'"), "{options}: {err}");
        }
    }

    #[test]
    fn fields_that_are_not_a_table_of_names_are_refused() {
        let lua = make_engine().unwrap();
        for fields in ["5", r#""text""#, r#"{ [true] = "v" }"#, r#"{ "a", "b" }"#] {
            let err = lua
                .load(format!(r#"require("lib:log").new({{ level = "off" }}):info("m", {fields})"#))
                .exec()
                .expect_err(fields)
                .to_string();
            assert!(err.contains("lib:log.Logger:info: argument #2 'fields'"), "{fields}: {err}");
        }
    }

    #[test]
    fn without_a_sink_a_line_is_a_tracing_event_and_a_log_call_does_not_fail() {
        // level off: nothing reaches the test's stderr
        make_engine()
            .unwrap()
            .load(
                r#"
                local log = require("lib:log")
                local logger = log.new({ level = "off", format = "text" })
                logger:error("hidden", { a = { b = { c = { d = 1 } } } })
                log.trace("hidden")
                log.debug("hidden", { k = "v" })
            "#,
            )
            .exec()
            .unwrap();
        emit_tracing(Level::Info, "no subscriber", &Map::new());
    }
}

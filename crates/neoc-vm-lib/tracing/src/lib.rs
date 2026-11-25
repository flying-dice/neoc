//! Spans and events, after Rust's `tracing`: `lib:tracing`.
//!
//! A script writes events (`tracing.info("read the overview", { units = 212 })`) and opens spans
//! (`tracing.span("parse units", { n = #raw }):in_scope(function() ... end)`), and they are the host's own `tracing`
//! spans and events: an event is in the span it was written in, a span is a child of the one it was opened in, and a
//! task the script starts (`task.spawn`) carries the span it was started in. In vos, that puts what an app's backend
//! does in the trace of the page's call.
//!
//! The API's rules:
//! - The message comes first, then the fields (Rust's macros put the fields first; a script reads better this way, and
//!   it is `lib:log`'s order).
//! - A span is put in scope once: `in_scope` runs the function in it and ends it when the function returns or raises
//!   (a raise is recorded as the span's failure, and raised again). A span never put in scope ends when it is
//!   collected.
//! - A field's value is written as JSON; what is not plain data (a function, userdata) is written as a placeholder.
//! - An event goes where `lib:log`'s lines go, whatever its level (the embedder's filter decides what it keeps): the embedder's sink when there is one ([`set_log_sink`]), else a JSON
//!   line on stderr and a `tracing` event at target `lua::lib::tracing`.
//!
//! [`set_log_sink`]: neoc_vm_core::log_sink::set_log_sink

#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

use std::io::Write;

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::{Map, Value as Json};

/// The JSON of a script's value; what JSON cannot hold is a placeholder naming its type.
fn json_of(lua: &Lua, value: Value) -> Json {
    let kind = value.type_name();
    lua.from_value::<Json>(value).unwrap_or_else(|_| Json::String(format!("<{kind}>")))
}

/// A script's fields as one JSON object; a `nil` field is left out.
fn object_of(lua: &Lua, fields: impl IntoIterator<Item = (String, Value)>) -> Map<String, Json> {
    fields.into_iter().filter(|(_, v)| !v.is_nil()).map(|(k, v)| (k, json_of(lua, v))).collect()
}

/// The line an event is written as: `level`, `ts`, `msg`, then the fields, as `lib:log` writes it.
fn line_of(level: &str, message: &str, fields: &Map<String, Json>) -> String {
    let mut line = Map::new();
    line.insert("level".into(), Json::String(level.into()));
    line.insert(
        "ts".into(),
        Json::String(jiff::Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
    );
    line.insert("msg".into(), Json::String(message.into()));
    for (k, v) in fields {
        if !matches!(k.as_str(), "level" | "ts" | "msg") {
            line.insert(k.clone(), v.clone());
        }
    }
    Json::Object(line).to_string()
}

#[mlua_ir::module("lib:tracing")]
pub mod tracing {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mlua::{Lua, MultiValue, Value};
    use mlua_ir::{Callback, Live, Object, Options, Shape};
    use neoc_vm_core::log_sink::log_sink;
    use neoc_vm_core::span::script_span;
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};
    use serde_json::{Map, Value as Json};
    use tracing::Instrument as _;

    use super::{Write, json_of, line_of, object_of};

    mlua_ir::unknown_reason!(
        /// Why a field's value has no declared shape.
        FieldValue = "whatever a script records under a field: written as JSON, and what is not plain data as a placeholder"
    );

    mlua_ir::unknown_reason!(
        /// Why what a span's function returns has no declared shape.
        ScopeValue = "whatever the script's function returned: handed back as it is"
    );

    /// The fields of an event or a span, by name.
    pub type Fields = BTreeMap<String, Live<FieldValue>>;

    /// How detailed an event or a span is, finest first. The host keeps what its filter lets through (`info` and
    /// above unless it was told otherwise).
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
    }

    impl Level {
        fn as_str(self) -> &'static str {
            match self {
                Level::Trace => "trace",
                Level::Debug => "debug",
                Level::Info => "info",
                Level::Warn => "warn",
                Level::Error => "error",
            }
        }

        fn of(self) -> ::tracing::Level {
            match self {
                Level::Trace => ::tracing::Level::TRACE,
                Level::Debug => ::tracing::Level::DEBUG,
                Level::Info => ::tracing::Level::INFO,
                Level::Warn => ::tracing::Level::WARN,
                Level::Error => ::tracing::Level::ERROR,
            }
        }

        /// The host's filter lets a span at this level through at all (a span is made by hand, so nothing else asks).
        fn kept(self) -> bool {
            self.of() <= ::tracing::level_filters::LevelFilter::current()
        }
    }

    /// How a span is opened.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct SpanOptions {
        /// How detailed it is. When not given: `info`.
        pub level: Option<Level>,
    }

    /// A span: a named stretch of the script's work, with fields, in the span it was opened in. Run work in it with
    /// `in_scope`; record more on it with `record`.
    #[derive(Clone, Object)]
    pub struct Span {
        /// The host's span; `None` once it has ended.
        inner: Arc<Mutex<Option<::tracing::Span>>>,
    }

    #[mlua_ir::object]
    impl Span {
        /// Record `value` under `key` on the span, as `tracing`'s `Span::record` does. Nothing happens once the span
        /// has ended.
        ///
        /// ```luau
        /// local span = tracing.span("import")
        /// span:in_scope(function()
        ///     span:record("rows", 10)
        /// end)
        /// ```
        pub fn record(&self, lua: &Lua, key: String, value: Live<FieldValue>) {
            if let Some(span) = self.inner.lock().as_ref() {
                let mut fields = Map::new();
                fields.insert(key, json_of(lua, value.value));
                span.record("fields", Json::Object(fields).to_string());
            }
        }

        /// Run `body` in the span and end the span: what `body` does (the events it writes, the spans it opens, the
        /// tasks it starts) is in this span. Gives what `body` returned; what it raises is recorded as the span's
        /// failure and raised again. A span is put in scope once: a second call runs `body` outside any span of its
        /// own.
        ///
        /// ```luau
        /// local units = tracing.span("list units", { scope = "system" }):in_scope(function()
        ///     return listUnits()
        /// end)
        /// ```
        #[host("Vm")]
        pub async fn in_scope(
            &self,
            body: Callback<fn() -> Live<ScopeValue>>,
        ) -> mlua::Result<Live<ScopeValue>> {
            let span = self.inner.lock().clone();
            let call = body.into_function();
            let Some(span) = span else {
                let values = call.call_async::<MultiValue>(()).await?;
                return Ok(Live::new(values.into_iter().next().unwrap_or(Value::Nil)));
            };
            let outcome = call.call_async::<MultiValue>(()).instrument(span.clone()).await;
            match &outcome {
                Ok(_) => {
                    span.record("outcome", "ok");
                }
                Err(e) => {
                    span.record("outcome", "error");
                    span.record("error", e.to_string().as_str());
                }
            }
            // the last handle goes: the span ends here, not when the script's value is collected
            self.inner.lock().take();
            drop(span);
            let values = outcome?;
            Ok(Live::new(values.into_iter().next().unwrap_or(Value::Nil)))
        }
    }

    /// An event goes to the sink or stderr whatever the level, as a `lib:log` line does: the embedder's filter
    /// decides what it keeps.
    fn emit(lua: &Lua, level: Level, message: &str, fields: Option<Fields>) {
        let fields = object_of(lua, fields.unwrap_or_default().into_iter().map(|(k, v)| (k, v.value)));
        let line = line_of(level.as_str(), message, &fields);
        if let Some(sink) = log_sink(lua) {
            sink(level.as_str(), &line);
            return;
        }
        // a failed write to stderr is nothing an event can mend
        let _ = writeln!(std::io::stderr(), "{line}");
        let fields = Json::Object(fields).to_string();
        match level {
            Level::Trace => {
                ::tracing::trace!(target: "lua::lib::tracing", message = %message, fields = %fields)
            }
            Level::Debug => {
                ::tracing::debug!(target: "lua::lib::tracing", message = %message, fields = %fields)
            }
            Level::Info => {
                ::tracing::info!(target: "lua::lib::tracing", message = %message, fields = %fields)
            }
            Level::Warn => {
                ::tracing::warn!(target: "lua::lib::tracing", message = %message, fields = %fields)
            }
            Level::Error => {
                ::tracing::error!(target: "lua::lib::tracing", message = %message, fields = %fields)
            }
        }
    }

    /// Open a span named `name` with `fields`, in the span the script is in now. It starts now; put work in it with
    /// `in_scope`.
    ///
    /// ```luau
    /// local parsed = tracing.span("parse units", { n = #raw }):in_scope(function()
    ///     return parse(raw)
    /// end)
    /// ```
    pub fn span(lua: &Lua, name: String, fields: Option<Fields>, options: Option<SpanOptions>) -> Span {
        let level = options.and_then(|o| o.level).unwrap_or(Level::Info);
        let span = if level.kept() {
            let fields = object_of(lua, fields.unwrap_or_default().into_iter().map(|(k, v)| (k, v.value)));
            script_span("lua::app", &name, level.of(), &Json::Object(fields).to_string())
        } else {
            ::tracing::Span::none()
        };
        Span { inner: Arc::new(Mutex::new(Some(span))) }
    }

    /// Write a `trace` event in the span the script is in now.
    pub fn trace(lua: &Lua, message: String, fields: Option<Fields>) {
        emit(lua, Level::Trace, &message, fields);
    }

    /// Write a `debug` event in the span the script is in now.
    pub fn debug(lua: &Lua, message: String, fields: Option<Fields>) {
        emit(lua, Level::Debug, &message, fields);
    }

    /// Write an `info` event in the span the script is in now.
    ///
    /// ```luau
    /// tracing.info("read the overview", { units = 212 })
    /// ```
    pub fn info(lua: &Lua, message: String, fields: Option<Fields>) {
        emit(lua, Level::Info, &message, fields);
    }

    /// Write a `warn` event in the span the script is in now.
    pub fn warn(lua: &Lua, message: String, fields: Option<Fields>) {
        emit(lua, Level::Warn, &message, fields);
    }

    /// Write an `error` event in the span the script is in now.
    ///
    /// ```luau
    /// local ok, err = pcall(restart, unit)
    /// if not ok then tracing.error("restart failed", { unit = unit, error = tostring(err) }) end
    /// ```
    pub fn error(lua: &Lua, message: String, fields: Option<Fields>) {
        emit(lua, Level::Error, &message, fields);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use mlua::Lua;
    use neoc_vm_core::log_sink::set_log_sink;
    use neoc_vm_core::make_engine;
    use serde_json::Value as Json;

    /// The events a sink was given: each level and JSON line.
    type Lines = Arc<Mutex<Vec<(String, Json)>>>;

    /// An engine whose events are kept, with `tracing` as a global.
    fn capturing() -> (Lua, Lines) {
        let lua = make_engine().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = lines.clone();
        set_log_sink(
            &lua,
            Arc::new(move |level, line| {
                kept.lock().unwrap().push((level.to_owned(), serde_json::from_str(line).unwrap()))
            }),
        );
        let tracing: mlua::Table = lua.load(r#"return require("lib:tracing")"#).eval().unwrap();
        lua.globals().set("tracing", tracing).unwrap();
        (lua, lines)
    }

    #[test]
    fn an_event_is_a_line_with_its_message_then_its_fields() {
        let (lua, lines) = capturing();
        lua.load(r#"tracing.warn("slow", { ms = 120, unit = "nginx", gone = nil, f = function() end })"#)
            .exec()
            .unwrap();
        let lines = lines.lock().unwrap();
        let (level, line) = &lines[0];
        assert_eq!(level, "warn");
        assert_eq!(
            (&line["msg"], &line["ms"], &line["unit"]),
            (&Json::from("slow"), &Json::from(120), &Json::from("nginx"))
        );
        assert_eq!(line["f"], "<function>", "what JSON cannot hold is a placeholder");
        assert!(line.get("gone").is_none(), "a nil field is left out");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn in_scope_gives_what_its_function_returned_and_raises_what_it_raised() {
        let (lua, _) = capturing();
        let (value, raised): (i64, String) = lua
            .load(
                r#"
                local value = tracing.span("sum", { n = 2 }):in_scope(function() return 1 + 1 end)
                local ok, err = pcall(function()
                    tracing.span("fails"):in_scope(function() error("broken") end)
                end)
                assert(not ok)
                return value, tostring(err)
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(value, 2);
        assert!(raised.contains("broken"), "{raised}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_span_is_in_scope_once_and_record_after_it_ended_does_nothing() {
        let (lua, _) = capturing();
        let second: i64 = lua
            .load(
                r#"
                local span = tracing.span("once", nil, { level = "debug" })
                span:in_scope(function() span:record("rows", 3) end)
                span:record("late", true)
                return span:in_scope(function() return 7 end)
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(second, 7);
    }
}

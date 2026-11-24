//! Where an embedder wants the lines of `lib:log` instead of stderr. It lives here, not in `lib:log`, because
//! `std:thread` hands a worker the parent's sink and must not depend on a `lib:` module for it.

use std::sync::Arc;

use mlua::Lua;

/// A function that takes a `lib:log` line: the level's name and the formatted line.
pub type LogFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Where an embedder wants `lib:log` lines instead of stderr.
struct LogSink(LogFn);

/// Install `sink` as this engine's `lib:log` output. A `std:thread` worker the script starts logs to the same sink.
pub fn set_log_sink(lua: &Lua, sink: LogFn) {
    lua.set_app_data(LogSink(sink));
}

/// The sink [`set_log_sink`] installed in this engine, for a worker the script starts to log to as well.
#[must_use]
pub fn log_sink(lua: &Lua) -> Option<LogFn> {
    lua.app_data_ref::<LogSink>().map(|sink| sink.0.clone())
}

//! Recorded calls: `lib:vcr`.
//!
//! A cassette is a JSON file of calls by key. `Cassette:play(key, call)` is, by the cassette's mode:
//!
//! | `vcr.cassette(...)`                    | `tape:play(key, call)`                                              |
//! |----------------------------------------|---------------------------------------------------------------------|
//! | `{ mode = "record", path = "..." }`    | runs `call`, records what it returned under `key`, writes the file  |
//! | `{ mode = "replay", path = "..." }`    | returns what the file holds under `key`; `call` is not run          |
//! | `{ mode = "off" }`                     | runs `call`, and that is all                                        |
//!
//! With `NEOC_VCR=off` in the environment every cassette is `off`, whatever the script asked for, so the calls can
//! stay in code that also runs for real.
//!
//! ```luau
//! local tape = assert(vcr.cassette({ path = "cassettes/users.json", mode = "record" }))
//! local user, err = tape:play("users.get.42", function()
//!     return api.get_user(42)
//! end)
//! ```
//!
//! What a call returns is its result and its error: the two values every fallible function of neoc returns. Both
//! are recorded (as plain data) and both are replayed.
//!
//! The file is `{ "<key>": { "result": [<value>, <error>] } }`, written whole after every recorded call. A file
//! neoc-vm wrote (which also has the call's `args` under each key) replays.
//!
//! Ported from neoc-vm's `lib:vcr`. What changed on purpose:
//! - `tape:play(key, function() return f(x) end)` replaces `tape:wrap(key, f)(x)`: mlua-ir has no function that
//!   returns a function, and none that takes or returns any number of values, so the cassette takes the call
//!   itself. The call's arguments are therefore not written to the file (replay never read them).
//! - A call returns at most two values (it was any number); a third is the raised `too_many_results`.
//! - `play` waits for a call that waits (a request, a sleep); a wrapped function was called without that, so it
//!   could not record one.
//! - A key is played once per cassette (it was wrapped once, and the function called any number of times, each
//!   call overwriting the last).
//! - What cannot be recorded as plain data (a function, a userdata, a buffer, a table that contains itself) is
//!   the raised `not_plain_data` (only a buffer was refused; anything else was recorded as the text `<function>`,
//!   `<userdata>`, which replay then returned in its place).
//! - Errors are typed: `cassette` returns a `CassetteError` (`not_found`, `io`, `malformed`; they were raised),
//!   `play` raises a `PlayError` (`duplicate_key`, `missing_key`, `too_many_results`, `not_plain_data`, `io`).
//! - The mode is typed: a mode that does not exist, a missing `path`, an unknown option are refused as arguments.
//! - Replay gives `nil` for a recorded `null` anywhere (inside a table it was mlua's `null` sentinel).
//! - `dump` is gone: it did nothing.

use std::collections::{BTreeMap, BTreeSet};

use mlua::{Lua, MultiValue, Value};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use vcr::{CassetteError, PlayError};

/// The most values a call may return: a result and its error.
const MAX_RESULTS: usize = 2;

fn forced_off() -> bool {
    std::env::var("NEOC_VCR").as_deref() == Ok("off")
}

/// What one call returned. (neoc-vm also wrote the call's `args` here; they are not read.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    result: Vec<Json>,
}

/// The calls of a cassette that records, and the keys played so far.
#[derive(Default)]
struct Recording {
    played: BTreeSet<String>,
    entries: BTreeMap<String, Entry>,
}

enum Tape {
    Off,
    Record { path: String, recording: Mutex<Recording> },
    Replay { entries: BTreeMap<String, Entry>, played: Mutex<BTreeSet<String>> },
}

/// Write the whole cassette.
fn flush(entries: &BTreeMap<String, Entry>, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entries).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

impl Tape {
    /// A cassette that records to `path`: the file is written at once, empty, so a path that cannot be written is
    /// found before the first call.
    fn record(path: String) -> Result<Self, CassetteError> {
        let recording = Recording::default();
        flush(&recording.entries, &path)
            .map_err(|e| CassetteError::Io { path: path.clone(), reason: e.to_string() })?;
        Ok(Tape::Record { path, recording: Mutex::new(recording) })
    }

    /// A cassette that replays the file at `path`.
    fn replay(path: String) -> Result<Self, CassetteError> {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CassetteError::NotFound { path });
            }
            Err(e) => return Err(CassetteError::Io { path, reason: e.to_string() }),
        };
        match serde_json::from_str(&content) {
            Ok(entries) => Ok(Tape::Replay { entries, played: Mutex::new(BTreeSet::new()) }),
            Err(e) => Err(CassetteError::Malformed { path, reason: e.to_string() }),
        }
    }
}

/// The values a call returned, as the two a cassette holds.
fn first_two(key: &str, values: MultiValue) -> Result<Vec<Value>, PlayError> {
    if values.len() > MAX_RESULTS {
        let count = u32::try_from(values.len()).unwrap_or(u32::MAX);
        return Err(PlayError::TooManyResults { key: key.to_owned(), count });
    }
    Ok(values.into_iter().collect())
}

/// What a call returned, as the cassette writes it.
fn recorded(lua: &Lua, key: &str, values: &[Value]) -> Result<Entry, PlayError> {
    let result = values
        .iter()
        .map(|value| mlua_ir::from_serde::<Json>(value.clone(), lua))
        .collect::<Result<Vec<Json>, String>>()
        .map_err(|reason| PlayError::NotPlainData { key: key.to_owned(), reason })?;
    Ok(Entry { result })
}

/// The result and the error among `values`: `nil` for one that is not there.
fn pair(values: Vec<Value>) -> (Value, Value) {
    let mut values = values.into_iter();
    (values.next().unwrap_or(Value::Nil), values.next().unwrap_or(Value::Nil))
}

#[mlua_ir::module("lib:vcr")]
pub mod vcr {
    use std::sync::Arc;

    use mlua::{Lua, MultiValue, Value};
    use mlua_ir::{Callback, Live, LuaError, Object, Raise, Shape, unknown_reason};
    use serde::{Deserialize, Serialize};

    use super::{Tape, first_two, flush, forced_off, pair, recorded};

    unknown_reason!(
        /// What a played call gives.
        Recorded = "what a call a script recorded returned: whatever the script's own function gives, or the cassette file holds"
    );

    /// Why a cassette was not opened.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CassetteError {
        /// There is no file to replay: nothing was recorded there yet.
        #[error("no cassette at {path}")]
        NotFound {
            /// The file asked for.
            path: String,
        },
        /// The file cannot be read (to replay) or written (to record).
        #[error("{path}: {reason}")]
        Io {
            /// The file asked for.
            path: String,
            /// What the operating system said.
            reason: String,
        },
        /// The file to replay is not a cassette.
        #[error("{path} is not a cassette: {reason}")]
        Malformed {
            /// The file asked for.
            path: String,
            /// What is wrong with it, and where.
            reason: String,
        },
    }

    /// Why a call was not played. Raised: what `play` returns is the call's own result and error.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum PlayError {
        /// The cassette already played this key: a second call would replace the first, or replay it again.
        #[error("duplicate key \"{key}\"")]
        DuplicateKey {
            /// The key given twice.
            key: String,
        },
        /// The cassette being replayed has no call under this key.
        #[error("no cassette entry for key \"{key}\"")]
        MissingKey {
            /// The key asked for.
            key: String,
        },
        /// The call returned more than a result and an error.
        #[error("the call \"{key}\" returned {count} values; a cassette holds a result and an error")]
        TooManyResults {
            /// The call's key.
            key: String,
            /// How many values it returned.
            count: u32,
        },
        /// What the call returned cannot be written to a cassette: a function, a userdata, a buffer, a table that
        /// contains itself.
        #[error("the call \"{key}\" returned what is not plain data: {reason}")]
        NotPlainData {
            /// The call's key.
            key: String,
            /// What cannot be written.
            reason: String,
        },
        /// The cassette file cannot be written.
        #[error("{path}: {reason}")]
        Io {
            /// The cassette's file.
            path: String,
            /// What the operating system said.
            reason: String,
        },
    }

    /// Which cassette to open, by `mode`.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
    pub enum Config {
        /// Run every call and record what it returns. The file is written at once (empty, replacing one already
        /// there) and again after every call.
        Record {
            /// The cassette file to write.
            path: String,
        },
        /// Return what was recorded; no call is run.
        Replay {
            /// The cassette file to read.
            path: String,
        },
        /// Run every call and record nothing.
        Off {
            /// Not used: for a call site that only changes the mode.
            path: Option<String>,
        },
    }

    /// A cassette, in the mode it was opened in.
    #[derive(Clone, Object)]
    pub struct Cassette {
        tape: Arc<Tape>,
    }

    #[mlua_ir::object]
    impl Cassette {
        /// Play the call `key`: run `call` and record its result and error (recording), return what was recorded
        /// without running it (replaying), or only run it (off). A key is played once. Raises a `PlayError`;
        /// what `call` raises is raised as it is, and nothing is recorded.
        ///
        /// ```luau
        /// local decoded, err = tape:play("base64.decode.hello", function()
        ///     return base64.decode("aGVsbG8=")
        /// end)
        /// ```
        #[host("Vm")]
        pub async fn play(
            &self,
            lua: Lua,
            key: String,
            call: Callback<fn() -> Live<Recorded>>,
        ) -> mlua::Result<Result<(Live<Recorded>, Live<Recorded>), Raise<PlayError>>> {
            // the call is run here, not through the callback: it may wait, and it returns two values
            let call = call.into_function();
            let values = match &*self.tape {
                Tape::Off => match first_two(&key, call.call_async::<MultiValue>(()).await?) {
                    Ok(values) => values,
                    Err(e) => return Ok(Err(e.into())),
                },
                Tape::Record { path, recording } => {
                    if !recording.lock().played.insert(key.clone()) {
                        return Ok(Err(PlayError::DuplicateKey { key }.into()));
                    }
                    let values = match first_two(&key, call.call_async::<MultiValue>(()).await?) {
                        Ok(values) => values,
                        Err(e) => return Ok(Err(e.into())),
                    };
                    let entry = match recorded(&lua, &key, &values) {
                        Ok(entry) => entry,
                        Err(e) => return Ok(Err(e.into())),
                    };
                    let mut recording = recording.lock();
                    recording.entries.insert(key, entry);
                    if let Err(e) = flush(&recording.entries, path) {
                        return Ok(Err(PlayError::Io { path: path.clone(), reason: e.to_string() }.into()));
                    }
                    values
                }
                Tape::Replay { entries, played } => {
                    if !played.lock().insert(key.clone()) {
                        return Ok(Err(PlayError::DuplicateKey { key }.into()));
                    }
                    let Some(entry) = entries.get(&key) else {
                        return Ok(Err(PlayError::MissingKey { key }.into()));
                    };
                    entry
                        .result
                        .iter()
                        .take(super::MAX_RESULTS)
                        .map(|value| mlua_ir::to_serde(value, &lua))
                        .collect::<mlua::Result<Vec<Value>>>()?
                }
            };
            let (result, error) = pair(values);
            Ok(Ok((Live::new(result), Live::new(error))))
        }
    }

    /// Open a cassette. With `NEOC_VCR=off` in the environment it is an `off` one, whatever `config` says.
    ///
    /// ```luau
    /// local tape = assert(vcr.cassette({ path = "cassettes/base64.json", mode = "replay" }))
    /// ```
    pub fn cassette(config: Config) -> Result<Cassette, CassetteError> {
        open(config, forced_off())
    }

    /// `cassette`, with what the environment says given.
    pub(super) fn open(config: Config, forced_off: bool) -> Result<Cassette, CassetteError> {
        let tape = match config {
            _ if forced_off => Tape::Off,
            Config::Off { .. } => Tape::Off,
            Config::Record { path } => Tape::record(path)?,
            Config::Replay { path } => Tape::replay(path)?,
        };
        Ok(Cassette { tape: Arc::new(tape) })
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Value};

    use super::vcr::{Config, open};
    use super::*;

    /// An engine with `vcr`, `base64` and `dir` (a directory of its own, with `/` separators) as globals.
    fn engine() -> (Lua, tempfile::TempDir) {
        let lua = crate::make_engine().expect("engine");
        let dir = tempfile::tempdir().expect("tempdir");
        lua.load(r#"vcr = require("lib:vcr") base64 = require("lib:base64")"#).exec().unwrap();
        lua.globals().set("dir", dir.path().to_str().expect("utf-8").replace('\\', "/")).unwrap();
        (lua, dir)
    }

    fn run<T: mlua::FromLuaMulti>(lua: &Lua, code: &str) -> T {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(lua.load(code).eval_async()).unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    /// The text of the error `code` raises.
    fn raised(lua: &Lua, code: &str) -> String {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(lua.load(code).exec_async()).expect_err(code).to_string()
    }

    fn cassette_file(dir: &tempfile::TempDir, name: &str) -> Json {
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(name)).expect("the cassette"))
            .expect("JSON")
    }

    // --- record ---

    #[test]
    fn record_returns_what_the_call_returned_and_writes_it() {
        let (lua, dir) = engine();
        let (encoded, none, decoded, no_err, failed, kind): (String, Value, String, Value, Value, String) =
            run(
                &lua,
                r#"
            local tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" }))
            local encoded, none = tape:play("base64.encode.hello", function() return base64.encode("hello") end)
            local decoded, no_err = tape:play("base64.decode.hello", function() return base64.decode("aGVsbG8=") end)
            local failed, err = tape:play("base64.decode.invalid", function() return base64.decode("not*valid!") end)
            -- the error is the function's own, not a copy: it still has its text
            assert(tostring(err):find("lib:base64.decode: ", 1, true))
            return encoded, none, decoded, no_err, failed, err.kind
            "#,
            );
        assert_eq!(
            (encoded.as_str(), decoded.as_str(), kind.as_str()),
            ("aGVsbG8=", "hello", "invalid_byte")
        );
        assert!(none.is_nil() && no_err.is_nil() && failed.is_nil());

        let file = cassette_file(&dir, "tape.json");
        assert_eq!(file["base64.encode.hello"], serde_json::json!({ "result": ["aGVsbG8="] }));
        assert_eq!(file["base64.decode.hello"], serde_json::json!({ "result": ["hello", null] }));
        let failure = &file["base64.decode.invalid"]["result"];
        assert!(failure[0].is_null() && failure[1]["kind"] == "invalid_byte", "{failure}");
        assert_eq!(file.as_object().map(serde_json::Map::len), Some(3));
    }

    #[test]
    fn the_file_is_written_when_the_cassette_opens_and_after_every_call() {
        let (lua, dir) = engine();
        let _: Value = run(
            &lua,
            r#"tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" })) return nil"#,
        );
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({}));
        let _: Value = run(&lua, r#"return tape:play("one", function() return 1 end)"#);
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({ "one": { "result": [1] } }));
        let _: Value = run(&lua, r#"return tape:play("none", function() end)"#);
        assert_eq!(cassette_file(&dir, "tape.json")["none"], serde_json::json!({ "result": [] }));
        // opening it again to record starts it over
        let _: Value =
            run(&lua, r#"assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" })) return nil"#);
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({}));
    }

    #[test]
    fn a_key_is_played_once_per_cassette() {
        let (lua, dir) = engine();
        let err = raised(
            &lua,
            r#"
            local tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" }))
            tape:play("base64.encode.dupe", function() return base64.encode("a") end)
            called = false
            tape:play("base64.encode.dupe", function() called = true return base64.encode("b") end)
            "#,
        );
        assert!(
            err.contains(r#"lib:vcr.Cassette:play: duplicate_key: duplicate key "base64.encode.dupe""#),
            "{err}"
        );
        assert!(!lua.globals().get::<bool>("called").unwrap(), "the second call was not run");
        assert_eq!(cassette_file(&dir, "tape.json")["base64.encode.dupe"]["result"][0], "YQ==");
        // the same key in two cassettes is two calls, each on its own tape
        let _: Value = run(
            &lua,
            r#"
            local one = assert(vcr.cassette({ path = dir .. "/one.json", mode = "record" }))
            local two = assert(vcr.cassette({ path = dir .. "/two.json", mode = "record" }))
            one:play("key", function() return "only in one" end)
            two:play("key", function() return "only in two" end)
            return nil
            "#,
        );
        assert_eq!(cassette_file(&dir, "one.json")["key"]["result"][0], "only in one");
        assert_eq!(cassette_file(&dir, "two.json")["key"]["result"][0], "only in two");
    }

    #[test]
    fn a_call_that_raises_is_raised_and_not_recorded() {
        let (lua, dir) = engine();
        let err = raised(
            &lua,
            r#"
            local tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" }))
            tape:play("raises", function() error("the call failed") end)
            "#,
        );
        assert!(err.contains("the call failed"), "{err}");
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({}));
    }

    /// A call may wait: `play` is async, so a sleep (or a request) inside the call is recorded like any other.
    #[test]
    fn a_call_that_waits_is_recorded() {
        let (lua, dir) = engine();
        let out: String = run(
            &lua,
            r#"
            local time = require("std:time")
            local tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" }))
            return tape:play("slow", function()
                time.sleep(time.Duration.from_millis(5))
                return "after the wait"
            end)
            "#,
        );
        assert_eq!(out, "after the wait");
        assert_eq!(cassette_file(&dir, "tape.json")["slow"]["result"][0], "after the wait");
    }

    /// From neoc-vm's `serde_guard` and `vcr.test.lua`: a buffer is never recorded as an array of numbers. Neither
    /// is anything else replay could not give back.
    #[test]
    fn what_is_not_plain_data_is_refused_and_not_recorded() {
        let (lua, dir) = engine();
        lua.load(r#"tape = assert(vcr.cassette({ path = dir .. "/tape.json", mode = "record" }))"#)
            .exec()
            .unwrap();
        for (key, call) in [
            ("buffer", "return buffer.create(2)"),
            ("nested", "return { data = buffer.create(2) }"),
            ("deep", "local t = { buffer.create(2) } for _ = 2, 100 do t = { inner = t } end return t"),
            ("second", "return nil, buffer.create(2)"),
            ("function", "return print"),
            ("inside", "return { callback = print }"),
            ("handle", "return tape"),
            ("cycle", "local t = {} t.self = t return t"),
        ] {
            let err = raised(&lua, &format!(r#"tape:play("{key}", function() {call} end)"#));
            let place = format!(
                r#"lib:vcr.Cassette:play: not_plain_data: the call "{key}" returned what is not plain data: "#
            );
            assert!(err.contains(&place), "{key}: {err}");
        }
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({}), "nothing was recorded");
        let err = raised(&lua, r#"tape:play("three", function() return 1, 2, 3 end)"#);
        assert!(err.contains(r#"too_many_results: the call "three" returned 3 values"#), "{err}");
        assert_eq!(cassette_file(&dir, "tape.json"), serde_json::json!({}));
    }

    // --- replay ---

    #[test]
    fn replay_returns_what_was_recorded_without_running_the_call() {
        let (lua, _dir) = engine();
        let (encoded, decoded, no_err, failed, kind, table, nothing): (
            String,
            String,
            Value,
            Value,
            String,
            String,
            i64,
        ) = run(
            &lua,
            r##"
            local path = dir .. "/tape.json"
            local rec = assert(vcr.cassette({ path = path, mode = "record" }))
            rec:play("base64.encode.hello", function() return base64.encode("hello") end)
            rec:play("base64.decode.hello", function() return base64.decode("aGVsbG8=") end)
            rec:play("base64.decode.invalid", function() return base64.decode("not*valid!") end)
            rec:play("table", function() return { list = { 1, 2 }, name = "x", nested = { flag = true } } end)
            rec:play("nothing", function() end)

            local rep = assert(vcr.cassette({ path = path, mode = "replay" }))
            local function never() error("replay ran the call") end
            local encoded = rep:play("base64.encode.hello", never)
            local decoded, no_err = rep:play("base64.decode.hello", never)
            local failed, err = rep:play("base64.decode.invalid", never)
            local t = rep:play("table", never)
            local shape = `{#t.list}{t.list[2]}{t.name}{tostring(t.nested.flag)}`
            return encoded, decoded, no_err, failed, err.kind, shape, select("#", rep:play("nothing", never))
            "##,
        );
        assert_eq!(
            (encoded.as_str(), decoded.as_str(), kind.as_str()),
            ("aGVsbG8=", "hello", "invalid_byte")
        );
        assert!(no_err.is_nil() && failed.is_nil());
        assert_eq!(table, "22xtrue");
        assert_eq!(nothing, 2, "a result and an error, both nil");
    }

    #[test]
    fn replay_refuses_a_key_twice_and_a_key_it_does_not_have() {
        let (lua, _dir) = engine();
        let _: Value = run(
            &lua,
            r#"
            path = dir .. "/tape.json"
            local rec = assert(vcr.cassette({ path = path, mode = "record" }))
            rec:play("base64.encode.key", function() return base64.encode("x") end)
            rep = assert(vcr.cassette({ path = path, mode = "replay" }))
            rep:play("base64.encode.key", function() return "ignored" end)
            return nil
            "#,
        );
        let twice = raised(&lua, r#"rep:play("base64.encode.key", function() return "ignored" end)"#);
        assert!(
            twice.contains(r#"lib:vcr.Cassette:play: duplicate_key: duplicate key "base64.encode.key""#),
            "{twice}"
        );
        let missing = raised(&lua, r#"rep:play("base64.encode.missing", function() return "ignored" end)"#);
        assert!(
            missing.contains(
                r#"lib:vcr.Cassette:play: missing_key: no cassette entry for key "base64.encode.missing""#
            ),
            "{missing}"
        );
    }

    /// A cassette neoc-vm wrote has the call's arguments too, and may hold more results than two.
    #[test]
    fn a_cassette_of_neoc_vm_replays() {
        let (lua, dir) = engine();
        let old = r#"{
          "base64.decode.hello": { "args": ["aGVsbG8="], "result": ["hello", null] },
          "many": { "args": [], "result": [1, 2, 3] },
          "nulls": { "args": [null], "result": [{ "a": null, "b": [1, null, 3] }] }
        }"#;
        std::fs::write(dir.path().join("old.json"), old).unwrap();
        let (decoded, err, first, second, a, third): (String, Value, i64, i64, Value, i64) = run(
            &lua,
            r#"
            local rep = assert(vcr.cassette({ path = dir .. "/old.json", mode = "replay" }))
            local decoded, err = rep:play("base64.decode.hello", function() return "ignored" end)
            local first, second = rep:play("many", function() return "ignored" end)
            local nulls = rep:play("nulls", function() return "ignored" end)
            return decoded, err, first, second, nulls.a, nulls.b[3]
            "#,
        );
        assert_eq!((decoded.as_str(), first, second, third), ("hello", 1, 2, 3));
        assert!(err.is_nil() && a.is_nil(), "a recorded null is nil");
    }

    // --- opening ---

    #[test]
    fn a_cassette_that_cannot_be_opened_is_a_typed_error() {
        let (lua, dir) = engine();
        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
        std::fs::write(dir.path().join("list.json"), "[1, 2]").unwrap();
        let kinds: String = run(
            &lua,
            r#"
            local out = {}
            for _, config in {
                { mode = "replay", path = dir .. "/nope.json" },
                { mode = "replay", path = dir .. "/broken.json" },
                { mode = "replay", path = dir .. "/list.json" },
                { mode = "replay", path = dir },
                { mode = "record", path = dir .. "/no-dir/tape.json" },
            } do
                local tape, err = vcr.cassette(config)
                assert(tape == nil and err.op == "lib:vcr.cassette" and err.path == config.path, tostring(err))
                table.insert(out, err.kind)
            end
            return table.concat(out, ",")
            "#,
        );
        assert_eq!(kinds, "not_found,malformed,malformed,io,io");
        for (config, reason) in [
            (r#"{ mode = "rewind", path = "x" }"#, r#"`mode` must be one of "record", "replay", "off""#),
            (r#"{ path = "x" }"#, "`mode` must be one of"),
            (r#"{ mode = "record" }"#, "path"),
            (r#"{ mode = "replay", path = 5 }"#, "expected a string"),
            (r#"{ mode = "record", path = "x", strict = true }"#, "unknown field `strict`"),
            (r#""record""#, "expected a table"),
        ] {
            let err = lua
                .load(format!("return vcr.cassette({config})"))
                .eval::<Value>()
                .expect_err(config)
                .to_string();
            assert!(
                err.contains("lib:vcr.cassette: argument #1 'config'") && err.contains(reason),
                "{config}: {err}"
            );
        }
        assert!(!dir.path().join("x").exists());
    }

    // --- off ---

    #[test]
    fn off_runs_the_call_and_nothing_else() {
        let (lua, dir) = engine();
        let (encoded, value, err, again): (String, Value, String, String) = run(
            &lua,
            r#"
            local tape = assert(vcr.cassette({ mode = "off" }))
            local encoded = tape:play("base64.encode.hello", function() return base64.encode("hello") end)
            -- no key is remembered, and nothing has to be plain data
            local again = tape:play("base64.encode.hello", function() return base64.encode("again") end)
            local value, err = tape:play("live", function() return nil, print end)
            assert(err == print)
            -- a path is allowed, and not written
            assert(vcr.cassette({ mode = "off", path = dir .. "/off.json" }))
            return encoded, value, type(err), again
            "#,
        );
        assert_eq!((encoded.as_str(), err.as_str(), again.as_str()), ("aGVsbG8=", "function", "YWdhaW4="));
        assert!(value.is_nil());
        assert!(!dir.path().join("off.json").exists());
        // the same two values as in the other modes
        let err = raised(
            &lua,
            r#"assert(vcr.cassette({ mode = "off" })):play("three", function() return 1, 2, 3 end)"#,
        );
        assert!(err.contains("too_many_results"), "{err}");
    }

    /// `NEOC_VCR=off` makes every cassette an `off` one: nothing is read, written or checked.
    #[test]
    fn the_environment_can_force_every_cassette_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tape.json").to_str().unwrap().to_owned();
        for config in [
            Config::Record { path: path.clone() },
            Config::Replay { path: path.clone() },
            Config::Off { path: None },
        ] {
            assert!(open(config, true).is_ok());
        }
        assert!(!dir.path().join("tape.json").exists(), "a cassette forced off writes nothing");
        // and without it, each is what it says
        assert!(matches!(
            open(Config::Replay { path: path.clone() }, false),
            Err(CassetteError::NotFound { .. })
        ));
        assert!(open(Config::Record { path }, false).is_ok());
        assert!(dir.path().join("tape.json").exists());
        assert!(!forced_off() || std::env::var("NEOC_VCR").as_deref() == Ok("off"));
    }

    #[test]
    fn play_refuses_what_is_not_a_call() {
        let (lua, _dir) = engine();
        lua.load(r#"tape = assert(vcr.cassette({ mode = "off" }))"#).exec().unwrap();
        for (code, place) in [
            (r#"tape:play("key")"#, "lib:vcr.Cassette:play: argument #2 'call'"),
            (r#"tape:play("key", base64.encode("x"))"#, "lib:vcr.Cassette:play: argument #2 'call'"),
            (r#"tape:play(function() end)"#, "lib:vcr.Cassette:play: argument #1 'key'"),
            (
                r#"tape:play("key", function() end, "x")"#,
                "lib:vcr.Cassette:play: takes 2 arguments, got more",
            ),
        ] {
            let err = raised(&lua, code);
            assert!(err.contains(place), "{code}: {err}");
        }
    }
}

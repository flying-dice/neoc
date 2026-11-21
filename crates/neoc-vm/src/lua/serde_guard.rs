//! The check every serde conversion of a script value makes first: a Luau `buffer` is refused.
//!
//! mlua serializes a buffer as bytes, which `serde_json::Value` and the encoders built on it turn into an array of
//! numbers: a script that sent a buffer to a worker, stored one in a `Map` or encoded one as TOML would get a table
//! back, or a file of numbers, without a word. A buffer is refused instead (Decision NEOC-256), and the script
//! converts it itself (`buffer.tostring`) to say what it means.
//!
//! [`contains_buffer`] makes the walk mlua's serializer makes: the value, then every key and value of each table it
//! reaches, raw (no metamethods), each table once (a cycle is the serializer's to report). The walk keeps its own
//! stack, so a deeply nested table cannot overflow the host's.

use std::collections::HashSet;
use std::ffi::c_void;

use mlua::{Table, Value};

/// Why a buffer was refused, and what the script does instead.
pub(crate) const BUFFER_REFUSED: &str =
    "a buffer cannot be serialized; convert it explicitly first (for example with buffer.tostring)";

/// Whether `value` is a buffer, or holds one as a key or a value of any table it reaches.
pub(crate) fn contains_buffer(value: &Value) -> mlua::Result<bool> {
    let mut seen = HashSet::new();
    let mut pending = Vec::new();
    if is_buffer_or_queue(value, &mut seen, &mut pending) {
        return Ok(true);
    }
    while let Some(table) = pending.pop() {
        let mut found = false;
        table.for_each::<Value, Value>(|k, v| {
            found = found
                || is_buffer_or_queue(&k, &mut seen, &mut pending)
                || is_buffer_or_queue(&v, &mut seen, &mut pending);
            Ok(())
        })?;
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `v` is a buffer; a table not seen before is queued for the walk.
fn is_buffer_or_queue(
    v: &Value,
    seen: &mut HashSet<*const c_void>,
    pending: &mut Vec<Table>,
) -> bool {
    match v {
        Value::Buffer(_) => true,
        Value::Table(t) => {
            if seen.insert(t.to_pointer()) {
                pending.push(t.clone());
            }
            false
        }
        _ => false,
    }
}

/// Raise `<prefix>: <BUFFER_REFUSED>` when `value` holds a buffer (see [`contains_buffer`]). `prefix` is the
/// `module.function` of the call the value was passed to.
pub(crate) fn refuse_buffer(value: &Value, prefix: &str) -> mlua::Result<()> {
    if contains_buffer(value)? {
        return Err(mlua::Error::external(format!("{prefix}: {BUFFER_REFUSED}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! The walk itself, then every call site through the script surface. Scripts cannot make a buffer yet (the
    //! sandbox strips `buffer`), so each engine gets one as the global `buf`, made with `create_buffer`.
    use std::sync::Arc;

    use mlua::{Lua, Value};

    use super::{BUFFER_REFUSED, contains_buffer};
    use crate::lua::sandbox::{make_engine, make_engine_in, set_worker_engine};

    fn engine_with_buffer() -> Lua {
        let lua = make_engine().expect("engine");
        let buf = lua.create_buffer(b"ab").expect("buffer");
        lua.globals().set("buf", buf).expect("set buf");
        lua
    }

    fn holds_buffer(lua: &Lua, expr: &str) -> bool {
        let v: Value = lua.load(format!("return {expr}")).eval().expect("value");
        contains_buffer(&v).expect("walk")
    }

    #[test]
    fn finds_a_buffer_anywhere_a_serializer_would_reach() {
        let lua = engine_with_buffer();
        assert!(holds_buffer(&lua, "buf"));
        assert!(holds_buffer(&lua, "{ a = { b = { 1, 2, buf } } }"));
        assert!(holds_buffer(&lua, "{ [buf] = true }"), "a key");
        // raw: an `__index` does not hide or add one
        assert!(holds_buffer(
            &lua,
            "setmetatable({ buf }, { __index = function() error('no') end })"
        ));
        assert!(!holds_buffer(
            &lua,
            "setmetatable({}, { __index = { buf } })"
        ));
    }

    #[test]
    fn no_buffer_is_none_whatever_the_shape() {
        let lua = engine_with_buffer();
        for expr in [
            "nil",
            "1",
            "'ab'",
            "{}",
            "{ 1, 'two', { three = true } }",
            "print",
        ] {
            assert!(!holds_buffer(&lua, expr), "{expr}");
        }
    }

    #[test]
    fn a_cycle_is_walked_once() {
        let lua = engine_with_buffer();
        assert!(!holds_buffer(
            &lua,
            "(function() local t = {} t.self = t t[t] = t return t end)()"
        ));
        assert!(holds_buffer(
            &lua,
            "(function() local t = { x = buf } t.self = t return t end)()"
        ));
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let lua = engine_with_buffer();
        let deep =
            "(function(leaf) local t = { leaf } for _ = 1, 100000 do t = { t } end return t end)";
        assert!(!holds_buffer(&lua, &format!("{deep}(1)")));
        assert!(holds_buffer(&lua, &format!("{deep}(buf)")));
    }

    /// Run `call` (a Lua expression) in a protected call and return its error text; panics when it did not raise.
    fn raised(lua: &Lua, call: &str) -> String {
        let (ok, err): (bool, String) = lua
            .load(format!(
                "local ok, err = pcall(function() return {call} end) return ok, tostring(err)"
            ))
            .eval()
            .expect("chunk");
        assert!(!ok, "{call} must raise");
        err
    }

    fn assert_refused(err: &str, prefix: &str) {
        assert!(
            err.contains(&format!("{prefix}: {BUFFER_REFUSED}")),
            "expected {prefix}, got {err}"
        );
    }

    #[test]
    fn encoders_raise_for_a_buffer() {
        let lua = engine_with_buffer();
        // any userdata: the buffer is refused before the writer is looked at
        let writer = "require('std:io').from_string('')";
        let cases = [
            (
                "require('vnd:serde_json').to_string({ buf })",
                "serde_json.to_string",
            ),
            (
                "require('vnd:serde_json').to_string_pretty(buf)",
                "serde_json.to_string_pretty",
            ),
            (
                &format!("require('vnd:serde_json').to_writer({{ b = buf }}, {writer})"),
                "serde_json.to_writer",
            ),
            ("require('vnd:toml').encode({ b = buf })", "toml.encode"),
            (
                "require('vnd:toml').encode_pretty({ b = buf })",
                "toml.encode_pretty",
            ),
            (
                &format!("require('vnd:toml').encode_to({{ b = buf }}, {writer})"),
                "toml.encode_to",
            ),
            (
                "require('vnd:serde_yaml').encode({ b = buf })",
                "serde_yaml.encode",
            ),
            (
                &format!("require('vnd:serde_yaml').encode_to({{ b = buf }}, {writer})"),
                "serde_yaml.encode_to",
            ),
            (
                "require('vnd:quick_xml').encode({ b = buf })",
                "quick_xml.encode",
            ),
            (
                &format!("require('vnd:quick_xml').encode_to({{ b = buf }}, {writer})"),
                "quick_xml.encode_to",
            ),
        ];
        for (call, prefix) in cases {
            assert_refused(&raised(&lua, call), prefix);
        }
    }

    #[test]
    fn schema_map_and_template_raise_for_a_buffer() {
        let lua = engine_with_buffer();
        let schema = r#"assert(require('vnd:jsonschema').compile('{"type":"object"}'))"#;
        let cases = [
            (
                format!("{schema}:is_valid({{ b = buf }})"),
                "jsonschema.Schema.is_valid",
            ),
            (
                format!("{schema}:validate({{ b = buf }})"),
                "jsonschema.Schema.validate",
            ),
            (
                "require('std:collections').Map.new():set('k', { buf })".to_owned(),
                "collections.Map.set",
            ),
            (
                "require('lib:template').render('{{ b }}', { b = buf })".to_owned(),
                "template.render",
            ),
            (
                "assert(require('lib:template').compile('{{ b }}')):render({ b = buf })".to_owned(),
                "Template.render",
            ),
        ];
        for (call, prefix) in cases {
            assert_refused(&raised(&lua, &call), prefix);
        }
    }

    #[test]
    fn a_template_filter_returning_a_buffer_fails_the_render() {
        let lua = engine_with_buffer();
        let (out, err): (Option<String>, String) = lua
            .load(
                r#"
                local template = require("lib:template")
                assert(template.add_filter("as_buffer", function() return buf end))
                local out, err = template.render("{{ x | as_buffer }}", { x = 1 })
                return out, tostring(err)
                "#,
            )
            .eval()
            .expect("chunk");
        assert_eq!(out, None);
        assert!(
            err.contains(&format!("return value: {BUFFER_REFUSED}")),
            "{err}"
        );
    }

    #[test]
    fn vcr_record_raises_for_a_buffer_in_the_arguments_or_the_result() {
        let lua = engine_with_buffer();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tape.json");
        lua.globals()
            .set("path", path.to_str().expect("utf-8 path"))
            .expect("set path");
        lua.load(
            r#"
            local tape = require("lib:vcr").cassette({ path = path, mode = "record" })
            called = false
            echo = tape:wrap("echo", function(x) called = true return x end)
            give = tape:wrap("give", function() return buf end)
            "#,
        )
        .exec()
        .expect("cassette");
        assert_refused(&raised(&lua, "echo(buf)"), "vcr.record");
        let called: bool = lua.globals().get("called").expect("called");
        assert!(
            !called,
            "the arguments are checked before the wrapped function runs"
        );
        assert_refused(&raised(&lua, "give()"), "vcr.record");
        let tape = std::fs::read_to_string(&path).expect("tape");
        assert_eq!(tape.trim(), "{}", "nothing was recorded");
    }

    #[test]
    fn a_log_field_holding_a_buffer_is_a_placeholder_not_numbers() {
        let lua = engine_with_buffer();
        let lines = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        crate::lua::lib::log::set_log_sink(
            &lua,
            Arc::new(move |_, line| sink.lock().push(line.to_owned())),
        );
        lua.load(r#"require("lib:log").info("bytes", { b = buf })"#)
            .exec()
            .expect("log");
        let lines = lines.lock();
        assert!(lines[0].contains(r#""b":"<unsupported>""#), "{lines:?}");
    }

    /// An engine whose `std:thread` workers also get `buf`, so a worker can try to send one back.
    fn engine_whose_workers_have_a_buffer() -> Lua {
        let lua = engine_with_buffer();
        set_worker_engine(
            &lua,
            Arc::new(|life| {
                let worker = make_engine_in(life)?;
                worker.globals().set("buf", worker.create_buffer(b"ab")?)?;
                Ok(worker)
            }),
        );
        lua
    }

    #[test]
    fn thread_transfers_refuse_a_buffer() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            let lua = engine_whose_workers_have_a_buffer();
            let (spawn, send, from_worker, returned): (String, String, String, String) = lua
                .load(
                    r#"
                    local thread = require("std:thread")
                    local _, spawn = pcall(thread.spawn, { source = "return 1", args = { buf } })

                    local echo = assert(thread.spawn({ source = [[
                        local parent = require("std:thread").parent
                        local ok, err = pcall(parent.send, parent, { buf })
                        parent:send(tostring(err))
                    ]] }))
                    local _, send = pcall(echo.send, echo, buf)
                    local from_worker = echo:receive()
                    echo:await()

                    local returner = assert(thread.spawn({ source = "return { buf }" }))
                    local value, returned = returner:await()
                    assert(value == nil)
                    return tostring(spawn), tostring(send), from_worker, tostring(returned)
                    "#,
                )
                .eval_async()
                .await
                .expect("chunk");
            assert_refused(&spawn, "thread.spawn");
            assert_refused(&send, "thread.send");
            assert_refused(&from_worker, "channel.send");
            assert_refused(&returned, "thread.await");
        });
    }
}

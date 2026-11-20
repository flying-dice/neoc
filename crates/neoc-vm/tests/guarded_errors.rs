#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! An error value whose `__tostring` fails must not abort the process where a script's own code raises it, at the
//! places a script error reaches mlua (NEOC-120): `call_guarded_async` and `call_guarded` (an embedder's call, `neoc
//! run`'s main chunk, a worker, a test body, a request handler, a filter), and a `std:task` task. An ordinary error
//! must read as it did, and each way it does not is pinned here. What is *not* closed is pinned too: a host function
//! that reads a script value through its metamethods from Rust (NEOC-123).
//!
//! On LuaJIT, mlua turns an error value into text with `luaL_tolstring` under a message handler of its own, and a
//! handler that has made a protected call and then fails is run again by its own failure until the C stack is
//! gone. The process aborts, so each test that would take the harness down runs in a child process of this test
//! binary (`isolated`): without the fix the child dies by SIGABRT and the test fails, the others still run.

use std::process::Command;

use futures::StreamExt as _;
use mlua::{Error, Function, Lua, MultiValue, Value};
use neoc_vm::lua::{
    lifecycle::Life,
    sandbox::{call_guarded_async, load_source, make_bare_engine, make_engine, make_engine_in},
    script_env,
    std::task::Scheduler,
};

mod common;

/// A value whose `__tostring` makes a protected call and then raises another such value: what aborts the process.
const HOSTILE: &str = r#"local mt = {}
mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
local function hostile() error(setmetatable({}, mt)) end"#;

/// Run `body` in a child process of this test binary, the one test `name` over again, and fail unless the child
/// ran exactly that test to the end (an abort, a stack overflow or a failed assertion all end it otherwise; a
/// misspelt `name` runs nothing). The body runs on a thread with a small stack, so that a runaway recursion
/// cannot hide in a big one.
fn isolated(name: &str, body: impl FnOnce() + Send + 'static) {
    if std::env::var("NEOC_ISOLATED_TEST").as_deref() == Ok(name) {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(body)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("NEOC_ISOLATED_TEST", name)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{name} did not finish ({}):\n{stdout}{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A multi-thread runtime, as the host's tasks run on: its workers have a 2 MiB stack.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn text(error: &Error) -> String {
    match error {
        Error::RuntimeError(text) => text.clone(),
        other => other.to_string(),
    }
}

// ---- no error value aborts the process ---------------------------------------------------------------------------

/// An embedder's async call of script code, on a full engine and on a bare one with a script environment.
#[test]
fn an_embedders_async_call_survives_an_error_value_whose_tostring_fails() {
    isolated(
        "an_embedders_async_call_survives_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                for body in [
                    "hostile()",
                    "require('std:task').wait(0) hostile()",
                    "local ok = pcall(hostile) hostile()",
                ] {
                    let f = load_source(&lua, format!("{HOSTILE}\n{body}"))
                        .set_name("=hostile")
                        .into_function()
                        .unwrap();
                    let e = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
                    let text = text(&e);
                    assert!(
                        text.starts_with("error object's __tostring failed"),
                        "{body}: {text}"
                    );
                    assert!(
                        text.contains("stack traceback:\n\t[C]: in function 'error'\n\thostile:"),
                        "{body}: the traceback says where it happened:\n{text}"
                    );
                }

                // an engine whose script runs in an environment of its own, as an embedder's does
                let lua = make_bare_engine().unwrap();
                let env = script_env::new_env(&lua).unwrap();
                script_env::freeze(&lua, &env).unwrap();
                let f = load_source(&lua, format!("{HOSTILE}\nhostile()"))
                    .set_environment(env)
                    .into_function()
                    .unwrap();
                let e = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
                assert!(text(&e).starts_with("error object's __tostring failed"));
            });
        },
    );
}

/// The error of a value whose `__tostring` fails *once* is its text; and the first failure is what is reported.
#[test]
fn the_text_of_the_error_is_the_text_of_its_tostring_and_a_failing_one_says_why() {
    isolated(
        "the_text_of_the_error_is_the_text_of_its_tostring_and_a_failing_one_says_why",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let source = r#"
                    local cases = {
                      fine = function() error(setmetatable({}, { __tostring = function() return "custom" end })) end,
                      fails = function() error(setmetatable({}, { __tostring = function() error("nope", 0) end })) end,
                      not_a_string = function() error(setmetatable({}, { __tostring = function() return {} end })) end,
                      hidden = function() error(setmetatable({}, { __metatable = "hidden", __tostring = function() error("hidden", 0) end })) end,
                    }
                    return cases"#;
                let cases: mlua::Table = load_source(&lua, source).eval().unwrap();
                let run = |name: &'static str| {
                    let lua = lua.clone();
                    let f: Function = cases.get(name).unwrap();
                    async move { call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err() }
                };
                let fine = text(&run("fine").await);
                assert!(fine.starts_with("custom\nstack traceback:"), "{fine}");
                let fails = text(&run("fails").await);
                assert!(
                    fails.starts_with("error object's __tostring failed: nope\nstack traceback:"),
                    "{fails}"
                );
                let not_a_string = text(&run("not_a_string").await);
                // what `luaL_tolstring` says, which is the text of the error
                assert!(
                    not_a_string.starts_with("'__tostring' must return a string\nstack traceback:"),
                    "{not_a_string}"
                );
                let hidden = text(&run("hidden").await);
                assert!(
                    hidden.starts_with("error object's __tostring failed: hidden"),
                    "{hidden}"
                );
            });
        },
    );
}

/// A task that fails: started, deferred or delayed, failing at once or after a wait, given as a function or as a
/// coroutine of the script's own. Awaited or not.
#[test]
fn a_failing_task_survives_an_error_value_whose_tostring_fails() {
    isolated(
        "a_failing_task_survives_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let source = format!(
                    r#"{HOSTILE}
                    local task = require("std:task")
                    local out = {{}}
                    local function show(name, t)
                      -- a failed task answers `nil, error` from `await`
                      local ok, v, e = pcall(function() return t:await() end)
                      out[name] = tostring(ok) .. ":" .. tostring(v) .. ":" .. tostring(e)
                    end
                    show("spawn", task.spawn(function() hostile() end))
                    show("spawn after wait", task.spawn(function() task.wait(0) hostile() end))
                    show("spawn after yield", task.spawn(function() coroutine.yield() hostile() end))
                    show("defer", task.defer(function() hostile() end))
                    show("delay", task.delay(0.01, function() hostile() end))
                    show("a coroutine", task.spawn(coroutine.create(function() task.wait(0) hostile() end)))
                    show("a coroutine at once", task.spawn(coroutine.create(function() hostile() end)))
                    -- never awaited
                    task.spawn(function() task.wait(0) hostile() end)
                    task.wait(0.05)
                    return out"#
                );
                let f = load_source(&lua, source).into_function().unwrap();
                let out: mlua::Table = call_guarded_async(&lua, &f, ()).await.unwrap();
                for (name, expected_traceback) in [
                    ("spawn", true),
                    ("spawn after wait", true),
                    ("spawn after yield", true),
                    ("defer", true),
                    ("delay", true),
                    // a coroutine the script made is driven from a thread of the host's, and its stack is gone
                    // by the time the error leaves it
                    ("a coroutine", false),
                    ("a coroutine at once", false),
                ] {
                    let got: String = out.get(name).unwrap();
                    assert!(
                        got.contains("error object's __tostring failed"),
                        "{name}: {got}"
                    );
                    assert_eq!(
                        got.contains("stack traceback:\n\t[C]: in function 'error'"),
                        expected_traceback,
                        "{name}: {got}"
                    );
                }
                // the task nobody awaited is reported when the script is over
                let scheduler = lua.app_data_ref::<Scheduler>().unwrap();
                scheduler.wait_all().await;
                assert!(scheduler.had_task_errors());
            });
        },
    );
}

/// A `std:thread` worker whose script fails.
#[test]
fn a_failing_worker_survives_an_error_value_whose_tostring_fails() {
    isolated(
        "a_failing_worker_survives_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let source = format!(
                    r#"local thread = require("std:thread")
                    local out = {{}}
                    for name, body in pairs {{
                      plain = [[{HOSTILE} hostile()]],
                      after_a_wait = [[{HOSTILE} require("std:task").wait(0) hostile()]],
                    }} do
                      local w = assert(thread.spawn {{ source = body }})
                      local v, e = w:await()
                      out[name] = tostring(v) .. ":" .. tostring(e)
                    end
                    return out"#
                );
                let f = load_source(&lua, source).into_function().unwrap();
                let out: mlua::Table = call_guarded_async(&lua, &f, ()).await.unwrap();
                for name in ["plain", "after_a_wait"] {
                    let got: String = out.get(name).unwrap();
                    assert!(
                        got.starts_with(
                            "nil:thread.await: runtime error: error object's __tostring failed"
                        ),
                        "{name}: {got}"
                    );
                }
            });
        },
    );
}

/// A test body of `lib:test` that fails, and a request handler of a script's own server.
#[test]
fn a_failing_test_body_and_request_handler_survive_an_error_value_whose_tostring_fails() {
    isolated(
        "a_failing_test_body_and_request_handler_survive_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let addr = common::free_addr();
                let source = format!(
                    r#"{HOSTILE}
                    local t = require("lib:test")
                    t.describe("suite", function()
                      t.test("fails", function() hostile() end)
                      t.test("passes", function() end)
                    end)
                    local listener = require("std:net").TcpListener.bind({addr:?})
                    _G.server = require("vnd:hyper").serve(listener, function() hostile() end)"#
                );
                let f = load_source(&lua, source).into_function().unwrap();
                call_guarded_async::<()>(&lua, &f, ()).await.unwrap();
                let (passed, failed) = neoc_vm::lua::lib::test::summary(&lua).unwrap();
                assert_eq!((passed, failed), (1, 1));
                let response = common::http_get(&addr, "/").await.unwrap();
                assert!(response.starts_with("HTTP/1.1 500"), "{response}");
            });
        },
    );
}

// ---- ordinary errors are as they were ----------------------------------------------------------------------------

/// What a plain, unguarded call of `function` gives: the answer mlua gave before.
async fn native(function: &Function) -> mlua::Result<MultiValue> {
    function.call_async(()).await
}

/// What a plain, unguarded task thread gave: an `into_async` of a thread over `function`.
async fn native_thread(lua: &Lua, function: &Function) -> mlua::Result<MultiValue> {
    let mut stream = lua
        .create_thread(function.clone())
        .unwrap()
        .into_async::<MultiValue>(())
        .unwrap();
    let mut last = Ok(MultiValue::new());
    while let Some(item) = stream.next().await {
        last = item;
    }
    last
}

fn with_pointers_hidden(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at + 2]);
        out.push('P');
        rest = rest[at + 2..].trim_start_matches(|c: char| c.is_ascii_hexdigit());
    }
    out.push_str(rest);
    out
}

/// The scripts of the comparison: each raises an error that mlua can show without aborting, on its own thread.
const ORDINARY: &[(&str, &str)] = &[
    (
        "string",
        "local function f() error('boom') end local function g() f() end g()",
    ),
    ("string at level 0", "error('boom', 0)"),
    ("string with a NUL", "error('with\\0nul', 0)"),
    (
        "string at level 2",
        "local function f() error('boom', 2) end f()",
    ),
    ("number", "error(42)"),
    ("nil", "error()"),
    ("boolean", "error(false)"),
    ("table", "error({ code = 1 })"),
    (
        "table with a working __tostring",
        "error(setmetatable({}, { __tostring = function() return 'custom' end }))",
    ),
    (
        "table with a __tostring that calls pcall",
        "error(setmetatable({}, { __tostring = function() pcall(error, 'x') return 'after pcall' end }))",
    ),
    (
        "table whose __tostring returns a number",
        "error(setmetatable({}, { __tostring = function() return 42 end }), 0)",
    ),
    (
        "table whose __tostring returns a float",
        "error(setmetatable({}, { __tostring = function() return 4.5 end }), 0)",
    ),
    (
        "table with an empty metatable",
        "error(setmetatable({}, {}), 0)",
    ),
    ("a builtin function", "error(print, 0)"),
    ("a Lua function", "error(function() end, 0)"),
    ("a coroutine", "error(coroutine.create(function() end), 0)"),
    (
        "a frame name that is not UTF-8",
        "local t = {} t['\\xff\\xfe'] = function() error('boom') end local function g() t['\\xff\\xfe']() end g()",
    ),
    (
        "table with a __name",
        "error(setmetatable({}, { __name = 'Named' }))",
    ),
    ("assert", "assert(false, 'assert message')"),
    ("assert without a message", "assert(false)"),
    ("index nil", "local x = nil; x.y = 1"),
    ("call nil", "local x = nil; x()"),
    ("arithmetic on nil", "local x = nil; return x + 1"),
    ("a builtin", "string.rep()"),
    ("a host function", "require('std:task').spawn(5)"),
    (
        "a host function in a callback",
        "local f = function() require('std:task').spawn(5) end f()",
    ),
    ("require", "require('std:nonexistent')"),
    ("after a wait", "require('std:task').wait(0) error('late')"),
    (
        "in a coroutine.wrap",
        "coroutine.wrap(function() error('inner') end)()",
    ),
    (
        "in a coroutine.wrap, a host error",
        "coroutine.wrap(function() require('std:task').spawn(5) end)()",
    ),
    (
        "rethrown",
        "local ok, e = pcall(error, 'first') error(e, 0)",
    ),
    (
        "in a metamethod",
        "local t = setmetatable({}, { __index = function() error('meta') end }) return t.x",
    ),
    (
        "in a pcall's handler",
        "xpcall(function() error('a') end, function(e) error('b') end) error('after')",
    ),
];

/// `ORDINARY`, and an error at each of the stack depths around where a traceback leaves its middle out.
fn ordinary_cases() -> Vec<(String, String)> {
    let mut cases: Vec<(String, String)> = ORDINARY
        .iter()
        .map(|(name, body)| ((*name).to_owned(), (*body).to_owned()))
        .collect();
    for depth in (0..=30).chain([40, 400, 5000]) {
        cases.push((
            format!("an error at depth {depth}"),
            format!("local function d(n) if n == 0 then error('deep') end local r = d(n - 1) return r end d({depth})"),
        ));
    }
    cases
}

/// An ordinary error reads exactly as it did without the guard: its text (traceback included), and what an
/// embedder can look into.
#[test]
fn an_ordinary_error_reads_as_it_did_without_the_guard() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let cases = ordinary_cases();
        for (name, body) in &cases {
            let f = load_source(&lua, body.as_str())
                .set_name("=case")
                .into_function()
                .unwrap();
            let before = native(&f).await.unwrap_err();
            let after = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
            assert_eq!(
                with_pointers_hidden(&before.to_string()),
                with_pointers_hidden(&after.to_string()),
                "{name}: the same text"
            );
            assert_eq!(
                std::mem::discriminant(&before),
                std::mem::discriminant(&after),
                "{name}: the same kind of error"
            );
        }
    });
}

/// A task fails with the text it had, deep stacks included.
#[test]
fn an_ordinary_task_failure_reads_as_it_did_without_the_guard() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        for (name, body) in ordinary_cases().iter().filter(|(name, _)| !name.contains("wait")) {
            let source = format!("return function() {body} end");
            let f: Function = load_source(&lua, source.as_str())
                .set_name("=case")
                .eval()
                .unwrap();
            let before = native_thread(&lua, &f).await.unwrap_err();
            let task: Function = load_source(
                &lua,
                "local f = ... return function() local t = require('std:task').spawn(f) local _, _, e = pcall(function() return t:await() end) return e end",
            )
            .call(f.clone())
            .unwrap();
            let after: String = task.call_async(()).await.unwrap();
            assert_eq!(
                with_pointers_hidden(&before.to_string()),
                with_pointers_hidden(&after),
                "{name}: the same text"
            );
        }
    });
}

/// What a script sees of an error it catches itself, and what it gets back, does not change.
#[test]
fn what_a_script_catches_and_returns_is_as_it_was() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let source = r##"
            local r = {}
            local mt = { __tostring = function() return "custom" end }
            local obj = setmetatable({}, mt)
            local ok, e = pcall(error, obj)
            r[#r + 1] = tostring(ok) .. " " .. tostring(rawequal(e, obj)) .. " " .. tostring(e)
            r[#r + 1] = tostring(select("#", pcall(error)))
            r[#r + 1] = tostring(select(2, pcall(error, "x", 0)))
            local co = coroutine.create(function() error(obj) end)
            local ok2, e2 = coroutine.resume(co)
            r[#r + 1] = tostring(ok2) .. " " .. tostring(rawequal(e2, obj))
            local ok3, e3 = pcall(coroutine.wrap(function() error(obj) end))
            r[#r + 1] = tostring(ok3) .. " " .. tostring(rawequal(e3, obj))
            r[#r + 1] = tostring(select(2, xpcall(function() error("a") end, function(m) return "handled " .. m end)))
            local t = require("std:task").spawn(function() require("std:task").wait(0) return 1, nil, "three" end)
            r[#r + 1] = table.concat({ tostring(select("#", t:await())), tostring((t:await())) }, ",")
            return table.concat(r, "\n")"##;
        let f = load_source(&lua, source).set_name("=case").into_function().unwrap();
        let before: String = f.call_async(()).await.unwrap();
        let after: String = call_guarded_async(&lua, &f, ()).await.unwrap();
        assert_eq!(before, after);
        assert!(before.starts_with("false true custom\n"), "{before}");
        assert!(before.ends_with("3,1"), "{before}");
    });
}

/// A host function that fails with a Rust error of its own is still that error to whoever called the script, and a
/// panic in a host function is still a panic.
#[test]
fn a_rust_error_is_handed_on_to_be_downcast() {
    #[derive(Debug)]
    struct Mine(u32);
    impl std::fmt::Display for Mine {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mine {}", self.0)
        }
    }
    impl std::error::Error for Mine {}

    runtime().block_on(async {
        let lua = make_engine().unwrap();
        lua.globals()
            .set(
                "fail",
                lua.create_function(|_, ()| -> mlua::Result<()> { Err(Error::external(Mine(7))) })
                    .unwrap(),
            )
            .unwrap();
        let f = load_source(&lua, "local function f() fail() end f()")
            .set_name("=case")
            .into_function()
            .unwrap();
        let mine = |e: &Error| match e {
            Error::CallbackError { cause, .. } => match &**cause {
                Error::ExternalError(external) => external.downcast_ref::<Mine>().map(|m| m.0),
                _ => None,
            },
            _ => None,
        };
        let before = native(&f).await.unwrap_err();
        let after = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
        assert_eq!(mine(&before), Some(7));
        assert_eq!(mine(&after), Some(7));
        assert_eq!(before.to_string(), after.to_string());

        // a task: the text of that failure, as before
        let task: Function = load_source(
            &lua,
            "return function() local t = require('std:task').spawn(function() fail() end) return select(3, pcall(function() return t:await() end)) end",
        )
        .eval()
        .unwrap();
        let in_task: String = task.call_async(()).await.unwrap();
        assert!(in_task.contains("mine 7\nstack traceback:\n\t[C]: in function 'fail'"), "{in_task}");
    });
}

/// A host function that panics panics the caller of the script, guarded or not: the guard does not swallow it.
#[test]
fn a_panic_in_a_host_function_is_still_a_panic() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        lua.globals()
            .set(
                "boom",
                lua.create_function(|_, ()| -> mlua::Result<()> { panic!("host bug") })
                    .unwrap(),
            )
            .unwrap();
        let f = load_source(&lua, "boom()").into_function().unwrap();
        let panicked =
            |what: &str, result: Result<Result<(), Error>, Box<dyn std::any::Any + Send>>| {
                let message = result.expect_err(what);
                let message = message
                    .downcast_ref::<&str>()
                    .map(|m| (*m).to_owned())
                    .or_else(|| message.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                assert_eq!(message, "host bug", "{what}");
            };
        let before = std::panic::AssertUnwindSafe(f.call_async::<()>(()));
        panicked("native", futures::FutureExt::catch_unwind(before).await);
        let after = std::panic::AssertUnwindSafe(call_guarded_async::<()>(&lua, &f, ()));
        panicked("guarded", futures::FutureExt::catch_unwind(after).await);
    });
}

/// What a guarded call returns, and what it is given.
#[test]
fn a_guarded_call_passes_arguments_and_results_through() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f: Function = load_source(
            &lua,
            "return function(...) require('std:task').wait(0) return select('#', ...), ... end",
        )
        .eval()
        .unwrap();
        let got: (i64, Value, String, Value) = call_guarded_async(&lua, &f, (Value::Nil, "two", 3))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(got.0, 3);
        assert_eq!(got.2, "two");
        let none: MultiValue = call_guarded_async(
            &lua,
            &load_source(&lua, "return").into_function().unwrap(),
            (),
        )
        .await
        .unwrap();
        assert!(none.is_empty());
    });
}

/// `Mutex:with_lock` runs a script function from Rust too.
#[test]
fn a_failing_with_lock_callback_survives_an_error_value_whose_tostring_fails() {
    isolated(
        "a_failing_with_lock_callback_survives_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let source = format!(
                    r#"{HOSTILE}
                    local mutex = require("std:collections").Mutex.new()
                    local ok, e = pcall(function() mutex:with_lock(hostile) end)
                    -- the lock was released
                    local again = mutex:with_lock(function() return "again" end)
                    return tostring(ok) .. ":" .. tostring(e) .. ":" .. again"#
                );
                let f = load_source(&lua, source).into_function().unwrap();
                let out: String = call_guarded_async(&lua, &f, ()).await.unwrap();
                assert!(
                    out.starts_with("false:") && out.contains("error object's __tostring failed"),
                    "{out}"
                );
                assert!(out.ends_with(":again"), "{out}");
            });
        },
    );
}

/// A stack overflow is an ordinary error: the same message and kind, though the handler has no stack left to take a
/// traceback with.
#[test]
fn a_stack_overflow_keeps_its_message_and_kind() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f = load_source(&lua, "local function f() return 1 + f() end f()")
            .set_name("=overflow")
            .into_function()
            .unwrap();
        let before = native(&f).await.unwrap_err();
        let after = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
        assert_eq!(
            before.to_string().lines().next(),
            after.to_string().lines().next()
        );
        assert_eq!(
            std::mem::discriminant(&before),
            std::mem::discriminant(&after)
        );
        // in a task too
        let task: Function = load_source(
            &lua,
            "return function() local t = require('std:task').spawn(function() local function f() return 1 + f() end f() end) return select(3, pcall(function() return t:await() end)) end",
        )
        .eval()
        .unwrap();
        let in_task: String = task.call_async(()).await.unwrap();
        assert!(in_task.starts_with("runtime error: stack overflow"), "{in_task}");
    });
}

/// A coroutine of the script's own given to `task.spawn` is driven from a thread of the host's: it still runs its
/// plain yields, its arguments and a start it already made, and it ends when it is cancelled or its script is shut
/// down while it waits.
#[test]
fn a_coroutine_given_to_a_task_runs_yields_arguments_and_stops() {
    runtime().block_on(async {
        let life = Life::new();
        let lua = make_engine_in(life.clone()).unwrap();
        let source = r#"
            local task = require("std:task")
            local out = {}
            -- plain yields, arguments, a result after a wait
            local co = coroutine.create(function(a)
              local x = coroutine.yield(a)
              task.wait(0)
              return a, tostring(x), "done"
            end)
            out[#out + 1] = table.concat({ task.spawn(co, 5):await() }, ",")
            -- one the script already started
            local half = coroutine.create(function(a) local b = coroutine.yield(a + 1) task.wait(0) return a, b end)
            coroutine.resume(half, 10)
            out[#out + 1] = table.concat({ task.spawn(half):await() }, ",")
            -- cancelled while it waits
            local parked = coroutine.create(function() task.wait(60) end)
            local t = task.spawn(parked)
            out[#out + 1] = tostring(task.cancel(t)) .. " " .. tostring(t:is_finished())
            -- left waiting for the shutdown
            task.spawn(coroutine.create(function() task.wait(60) end))
            return table.concat(out, "\n")"#;
        let f = load_source(&lua, source).into_function().unwrap();
        let out: String = call_guarded_async(&lua, &f, ()).await.unwrap();
        assert_eq!(out, "5,nil,done\n10\ntrue true");
        life.shutdown();
        let scheduler = lua.app_data_ref::<Scheduler>().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), scheduler.wait_all())
            .await
            .expect("the tasks ended with the script");
    });
}

/// The callbacks a module runs synchronously on a script's behalf (a `lib:template` filter, a `lib:vcr` wrapped
/// function), and a `lib:test` suite body that fails (not only its tests).
#[test]
fn a_failing_filter_wrapped_function_and_suite_body_survive_an_error_value_whose_tostring_fails() {
    isolated(
        "a_failing_filter_wrapped_function_and_suite_body_survive_an_error_value_whose_tostring_fails",
        || {
            runtime().block_on(async {
                let lua = make_engine().unwrap();
                let path = std::env::temp_dir().join(format!("neoc-guarded-vcr-{}.json", std::process::id()));
                let source = format!(
                    r#"{HOSTILE}
                    local out = {{}}
                    local template = require("lib:template")
                    template.add_filter("boom", function() hostile() end)
                    local rendered, e = template.render("{{{{ x | boom }}}}", {{ x = 1 }})
                    out[1] = tostring(rendered) .. ":" .. tostring(e)
                    local vcr = require("lib:vcr").cassette {{ path = {path:?}, mode = "record" }}
                    local ok, e2 = pcall(vcr:wrap("boom", function() hostile() end))
                    out[2] = tostring(ok) .. ":" .. tostring(e2)
                    local t = require("lib:test")
                    local ok3, e3 = pcall(function() t.describe("suite", function() hostile() end) end)
                    out[3] = tostring(ok3) .. ":" .. tostring(e3)
                    return out"#
                );
                let f = load_source(&lua, source).into_function().unwrap();
                let out: mlua::Table = call_guarded_async(&lua, &f, ()).await.unwrap();
                let _ = std::fs::remove_file(&path);
                for index in 1..=3 {
                    let got: String = out.get(index).unwrap();
                    // the synchronous guard (`call_guarded`) says `<error object>`, the async one says why
                    assert!(got.contains("error object"), "{index}: {got}");
                }
            });
        },
    );
}

/// An engine that was not built by neoc-vm has nothing to guard with: the call says so.
#[test]
fn an_engine_not_built_by_neoc_vm_is_refused_with_a_message() {
    runtime().block_on(async {
        let lua = Lua::new();
        let f = lua.load("return 1").into_function().unwrap();
        let e = call_guarded_async::<i64>(&lua, &f, ()).await.unwrap_err();
        assert!(text(&e).contains("not built by neoc-vm"), "{e}");
    });
}

/// A coroutine of the script's own that fails with an ordinary error reports just its message; a table with no
/// `__tostring` or `__name` is named by its type.
#[test]
fn a_failing_coroutine_task_reports_its_message_and_a_bare_table_its_type() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let source = r#"
            local task = require("std:task")
            local function fails(f)
              local _, _, e = pcall(function() return task.spawn(coroutine.create(f)):await() end)
              return (tostring(e):gsub("0x%x+", "0xP"))
            end
            return table.concat({
              fails(function() task.wait(0) error("plain", 0) end),
              fails(function() error(42, 0) end),
              fails(function() error(setmetatable({}, {}), 0) end),
              fails(function() error(setmetatable({}, { __tostring = function() return {} end }), 0) end),
            }, "|")"#;
        let f = load_source(&lua, source).into_function().unwrap();
        let out: String = call_guarded_async(&lua, &f, ()).await.unwrap();
        let parts: Vec<&str> = out.split('|').collect();
        assert_eq!(parts[0], "runtime error: plain");
        assert_eq!(parts[1], "runtime error: 42");
        assert_eq!(parts[2], "runtime error: table: 0xP");
        assert_eq!(parts[3], "runtime error: '__tostring' must return a string");
    });
}

// ---- ordinary errors that used to differ, and the differences that remain ---------------------------------------

/// An out-of-memory error under `Lua::set_memory_limit` is a `MemoryError` as it is from a plain call: an embedder that
/// downcasts it to see a limit breach still sees it. A script's own `error("not enough memory")` is a script error.
#[test]
fn an_out_of_memory_error_is_a_memory_error_as_it_was() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let hog = load_source(
            &lua,
            "local t = {} local i = 0 while true do i = i + 1 t[i] = { i } end",
        )
        .into_function()
        .unwrap();
        lua.set_memory_limit(lua.used_memory() + 64 * 1024 * 1024)
            .unwrap();
        let before = native(&hog).await.unwrap_err();
        lua.gc_collect().unwrap();
        lua.set_memory_limit(lua.used_memory() + 64 * 1024 * 1024)
            .unwrap();
        let after = call_guarded_async::<()>(&lua, &hog, ()).await.unwrap_err();
        assert!(matches!(&before, Error::MemoryError(_)), "{before:?}");
        assert!(matches!(&after, Error::MemoryError(_)), "{after:?}");
        assert_eq!(before.to_string(), after.to_string());

        lua.gc_collect().unwrap();
        lua.set_memory_limit(0).unwrap();
        let own = load_source(&lua, "error('not enough memory', 0)")
            .into_function()
            .unwrap();
        let own = call_guarded_async::<()>(&lua, &own, ()).await.unwrap_err();
        assert!(matches!(&own, Error::RuntimeError(_)), "{own:?}");
    });
}

/// The first line of an error text: what it says, before its traceback.
fn first_line(error: &Error) -> String {
    with_pointers_hidden(error.to_string().lines().next().unwrap_or_default())
}

/// A host userdata raised as an error value is named by its type, or told by its own `__tostring`, as before; it has
/// no traceback (the message handler is not run for a userdata: a Rust error is one, and looking at a panic raises
/// it).
#[test]
fn a_host_userdata_as_an_error_value_is_named_as_before_without_its_traceback() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        for body in [
            "error(require('std:task').spawn(function() end), 0)",
            "error(require('std:collections').Mutex.new(), 0)",
            "error(require('std:time').Duration.from_secs(3), 0)",
        ] {
            let f = load_source(&lua, body)
                .set_name("=case")
                .into_function()
                .unwrap();
            let before = native(&f).await.unwrap_err();
            let after = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
            assert_eq!(first_line(&before), first_line(&after), "{body}");
            assert!(before.to_string().contains("stack traceback"), "{body}");
            assert!(!after.to_string().contains("stack traceback"), "{body}");
        }
    });
}

/// A `__tostring` that returns something that is not text fails the conversion with `luaL_tolstring`'s own words.
#[test]
fn a_tostring_that_returns_no_text_says_what_lua_l_tolstring_says() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        for value in ["nil", "true", "{}", "print"] {
            let f = load_source(
                &lua,
                format!(
                    "error(setmetatable({{}}, {{ __tostring = function() return {value} end }}), 0)"
                ),
            )
            .into_function()
            .unwrap();
            let before = native(&f).await.unwrap_err();
            let after = call_guarded_async::<()>(&lua, &f, ()).await.unwrap_err();
            assert_eq!(
                first_line(&before),
                "runtime error: '__tostring' must return a string"
            );
            assert_eq!(first_line(&before), first_line(&after), "{value}");
        }
    });
}

/// A script that catches a failed host function itself and prints it sees one more frame at the foot of the
/// traceback, the base of the thread it runs in: a difference of this design (the text is made inside the thread).
#[test]
fn a_script_that_catches_a_host_error_sees_the_base_frame_of_its_thread() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f = load_source(
            &lua,
            "local ok, e = pcall(require('std:task').spawn, 5) return tostring(e)",
        )
        .set_name("=case")
        .into_function()
        .unwrap();
        let before: String = f.call_async(()).await.unwrap();
        let after: String = call_guarded_async(&lua, &f, ()).await.unwrap();
        assert!(!before.trim_end().ends_with("[C]: in ?"), "{before}");
        assert_eq!(
            after.trim_end(),
            format!("{}\n\t[C]: in ?", before.trim_end()),
            "{after}"
        );
    });
}

/// A script's recursion limit is two frames lower in a guarded thread: the frames the thread body takes.
#[test]
fn the_recursion_limit_is_two_frames_lower() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f = load_source(
            &lua,
            "local n = 0 local function f() n = n + 1 return 1 + f() end pcall(f) return n",
        )
        .into_function()
        .unwrap();
        let before: i64 = f.call_async(()).await.unwrap();
        let after: i64 = call_guarded_async(&lua, &f, ()).await.unwrap();
        assert_eq!(before - after, 2, "{before} and {after}");
    });
}

/// A script that resumes the thread of its own task (the raw `coroutine` library of `neoc run` allows it) sees the
/// results of the thread body, `xpcall`: a leading `true`, and on failure the handler's answer. A difference of this
/// design: hiding it takes a Lua frame under every guarded thread.
#[test]
fn a_script_that_resumes_its_own_tasks_thread_sees_the_results_of_xpcall() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f = load_source(
            &lua,
            r"local task = require('std:task')
              local th
              task.spawn(function() th = coroutine.running() coroutine.yield() return 'one' end)
              local a, b, c = coroutine.resume(th)
              return tostring(a) .. ' ' .. tostring(b) .. ' ' .. tostring(c)",
        )
        .into_function()
        .unwrap();
        let after: String = call_guarded_async(&lua, &f, ()).await.unwrap();
        assert_eq!(after, "true true one");
    });
}

/// A stack overflow is an ordinary error: the message and the kind are the plain call's. Whether the traceback fits in
/// the stack the failing code left is up to that stack: a traceback that is kept has the top frame's line, as mlua's.
#[test]
fn a_stack_overflow_has_the_message_and_the_top_frame_of_a_plain_call() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f = load_source(&lua, "local function r(n) return 1 + r(n + 1) end r(1)")
            .set_name("=overflow")
            .into_function()
            .unwrap();
        let before = native(&f).await.unwrap_err().to_string();
        let after = call_guarded_async::<()>(&lua, &f, ())
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(before.lines().next(), after.lines().next());
        if after.contains("stack traceback") {
            let top = |text: &str| text.lines().nth(2).map(str::to_owned);
            assert_eq!(top(&before), top(&after));
        }
    });
}

/// The known limit (NEOC-123): a host function that reads a script table through its metamethods, from Rust, is
/// run by mlua under a protected call whose message handler runs the `__tostring` of what the metamethod raises.
/// This guard does not reach it. The test says so: it passes while the process aborts, and fails the day it does
/// not (then the known limit in CLAUDE.md and the module doc of `guard.rs` are to go).
#[test]
fn a_host_read_of_a_script_table_through_its_metamethods_still_aborts_the_known_limit() {
    let name = "a_host_read_of_a_script_table_through_its_metamethods_still_aborts_the_known_limit";
    if std::env::var("NEOC_ISOLATED_TEST").as_deref() == Ok(name) {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                runtime().block_on(async {
                    let lua = make_engine().unwrap();
                    let source = format!(
                        r#"{HOSTILE}
                        local opts = setmetatable({{}}, {{ __index = function() error(setmetatable({{}}, mt)) end }})
                        pcall(require("std:thread").spawn, opts)"#
                    );
                    let f = load_source(&lua, source).into_function().unwrap();
                    let _ = call_guarded_async::<()>(&lua, &f, ()).await;
                });
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("NEOC_ISOLATED_TEST", name)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "the process no longer aborts: the known limit is closed ({})",
        out.status
    );
}

/// A stack overflow, at some depths of the stack it starts from, leaves no room for a traceback: it is reported
/// without one (the handler cannot take it), not lost, and its message is the plain call's.
#[test]
fn a_stack_overflow_that_leaves_no_room_for_a_traceback_is_reported_without_one() {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let f: Function = load_source(
            &lua,
            r"local function overflow() return 1 + overflow() end
              return function(prefix)
                local function down(n) if n == 0 then return overflow() end return 1 + down(n - 1) end
                return down(prefix)
              end",
        )
        .eval()
        .unwrap();
        let mut without_traceback = 0;
        for prefix in 0..300 {
            let e = call_guarded_async::<()>(&lua, &f, prefix).await.unwrap_err();
            let text = e.to_string();
            assert!(text.starts_with("runtime error: stack overflow"), "{prefix}: {text}");
            without_traceback += i32::from(!text.contains("stack traceback"));
        }
        assert!(without_traceback > 0, "an overflow that had room for its traceback every time");
    });
}

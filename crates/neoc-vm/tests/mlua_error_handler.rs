#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! mlua's message handler on LuaJIT (NEOC-124, `vendor/mlua-patches/`).
//!
//! mlua runs every protected call it makes for Rust (`Table::get`, a conversion, `Function::call`, `call_async`,
//! `exec`) under a handler that turns a non-string error value into text with `luaL_tolstring`, which runs its
//! `__tostring`. LuaJIT marks "a handler is running" in `L->status`, which makes a nested error answer "error in
//! error handling", but a `pcall` inside the handler that catches an error clears the mark, and the next error
//! is handed to the same handler again, until the C stack is gone and the process aborts. A script can make a
//! host function raise such a value by giving it an options table whose `__index` does.
//!
//! The first group below runs each way to reach the handler in a child process (a regression aborts the whole
//! process, which a test cannot survive): it must end with an error value or a result, never an abort. The
//! second group holds ordinary errors to what they were: text, traceback, `__tostring`, a Rust error out of
//! a callback, a memory error.

use std::{
    fmt,
    process::{Command, Stdio},
};

use mlua::{Error, Function, Table, Value};
use neoc_vm::lua::sandbox::make_engine;

/// The error value the handler must not choke on: its `__tostring` makes a protected call that catches an
/// error (which clears LuaJIT's mark that a handler is running) and then raises another such value.
const HOSTILE: &str = r#"
local mt = {}
mt.__tostring = function()
  pcall(error, "x")
  error(setmetatable({}, mt))
end
local function hostile() return setmetatable({}, mt) end
-- an options table a host function reads: reading any field raises the hostile value
local function hostile_options()
  return setmetatable({}, { __index = function() error(hostile()) end })
end
"#;

const CASE_VAR: &str = "NEOC_MLUA_HANDLER_CASE";
const HANDLED: &str = "error in error handling";

// ---- the child-process harness ---------------------------------------------------------------------------------

/// Runs one case of `child` in a fresh process of this test binary and requires it to finish (an abort is
/// a failed test, not a dead test binary).
fn in_child(case: &str) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env(CASE_VAR, case)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success() && stdout.contains(&format!("case {case} ok")),
        "case {case}: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        out.status
    );
}

/// The child: with `CASE_VAR` set it runs that case (on a 2 MiB thread, the stack of a default runtime worker, so
/// a deep recursion cannot hide in a roomy one); without it, as in a plain `cargo test`, it does nothing.
#[test]
fn child() {
    let Ok(case) = std::env::var(CASE_VAR) else {
        return;
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        match case.as_str() {
            "main_chunk_after_task_wait" => case_main_chunk_after_task_wait().await,
            "host_function_reads_hostile_options" => {
                case_host_function_reads_hostile_options().await
            }
            "hyper_request_with_hostile_options_in_a_task" => {
                case_hyper_request_with_hostile_options_in_a_task().await;
            }
            "hyper_handler_returns_hostile_table" => {
                case_hyper_handler_returns_hostile_table().await
            }
            "plain_mlua_calls_survive_the_value" => case_plain_mlua_calls_survive_the_value().await,
            "tostring_that_fails_through_the_rust_api" => {
                case_tostring_that_fails_through_the_rust_api().await;
            }
            "a_failed_conversion_leaves_no_guard_behind" => {
                case_a_failed_conversion_leaves_no_guard_behind().await;
            }
            other => panic!("unknown case {other}"),
        }
    });
    println!("case {case} ok");
}

// ---- the cases (reproductions 1 to 4 of NEOC-124, then what the guard must also hold) -----------------------

fn chunk(src: &str) -> String {
    format!("{HOSTILE}\n{src}")
}

/// Reproduction 1: `neoc run` of `task.wait(0); error(hostile)`: the main chunk ends in an error at `exec_async`
/// after a yield, and the engine is still usable.
async fn case_main_chunk_after_task_wait() {
    let lua = make_engine().unwrap();
    let err = lua
        .load(chunk(r#"require("std:task").wait(0); error(hostile())"#))
        .exec_async()
        .await
        .unwrap_err();
    assert!(err.to_string().contains(HANDLED), "{err}");
    assert_eq!(lua.load("return 1 + 1").eval::<i32>().unwrap(), 2);
}

/// Reproduction 2: a host function that reads an options table whose `__index` raises the value, even inside
/// `pcall`, in the main chunk, in a coroutine and in a `std:task` task.
async fn case_host_function_reads_hostile_options() {
    let lua = make_engine().unwrap();
    let src = chunk(
        r#"
        local spawn = require("std:thread").spawn
        local function try()
          local ok, value, message = pcall(spawn, hostile_options())
          return ok and value == nil and message
        end
        local direct = try()
        local in_coroutine = coroutine.wrap(try)()
        local in_task = require("std:task").spawn(try):await()
        return { direct, in_coroutine, in_task }
        "#,
    );
    let results: Table = lua.load(src).eval_async().await.unwrap();
    for i in 1..=3 {
        let message: String = results.get(i).unwrap();
        assert!(message.starts_with("thread.spawn:"), "{i}: {message}");
        assert!(message.contains(HANDLED), "{i}: {message}");
    }
}

/// Reproduction 3: `vnd:hyper.request(opts)` with such an `opts`, in a task.
async fn case_hyper_request_with_hostile_options_in_a_task() {
    let lua = make_engine().unwrap();
    let src = chunk(
        r#"
        return require("std:task").spawn(function()
          local response, err = require("vnd:hyper").request(hostile_options())
          return tostring(response) .. "|" .. tostring(err)
        end):await()
        "#,
    );
    let out: String = lua.load(src).eval_async().await.unwrap();
    assert!(out.starts_with("nil|hyper.request:"), "{out}");
    assert!(out.contains(HANDLED), "{out}");
}

/// Reproduction 4: a `vnd:hyper` handler that returns such a table. The server answers 500 and keeps serving.
async fn case_hyper_handler_returns_hostile_table() {
    let lua = make_engine().unwrap();
    let src = chunk(
        r#"
        local net, hyper = require("std:net"), require("vnd:hyper")
        local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
        local calls = 0
        local server = assert(hyper.serve(listener, function()
          calls = calls + 1
          if calls == 1 then return hostile_options() end
          return { status = 200, body = "fine" }
        end))
        local url = "http://" .. server:addr() .. "/"
        local first = assert(hyper.get(url))
        local second = assert(hyper.get(url))
        return first.status .. "|" .. second.status .. "|" .. second.body
        "#,
    );
    let out: String = lua.load(src).eval_async().await.unwrap();
    assert_eq!(out, "500|200|fine");
}

/// What `neoc_vm::lua::sandbox::call_guarded` and the engine's guarded `xpcall` exist for, with neither: a plain
/// `Function::call`, `call_async`, `Table::get` through a metamethod, `exec`/`exec_async` and a coroutine
/// resumed from Rust each end in an error.
async fn case_plain_mlua_calls_survive_the_value() {
    let lua = make_engine().unwrap();
    let env: Table = lua
        .load(chunk(
            r"return {
              raise = function() error(hostile()) end,
              options = hostile_options(),
              coroutine = coroutine.create(function() error(hostile()) end),
            }",
        ))
        .eval()
        .unwrap();
    let raise: Function = env.get("raise").unwrap();
    assert!(
        raise
            .call::<()>(())
            .unwrap_err()
            .to_string()
            .contains(HANDLED)
    );
    assert!(
        raise
            .call_async::<()>(())
            .await
            .unwrap_err()
            .to_string()
            .contains(HANDLED)
    );
    let options: Table = env.get("options").unwrap();
    assert!(
        options
            .get::<Value>("anything")
            .unwrap_err()
            .to_string()
            .contains(HANDLED)
    );
    let thread: mlua::Thread = env.get("coroutine").unwrap();
    assert!(thread.resume::<()>(()).is_err());
    assert!(lua.load(chunk("error(hostile())")).exec().is_err());
    assert!(
        lua.load(chunk("error(hostile())"))
            .exec_async()
            .await
            .is_err()
    );
}

/// A `__tostring` that fails through the Rust API each time (it reads a hostile options table) makes the
/// handler run nested. It must not run another `__tostring`: the recursion would be as deep as before.
async fn case_tostring_that_fails_through_the_rust_api() {
    let lua = make_engine().unwrap();
    let src = chunk(
        r#"
        local spawn = require("std:thread").spawn
        local nested = {}
        nested.__tostring = function()
          pcall(error, "x")
          spawn(hostile_options())   -- raises through the Rust API, into the handler again
          spawn(setmetatable({}, { __index = function() error(setmetatable({}, nested)) end }))
          return "x"
        end
        local response, err = require("vnd:hyper").request(
          setmetatable({}, { __index = function() error(setmetatable({}, nested)) end }))
        return tostring(response) .. "|" .. tostring(err)
        "#,
    );
    let out: String = lua.load(src).eval_async().await.unwrap();
    assert!(out.starts_with("nil|hyper.request:"), "{out}");
}

/// A `__tostring` that raises (the handler unwinds by error) must not leave the guard set: an independent error
/// afterwards, on the same state, on a coroutine and through `Table::get`, still gets its own `__tostring` text.
async fn case_a_failed_conversion_leaves_no_guard_behind() {
    let lua = make_engine().unwrap();
    let src = chunk(
        r#"
        local spawn = require("std:thread").spawn
        local function later_options()
          return setmetatable({}, { __index = function()
            error(setmetatable({}, { __tostring = function() return "later text" end }))
          end })
        end
        local function message(options) local _, text = spawn(options) return text end
        local function round()
          return {
            hostile = message(hostile_options()),
            later = message(later_options()),
            in_coroutine = coroutine.wrap(function()
              return { hostile = message(hostile_options()), later = message(later_options()) }
            end)(),
          }
        end
        return { round(), round(), round(), hostile_options(), later_options() }
        "#,
    );
    let out: Table = lua.load(src).eval_async().await.unwrap();
    for i in 1..=3 {
        let round: Table = out.get(i).unwrap();
        let in_coroutine: Table = round.get("in_coroutine").unwrap();
        for hostile in [
            round.get::<String>("hostile").unwrap(),
            in_coroutine.get("hostile").unwrap(),
        ] {
            assert!(hostile.contains(HANDLED), "{hostile}");
        }
        for later in [
            round.get::<String>("later").unwrap(),
            in_coroutine.get("later").unwrap(),
        ] {
            assert!(later.contains("later text"), "{later}");
        }
    }
    let (hostile, later): (Table, Table) = (out.get(4).unwrap(), out.get(5).unwrap());
    for _ in 0..3 {
        let e = hostile.get::<Value>("k").unwrap_err().to_string();
        assert!(e.contains(HANDLED), "{e}");
        let e = later.get::<Value>("k").unwrap_err().to_string();
        assert!(e.contains("later text"), "{e}");
    }
}

#[test]
fn reproduction_1_main_chunk_after_task_wait() {
    in_child("main_chunk_after_task_wait");
}

#[test]
fn reproduction_2_host_function_reads_hostile_options() {
    in_child("host_function_reads_hostile_options");
}

#[test]
fn reproduction_3_hyper_request_with_hostile_options_in_a_task() {
    in_child("hyper_request_with_hostile_options_in_a_task");
}

#[test]
fn reproduction_4_hyper_handler_returns_hostile_table() {
    in_child("hyper_handler_returns_hostile_table");
}

#[test]
fn plain_mlua_calls_survive_the_value() {
    in_child("plain_mlua_calls_survive_the_value");
}

#[test]
fn a_tostring_that_fails_through_the_rust_api_does_not_recurse() {
    in_child("tostring_that_fails_through_the_rust_api");
}

#[test]
fn a_failed_conversion_leaves_no_guard_behind() {
    in_child("a_failed_conversion_leaves_no_guard_behind");
}

// ---- ordinary errors are unchanged ---------------------------------------------------------------------------

fn error_of(src: &str) -> Error {
    let lua = make_engine().unwrap();
    lua.load(src).exec().unwrap_err()
}

#[test]
fn a_string_error_keeps_its_message_and_traceback() {
    let text = error_of(
        "local function inner() error('boom') end\nlocal function outer() inner() end\nouter()",
    )
    .to_string();
    assert!(text.contains("boom"), "{text}");
    assert!(text.contains("stack traceback:"), "{text}");
    assert!(text.contains("in function"), "{text}");
}

#[test]
fn a_table_with_a_working_tostring_is_shown_as_its_text() {
    let text =
        error_of("error(setmetatable({}, { __tostring = function() return 'custom text' end }))")
            .to_string();
    assert!(text.contains("custom text"), "{text}");
    assert!(text.contains("stack traceback:"), "{text}");
}

#[test]
fn a_tostring_with_a_protected_call_of_its_own_still_returns_its_text() {
    let text = error_of(
        "error(setmetatable({}, { __tostring = function() pcall(error, 'x') return 'after pcall' end }))",
    )
    .to_string();
    assert!(text.contains("after pcall"), "{text}");
}

#[test]
fn a_tostring_that_raises_is_error_in_error_handling() {
    let text = error_of("error(setmetatable({}, { __tostring = function() error('inner') end }))")
        .to_string();
    assert!(text.contains(HANDLED), "{text}");
}

#[test]
fn a_tostring_that_returns_a_number_is_its_text() {
    let text =
        error_of("error(setmetatable({}, { __tostring = function() return 1 end }))").to_string();
    assert!(
        text.starts_with("runtime error: 1\nstack traceback:"),
        "{text}"
    );
}

#[test]
fn a_tostring_that_returns_a_table_is_error_in_error_handling() {
    let text =
        error_of("error(setmetatable({}, { __tostring = function() return {} end }))").to_string();
    assert!(text.contains(HANDLED), "{text}");
}

#[test]
fn non_string_values_without_a_tostring_are_shown_as_their_type_and_address() {
    for (src, prefix) in [
        ("error({})", "table: "),
        ("error(print)", "function: "),
        ("error(42)", "42"),
        ("error(true)", "true"),
        ("error(nil)", "nil"),
    ] {
        let text = error_of(src).to_string();
        assert!(text.contains(prefix), "{src}: {text}");
    }
}

#[derive(Debug)]
struct HostFailure(&'static str);

impl fmt::Display for HostFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "host failure: {}", self.0)
    }
}

impl std::error::Error for HostFailure {}

#[test]
fn a_rust_error_out_of_a_callback_is_downcast_through_its_wrapped_failure() {
    let lua = make_engine().unwrap();
    let fail = lua
        .create_function(|_, ()| -> mlua::Result<()> { Err(Error::external(HostFailure("disk"))) })
        .unwrap();
    lua.globals().set("fail", fail).unwrap();
    let err = lua
        .load("local function f() fail() end f()")
        .exec()
        .unwrap_err();
    let Error::CallbackError { cause, traceback } = &err else {
        panic!("not a callback error: {err:?}");
    };
    assert!(traceback.contains("stack traceback:"), "{traceback}");
    let Error::ExternalError(source) = &**cause else {
        panic!("not an external error: {cause:?}");
    };
    assert_eq!(source.downcast_ref::<HostFailure>().unwrap().0, "disk");
    // and a Rust error that goes through a Lua `pcall` and is raised again keeps its type
    let err = lua
        .load("local ok, e = pcall(fail) error(e, 0)")
        .exec()
        .unwrap_err();
    assert!(err.to_string().contains("host failure: disk"), "{err}");
}

#[test]
fn running_out_of_memory_is_a_memory_error() {
    let err = error_of("return string.rep('x', 1e10)");
    assert!(matches!(err, Error::MemoryError(_)), "{err:?}");
}

#[test]
fn a_syntax_error_is_a_syntax_error() {
    let lua = make_engine().unwrap();
    let err = lua.load("local = 1").exec().unwrap_err();
    assert!(matches!(err, Error::SyntaxError { .. }), "{err:?}");
}

#[test]
fn a_table_get_through_a_metamethod_reports_a_string_error_with_its_traceback() {
    let lua = make_engine().unwrap();
    let t: Table = lua
        .load("return setmetatable({}, { __index = function() error('from index') end })")
        .eval()
        .unwrap();
    let text = t.get::<Value>("k").unwrap_err().to_string();
    assert!(text.contains("from index"), "{text}");
    assert!(text.contains("stack traceback:"), "{text}");
}

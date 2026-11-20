#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Recursions that abort the process on LuaJIT and must be an error a script (or the embedder) can catch on Luau.
//!
//! LuaJIT 2.1 has no C-call depth limit, and mlua's error handler and LuaJIT's `xpcall` handler call an error
//! value's `__tostring` while they run: a `__tostring` that makes a protected call and then raises itself was
//! called again by its own error until the C stack of a runtime worker (2 MiB) was gone, and the process aborted
//! (NEOC-120, patched by a vendored mlua and a guard layer, both removed with the switch to Luau). A script that
//! recursed through a host function that calls back into Lua (`string.gsub`, `table.sort`, a module that runs a
//! callback) aborted the same way.
//!
//! Each case here runs in a **child process** (this test binary, run again with `NEOC_ABORT_CASE`) on a **2 MiB
//! thread**, so a case that still aborts fails its own test (the child dies on a signal) instead of taking the whole
//! test binary with it. A case passes when the child exits normally and reports what it caught.

use std::process::Command;

use mlua::{Function, Table, Thread, Value};
use neoc_vm::lua::sandbox::make_engine;

/// The value LuaJIT could not survive: `__tostring` makes a protected call, then raises another value of its own kind.
const POISON: &str = r#"
    local poison_mt = {}
    poison_mt.__tostring = function()
      pcall(error, "x")
      error(setmetatable({}, poison_mt))
    end
    local function poison() return setmetatable({}, poison_mt) end
    -- a table whose every read raises the poison: a hostile options table, or response
    local function poisoned() return setmetatable({}, { __index = function() error(poison()) end }) end
"#;

/// What a case reports: the error it caught, or what the script returned.
type Outcome = Result<String, String>;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Run `body` of a script on the engine, as a call the embedder drives (`exec_async`), and report its result string
/// or the error that came back.
fn script(body: &str) -> Outcome {
    runtime().block_on(async {
        let lua = make_engine().unwrap();
        let source = format!("{POISON}\n{body}");
        match lua.load(&source).eval_async::<Value>().await {
            Ok(Value::String(s)) => Ok(s.to_string_lossy()),
            Ok(other) => Ok(format!("{other:?}")),
            Err(e) => Err(e.to_string()),
        }
    })
}

/// Every case: its name, and what it does in the child.
type Case = (&'static str, fn() -> Outcome);

const CASES: &[Case] = &[
    // ---- an error value whose `__tostring` fails the way LuaJIT could not survive ------------------------------
    ("main_chunk_raises_the_poison", || script("error(poison())")),
    ("failing_task", || {
        script(
            r#"local task = require("std:task")
               local t = task.spawn(function() error(poison()) end)
               local v, err = t:await()
               return "task finished: " .. type(v) .. "," .. type(err)"#,
        )
    }),
    ("failing_worker", || {
        script(
            r#"local thread = require("std:thread")
               local w = assert(thread.spawn({ source = [[
                 local mt = {}
                 mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
                 error(setmetatable({}, mt))
               ]] }))
               local v, err = w:await()
               return "worker finished: " .. type(v) .. "," .. type(err)"#,
        )
    }),
    ("embedder_call_async", || {
        runtime().block_on(async {
            let lua = make_engine().unwrap();
            let f: Function = lua
                .load(format!("{POISON}\nreturn function() error(poison()) end"))
                .eval()
                .unwrap();
            f.call_async::<()>(())
                .await
                .map(|()| "returned".into())
                .map_err(|e| e.to_string())
        })
    }),
    ("table_get_through_a_poisoned_index", || {
        let lua = make_engine().unwrap();
        let t: Table = lua
            .load(format!("{POISON}\nreturn poisoned()"))
            .eval()
            .unwrap();
        t.get::<Value>("key")
            .map(|_| "read".into())
            .map_err(|e| e.to_string())
    }),
    ("thread_spawn_with_a_poisoned_options_table", || {
        script(
            r#"local thread = require("std:thread")
               local ok, w, err = pcall(thread.spawn, poisoned())
               return "spawn: " .. tostring(ok) .. "," .. type(w) .. "," .. type(err)"#,
        )
    }),
    ("hyper_request_with_a_poisoned_options_table", || {
        script(
            r#"local hyper = require("vnd:hyper")
               local ok, r, err = pcall(hyper.request, poisoned())
               return "request: " .. tostring(ok) .. "," .. type(r) .. "," .. type(err)"#,
        )
    }),
    ("hyper_handler_returns_the_poison", || {
        script(
            r#"local hyper = require("vnd:hyper")
               local listener = require("std:net").TcpListener.bind("127.0.0.1:0")
               local server = hyper.serve(listener, function() return poisoned() end)
               local resp, err = hyper.get("http://" .. server:addr() .. "/")
               server:close()
               return "handler: " .. type(resp) .. "," .. type(err)"#,
        )
    }),
    ("hyper_handler_raises_the_poison", || {
        script(
            r#"local hyper = require("vnd:hyper")
               local listener = require("std:net").TcpListener.bind("127.0.0.1:0")
               local server = hyper.serve(listener, function() error(poison()) end)
               local resp, err = hyper.get("http://" .. server:addr() .. "/")
               server:close()
               return "handler: " .. type(resp) .. "," .. type(err)"#,
        )
    }),
    ("xpcall_handler_recursion", || {
        script(
            r#"local never = function() pcall(error, "z") error("again") end
               local _, e = xpcall(function() error("x", 0) end, never)
               return "xpcall answered: " .. tostring(e)"#,
        )
    }),
    ("lib_test_to_throw_message_handler", || {
        script(
            r#"local test = require("lib:test")
               local ok, e = pcall(function() test.expect(function() error(poison()) end).toThrow() end)
               return "toThrow: " .. tostring(ok)"#,
        )
    }),
    (
        "tostring_resumes_a_coroutine_that_fails_with_the_same_value",
        || {
            // `error_traceback_thread` -> `__tostring` -> a coroutine failing with the same value -> ... through mlua
            let lua = make_engine().unwrap();
            let thread: Thread = lua
                .load(
                    r#"local mt = {}
                   mt.__tostring = function(self)
                     local co = coroutine.create(function() error(setmetatable({}, mt)) end)
                     coroutine.resume(co)
                     return "x"
                   end
                   return coroutine.create(function() error(setmetatable({}, mt)) end)"#,
                )
                .eval()
                .unwrap();
            thread
                .resume::<()>(())
                .map(|()| "resumed".into())
                .map_err(|e| e.to_string())
        },
    ),
    // ---- recursion through a host function that calls back into Lua: no C-call depth limit on LuaJIT -------------
    ("gsub_callback_recursion", || {
        script(
            r#"local function f() return (string.gsub("a", "a", f)) end
               local ok, e = pcall(f)
               return "gsub: " .. tostring(ok) .. "," .. tostring(e)"#,
        )
    }),
    ("sort_comparator_recursion", || {
        script(
            r#"local function s() table.sort({ 2, 1 }, function(a, b) s() return a < b end) end
               local ok, e = pcall(s)
               return "sort: " .. tostring(ok) .. "," .. tostring(e)"#,
        )
    }),
    (
        "neoc_host_function_running_a_callback_recurses_into_itself",
        || {
            // `lib:cache` runs `on_evict` from Rust; the callback sets another key, which evicts, which runs it again
            script(
                r#"local cache
               local depth = 0
               cache = require("lib:cache").Cache.new {
                 max_size = 1,
                 on_evict = function() depth = depth + 1 cache:set("k" .. depth, depth) end,
               }
               cache:set("a", 1)
               local ok, e = pcall(function() cache:set("b", 2) end)
               return "on_evict: " .. tostring(ok) .. "," .. tostring(e) .. ",depth>100:" .. tostring(depth > 100)"#,
            )
        },
    ),
    (
        "async_host_function_running_a_callback_recurses_into_itself",
        || {
            // `lib:test`'s `describe` is an async host function that runs its body: a body that calls `describe` again
            script(
                r#"local test = require("lib:test")
               local depth = 0
               local function nest() depth = depth + 1 test.describe("d", nest) end
               local ok, e = pcall(nest)
               return "describe: " .. tostring(ok) .. "," .. tostring(e) .. ",depth>100:" .. tostring(depth > 100)"#,
            )
        },
    ),
    ("tostring_recursion_through_a_metamethod", || {
        script(
            r#"local t = setmetatable({}, { __tostring = function(self) return tostring(self) end })
               local ok, e = pcall(tostring, t)
               return "tostring: " .. tostring(ok) .. "," .. tostring(e)"#,
        )
    }),
];

/// The child half: run the case named by `NEOC_ABORT_CASE` on a 2 MiB thread and print what it caught. Does nothing
/// when the variable is not set (the normal run of this test binary).
#[test]
fn child() {
    let Ok(name) = std::env::var("NEOC_ABORT_CASE") else {
        return;
    };
    let (_, run) = CASES
        .iter()
        .find(|(n, _)| *n == name)
        .expect("unknown case");
    // 2 MiB, the stack of a runtime worker (`NEOC_ABORT_STACK_KIB` only to measure how much a case needs)
    let kib = std::env::var("NEOC_ABORT_STACK_KIB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2048);
    let outcome = std::thread::Builder::new()
        .stack_size(kib * 1024)
        .spawn(*run)
        .unwrap()
        .join()
        .expect("the case panicked");
    match outcome {
        Ok(text) => println!("OUTCOME: returned: {}", text.replace('\n', " ")),
        Err(text) => println!("OUTCOME: error: {}", text.replace('\n', " ")),
    }
}

/// Run `name` in a child process and return what it reported. A child that aborts, is killed by a signal or fails
/// to report is a failed test, with the child's own output.
fn in_child(name: &str) -> String {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env("NEOC_ABORT_CASE", name)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{name}: the child process did not survive ({:?})\nstdout: {stdout}\nstderr: {stderr}",
        out.status
    );
    stdout
        .lines()
        .find_map(|l| l.split_once("OUTCOME: ").map(|(_, outcome)| outcome))
        .unwrap_or_else(|| panic!("{name}: no outcome\nstdout: {stdout}\nstderr: {stderr}"))
        .to_owned()
}

macro_rules! survives {
    ($test:ident, $case:literal, $($expected:literal),+) => {
        #[test]
        fn $test() {
            let outcome = in_child($case);
            $(assert!(outcome.contains($expected), "{}: expected `{}` in `{}`", $case, $expected, outcome);)+
        }
    };
}

survives!(
    main_chunk_raises_the_poison,
    "main_chunk_raises_the_poison",
    "error:"
);
survives!(failing_task, "failing_task", "task finished");
survives!(failing_worker, "failing_worker", "worker finished");
survives!(embedder_call_async, "embedder_call_async", "error:");
survives!(
    table_get_through_a_poisoned_index,
    "table_get_through_a_poisoned_index",
    "error:"
);
survives!(
    thread_spawn_with_a_poisoned_options_table,
    "thread_spawn_with_a_poisoned_options_table",
    "spawn:"
);
survives!(
    hyper_request_with_a_poisoned_options_table,
    "hyper_request_with_a_poisoned_options_table",
    "request:"
);
survives!(
    hyper_handler_returns_the_poison,
    "hyper_handler_returns_the_poison",
    "handler:"
);
survives!(
    hyper_handler_raises_the_poison,
    "hyper_handler_raises_the_poison",
    "handler:"
);
survives!(
    xpcall_handler_recursion,
    "xpcall_handler_recursion",
    "error in error handling"
);
survives!(
    lib_test_to_throw_message_handler,
    "lib_test_to_throw_message_handler",
    "toThrow: true"
);
survives!(
    tostring_resumes_a_coroutine_that_fails_with_the_same_value,
    "tostring_resumes_a_coroutine_that_fails_with_the_same_value",
    "error:"
);
survives!(
    gsub_callback_recursion,
    "gsub_callback_recursion",
    "gsub: false",
    "overflow"
);
survives!(
    sort_comparator_recursion,
    "sort_comparator_recursion",
    "sort: false",
    "overflow"
);
/// The one case that needs more than 2 MiB in a **debug** build: each level of `Lua -> lib:cache (Rust) -> on_evict
/// (Lua)` takes about 25 KiB of Rust stack there (about 1 KiB in release), and Luau stops the recursion at 200 C
/// levels with "C stack overflow", which is 5 MiB of frames in debug and 0.2 MiB in release. Measured: the child aborts
/// at 2, 3 and 4 MiB in debug, survives at 6 MiB in debug and at 2 MiB in release. Run it with
/// `cargo test --release -p neoc-vm --test abort_regressions`.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "a debug build needs ~5 MiB of stack to reach Luau's C-call limit through a host function; run with --release"
)]
fn neoc_host_function_running_a_callback_recurses_into_itself() {
    let outcome = in_child("neoc_host_function_running_a_callback_recurses_into_itself");
    // Luau's "C stack overflow" is raised in the deepest call; the message handler that runs there fails in turn
    // (it is that deep), and what comes back to the script is `error in error handling`
    assert!(
        outcome.contains("on_evict: false")
            && (outcome.contains("overflow") || outcome.contains("error in error handling")),
        "{outcome}"
    );
}
/// The same as the case above through an async host function (`lib:test`'s `describe`): 8 MiB in debug, 2 MiB in release.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "a debug build needs ~8 MiB of stack to reach Luau's C-call limit through a host function; run with --release"
)]
fn async_host_function_running_a_callback_recurses_into_itself() {
    let outcome = in_child("async_host_function_running_a_callback_recurses_into_itself");
    assert!(
        outcome.contains("describe: false") && outcome.contains("C stack overflow"),
        "{outcome}"
    );
}

survives!(
    tostring_recursion_through_a_metamethod,
    "tostring_recursion_through_a_metamethod",
    "tostring: false",
    "overflow"
);

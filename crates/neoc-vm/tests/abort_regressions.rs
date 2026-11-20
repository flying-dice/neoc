#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Error values and recursions that abort the process on LuaJIT and must be an error a script (or the embedder) can
//! catch on Luau.
//!
//! LuaJIT 2.1 has no C-call depth limit, and mlua's error handler and LuaJIT's `xpcall` handler call an error
//! value's `__tostring` while they run: a `__tostring` that makes a protected call and then raises itself was
//! called again by its own error until the C stack of a runtime worker was gone, and the process aborted (NEOC-120).
//! A script that recursed through a host function that calls back into Lua (`string.gsub`, a module that runs a
//! callback, a request whose options table is read inside the future) aborted the same way. The first version of this
//! file (17 cases), copied onto main 7aeeafd0 with its guards, aborted in 14 of them and hung in one (the tail-call form
//! of `tostring_recursion`, since changed); main's `xpcall` guard and `call_guarded` covered only
//! `xpcall_handler_recursion` and `lib_test_to_throw_message_handler`.
//!
//! Each case here runs in a **child process** (this test binary, run again with `NEOC_ABORT_CASE`) on a thread of a
//! **stated size**, so a case that still aborts fails its own test (the child dies on a signal) instead of taking the
//! whole test binary with it, and one that hangs is killed after [`TIMEOUT`]. The size is set by the parent for each
//! child, never inherited from the environment: `neoc_vm::VM_THREAD_STACK` in a debug build (what `neoc run` and an
//! embedder following `lifecycle.rs` give every thread that polls a VM) and 2 MiB in a release build (tokio's default,
//! what an embedder that ignores the constant gets). A case passes when the child exits normally and reports what it
//! caught. To measure how much a case needs, run the child binary yourself: `NEOC_ABORT_CASE=<case>
//! NEOC_ABORT_STACK_KIB=<n> <test binary> --exact child --nocapture`.

use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
    -- a table whose every read raises the poison: a hostile options table, or response. `reads` counts the reads
    -- that ran, so a case can tell that the host really read the table (and did not refuse it as empty)
    local reads = 0
    local function poisoned()
      return setmetatable({}, { __index = function() reads = reads + 1 error(poison()) end })
    end
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
               return "spawn: " .. tostring(ok) .. "," .. type(w) .. "," .. tostring(err) .. ",reads:" .. reads"#,
        )
    }),
    ("hyper_request_with_a_poisoned_options_table", || {
        script(
            r#"local hyper = require("vnd:hyper")
               local ok, r, err = pcall(hyper.request, poisoned())
               return "request: " .. tostring(ok) .. "," .. type(r) .. "," .. tostring(err) .. ",reads:" .. reads"#,
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
            // `error_traceback_thread` -> `__tostring` -> `Thread::resume` (a host function, so the resume goes
            // through mlua: Lua's own `coroutine.resume` never calls `__tostring`) of a coroutine failing with the
            // same value -> `error_traceback_thread` -> ... The recursion is mlua's, one nested resume per level
            let lua = make_engine().unwrap();
            lua.globals()
                .set(
                    "RESUME",
                    lua.create_function(|_, thread: Thread| thread.resume::<()>(()))
                        .unwrap(),
                )
                .unwrap();
            let thread: Thread = lua
                .load(
                    r#"local mt = {}
                       mt.__tostring = function(self)
                         local co = coroutine.create(function() error(setmetatable({}, mt)) end)
                         pcall(RESUME, co)
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
    // ---- recursion through a host function that calls back into Lua: Luau stops it at its C-call limit ----------
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
    ("with_lock_nested_in_itself", || {
        // `Mutex:with_lock` runs its body with `call_async` inside its own future: each level is another polled future
        script(
            r#"local Mutex = require("std:collections").Mutex
               local depth = 0
               local function nest() depth = depth + 1 return Mutex.new():with_lock(nest) end
               local ok, e = pcall(nest)
               return "with_lock: " .. tostring(ok) .. "," .. tostring(e) .. ",depth>100:" .. tostring(depth > 100)"#,
        )
    }),
    ("hyper_request_options_recursion", || {
        // an options table whose `__index` calls `hyper.request` again with another such table: the options are read
        // before the future is built, so a level is small
        script(
            r#"local hyper = require("vnd:hyper")
               local depth = 0
               local function opts()
                 local first = true -- only the first read recurses: a level is one deeper, not several
                 return setmetatable({}, {
                   __index = function() if first then first = false depth = depth + 1 pcall(hyper.request, opts()) end end,
                 })
               end
               local ok, r, e = pcall(hyper.request, opts())
               return "hyper.request recursion: " .. tostring(ok) .. ",depth>50:" .. tostring(depth > 50)"#,
        )
    }),
    ("thread_spawn_options_recursion", || {
        script(
            r#"local thread = require("std:thread")
               local depth = 0
               local function opts()
                 local first = true -- only the first read recurses: `prepare_spawn` reads several options
                 return setmetatable({}, {
                   __index = function() if first then first = false depth = depth + 1 pcall(thread.spawn, opts()) end end,
                 })
               end
               local ok, w, e = pcall(thread.spawn, opts())
               return "thread.spawn recursion: " .. tostring(ok) .. ",depth>50:" .. tostring(depth > 50)"#,
        )
    }),
    ("tostring_recursion_through_a_metamethod", || {
        script(
            r#"-- not a tail call (`return tostring(self)` is one, and LuaJIT loops on it for ever instead of aborting)
               local t = setmetatable({}, { __tostring = function(self) return (tostring(self)) end })
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
    // set by the parent for every child (see the header), never defaulted here
    let kib: usize = std::env::var("NEOC_ABORT_STACK_KIB")
        .expect("NEOC_ABORT_STACK_KIB is set by the parent")
        .parse()
        .expect("NEOC_ABORT_STACK_KIB is a number");
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

/// How long a child may run: a hang (a recursion that never ends, as `return tostring(self)` does on LuaJIT) fails
/// the test instead of stalling the suite.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The thread size the parent gives every child: what an embedder that follows `lifecycle.rs` gives a thread that
/// polls a VM in a debug build, and tokio's default in a release build.
fn stack_kib() -> usize {
    if cfg!(debug_assertions) {
        neoc_vm::VM_THREAD_STACK / 1024
    } else {
        2048
    }
}

/// Run `name` in a child process and return what it reported. A child that aborts, is killed by a signal, hangs or
/// fails to report is a failed test, with the child's own output.
fn in_child(name: &str) -> String {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env("NEOC_ABORT_CASE", name)
        .env("NEOC_ABORT_STACK_KIB", stack_kib().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    };
    let stdout = drain(Box::new(child.stdout.take().unwrap()));
    let stderr = drain(Box::new(child.stderr.take().unwrap()));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout.join().unwrap();
    let stderr = stderr.join().unwrap();
    let status = status.unwrap_or_else(|| {
        panic!(
            "{name}: the child hung (killed after {TIMEOUT:?})\nstdout: {stdout}\nstderr: {stderr}"
        )
    });
    assert!(
        status.success(),
        "{name}: the child process did not survive ({status:?}) with {} KiB of stack\nstdout: {stdout}\nstderr: {stderr}",
        stack_kib()
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
    "error: runtime error: error in error handling"
);
survives!(failing_task, "failing_task", "task finished: nil,string");
survives!(
    failing_worker,
    "failing_worker",
    "worker finished: nil,string"
);
survives!(
    embedder_call_async,
    "embedder_call_async",
    "error: runtime error: error in error handling"
);
survives!(
    table_get_through_a_poisoned_index,
    "table_get_through_a_poisoned_index",
    "error: runtime error: error in error handling"
);
survives!(
    thread_spawn_with_a_poisoned_options_table,
    "thread_spawn_with_a_poisoned_options_table",
    "spawn: true,nil,thread.spawn: runtime error: error in error handling,reads:1"
);
survives!(
    hyper_request_with_a_poisoned_options_table,
    "hyper_request_with_a_poisoned_options_table",
    "request: true,nil,hyper.request: runtime error: error in error handling,reads:1"
);
survives!(
    hyper_handler_returns_the_poison,
    "hyper_handler_returns_the_poison",
    "handler: table,nil"
);
survives!(
    hyper_handler_raises_the_poison,
    "hyper_handler_raises_the_poison",
    "handler: table,nil"
);
survives!(
    xpcall_handler_recursion,
    "xpcall_handler_recursion",
    "returned: xpcall answered: error in error handling"
);
survives!(
    lib_test_to_throw_message_handler,
    "lib_test_to_throw_message_handler",
    "toThrow: true"
);
survives!(
    tostring_resumes_a_coroutine_that_fails_with_the_same_value,
    "tostring_resumes_a_coroutine_that_fails_with_the_same_value",
    "error: runtime error: x"
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
survives!(
    neoc_host_function_running_a_callback_recurses_into_itself,
    "neoc_host_function_running_a_callback_recurses_into_itself",
    "on_evict: false",
    "depth>100:true"
);
survives!(
    async_host_function_running_a_callback_recurses_into_itself,
    "async_host_function_running_a_callback_recurses_into_itself",
    "describe: false",
    "depth>100:true"
);
survives!(
    with_lock_nested_in_itself,
    "with_lock_nested_in_itself",
    "with_lock: false",
    "depth>100:true"
);
survives!(
    hyper_request_options_recursion,
    "hyper_request_options_recursion",
    "hyper.request recursion:",
    "depth>50:true"
);
survives!(
    thread_spawn_options_recursion,
    "thread_spawn_options_recursion",
    "thread.spawn recursion:",
    "depth>50:true"
);
survives!(
    tostring_recursion_through_a_metamethod,
    "tostring_recursion_through_a_metamethod",
    "tostring: false",
    "overflow"
);

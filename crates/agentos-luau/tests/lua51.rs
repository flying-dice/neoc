#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! What running on standard Lua 5.1 changes: the sandbox of an app's environment, the CPU budget enforced by
//! the VM's count hook (in coroutines, in async host calls, in C-driven callbacks), the host-side string
//! functions that bound pattern matching, the memory limit, and async host calls where 5.1 cannot yield
//! (across `pcall`, metamethods, C functions).

mod common;

use agentos_luau::VmError;
use common::{agent_ctx, db, load, quick, try_load, try_load_with};
use serde_json::{json, Value};
use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

/// "Killed at the budget, not by the wall-clock backstop": far below the backstop even on a busy machine.
const PROMPTLY: Duration = Duration::from_secs(8);
/// A small slice: 200 000 ticks of 100 VM instructions.
const SLICE: u64 = 200_000;

async fn emit(vm: &agentos_luau::AppVm, event: &str) -> Result<Value, VmError> {
    vm.emit(event, json!(null)).await
}

fn budget_error(e: &VmError) -> bool {
    e.to_string().contains("cpu budget exceeded")
}

// ---- the sandbox ----------------------------------------------------------------------------------------

#[tokio::test]
async fn none_of_the_escape_hatches_exist_in_the_apps_environment() {
    let src = r#"
        local missing = {}
        local function check(name, value) if value ~= nil then missing[#missing + 1] = name end end
        for _, name in ipairs { "load", "loadstring", "dofile", "loadfile", "getfenv", "setfenv", "newproxy",
          "module", "collectgarbage", "gcinfo", "io", "os", "debug", "package", "print", "jit", "arg" } do
          check(name, _G[name])
          check("global " .. name, rawget(_G, name))
        end
        check("string.dump", string.dump)
        check("string.gfind", string.gfind)
        check("coroutine.debug", coroutine.debug)
        app.on("report", function() return missing end)
    "#;
    let vm = load(src).await;
    assert_eq!(emit(&vm, "report").await.unwrap(), json!([]), "these exist in the app's environment");
}

#[tokio::test]
async fn text_only_loading_binary_chunks_never_run() {
    let bytecode = agentos_luavm::mlua::Lua::new()
        .load("return string.dump(function() return 7 end)")
        .eval::<agentos_luavm::mlua::LuaString>()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert!(bytecode.starts_with(b"\x1bLua"));
    let package = agentos_application::ports::AppPackage {
        entry: "app.lua".into(),
        files: [("app.lua".to_owned(), bytecode)].into(),
    };
    let err = agentos_luau::AppVm::load(
        &common::id("probe"),
        package,
        db(),
        common::config(agentos_luau::Limits::default()),
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(err.contains("binary"), "{err}");
}

#[tokio::test]
async fn the_standard_libraries_are_read_only_in_every_spelling() {
    // every attempt must raise, and the library must afterwards be exactly as it was
    let attempts = [
        "string.evil = 1",
        "string.rep = nil",
        "string.find = function() return 'pwned' end",
        "string[1] = 'x'",
        "table.insert = nil",
        "math.floor = function() return 0 end",
        "coroutine.wrap = nil",
        "bit.band = nil",
        "utf8.len = nil",
        "rawset(string, 'evil', 1)",
        "rawset(table, 'insert', nil)",
        "rawset(math, 'pi', 3)",
        "setmetatable(string, {})",
        "setmetatable(table, { __index = function() return 1 end })",
        "getmetatable(string).__index = {}",
        "getmetatable('').__index.evil = 1",
        "getmetatable('').__index = {}",
        "getmetatable('').__newindex = function() end",
        "getmetatable('').__metatable = nil",
        "local s = getmetatable('') s.__index.rep = nil",
    ];
    let mut src = String::from("local results = {}\n");
    for (i, attempt) in attempts.iter().enumerate() {
        let _ = writeln!(src, "results[{}] = pcall(function() {attempt} end)", i + 1);
    }
    src.push_str(
        r#"
        app.on("results", function() return results end)
        app.on("intact", function()
          return {
            string.rep("ab", 2) == "abab", ("x"):rep(3) == "xxx", string.find("abc", "b") == 2,
            string.evil == nil, table.insert ~= nil, math.floor(1.5) == 1, bit.band(6, 3) == 2, utf8.len("é") == 1,
            type(getmetatable("")) == "string", type(getmetatable(string)) == "string",
          }
        end)
    "#,
    );
    let vm = load(&src).await;
    let results = emit(&vm, "results").await.unwrap();
    for (i, attempt) in attempts.iter().enumerate() {
        assert_eq!(results[i], json!(false), "`{attempt}` must raise");
    }
    assert_eq!(emit(&vm, "intact").await.unwrap(), json!(vec![true; 10]));
}

#[tokio::test]
async fn the_real_library_tables_are_not_reachable_from_the_app() {
    // a handler that tries every path to the engine's shared tables; none may expose a writable one
    let src = r#"
        local reachable = {}
        local function probe(name, fn) local ok, t = pcall(fn) if ok and type(t) == "table" then reachable[#reachable + 1] = name end end
        probe("getmetatable(string)", function() return getmetatable(string) end)
        probe("getmetatable('')", function() return getmetatable("") end)
        probe("getmetatable('').__index", function() return getmetatable("").__index end)
        probe("string.__index", function() return string.__index end)
        probe("(''):__index", function() return ("").__index end)
        probe("_G.string.__index", function() return _G.string.__index end)
        probe("getmetatable(_G)", function() return getmetatable(_G) end)
        probe("getmetatable(app)", function() return getmetatable(app) end)
        probe("_G == getfenv", function() return getfenv and getfenv(1) end)
        app.on("report", function() return reachable end)
    "#;
    let vm = load(src).await;
    assert_eq!(emit(&vm, "report").await.unwrap(), json!([]));
}

#[tokio::test]
async fn an_apps_global_writes_stay_in_its_own_environment() {
    let src = r#"
        shared = (shared or 0) + 1
        app.on("n", function() shared = shared + 1 return shared end)
    "#;
    let a = try_load("alpha", src).await.unwrap();
    let b = try_load("beta", src).await.unwrap();
    assert_eq!(emit(&a, "n").await.unwrap(), json!(2));
    assert_eq!(emit(&a, "n").await.unwrap(), json!(3));
    assert_eq!(emit(&b, "n").await.unwrap(), json!(2), "a second VM has its own globals");
}

#[tokio::test]
async fn the_preludes_polyfills_are_there_and_sealed() {
    let src = r#"
        app.on("t", function()
          local clone = table.clone({ 1, 2 })
          return {
            typeof = typeof("x"), utf8 = utf8.len("h\195\169llo"), clone = #clone, pack = table.pack(1, nil, 3).n,
            moved = table.concat(table.move({ 1, 2, 3 }, 1, 3, 2), ","), writable = pcall(function() utf8.evil = 1 end),
          }
        end)
    "#;
    let vm = load(src).await;
    assert_eq!(
        emit(&vm, "t").await.unwrap(),
        json!({"typeof": "string", "utf8": 5, "clone": 2, "pack": 3, "moved": "1,1,2,3", "writable": false})
    );
}

// ---- the CPU budget in every kind of thread ---------------------------------------------------------------

#[tokio::test]
async fn a_runaway_coroutine_is_killed_by_the_budget() {
    let src = r#"
        app.on("wrap", function() coroutine.wrap(function() while true do end end)() end)
        app.on("resume", function()
          local ok, err = coroutine.resume(coroutine.create(function() while true do end end))
          return { ok, err }
        end)
        -- a coroutine that swallows the first kill and loops again is killed again: the kill is sticky
        app.on("persistent", function()
          while true do
            local co = coroutine.create(function() while true do end end)
            coroutine.resume(co)
          end
        end)
        app.on("ok", function() return 1 end)
    "#;
    let vm = try_load_with("corou", src, db(), quick(SLICE)).await.unwrap();
    for event in ["wrap", "persistent"] {
        let started = Instant::now();
        let err = emit(&vm, event).await.unwrap_err();
        assert!(budget_error(&err), "{event}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{event}: {:?}", started.elapsed());
    }
    // `resume` hands the error back to the app, which carries on: the budget stays spent
    let started = Instant::now();
    let out = emit(&vm, "resume").await;
    assert!(out.is_err() || out.as_ref().is_ok_and(|v| v[0] == json!(false)), "{out:?}");
    assert!(started.elapsed() < PROMPTLY);
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!(1), "the next call has a fresh budget");
}

#[tokio::test]
async fn a_pcall_inside_a_coroutine_inside_a_pcall_cannot_swallow_the_kill() {
    let src = r#"
        app.on("evil", function()
          while true do
            pcall(function()
              local co = coroutine.wrap(function() pcall(function() while true do end end) end)
              co()
            end)
          end
        end)
        app.on("xpcall", function() while true do xpcall(function() while true do end end, function() end) end end)
    "#;
    let vm = try_load_with("nest", src, db(), quick(SLICE)).await.unwrap();
    for event in ["evil", "xpcall"] {
        let started = Instant::now();
        let err = emit(&vm, event).await.unwrap_err();
        assert!(budget_error(&err), "{event}: {err}");
        assert!(started.elapsed() < PROMPTLY);
    }
}

#[tokio::test]
async fn the_hook_runs_after_an_async_host_call_and_in_coroutines_created_after_it() {
    let src = r#"
        app.tools.register { name = "after_await", run = function()
          app.db:all("select 1 as x")      -- the rest of the call runs on mlua's async thread
          while true do end
        end }
        app.tools.register { name = "coroutine_after_await", run = function()
          app.kv:get("k")
          coroutine.wrap(function() while true do end end)()
        end }
        app.tools.register { name = "callback", run = function()
          app.kv:get("k")
          local t = {}
          for i = 1, 1000 do t[i] = i end
          table.sort(t, function(a, b) while true do end end)
        end }
        app.tools.register { name = "gsub_callback", run = function()
          return (string.gsub("abc", "%w", function(c) while true do end end))
        end }
    "#;
    let vm = try_load_with("hook", src, db(), quick(SLICE)).await.unwrap();
    for tool in ["after_await", "coroutine_after_await", "callback", "gsub_callback"] {
        let started = Instant::now();
        let err = vm.call_tool(tool, json!({}), &agent_ctx(0), false).await.unwrap_err();
        assert!(budget_error(&err), "{tool}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{tool}: {:?}", started.elapsed());
    }
}

// ---- a host call that cannot yield is dropped half done: the budget must come back ------------------------

/// Lua 5.1 cannot yield out of a C boundary (`table.sort` is not one: it is written in Lua here, see
/// `host_calls_work_in_a_table_sort_comparison`), so a host call in one of these fails with "attempt to yield across
/// metamethod/C-call boundary" and its future is dropped. The "waiting on the host" state it had set must go
/// with it: if it stayed, nothing the app ran afterwards would be counted and `while true do end` would never end.
#[tokio::test]
async fn a_host_call_dropped_at_a_c_boundary_does_not_switch_the_budget_off() {
    let boundaries = [
        ("gsub", "string.gsub('abc', '%w', function(c) app.kv:get('k') return c end)"),
        ("gsub_method", "('abc'):gsub('b', function(c) app.kv:get('k') return c end)"),
        (
            "index",
            "local t = setmetatable({}, { __index = function() app.kv:get('k') return 1 end }) return t.x",
        ),
        ("newindex", "local t = setmetatable({}, { __newindex = function() app.kv:get('k') end }) t.x = 1"),
        (
            "arith",
            "local t = setmetatable({}, { __add = function() app.kv:get('k') return 1 end }) return t + 1",
        ),
        (
            "tostring",
            "return tostring(setmetatable({}, { __tostring = function() app.kv:get('k') return 'x' end }))",
        ),
        ("iterator", "for _ in function() app.kv:get('k') end do end"),
    ];
    let mut src = String::new();
    for (name, body) in &boundaries {
        write!(
            src,
            "app.tools.register {{ name = '{name}', run = function()
               local ok, why = pcall(function() {body} end)
               if ok then error('the host call inside a C boundary was expected to fail') end
               while true do end
             end }}\n"
        )
        .unwrap();
    }
    // only the slice can end these: the call's own budget is out of reach, so the "waiting on the host" state the
    // dropped call set must really be gone
    let limits = agentos_luau::Limits {
        cpu_ticks: SLICE,
        call_ticks: u64::MAX / 2,
        call_wall: Duration::from_secs(3600),
        ..Default::default()
    };
    let vm = try_load_with("dropped", &src, db(), limits).await.unwrap();
    for (name, _) in &boundaries {
        let started = Instant::now();
        let err = vm.call_tool(name, json!({}), &agent_ctx(0), false).await.unwrap_err();
        assert!(budget_error(&err), "{name}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{name}: {:?}", started.elapsed());
    }
}

/// A host call that failed at a C boundary leaves its future in the dead coroutine until the garbage collector
/// frees it, and that future can hold the database's lock: the next host call then waited ten seconds for it.
#[tokio::test]
async fn a_host_call_that_failed_at_a_c_boundary_does_not_block_the_next_one() {
    let src = r#"
        local function try(fn) local ok = pcall(fn) return ok end
        app.tools.register { name = "caught", run = function()
          local failed = try(function() return (string.gsub("a", "a", function() app.kv:get("k") return "b" end)) end)
          app.kv:set("after", "x")
          return { failed = failed, after = app.kv:get("after") }
        end }
        app.tools.register { name = "uncaught", run = function()
          return (string.gsub("a", "a", function() app.kv:get("k") return "b" end))
        end }
        app.tools.register { name = "next", run = function() app.kv:set("n", 1) return app.kv:get("n") end }
    "#;
    let vm = load(src).await;
    let started = Instant::now();
    let out = vm.call_tool("caught", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({"failed": false, "after": "x"}));
    let err = vm.call_tool("uncaught", json!({}), &agent_ctx(0), false).await.unwrap_err();
    assert!(err.to_string().contains("yield across"), "{err}");
    assert_eq!(vm.call_tool("next", json!({}), &agent_ctx(0), false).await.unwrap(), json!(1));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a left-behind future held the database: {:?}",
        started.elapsed()
    );
}

/// The same failure inside a coroutine the script keeps: the dead coroutine (and the future in it) stays alive,
/// so the slice stays "waiting", but the call's own budget still counts every tick and ends the loop.
#[tokio::test]
async fn a_left_behind_host_call_in_a_kept_coroutine_still_ends_at_the_calls_budget() {
    let src = r#"
        app.tools.register { name = "kept", run = function()
          local co = coroutine.create(function() string.gsub("a", "a", function() app.kv:get("k") end) end)
          coroutine.resume(co)
          while true do end
        end }
    "#;
    let limits = agentos_luau::Limits { cpu_ticks: SLICE, call_ticks: 3 * SLICE, ..Default::default() };
    let vm = try_load_with("kept", src, db(), limits).await.unwrap();
    let started = Instant::now();
    let err = vm.call_tool("kept", json!({}), &agent_ctx(0), false).await.unwrap_err();
    assert!(budget_error(&err), "{err}");
    assert!(started.elapsed() < PROMPTLY, "{:?}", started.elapsed());
}

#[tokio::test]
async fn host_calls_work_in_a_table_sort_comparison() {
    let src = r#"
        app.tools.register { name = "sorted", run = function()
          local calls = 0
          local t = { 3, 1, 2 }
          table.sort(t, function(a, b) calls = calls + 1 app.kv:set("last", a) return a < b end)
          return { t = t, hostCalls = calls > 0 and app.kv:get("last") ~= nil }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("sorted", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({"t": [1, 2, 3], "hostCalls": true}));
}

// ---- a stop cannot be swallowed -----------------------------------------------------------------------------

/// The hook stops a script by raising an error, and a script can catch it. The kill is sticky, but a script that
/// catches it and returns before the next hook tick (`xpcall(spin, spin); return n`) was reported as a success.
#[tokio::test]
async fn pcall_xpcall_and_resume_re_raise_a_budget_kill_whatever_comes_next() {
    let spellings = [
        ("xpcall_same_handler", "xpcall(spin, spin) return n"),
        ("xpcall_plain_handler", "xpcall(spin, function() return 1 end) return n"),
        ("pcall", "pcall(spin) return n"),
        ("pcall_nested", "pcall(function() pcall(function() xpcall(spin, spin) end) end) return n"),
        (
            "xpcall_in_pcall_in_xpcall",
            "xpcall(function() pcall(function() xpcall(spin, spin) end) return 1 end, spin) return n",
        ),
        ("resume", "coroutine.resume(coroutine.create(spin)) return n"),
        ("resume_pcall", "coroutine.resume(coroutine.create(function() pcall(spin) end)) return n"),
        ("wrap_pcall", "coroutine.wrap(function() pcall(spin) end)() return n"),
        ("pcall_in_handler", "xpcall(spin, function() pcall(spin) return 1 end) return n"),
    ];
    let mut src = String::from("local n = 0\nlocal function spin() while true do n = n + 1 end end\n");
    for (name, body) in &spellings {
        writeln!(src, "app.on('{name}', function() {body} end)").unwrap();
    }
    src.push_str(
        "app.on('ok', function() local ok, e = pcall(error, 'plain', 0) return { ok = ok, e = e } end)\n",
    );
    let vm = try_load_with("swallow", &src, db(), quick(SLICE)).await.unwrap();
    for (name, _) in &spellings {
        let started = Instant::now();
        let out = emit(&vm, name).await;
        let err =
            out.expect_err(&format!("{name}: a kill that was caught and ignored must still fail the call"));
        assert!(budget_error(&err), "{name}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{name}: {:?}", started.elapsed());
    }
    // and the next call is a new call: ordinary errors are caught as always
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!({"ok": false, "e": "plain"}));
}

// ---- work inside library calls is charged -----------------------------------------------------------------------

/// Each of these is one VM instruction that does work proportional to its argument, which no tick paid for: a
/// loop of them ran for as long as the wall clock allowed (or, `table.sort` of 200 000 elements, 33 seconds).
#[tokio::test]
async fn work_inside_library_calls_is_charged_so_a_loop_of_them_ends_at_the_budget() {
    let bodies = [
        ("upper", "local _ = big:upper()"),
        ("lower", "local _ = big:lower()"),
        ("reverse", "local _ = big:reverse()"),
        ("format", "local _ = string.format('%s', big)"),
        ("sub", "local _ = big:sub(2)"),
        ("concat", "local _ = table.concat(pieces)"),
        ("char_byte", "local _ = string.char(string.byte(big, 1, 7000))"),
        ("insert_front", "table.insert(pieces, 1, 'x') table.remove(pieces, 1)"),
        ("maxn", "local _ = table.maxn(pieces)"),
        ("sort_default", "table.sort(numbers)"),
        ("sort_compare", "table.sort(numbers, function(a, b) return a > b end)"),
        ("rep", "local _ = string.rep('xy', 2000000)"),
    ];
    let mut src = String::from(
        "local big = string.rep('x', 1024 * 1024)
         local pieces, numbers = {}, {}
         for i = 1, 100000 do pieces[i] = 'ab' end
         for i = 1, 2000 do numbers[i] = (i * 7919) % 2003 end\n",
    );
    for (name, body) in &bodies {
        writeln!(src, "app.on('{name}', function() for _ = 1, 1e9 do {body} end end)").unwrap();
    }
    // the walls are what the review measured: no case may outlive them
    let limits = agentos_luau::Limits {
        cpu_wall: Duration::from_secs(4),
        call_wall: Duration::from_secs(8),
        ..Default::default()
    };
    let vm = try_load_with("charged", &src, db(), limits.clone()).await.unwrap();
    for (name, _) in &bodies {
        let started = Instant::now();
        let err = emit(&vm, name).await.expect_err(name);
        assert!(budget_error(&err), "{name}: {err}");
        assert!(
            started.elapsed() < limits.cpu_wall,
            "{name} ran {:?}, past the slice's wall",
            started.elapsed()
        );
    }
}

#[tokio::test]
async fn table_sort_of_a_huge_array_is_stopped_by_the_budget_not_by_the_wall() {
    let src = r#"
        app.on("million", function()
          local t = {}
          for i = 1, 150000 do t[i] = (i * 7919) % 150001 end
          table.sort(t)
          return #t
        end)
        app.on("honest", function()
          local t = {}
          for i = 1, 5000 do t[i] = (i * 7919) % 5003 end
          table.sort(t)
          table.sort(t, function(a, b) return a > b end)
          return { t[1], t[5000] }
        end)
    "#;
    // the whole sort is ~300 000 ticks of VM work: more than this slice, far less than the walls
    let limits = agentos_luau::Limits {
        cpu_ticks: 100_000,
        cpu_wall: Duration::from_secs(4),
        call_wall: Duration::from_secs(8),
        ..Default::default()
    };
    let vm = try_load_with("hugesort", src, db(), limits.clone()).await.unwrap();
    let started = Instant::now();
    let err = emit(&vm, "million").await.expect_err("150 000 elements are past the slice's count");
    assert!(budget_error(&err), "{err}");
    assert!(started.elapsed() < limits.cpu_wall, "{:?}", started.elapsed());
    assert_eq!(emit(&vm, "honest").await.unwrap(), json!([5002, 1]));
}

// ---- nothing built outside the Lua heap outgrows the memory limit ----------------------------------------------

/// A `gsub` whose replacement function runs another `gsub` keeps one output buffer per level: a cap per call
/// let sixty levels build 920 MB against a 16 MiB limit. The buffers share one byte budget now.
#[tokio::test]
async fn nested_gsubs_end_with_a_memory_error_not_a_gigabyte_of_buffers() {
    let src = r#"
        app.on("nest", function()
          local chunk = string.rep("x", 1024 * 1024)
          local function nest(n)
            if n == 0 then return "leaf" end
            return (string.gsub(chunk .. "!", "!", function() return nest(n - 1) end))
          end
          return #nest(60)
        end)
        app.on("shallow", function()
          local function nest(n)
            if n == 0 then return "leaf" end
            return (string.gsub("ab!", "!", function() return nest(n - 1) end))
          end
          return nest(20)
        end)
        app.on("rep_in_gsub", function()
          local function nest(n)
            if n == 0 then return "leaf" end
            return (string.gsub("!", "!", function() return string.rep("y", 1024 * 1024) .. nest(n - 1) end))
          end
          return #nest(60)
        end)
    "#;
    let vm = load(src).await;
    for event in ["nest", "rep_in_gsub"] {
        let err = emit(&vm, event).await.expect_err(event).to_string().to_lowercase();
        assert!(err.contains("memory"), "{event}: {err}");
    }
    assert!(vm.used_memory() < 16 * 1024 * 1024 + 1024 * 1024, "{}", vm.used_memory());
    assert_eq!(
        emit(&vm, "shallow").await.unwrap(),
        json!("ab".repeat(20) + "leaf"),
        "a nest that fits still works"
    );
}

// ---- patterns and strings, which C code would run unchecked ------------------------------------------------

#[tokio::test]
async fn catastrophic_patterns_end_at_the_budget_in_every_spelling() {
    let src = r#"
        local subject = string.rep("a", 100000)
        app.on("find", function() return string.find(subject, ".-.-.-.-b") end)
        app.on("method", function() return subject:find(".-.-.-.-b") end)
        app.on("match", function() return subject:match("(a*)*b") end)
        app.on("gmatch", function() for _ in subject:gmatch("a*a*a*b") do end end)
        app.on("gsub", function() return subject:gsub(".-.-.-b", "x") end)
        app.on("optional", function() return string.rep("a", 40):match(string.rep("a?", 40) .. string.rep("a", 40) .. "b") end)
        app.on("ok", function() return ("hello world"):match("^(%w+)") end)
    "#;
    let vm = try_load_with("rex", src, db(), quick(SLICE)).await.unwrap();
    for event in ["find", "method", "match", "gmatch", "gsub", "optional"] {
        let started = Instant::now();
        let err = emit(&vm, event).await.unwrap_err();
        assert!(budget_error(&err), "{event}: {err}");
        assert!(started.elapsed() < PROMPTLY, "{event}: {:?}", started.elapsed());
    }
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!("hello"), "ordinary patterns are untouched");
}

#[tokio::test]
async fn a_pattern_too_deep_for_the_stack_is_an_error_not_a_crash() {
    let src = r#"app.on("deep", function() return string.find("aaaa", string.rep("a*", 100000)) end)"#;
    let vm = load(src).await;
    let err = emit(&vm, "deep").await.unwrap_err().to_string();
    assert!(err.contains("pattern too complex"), "{err}");
}

#[tokio::test]
async fn rep_cannot_loop_for_minutes_or_outgrow_the_memory_limit() {
    let src = r#"
        app.on("empty", function() return #string.rep("", 1e10) end)
        app.on("huge", function() return #string.rep("xxxxxxxx", 1e9) end)
        app.on("gsub", function() return #string.rep("a", 10 * 1024 * 1024):gsub("a", "bbbbbbbb") end)
        app.on("ok", function() return #string.rep("ab", 1000) end)
    "#;
    let limits = agentos_luau::Limits { memory_bytes: 8 * 1024 * 1024, ..Default::default() };
    let vm = try_load_with("rep", src, db(), limits).await.unwrap();
    let started = Instant::now();
    assert_eq!(emit(&vm, "empty").await.unwrap(), json!(0));
    for event in ["huge", "gsub"] {
        let err = emit(&vm, event).await.unwrap_err().to_string().to_lowercase();
        assert!(err.contains("memory"), "{event}: {err}");
    }
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!(2000));
    assert!(started.elapsed() < PROMPTLY);
}

// ---- numbers the host must never panic on -----------------------------------------------------------------

/// A script picks the numbers: huge, infinite, `NaN`, negative, reversed. Every library function that takes one
/// must answer with a Lua error or a value, never a panic or an abort of the host (an abort would take every
/// app down with it).
#[tokio::test]
async fn no_number_a_script_passes_to_a_library_function_panics_the_host() {
    let numbers =
        ["1e300", "-1e300", "1/0", "-1/0", "0/0", "2^63", "-2^63", "2^53", "2^31", "-1", "0", "0.5"];
    let calls = [
        "math.random(N)",
        "math.random(N, N)",
        "math.random(1, N)",
        "math.random(N, 1)",
        "math.randomseed(N)",
        "string.rep('xy', N)",
        "string.rep('', N)",
        "string.rep('x', 3, N)",
        "('abc'):sub(N)",
        "('abc'):sub(N, N)",
        "('abc'):sub(1, N)",
        "('abc'):byte(N)",
        "('abc'):byte(N, N)",
        "('abc'):byte(1, N)",
        "string.char(N)",
        "string.find('abc', 'b', N)",
        "string.find('abc', 'b', N, true)",
        "string.match('abc', '(b)', N)",
        "string.gsub('abc', 'b', 'x', N)",
        "string.gsub('abc', '', '-', N)",
        "string.format('%d', N)",
        "string.format('%5.2f', N)",
        "string.format('%c', N)",
        "string.format('%x', N)",
        "string.format('%' .. N .. 'd', 1)",
        "bit.lshift(1, N)",
        "bit.rshift(N, N)",
        "bit.arshift(N, N)",
        "bit.rol(N, N)",
        "bit.ror(N, N)",
        "bit.tobit(N)",
        "bit.bnot(N)",
        "bit.band(N, N)",
        "bit.tohex(N)",
        "bit.tohex(1, N)",
        "bit.bswap(N)",
        "table.concat({ 'a', 'b' }, ',', N)",
        "table.concat({ 'a', 'b' }, ',', 1, N)",
        "table.concat({ 'a', 'b' }, ',', N, N)",
        "unpack({ 1, 2 }, N)",
        "unpack({ 1, 2 }, 1, N)",
        "unpack({ 1, 2 }, N, N)",
        "select(N, 1, 2)",
        "table.insert({ 1 }, N, 2)",
        "table.remove({ 1 }, N)",
        "table.move({ 1, 2 }, N, N, N)",
        "table.move({ 1, 2 }, 1, N, 1)",
        "table.pack(1, 2).n + N",
        "math.floor(N) + math.ceil(N) + math.fmod(N, N) + math.max(N, N) + math.abs(N)",
        "math.fmod(1, 0)",
        "string.rep('x', 2^20 * 20)",
        "tostring(N)",
        "tonumber('1e999')",
        "tonumber('0x' .. string.rep('f', 100))",
    ];
    let mut src = String::new();
    for (i, call) in calls.iter().enumerate() {
        for (j, n) in numbers.iter().enumerate() {
            let call = call.replace('N', n);
            writeln!(
                src,
                "app.on('c{i}_{j}', function() local ok, e = pcall(function() return {call} end) return tostring(ok) end)"
            )
            .unwrap();
        }
    }
    let limits = agentos_luau::Limits { memory_bytes: 8 * 1024 * 1024, ..Default::default() };
    let vm = try_load_with("numbers", &src, db(), limits).await.unwrap();
    let started = Instant::now();
    for (i, call) in calls.iter().enumerate() {
        for (j, number) in numbers.iter().enumerate() {
            let out = emit(&vm, &format!("c{i}_{j}")).await;
            // a value, a Lua error the script caught, or (a loop over a huge range) the budget's stop: the host
            // is alive in every case, and that is what is asserted
            assert!(out.is_ok() || out.as_ref().is_err_and(budget_error), "{call} with {number}: {out:?}");
        }
    }
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}

// ---- the memory limit ---------------------------------------------------------------------------------------

#[tokio::test]
async fn the_memory_limit_holds_in_coroutines_and_for_every_allocator() {
    let limits = agentos_luau::Limits { memory_bytes: 4 * 1024 * 1024, ..Default::default() };
    let src = r#"
        app.on("table", function() local t = {} for i = 1, 1e7 do t[i] = { i } end end)
        app.on("coroutine", function()
          coroutine.wrap(function() local t = {} for i = 1, 1e7 do t[i] = string.rep("x", 100) .. i end end)()
        end)
        app.on("concat", function()
          local parts = {} for i = 1, 100 do parts[i] = string.rep("x", 100000) end
          local s = table.concat(parts)
          for _ = 1, 20 do s = s .. s end
        end)
        app.on("closures", function() local t = {} for i = 1, 1e7 do t[i] = function() return i end end end)
        app.on("ok", function() return 1 end)
    "#;
    let vm = try_load_with("mem", src, db(), limits).await.unwrap();
    for event in ["table", "coroutine", "concat", "closures"] {
        let started = Instant::now();
        let err = emit(&vm, event).await.unwrap_err().to_string().to_lowercase();
        assert!(err.contains("memory"), "{event}: {err}");
        assert!(started.elapsed() < PROMPTLY);
    }
    vm.gc();
    assert!(vm.used_memory() < 5 * 1024 * 1024, "{}", vm.used_memory());
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!(1), "the VM survives its own allocation failures");
}

// ---- async host calls where Lua 5.1 cannot yield ------------------------------------------------------------

#[tokio::test]
async fn host_calls_that_await_work_inside_pcall_coroutines_and_event_handlers() {
    let src = r#"
        app.db:migrate { [1] = "create table n(id integer primary key, body text)" }
        app.tools.register { name = "in_pcall", run = function()
          local ok, rows = pcall(function() return app.db:all("select 1 as x") end)
          local failed, why = pcall(function() app.db:exec("insert into nope values (1)") end)
          local nested = select(2, pcall(function() return select(2, pcall(function() app.kv:set("a", 1) return app.kv:get("a") end)) end))
          return { ok = ok, rows = rows, failed = failed, why = tostring(why), nested = nested }
        end }
        app.tools.register { name = "in_xpcall", run = function()
          local ok, value = xpcall(function() app.kv:set("k", "v") return app.kv:get("k") end, function(e) return "handled: " .. tostring(e) end)
          local bad, handled = xpcall(function() app.db:exec("select * from nope") end, function(e) return "handled" end)
          return { ok = ok, value = value, bad = bad, handled = handled }
        end }
        app.tools.register { name = "in_coroutine", run = function()
          local co = coroutine.create(function()
            local first = app.db:all("select 1 as x")
            coroutine.yield(#first)
            app.kv:set("co", "done")
            return app.kv:get("co")
          end)
          local _, a = coroutine.resume(co)
          local _, b = coroutine.resume(co)
          return { a = a, b = b, status = coroutine.status(co) }
        end }
        app.tools.register { name = "in_wrap", run = function()
          local gen = coroutine.wrap(function() for i = 1, 3 do app.kv:set("i", i) coroutine.yield(app.kv:get("i")) end end)
          return { gen(), gen(), gen() }
        end }
        app.on("event", function(payload)
          local ok, v = pcall(function() app.kv:set("e", payload.n) return app.kv:get("e") end)
          return { ok = ok, v = v }
        end)
    "#;
    let vm = load(src).await;
    let call = |tool: &'static str| {
        let vm = &vm;
        async move { vm.call_tool(tool, json!({}), &agent_ctx(0), false).await.unwrap() }
    };
    let out = call("in_pcall").await;
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["rows"], json!([{"x": 1}]), "{out}");
    assert_eq!(out["failed"], json!(false), "{out}");
    assert!(out["why"].as_str().unwrap().contains("no such table"), "{out}");
    assert_eq!(out["nested"], json!(1), "{out}");

    let out = call("in_xpcall").await;
    assert_eq!(out, json!({"ok": true, "value": "v", "bad": false, "handled": "handled"}));

    let out = call("in_coroutine").await;
    assert_eq!(out, json!({"a": 1, "b": "done", "status": "dead"}), "a host call inside a user coroutine");
    assert_eq!(call("in_wrap").await, json!([1, 2, 3]));

    let out = vm.emit("event", json!({"n": 5})).await.unwrap();
    assert_eq!(out, json!({"ok": true, "v": 5}));
}

#[tokio::test]
async fn the_proxies_apps_index_never_yield_so_they_work_on_5_1() {
    // app.state, a window's state / client / env, and a source handle are tables with __index / __newindex
    // functions: Lua 5.1 cannot yield across a metamethod, so they must stay synchronous Rust functions
    let src = r#"
        local ui = require("@agentos/ui")
        local s = app.state { n = 0, items = {} }
        app.window("main", {
          client = { search = { init = "", persist = "app" } },
          render = function(w)
            local seen = s.n + #w.state.items
            return ui.VStack { key = "root", ui.Text { "n=" .. tostring(seen), key = "t" },
              ui.TextField { key = "q", text = w.client.search, label = "Search" } }
          end,
        })
        app.tools.register { name = "bump", run = function()
          s.n = s.n + 1
          local viaPcall = select(2, pcall(function() s.n = s.n + 1 return s.n end))
          app.kv:set("n", s.n)
          s.n = app.kv:get("n") + 1
          return { n = s.n, viaPcall = viaPcall }
        end }
    "#;
    let host_modules =
        std::collections::HashMap::from([("ui".to_owned(), agentos_luau::UI_MODULE.as_bytes().to_vec())]);
    let config =
        agentos_luau::VmConfig { host_modules: std::sync::Arc::new(host_modules), ..Default::default() };
    let vm = agentos_luau::AppVm::load(&common::id("probe"), common::package(src, &[]), db(), config)
        .await
        .unwrap();
    let out = vm.call_tool("bump", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({"n": 3, "viaPcall": 2}));
    assert_eq!(vm.window_names(), ["main"]);
}

#[tokio::test]
async fn dropping_a_call_that_waits_inside_coroutines_ends_it_without_running_app_code_again() {
    let src = r#"
        app.tools.register { name = "spin", run = function()
          while true do
            local gen = coroutine.wrap(function() pcall(function() app.kv:get("k") end) coroutine.yield(1) end)
            gen()
          end
        end }
        app.on("ok", function() return 42 end)
    "#;
    let vm = std::sync::Arc::new(load(src).await);
    // cut the call off wherever it happens to be waiting; each cancellation must complete at once
    for millis in [20, 35, 50] {
        let ctx = agent_ctx(0);
        let call = vm.call_tool("spin", json!({}), &ctx, false);
        let started = Instant::now();
        let _ = tokio::time::timeout(Duration::from_millis(millis), call).await;
        assert!(started.elapsed() < PROMPTLY, "cancelling took {:?}", started.elapsed());
    }
    assert_eq!(emit(&vm, "ok").await.unwrap(), json!(42), "the VM is usable after being cut off");
}

#[tokio::test]
async fn a_host_call_where_lua_5_1_cannot_yield_fails_with_a_clear_error_not_a_hang() {
    // the known limits: callbacks code in Rust runs (gsub) and `for ... in` iterator functions
    let src = r#"
        app.tools.register { name = "limits", run = function()
          local function message(fn) local ok, e = pcall(fn) return ok and "worked" or tostring(e) end
          return {
            gsub = message(function() return (string.gsub("a", "a", function() app.kv:get("k") return "b" end)) end),
            iterator = message(function() for _ in function() app.kv:get("k") end do break end end),
          }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("limits", json!({}), &agent_ctx(0), false).await.unwrap();
    for field in ["gsub", "iterator"] {
        let message = out[field].as_str().unwrap();
        assert!(message.contains("yield across"), "{field}: {message}");
    }
}

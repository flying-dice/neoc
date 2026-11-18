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
    // the known limits: callbacks C code runs (gsub, sort comparators) and `for ... in` iterator functions
    let src = r#"
        app.tools.register { name = "limits", run = function()
          local function message(fn) local ok, e = pcall(fn) return ok and "worked" or tostring(e) end
          return {
            gsub = message(function() return (string.gsub("a", "a", function() app.kv:get("k") return "b" end)) end),
            sort = message(function() table.sort({ 3, 2, 1 }, function(a, b) app.kv:get("k") return a < b end) end),
            iterator = message(function() for _ in function() app.kv:get("k") end do break end end),
          }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("limits", json!({}), &agent_ctx(0), false).await.unwrap();
    for field in ["gsub", "sort", "iterator"] {
        let message = out[field].as_str().unwrap();
        assert!(message.contains("yield across"), "{field}: {message}");
    }
}

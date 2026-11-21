#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! Luau notes (the VM is `neoc-vm`, Luau, here; the apps are Lua 5.1 syntax): the tests that were PUC Lua 5.1 facts
//! hold on Luau, whose yield boundaries are those of PUC 5.1, with these differences, each said where it matters:
//! a Luau bytecode chunk starts with a version byte (`string.dump` does not exist), `bit` is `bit32`, the pattern
//! matcher bounds its own recursion (`pattern too complex`: no guard of ours, no abort), a wait suspends inside
//! `pcall`, `xpcall`, `__call` and the iterator function of a `for ... in` (PUC 5.1 refused it in an iterator) and is
//! refused in every other metamethod, in `tostring`, in a `string.gsub` or `table.sort` callback and in the handler of
//! an `xpcall`; Luau has no proper tail calls and messages name no variable.
//!
//! What running on this VM changes: the sandbox of an app's environment (nothing is budgeted: apps get Luau's own
//! `string` and `table` functions), and async host calls where the engine cannot yield.

mod common;

use vos_lua::VmError;
use common::{agent_ctx, db, load, try_load, try_load_with};
use serde_json::{json, Value};
use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

/// "Ended, not hung": far above what any of these takes, even on a busy machine.
const PROMPTLY: Duration = Duration::from_secs(8);

async fn emit(vm: &vos_lua::AppVm, event: &str) -> Result<Value, VmError> {
    vm.emit(event, json!(null)).await
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
    // Luau has no `string.dump`: the fixture is what its compiler writes for `return 7`, which starts with a version
    // byte (never `0`, the compiler's error marker) and not with source text (PUC Lua 5.1: `\x1bLua`)
    let bytecode = neoc_vm::mlua::chunk::Compiler::new().set_debug_level(0).compile("return 7").unwrap();
    assert!(bytecode[0] != 0 && bytecode[0] < b' ', "the fixture must be Luau bytecode: {bytecode:?}");
    let package = vos_application::ports::AppPackage {
        entry: "app.lua".into(),
        files: [("app.lua".to_owned(), bytecode)].into(),
    };
    let err = vos_lua::AppVm::load(&common::id("probe"), package, db(), common::config())
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
        "bit32.band = nil",
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
            string.evil == nil, table.insert ~= nil, math.floor(1.5) == 1, bit32.band(6, 3) == 2, utf8.len("é") == 1,
            type(getmetatable("")) == "string", getmetatable(string) == nil,
          }
        end)
    "#,
    );
    let vm = load(&src).await;
    let results = emit(&vm, "results").await.unwrap();
    for (i, attempt) in attempts.iter().enumerate() {
        assert_eq!(results[i], json!(false), "`{attempt}` must raise");
    }
    // the string metatable is locked (`getmetatable('')` answers a string, not the table); a library table has no
    // metatable on Luau: it is sealed by Luau's `readonly` flag, which the attempts above show nothing gets round
    // (LuaJIT sealed it with a locked metatable, so `getmetatable(string)` answered a string there)
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
            typeof = typeof("x"), typeof_table = typeof({}), typeof_nil = typeof(nil),
            typeof_userdata = typeof(require("lib:json").object()), utf8 = utf8.len("h\195\169llo"), clone = #clone, pack = table.pack(1, nil, 3).n,
            moved = table.concat(table.move({ 1, 2, 3 }, 1, 3, 2), ","), writable = pcall(function() utf8.evil = 1 end),
          }
        end)
    "#;
    let vm = load(src).await;
    assert_eq!(
        emit(&vm, "t").await.unwrap(),
        json!({"typeof": "string", "typeof_table": "table", "typeof_nil": "nil", "typeof_userdata": "userdata", "utf8": 5, "clone": 2, "pack": 3, "moved": "1,1,2,3", "writable": false})
    );
}

// ---- patterns and strings, which C code would run unchecked ------------------------------------------------

/// Luau's matcher bounds its own recursion and its captures, so no guard of ours is in the way (the deleted
/// `lua::patterns` refused a pattern of more than 512 items before the matcher saw it). A pattern of 20000 items either
/// answers (the matcher got through it: `a*` repeated does) or is refused with the matcher's own message
/// (`pattern too complex` for `a?` repeated, `too many captures` for `(a)` repeated); what it must never do is run the host out of stack. PUC Lua 5.1's C pattern matcher recurses
/// with no depth limit: `("a?"):rep(20000)` overflowed a 2 MiB stack (a `#[tokio::test]` runs on one) and aborted the
/// process. In an app it is a Lua value or a Lua error the app can catch. Each of the six calls must be one or the
/// other, and the error one must be the matcher's refusal, not some other failure.
#[tokio::test]
async fn a_huge_pattern_in_an_app_is_a_catchable_error_not_a_stack_overflow() {
    let src = r#"
        app.on("t", function()
          local subject = ("a"):rep(20000)
          local results = {}
          local function refused(e)
            local text = tostring(e)
            return text:find("pattern too complex", 1, true) ~= nil or text:find("too many captures", 1, true) ~= nil
          end
          for _, item in ipairs { "a?", "a*", "(a)" } do
            local pattern = item:rep(20000)
            local ok, err = pcall(string.find, subject, pattern)
            results[#results + 1] = { ok = ok, refused = refused(err), err = ok and "" or tostring(err) }
            ok, err = pcall(function() return subject:gsub(pattern, "") end)
            results[#results + 1] = { ok = ok, refused = refused(err), err = ok and "" or tostring(err) }
          end
          return results
        end)
    "#;
    let vm = load(src).await;
    let out = emit(&vm, "t").await.unwrap();
    let out = out.as_array().unwrap();
    assert_eq!(out.len(), 6);
    assert!(out.iter().all(|r| r["ok"] == json!(true) || r["refused"] == json!(true)), "{out:?}");
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
        // Luau's `bit32` (LuaJIT's `bit` is gone): the same shapes, over the functions it has
        "bit32.lshift(1, N)",
        "bit32.rshift(N, N)",
        "bit32.arshift(N, N)",
        "bit32.lrotate(N, N)",
        "bit32.rrotate(N, N)",
        "bit32.bnot(N)",
        "bit32.band(N, N)",
        "bit32.extract(N, N)",
        "bit32.extract(1, N, N)",
        "bit32.countlz(N)",
        "bit32.byteswap(N)",
        "table.concat({ 'a', 'b' }, ',', N)",
        "table.concat({ 'a', 'b' }, ',', 1, N)",
        "table.concat({ 'a', 'b' }, ',', N, N)",
        "unpack({ 1, 2 }, N)",
        "unpack({ 1, 2 }, 1, N)",
        "unpack({ 1, 2 }, N, N)",
        "select(N, 1, 2)",
        "math.floor(N) + math.ceil(N) + math.fmod(N, N) + math.max(N, N) + math.abs(N)",
        "math.fmod(1, 0)",
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
    let vm = try_load_with("numbers", &src, db()).await.unwrap();
    let started = Instant::now();
    for (i, call) in calls.iter().enumerate() {
        for (j, number) in numbers.iter().enumerate() {
            let out = emit(&vm, &format!("c{i}_{j}")).await;
            // a value or a Lua error the script caught: the host is alive in every case, and that is what is
            // asserted
            assert!(out.is_ok(), "{call} with {number}: {out:?}");
        }
    }
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}

// ---- the memory limit ---------------------------------------------------------------------------------------

// ---- async host calls, and where the engine cannot yield ------------------------------------------------------------

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
async fn the_proxies_apps_index_never_yield_so_they_work_anywhere() {
    // app.state (and a window's state / client / env and a source handle, which this test does not render) are
    // tables with __index / __newindex functions: they are plain synchronous Rust functions (PUC Lua 5.1 could not yield across a metamethod, which is
    // why; Luau still cannot). What shows that they never wait is a place where Luau refuses a yield: a `string.gsub`
    // callback. A proxy that suspended would fail there with "attempt to yield across C-call boundary".
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
          local inGsub, why = pcall(string.gsub, "a", "a", function() s.n = s.n + 1 return tostring(s.n) end)
          return { n = s.n, viaPcall = viaPcall, inGsub = inGsub, why = tostring(why) }
        end }
    "#;
    let host_modules =
        std::collections::HashMap::from([("ui".to_owned(), vos_lua::UI_MODULE.as_bytes().to_vec())]);
    let config =
        vos_lua::VmConfig { host_modules: std::sync::Arc::new(host_modules), ..Default::default() };
    let vm = vos_lua::AppVm::load(&common::id("probe"), common::package(src, &[]), db(), config)
        .await
        .unwrap();
    let out = vm.call_tool("bump", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out["n"], 4, "{out}");
    assert_eq!(out["viaPcall"], 2, "{out}");
    assert_eq!(out["inGsub"], true, "a state proxy waited where the engine cannot yield: {out}");
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
async fn a_host_call_where_lua_cannot_yield_fails_with_a_clear_error_not_a_hang() {
    // The places Luau refuses a yield: the C functions that call back into Lua (a gsub callback, a sort comparator),
    // a metamethod other than `__call` (`__index`, `__add`, ...), `tostring` of an object whose `__tostring` waits,
    // and the handler of an `xpcall`. (PUC Lua 5.1 refused an iterator function too; Luau suspends in one, see
    // `host_calls_in_iterators_and_call_metamethods_yield`. `string.format("%s", obj)` is not a row: Luau's `%s`
    // takes only strings and numbers, so it fails before it could call `__tostring`.)
    // The host call inside each boundary is a sleep, which is always pending on its first poll, so it always has
    // to suspend. `app.kv:get` is not: its future can be ready at once, and then there is no yield to refuse and
    // the call "worked" (NEOC-121).
    let src = r#"
        local time = require("std:time")
        local function wait() time.sleep(time.Duration.from_millis(1)) end
        app.tools.register { name = "limits", run = function()
          local function message(fn) local ok, e = pcall(fn) return ok and "worked" or tostring(e) end
          return {
            gsub = message(function() return (string.gsub("a", "a", function() wait() return "b" end)) end),
            sort = message(function() table.sort({ 2, 1 }, function(a, b) wait() return a < b end) end),
            index = message(function() return setmetatable({}, { __index = function() wait() return 1 end }).x end),
            add = message(function() return setmetatable({}, { __add = function() wait() return 1 end }) + 1 end),
            tostring = message(function() return tostring(setmetatable({}, { __tostring = function() wait() return "s" end })) end),
            xpcall_handler = select(2, xpcall(function() error("e") end, function(m) wait() return m end)),
          }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("limits", json!({}), &agent_ctx(0), false).await.unwrap();
    for field in ["gsub", "sort", "index", "add", "tostring"] {
        let message = out[field].as_str().unwrap();
        assert!(message.contains("yield across"), "{field}: {message}");
    }
    // a wait refused inside the message handler is an error in the handler: xpcall reports it, the script goes on
    assert_eq!(out["xpcall_handler"], json!("error in error handling"), "{out}");
}

/// Luau suspends in the iterator function of a `for ... in` and in `__call`, which PUC Lua 5.1 refused in an iterator:
/// an async host call there is an ordinary wait, not an error. (Every other metamethod refuses it, see the test
/// above.) The calls here are `app.kv` ones, which may be ready at once; the test below shows that these contexts
/// suspend.
#[tokio::test]
async fn host_calls_in_iterators_and_call_metamethods_yield() {
    let src = r#"
        app.tools.register { name = "yields", run = function()
          local callable = setmetatable({}, { __call = function() app.kv:set("k", "call") return app.kv:get("k") end })
          local seen = {}
          for v in function() if #seen < 2 then seen[#seen + 1] = app.kv:get("k") or "iter" return #seen end end do end
          return { call = callable(), iterations = #seen }
        end }
    "#;
    let vm = load(src).await;
    let out = vm.call_tool("yields", json!({}), &agent_ctx(0), false).await.unwrap();
    assert_eq!(out, json!({ "call": "call", "iterations": 2 }));
}

/// The test above shows that a call in an iterator or `__call` works, not that it suspended (`app.kv:get` can be ready
/// on its first poll). A sleep always suspends, so here the call that makes it must lend the turn. Each context has a
/// tool of its own that sleeps 300 ms inside it, and a second tool is called 50 ms in: it gets in at once, which it
/// could not if the first held the turn. (One tool with both would let the second call land in the first nap only.)
#[tokio::test]
async fn a_host_call_in_an_iterator_or_a_call_metamethod_lends_the_turn_while_it_waits() {
    let src = r#"
        local time = require("std:time")
        local function nap() time.sleep(time.Duration.from_millis(300)) return "x" end
        app.tools.register { name = "iterator", run = function()
          for _ in function() nap() return nil end do end
          return "x"
        end }
        app.tools.register { name = "call", run = function()
          return setmetatable({}, { __call = function() return nap() end })()
        end }
        app.tools.register { name = "ping", run = function() return "pong" end }
    "#;
    let vm = load(src).await;
    let ctx = agent_ctx(0);
    for context in ["iterator", "call"] {
        let first = vm.call_tool(context, json!({}), &ctx, false);
        let second = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let started = Instant::now();
            let out = vm.call_tool("ping", json!({}), &ctx, false).await.unwrap();
            (out, started.elapsed())
        };
        let (done, (pong, waited)) = tokio::join!(first, second);
        assert_eq!(done.unwrap(), json!("x"), "{context}");
        assert_eq!(pong, json!("pong"));
        assert!(
            waited < Duration::from_millis(150),
            "{context}: the second call waited {waited:?}: the first held the turn"
        );
    }
}

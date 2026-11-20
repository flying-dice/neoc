#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! What a program that embeds the VM gets, proven on LuaJIT: the call model of `lifecycle` (a turn at a time
//! in one VM, lent at the waits that suspend and kept where LuaJIT refuses a yield), the script environment of
//! `script_env`, the lifetimes of what a script starts (`Life`), the log sink and the module registry seam.
//!
//! Nothing here is a product policy: the embedder in these tests is a mutex and a counter. The same rules are
//! exercised end to end by a real embedder's own generated-program tests.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use mlua::{Function, Lua, Table, Value};
use neoc_vm::lua::{
    lifecycle::{
        Baton, BoxFuture, CallEntry, IdleKind, Inherited, Life, Role, async_function, hold, idle,
        scoped, set_entry,
    },
    modules,
    sandbox::{install_runtime_state, make_bare_engine, make_engine, make_engine_in},
    script_env,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

// ---- an embedder made of a mutex and a counter -----------------------------------------------------------------

/// The queue of calls into one VM: one at a time, through a mutex.
struct Gate {
    gate: Arc<Mutex<()>>,
    /// How many times a call handed its turn back (`Baton::park`): the exact count of lends, so a test does not
    /// have to catch another caller getting in.
    parks: Arc<AtomicUsize>,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            gate: Arc::new(Mutex::new(())),
            parks: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn turn(&self, held: Option<OwnedMutexGuard<()>>) -> Turn {
        Turn {
            gate: self.gate.clone(),
            parks: self.parks.clone(),
            held,
            waiting: None,
        }
    }
}

struct Turn {
    gate: Arc<Mutex<()>>,
    parks: Arc<AtomicUsize>,
    held: Option<OwnedMutexGuard<()>>,
    waiting: Option<BoxFuture<'static, OwnedMutexGuard<()>>>,
}

impl Baton for Turn {
    fn id(&self) -> u64 {
        1
    }
    fn holding(&self) -> bool {
        self.held.is_some()
    }
    fn inherited(&self) -> Inherited {
        Inherited::default()
    }
    fn can_park(&self) -> Result<(), String> {
        Ok(())
    }
    fn park(&mut self) {
        self.parks.fetch_add(1, Ordering::SeqCst);
        self.held = None;
    }
    fn idle_at(&mut self, _: IdleKind) {}
    fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.held.is_none() {
            let gate = self.gate.clone();
            let waiting = self
                .waiting
                .get_or_insert_with(|| Box::pin(gate.lock_owned()));
            let Poll::Ready(guard) = waiting.as_mut().poll(cx) else {
                return Poll::Pending;
            };
            self.waiting = None;
            self.held = Some(guard);
        }
        Poll::Ready(())
    }
}

impl CallEntry for Gate {
    fn enter(&self, _: Inherited) -> BoxFuture<'static, Box<dyn Baton>> {
        let gate = self.gate.clone();
        let turn = self.turn(None);
        Box::pin(async move {
            let held = gate.lock_owned().await;
            Box::new(Turn {
                held: Some(held),
                ..turn
            }) as Box<dyn Baton>
        })
    }
    fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
        self.enter(Inherited::default())
    }
    fn detached(&self, _: Inherited) -> Box<dyn Baton> {
        Box::new(self.turn(None))
    }
}

/// An engine whose calls go through a [`Gate`], and a *bumper*: another caller that takes a turn whenever the VM is
/// free and counts it. A script that holds the turn through a wait sees the count stand still; one that lends it
/// sees the count move.
struct Rig {
    lua: Lua,
    entry: Arc<Gate>,
    bumps: Arc<AtomicUsize>,
}

impl Rig {
    fn new() -> Self {
        let lua = make_engine_in(Life::new()).expect("engine");
        let entry = Gate::new();
        set_entry(&lua, entry.clone());
        let bumps = Arc::new(AtomicUsize::new(0));
        let counter = bumps.clone();
        lua.globals()
            .set(
                "BUMPS",
                lua.create_function(move |_, (): ()| Ok(counter.load(Ordering::SeqCst)))
                    .unwrap(),
            )
            .unwrap();
        let parks = entry.parks.clone();
        lua.globals()
            .set(
                "PARKS",
                lua.create_function(move |_, (): ()| Ok(parks.load(Ordering::SeqCst)))
                    .unwrap(),
            )
            .unwrap();
        // an async host function that uses the VM and suspends once, without being an idle wait: what the
        // embedder's own database call is
        lua.globals()
            .set(
                "HOST_VM",
                async_function(&lua, Role::Vm, |_, (): ()| async {
                    tokio::task::yield_now().await;
                    Ok(())
                })
                .unwrap(),
            )
            .unwrap();
        // a host function that calls back into Lua synchronously: the C boundary every embedder has
        lua.globals()
            .set(
                "HOST_CALLS",
                lua.create_function(|_, f: Function| f.call::<()>(()))
                    .unwrap(),
            )
            .unwrap();
        Self { lua, entry, bumps }
    }

    /// Run `script` as one call, with the bumper trying for the turn all the while.
    async fn run(&self, script: &str) -> Table {
        let bumper = {
            let entry = self.entry.clone();
            let bumps = self.bumps.clone();
            tokio::spawn(async move {
                loop {
                    drop(entry.enter(Inherited::default()).await);
                    bumps.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
        };
        let baton = self.entry.enter(Inherited::default()).await;
        let out = scoped(baton, self.lua.load(script).eval_async::<Table>())
            .await
            .expect("script");
        bumper.abort();
        out
    }
}

/// Where a host call (a wait of 50 ms) runs, with whether LuaJIT can suspend there: the boundaries this VM claims to
/// know. (Metamethods other than these were not measured and are not claimed.)
const PLACES: &[(&str, &str, bool)] = &[
    ("pcall", "pcall(function() WAIT end)", true),
    ("xpcall body", "xpcall(function() WAIT end, print)", true),
    (
        "__index",
        "local _ = setmetatable({}, { __index = function() WAIT end }).x",
        true,
    ),
    (
        "__newindex",
        "setmetatable({}, { __newindex = function() WAIT end }).x = 1",
        true,
    ),
    (
        "__call",
        "setmetatable({}, { __call = function() WAIT end })()",
        true,
    ),
    (
        "__add",
        "local _ = setmetatable({}, { __add = function() WAIT return 1 end }) + 1",
        true,
    ),
    (
        "__concat",
        "local _ = setmetatable({}, { __concat = function() WAIT return 'x' end }) .. 'a'",
        true,
    ),
    (
        "__eq",
        "local f = function() WAIT return true end local _ = setmetatable({}, { __eq = f }) == setmetatable({}, { __eq = f })",
        true,
    ),
    (
        "__lt",
        "local f = function() WAIT return true end local _ = setmetatable({}, { __lt = f }) < setmetatable({}, { __lt = f })",
        true,
    ),
    (
        "__unm",
        "local _ = -setmetatable({}, { __unm = function() WAIT return 1 end })",
        true,
    ),
    (
        "tostring()",
        "local _ = tostring(setmetatable({}, { __tostring = function() WAIT return 's' end }))",
        true,
    ),
    ("for iterator", "for _ in function() WAIT end do end", true),
    (
        "string.gsub callback",
        "local _ = string.gsub('a', 'a', function() WAIT end)",
        false,
    ),
    (
        "string.gsub table __index",
        "local _ = string.gsub('a', 'a', setmetatable({}, { __index = function() WAIT end }))",
        false,
    ),
    (
        "table.sort comparator",
        "table.sort({ 3, 2, 1 }, function(a, b) WAIT return a < b end)",
        false,
    ),
    (
        "string.format %s",
        "local _ = string.format('%s', setmetatable({}, { __tostring = function() WAIT return 's' end }))",
        false,
    ),
    // the handler cannot raise the wait's error to the caller (`xpcall` would answer "error in error handling"),
    // so it keeps it and the script raises it afterwards, as an embedder's own tests do
    (
        "xpcall handler",
        "local why xpcall(function() error('e') end, function(m) local ok, e = pcall(function() WAIT end) why = e return m end) if why then error(why, 0) end",
        false,
    ),
    (
        "a host function that calls back",
        "HOST_CALLS(function() WAIT end)",
        false,
    ),
];

/// The yield boundaries of LuaJIT, against the rules of `lifecycle`, counted at the baton (`Baton::park`): where
/// LuaJIT suspends, the wait lends the turn; where it refuses, the host call fails with Lua's own error and the
/// call *keeps* the turn, so whatever the embedder attached to the turn stays with it, and the host calls that follow
/// (not idle waits) do not lend either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wait_lends_the_turn_where_luajit_suspends_and_keeps_it_where_it_refuses() {
    let rig = Rig::new();
    let mut script = String::from(
        r#"local task = require("std:task")
           local out = {}
           -- `wait_lent` counts the times the call handed its turn back at the wait itself; `later_lent` those at three
           -- host calls that suspend without being an idle wait, which a call that still believed it was suspended at
           -- the wait would lend at (the shape an embedder's own tests use)
           local function try(name, f)
             local before = PARKS()
             local ok, e = pcall(f)
             local after_wait = PARKS()
             for _ = 1, 3 do HOST_VM() end
             out[name] = { ok = ok, err = tostring(e), wait_lent = after_wait - before, later_lent = PARKS() - after_wait }
           end
        "#,
    );
    for (name, template, _) in PLACES {
        let body = template.replace("WAIT", "task.wait(0.05)");
        script.push_str(&format!("try({name:?}, function() {body} end)\n"));
    }
    script.push_str("return out");
    let out = rig.run(&script).await;
    for (name, _, suspends) in PLACES {
        let r: Table = out.get(*name).unwrap();
        let (ok, err, wait_lent, later_lent): (bool, String, usize, usize) = (
            r.get("ok").unwrap(),
            r.get("err").unwrap(),
            r.get("wait_lent").unwrap(),
            r.get("later_lent").unwrap(),
        );
        assert_eq!(
            later_lent, 0,
            "{name}: a host call that is not an idle wait never lends the turn"
        );
        if *suspends {
            assert!(ok, "{name}: LuaJIT suspends here, got {err}");
            assert!(
                wait_lent >= 1,
                "{name}: a wait that suspends lends the turn"
            );
        } else {
            assert!(!ok, "{name}: LuaJIT refuses a yield here");
            assert!(err.contains("yield across"), "{name}: {err}");
            assert_eq!(
                wait_lent, 0,
                "{name}: a wait that did not suspend must not lend the turn"
            );
        }
    }
}

/// `Role::Mixed` is a public contract: an embedder's own host function that waits for part of its work calls
/// `lifecycle::idle` for the wait and `lifecycle::hold` before it touches what needs the turn, and so lends the turn
/// at the wait and has it back after.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_embedders_mixed_host_function_lends_at_its_idle_wait_and_holds_after() {
    let rig = Rig::new();
    rig.lua
        .globals()
        .set(
            "MIXED",
            async_function(&rig.lua, Role::Mixed, |_, (): ()| async {
                // the body starts without the turn: wait for something that does not use the VM...
                idle(tokio::time::sleep(Duration::from_millis(50))).await?;
                // ...and the turn is back (`idle` takes it), so this may use the embedder's state
                hold().await;
                Ok(neoc_vm::lua::lifecycle::turn_id())
            })
            .unwrap(),
        )
        .unwrap();
    let out = rig
        .run(
            "local before = PARKS() local id = MIXED() return { lent = PARKS() - before, id = id }",
        )
        .await;
    assert_eq!(
        out.get::<usize>("lent").unwrap(),
        1,
        "the wait lent the turn once"
    );
    assert_eq!(
        out.get::<u64>("id").unwrap(),
        1,
        "the call holds its turn after the wait"
    );
}

/// The reason `yieldable_pcall.lua` is gone: LuaJIT's own `pcall` and `xpcall` let an async host call suspend the
/// coroutine they run in, with no engine support and in the engine a plain `neoc run` uses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pcall_and_xpcall_suspend_natively_on_the_default_engine() {
    let lua = make_engine().expect("engine");
    let out: Table = lua
        .load(
            r#"local task = require("std:task")
               local a, b = pcall(function() task.wait(0.01) return 1 end)
               local c, d = xpcall(function() task.wait(0.01) error("boom", 0) end, function(e) return "h:" .. e end)
               local e = select(2, pcall(function() return select(2, pcall(function() task.wait(0.01) return "nested" end)) end))
               local t = task.spawn(function() return select(2, pcall(function() task.wait(0.01) return "in a task" end)) end)
               return { a = a, b = b, c = c, d = d, e = e, f = t:await() }"#,
        )
        .eval_async()
        .await
        .expect("script");
    assert!(out.get::<bool>("a").unwrap());
    assert_eq!(out.get::<i64>("b").unwrap(), 1);
    assert!(!out.get::<bool>("c").unwrap());
    assert_eq!(out.get::<String>("d").unwrap(), "h:boom");
    assert_eq!(out.get::<String>("e").unwrap(), "nested");
    assert_eq!(out.get::<String>("f").unwrap(), "in a task");
}

/// LuaJIT runs an `xpcall` handler at the point of the error and keeps it installed: a handler that makes a
/// protected call of its own and then fails is called again by its own error, and when that never ends it is a stack
/// overflow, past a 2 MiB thread. The engine's `xpcall` bounds the runs (`guarded_xpcall.lua`): a handler that is
/// handled on its second or fifth run answers what LuaJIT answers, one that never is answers `error in error
/// handling`, and none of it needs more than 256 KiB of stack.
#[test]
fn an_xpcall_handler_that_fails_is_run_again_as_on_luajit_up_to_a_bound() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for lua in [make_engine().unwrap(), make_bare_engine().unwrap()] {
                let out: Vec<String> = lua
                    .load(
                        r#"local r = {}
                           local function show(...) r[#r + 1] = table.concat({ ... }, ",") end
                           local function fails(handler) return select(2, xpcall(function() error("x", 0) end, handler)) end
                           -- never handled: bounded
                           local never = function() pcall(error, "z") error("again") end
                           show("never", tostring(fails(never)))
                           show("never again", tostring(fails(never)))
                           -- a failure that is handled by the re-run, as LuaJIT's own xpcall does: a handler with a bug
                           -- (it concatenates a table) is run again with its own error and succeeds
                           local buggy = function(e) pcall(error, "p") return "H:" .. e end
                           local _, e = xpcall(function() error({}) end, buggy)
                           show("buggy", tostring(e):match("^H:.*attempt to concatenate") and "re-run handled it" or tostring(e))
                           -- handled after n runs: n below the bound is LuaJIT's, n past it is the bound
                           local function after(n)
                             local runs = 0
                             return function() pcall(error, "z") runs = runs + 1 if runs < n then error("again") end return "ok after " .. runs end
                           end
                           show("5 runs", tostring(fails(after(5))))
                           show("1000 runs", tostring(fails(after(1000))))
                           -- the count does not carry over to the next xpcall, or the next handler
                           show("fresh", tostring(fails(after(5))))
                           show("plain error", tostring(fails(function() error("again") end)))
                           show("a host error", tostring(fails(function() local _ = nil + 1 end)))
                           show("ok", tostring(fails(function(m) return "handled:" .. m end)))
                           show("args", tostring(select(2, xpcall(function(a, b) return a + b end, print, 1, 2))))
                           -- inside a coroutine the handler cannot yield, and a failing one is bounded there too
                           local co = coroutine.wrap(function() return fails(never) end)
                           show("coroutine", tostring(co()))
                           -- xpcalls suspended in other coroutines, finished from inside a handler or long after, do not change
                           -- the count of this chain (LuaJIT yields across xpcall; the count is per coroutine)
                           local pool = {}
                           for i = 1, 100 do
                             local co = coroutine.create(function() xpcall(function() coroutine.yield() end, function(e) return e end) end)
                             coroutine.resume(co)
                             pool[i] = co
                           end
                           local runs = 0
                           local bounded = select(2, xpcall(function() error("x") end, function()
                             runs = runs + 1
                             if pool[runs] then coroutine.resume(pool[runs]) end
                             pcall(error, "z") error("again")
                           end))
                           show("pool", tostring(bounded) .. " in " .. (runs <= 24 and "at most 24" or runs) .. " runs")
                           -- an xpcall that starts inside a handler and finishes outside one leaves nothing behind
                           local parked = {}
                           for _ = 1, 40 do
                             xpcall(function() error("x") end, function(e)
                               local co = coroutine.create(function() xpcall(function() coroutine.yield() end, function(m) return m end) end)
                               coroutine.resume(co)
                               parked[#parked + 1] = co
                               return "h"
                             end)
                           end
                           for _, co in ipairs(parked) do coroutine.resume(co) end
                           show("after parked", tostring(fails(function(m) return "handled:" .. m end)))
                           -- a handler that is not a function raises, as the builtin does
                           show("not a function", tostring(pcall(xpcall, function() end, nil)))
                           return r"#,
                    )
                    .eval()
                    .unwrap();
                assert_eq!(
                    out,
                    [
                        "never,error in error handling",
                        "never again,error in error handling",
                        "buggy,re-run handled it",
                        "5 runs,ok after 5",
                        "1000 runs,error in error handling",
                        "fresh,ok after 5",
                        "plain error,error in error handling",
                        "a host error,error in error handling",
                        "ok,handled:x",
                        "args,3",
                        "coroutine,error in error handling",
                        "pool,error in error handling in at most 24 runs",
                        "after parked,handled:x",
                        "not a function,false",
                    ]
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

/// The same recursion through mlua's own message handler, which runs a value's `__tostring` to make the text of
/// an error: it aborts a runtime worker unless the host call that runs script code goes through `call_guarded`
/// (`lib:test`'s `toThrow` and `lib:cache`'s `on_evict` do, and so must an embedder's).
#[test]
fn an_error_value_whose_tostring_fails_cannot_overflow_the_stack_through_a_host_call() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let lua = make_engine().unwrap();
                let out: Vec<String> = lua
                    .load(
                        r#"local r = {}
                           local mt = {}
                           mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
                           local test = require("lib:test")
                           -- `toThrow` calls the function from Rust: the error is caught, not rendered by mlua
                           r[1] = tostring((pcall(function() test.expect(function() error(setmetatable({}, mt)) end).toThrow() end)))
                           -- an ordinary error value is as before
                           r[2] = tostring((pcall(function() test.expect(function() error("plain") end).toThrow() end)))
                           r[3] = tostring(select(2, pcall(function() test.expect(function() end).toThrow() end)))
                           -- on_evict raises through the same path: the script sees an error with text
                           local cache = require("lib:cache").Cache.new { max_size = 1, on_evict = function() error(setmetatable({}, mt)) end }
                           cache:set("a", 1)
                           local ok, e = pcall(function() cache:set("b", 2) end)
                           r[4] = tostring(ok) .. ":" .. tostring(e):sub(1, 12)
                           return r"#,
                    )
                    .eval_async()
                    .await
                    .unwrap();
                assert_eq!(out[0], "true", "toThrow caught the failing error value");
                assert_eq!(out[1], "true");
                assert!(out[2].contains("expected function to throw"), "{}", out[2]);
                assert!(out[3].starts_with("false:"), "{}", out[3]);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

// ---- the script environment on LuaJIT ---------------------------------------------------------------------------

fn sealed() -> (Lua, Table) {
    let lua = make_bare_engine().expect("engine");
    let env = script_env::new_env(&lua).expect("env");
    script_env::freeze(&lua, &env).expect("freeze");
    (lua, env)
}

fn run_in(lua: &Lua, env: &Table, source: &str) -> mlua::Result<Value> {
    lua.load(source)
        .set_mode(mlua::chunk::ChunkMode::Text)
        .set_environment(env.clone())
        .eval()
}

/// LuaJIT's runtime reaches beyond Lua 5.1's: `jit`, `ffi`, `bit`, `debug` and the bytecode loaders. None of them,
/// nor any way to read or change a function's environment, is in a script's environment, and the sealed libraries
/// and the shared string metatable hold. (The engines here do not open `jit`, so the JIT compiler is off and this
/// runs in LuaJIT's interpreter; whether a compiled loop could get past a proxy is not claimed.)
#[test]
fn the_sealed_environment_holds_on_luajit() {
    let (lua, env) = sealed();
    let escapes: String = run_in(
        &lua,
        &env,
        r#"local found = {}
           for _, name in ipairs { "jit", "ffi", "debug", "io", "os", "package", "require", "print", "load", "loadstring",
             "loadfile", "dofile", "getfenv", "setfenv", "newproxy", "module", "collectgarbage", "gcinfo" } do
             if _G[name] ~= nil then found[#found + 1] = name end
           end
           if string.dump then found[#found + 1] = "string.dump" end
           -- the metatable of every string is locked, and the string table behind it cannot be reached
           if getmetatable("") ~= "locked" then found[#found + 1] = "string metatable" end
           return table.concat(found, ",")"#,
    )
    .and_then(|v| mlua::FromLua::from_lua(v, &lua))
    .unwrap();
    assert_eq!(escapes, "");
    // a library cannot be written to, however it is spelled (the error says so), and the engine's own tables are
    // untouched
    for write in [
        "string.evil = 1",
        "table.insert = nil",
        "math.pi = 3",
        "bit.band = nil",
        "coroutine.yield = nil",
        "rawset(string, 'evil', 1)",
        "getmetatable('').__index.evil = 1",
        "getmetatable('').__index = {}",
        "getmetatable(string).__index = {}",
        "setmetatable(string, {})",
        // LuaJIT's table functions store with raw sets, past a proxy's `__newindex`
        "table.insert(string, 'x')",
        "table.insert(math, 1, 'x')",
        "table.insert(table, 'x')",
        "table.remove(string)",
        "table.remove(string, 1)",
        "table.sort(string)",
        "table.sort(math, function(a, b) return a < b end)",
        "table.move({ 'a' }, 1, 1, 1, string)",
        "table.move(string, 1, 1, 2)",
        "setmetatable(table, nil)",
        "setmetatable(coroutine, { __index = function() end })",
    ] {
        assert!(run_in(&lua, &env, write).is_err(), "{write} must fail");
    }
    // nothing landed in a proxy either
    let landed: f64 = run_in(
        &lua,
        &env,
        "return #string + #table + #math + #bit + #coroutine",
    )
    .and_then(|v| mlua::FromLua::from_lua(v, &lua))
    .unwrap();
    assert!(landed == 0.0, "something was written into a library proxy");
    // and the table functions still work on a table that is the script's own
    let ok: String = run_in(
        &lua,
        &env,
        "local t = { 3, 1, 2 } table.insert(t, 4) table.insert(t, 1, 0) table.sort(t) local last = table.remove(t) \
         table.remove(t, 1) return table.concat(t, ',') .. '|' .. last",
    )
    .and_then(|v| mlua::FromLua::from_lua(v, &lua))
    .unwrap();
    assert_eq!(ok, "1,2,3|4");
    for table in ["string", "table", "math", "bit", "coroutine"] {
        let leaked: Value = lua.load(format!("return {table}.evil")).eval().unwrap();
        assert!(matches!(leaked, Value::Nil), "{table} was written to");
    }
    // a sealed library still answers from inside a long loop
    let sum: f64 = run_in(
        &lua,
        &env,
        "local n = 0 for i = 1, 200000 do n = n + math.floor(i / 2) + #string.rep('x', 1) end return n",
    )
    .and_then(|v| mlua::FromLua::from_lua(v, &lua))
    .unwrap();
    assert!(sum > 0.0);
    // a function made by the script keeps the script's environment (there is no `setfenv` to change it)
    let kept: String = run_in(
        &lua,
        &env,
        "local f = function() return tostring(_G.string ~= nil) .. type(require) end return f()",
    )
    .and_then(|v| mlua::FromLua::from_lua(v, &lua))
    .unwrap();
    assert_eq!(kept, "truenil");
}

// ---- the call model's other rules, on a real engine --------------------------------------------------------------

/// A bare engine, its runtime state and a sealed script environment with a `require` that builds modules on first
/// use: what an embedder that runs scripts in environments sets up.
fn script_engine(extra: impl FnOnce(&Lua, &Table)) -> (Lua, Table) {
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    let env = script_env::new_env(&lua).unwrap();
    let cache = lua.create_table().unwrap();
    env.set(
        "require",
        lua.create_function(move |lua, name: String| {
            if let Value::Table(done) = cache.get::<Value>(name.clone())? {
                return Ok(done);
            }
            let built = modules::build(lua, &name)
                .ok_or_else(|| mlua::Error::runtime("unknown module"))??;
            cache.set(name, built.clone())?;
            Ok(built)
        })
        .unwrap(),
    )
    .unwrap();
    extra(&lua, &env);
    script_env::freeze(&lua, &env).unwrap();
    (lua, env)
}

/// A script resumes only the coroutines it made. The thread it reaches through `coroutine.running()` inside a task
/// is the host's: parked at `task.wait` (suspended, so LuaJIT itself would let it be resumed), `coroutine.resume`
/// refuses it, and nothing the task does after the wait runs because of the attempt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_script_resumes_only_the_coroutines_it_made() {
    let (lua, env) = script_engine(|_, _| {});
    let chunk = lua
        .load(
            r#"local task = require("std:task")
               local ran, host = 0, nil
               task.spawn(function() host = coroutine.running() task.wait(0.2) ran = ran + 1 end)
               -- the task is parked at its wait: a suspended thread the script did not make
               local status = coroutine.status(host)
               local attempt = { pcall(coroutine.resume, host) }
               local with_args = { pcall(coroutine.resume, host, 1, 2) }
               local after_attempts = ran
               -- the task is not harmed: it finishes its wait and runs on
               task.wait(0.4)
               local mine = coroutine.create(function() coroutine.yield("a") return "b" end)
               local made = { coroutine.resume(mine) }
               local wrapped = coroutine.wrap(function() coroutine.yield(1) return 2 end)
               return {
                 status = status,
                 refused = (not attempt[2]) and (not with_args[2]),
                 msg = tostring(attempt[3]),
                 ran_after_attempts = after_attempts,
                 ran_at_the_end = ran,
                 made = made[1] and made[2],
                 wrapped = wrapped(),
               }"#,
        )
        .set_environment(env);
    // a script that could resume the host's thread would not come back: fail instead of hanging
    let out: Table = tokio::time::timeout(Duration::from_secs(10), chunk.eval_async())
        .await
        .expect("the script came back")
        .unwrap();
    assert_eq!(out.get::<String>("status").unwrap(), "suspended");
    assert!(out.get::<bool>("refused").unwrap());
    assert_eq!(
        out.get::<String>("msg").unwrap(),
        "cannot resume non-suspended coroutine"
    );
    assert_eq!(
        out.get::<i64>("ran_after_attempts").unwrap(),
        0,
        "script code of the task ran because of the attempt"
    );
    assert_eq!(out.get::<i64>("ran_at_the_end").unwrap(), 1);
    assert_eq!(out.get::<String>("made").unwrap(), "a");
    assert_eq!(out.get::<i64>("wrapped").unwrap(), 1);
}

/// The scheduler and the script do not both drive one coroutine (`script_env::mark_driven`): `task.spawn` takes only a
/// coroutine the script made (refusing a thread it reached through `coroutine.running()`), refuses one that is
/// already being driven, and the script may no longer resume a coroutine the scheduler has, nor call a `wrap`
/// whose coroutine another thread is waiting in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_scheduler_and_the_script_never_drive_one_coroutine() {
    let (lua, env) = script_engine(|_, _| {});
    let out: Vec<String> = lua
        .load(
            r#"local task = require("std:task")
               local r = {}
               local function why(ok, e) return ok and "ok" or tostring(e):gsub("^.-: ", "") end
               -- not the script's: a thread it reached through coroutine.running()
               local host
               task.spawn(function() host = coroutine.running() task.wait(0.3) end)
               r[1] = why(pcall(task.spawn, host))
               -- the script's own: the scheduler drives it from here on
               local mine = coroutine.create(function() task.wait(0.2) return "finished" end)
               local handle = task.spawn(mine)
               r[2] = why(pcall(task.spawn, mine))               -- already being driven
               local resumed, why3 = coroutine.resume(mine) -- and the script may not resume it
               r[3] = tostring(resumed) .. ":" .. tostring(why3)
               r[4] = why(pcall(task.defer, mine))
               r[5] = why(pcall(task.delay, 0, mine))
               -- a wrap that a task is waiting in
               local w = coroutine.wrap(function() task.wait(0.2) return 1 end)
               task.spawn(w)
               r[6] = why(pcall(w))
               -- once it is done the coroutine is the script's again
               r[7] = tostring((handle:await()))
               r[8] = tostring(coroutine.status(mine))
               return r"#,
        )
        .set_environment(env)
        .eval_async()
        .await
        .unwrap();
    assert!(out[0].contains("not yours"), "{out:?}");
    assert!(out[1].contains("already running"), "{out:?}");
    assert_eq!(
        out[2], "false:cannot resume non-suspended coroutine",
        "{out:?}"
    );
    assert!(out[3].contains("already running"), "{out:?}");
    assert!(out[4].contains("already running"), "{out:?}");
    assert_eq!(out[5], "cannot resume non-suspended coroutine", "{out:?}");
    assert_eq!(out[6], "finished", "{out:?}");
    assert_eq!(out[7], "dead", "{out:?}");
}

/// A host call inside a coroutine of the script's own suspends the whole call, in a script environment (whose
/// `coroutine` is `async_coroutines.lua`'s): the marker an async host function yields passes out through the
/// script's `coroutine.resume` / `wrap` to the executor, and the turn is lent at the wait as anywhere else. (On the
/// plain engine's own `coroutine` the marker would come back from `resume` as a yielded value.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_call_inside_the_scripts_own_coroutines_suspends_the_call_in_a_script_environment() {
    let bumps = Arc::new(AtomicUsize::new(0));
    let counter = bumps.clone();
    let (lua, env) = script_engine(|lua, env| {
        env.set(
            "BUMPS",
            lua.create_function(move |_, (): ()| Ok(counter.load(Ordering::SeqCst)))
                .unwrap(),
        )
        .unwrap();
    });
    let entry = Gate::new();
    set_entry(&lua, entry.clone());

    let bumper = {
        let (entry, bumps) = (entry.clone(), bumps.clone());
        tokio::spawn(async move {
            loop {
                drop(entry.enter(Inherited::default()).await);
                bumps.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
    };
    let baton = entry.enter(Inherited::default()).await;
    let out: Table = scoped(
        baton,
        lua.load(
            r#"local task = require("std:task")
               local function lent(f)
                 local before = BUMPS()
                 local result = f()
                 return { result = result, lent = BUMPS() ~= before }
               end
               local wrapped = coroutine.wrap(function() task.wait(0.05) return "wrapped" end)
               local created = coroutine.create(function() task.wait(0.05) return "created" end)
               local nested = coroutine.wrap(function()
                 return select(2, pcall(function()
                   return coroutine.wrap(function() task.wait(0.05) return "nested" end)()
                 end))
               end)
               return {
                 wrap = lent(wrapped),
                 create = lent(function() return select(2, coroutine.resume(created)) end),
                 nested = lent(nested),
               }"#,
        )
        .set_environment(env)
        .eval_async::<Table>(),
    )
    .await
    .unwrap();
    bumper.abort();
    for (name, want) in [
        ("wrap", "wrapped"),
        ("create", "created"),
        ("nested", "nested"),
    ] {
        let r: Table = out.get(name).unwrap();
        assert_eq!(r.get::<String>("result").unwrap(), want, "{name}");
        assert!(
            r.get::<bool>("lent").unwrap(),
            "{name}: the wait lends the turn"
        );
    }
}

// ---- the log sink and the module registry --------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lib_log_lines_go_to_the_embedders_sink_and_a_worker_logs_there_too() {
    let lua = make_engine().unwrap();
    let lines = Arc::new(parking_lot::Mutex::new(Vec::<(String, String)>::new()));
    let sink = lines.clone();
    neoc_vm::lua::lib::log::set_log_sink(
        &lua,
        Arc::new(move |level, line| sink.lock().push((level.to_owned(), line.to_owned()))),
    );
    lua.load(
        r#"local log = require("lib:log")
           log.info("hello", { n = 1 })
           log.new({ format = "text", level = "debug" }):debug("quiet")"#,
    )
    .exec()
    .unwrap();
    {
        let lines = lines.lock();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].0, "info");
        assert!(lines[0].1.contains(r#""msg":"hello""#) && lines[0].1.contains(r#""n":1"#));
        assert_eq!(lines[1].0, "debug");
        assert!(lines[1].1.contains("[debug] quiet"), "{}", lines[1].1);
    }
    // a worker the script starts logs to the same sink
    let _: Value = lua
        .load(
            r#"local worker = require("std:thread").spawn { source = "require('lib:log').warn('from the worker')" }
               return worker:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    let lines = lines.lock();
    assert!(
        lines
            .iter()
            .any(|(level, line)| level == "warn" && line.contains("from the worker")),
        "{lines:?}"
    );
}

#[test]
fn the_registry_is_an_api_an_embedder_can_build_its_own_require_from() {
    let names: Vec<&str> = modules::names().collect();
    assert_eq!(names.len(), 50);
    assert!(names.contains(&"std:task") && names.contains(&"vnd:hyper"));
    let lua = make_bare_engine().unwrap();
    // no `require` at all on a bare engine, and a module is built on request, a new table each time
    assert!(matches!(
        lua.globals().get::<Value>("require").unwrap(),
        Value::Nil
    ));
    let (a, b) = (
        modules::build(&lua, "lib:json").unwrap().unwrap(),
        modules::build(&lua, "lib:json").unwrap().unwrap(),
    );
    assert_ne!(a.to_pointer(), b.to_pointer());
    assert!(modules::build(&lua, "lib:nope").is_none());
    // the error `require` raises, for the embedder's own `require`
    let e = modules::unknown_module("lib:jsn", &names).to_string();
    assert!(e.contains("did you mean \"lib:json\""), "{e}");
    // a subset: the rest is unknown to this engine's `require`
    modules::install_allowed(&lua, &["lib:json", "lib:hex"]).unwrap();
    let out: Vec<String> = lua
        .load(
            r#"local r = {}
               for _, n in ipairs { "lib:json", "lib:hex", "std:fs", "lib:regex" } do
                 local ok, m = pcall(require, n)
                 r[#r + 1] = n .. "=" .. (ok and type(m) or "unknown")
               end
               return r"#,
        )
        .eval()
        .unwrap();
    assert_eq!(
        out,
        [
            "lib:json=table",
            "lib:hex=table",
            "std:fs=unknown",
            "lib:regex=unknown"
        ]
    );
}

/// With no embedder the hooks are no-ops: there is no entry, the registry is open, and tasks run under the VM's own
/// executor with nobody's turn to take.
#[tokio::test]
async fn without_an_embedder_the_hooks_do_nothing() {
    let lua = make_engine().unwrap();
    assert!(!neoc_vm::lua::lifecycle::entrance(&lua).is_set());
    assert!(!Life::of(&lua).is_closed());
    let value: i64 = lua
        .load(
            r#"local task = require("std:task")
               local h = task.spawn(function() task.wait(0.01) return 42 end)
               return h:await()"#,
        )
        .eval_async()
        .await
        .unwrap();
    assert_eq!(value, 42);
    assert_eq!(neoc_vm::lua::lifecycle::turn_id(), None);
}

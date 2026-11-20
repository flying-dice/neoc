#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! What a program that embeds the VM gets, proven on LuaJIT: the call model of `lifecycle` (a turn at a time
//! in one VM, lent at the waits that suspend and kept where LuaJIT refuses a yield), the script environment of
//! `script_env`, the lifetimes of what a script starts (`Life`), the log sink and the module registry seam.
//!
//! Nothing here is a product policy: the embedder in these tests is a mutex and a counter. The same rules are
//! exercised end to end by the adapter of a real embedder (see the pull request that ported them).

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
    lifecycle::{Baton, BoxFuture, CallEntry, IdleKind, Inherited, Life, scoped, set_entry},
    modules,
    sandbox::{install_runtime_state, make_bare_engine, make_engine, make_engine_in},
    script_env,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

// ---- an embedder made of a mutex and a counter -----------------------------------------------------------------

/// The queue of calls into one VM: one at a time, through a mutex.
struct Gate {
    gate: Arc<Mutex<()>>,
}

struct Turn {
    gate: Arc<Mutex<()>>,
    held: Option<OwnedMutexGuard<()>>,
    waiting: Option<BoxFuture<'static, OwnedMutexGuard<()>>>,
}

impl Turn {
    fn new(gate: &Arc<Mutex<()>>, held: Option<OwnedMutexGuard<()>>) -> Self {
        Self {
            gate: gate.clone(),
            held,
            waiting: None,
        }
    }
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
        Box::pin(async move {
            let held = gate.clone().lock_owned().await;
            Box::new(Turn::new(&gate, Some(held))) as Box<dyn Baton>
        })
    }
    fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
        self.enter(Inherited::default())
    }
    fn detached(&self, _: Inherited) -> Box<dyn Baton> {
        Box::new(Turn::new(&self.gate, None))
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
        let entry = Arc::new(Gate {
            gate: Arc::new(Mutex::new(())),
        });
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

/// Where a host call (a wait of 50 ms) runs, with whether LuaJIT can suspend there.
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
    (
        "a host function that calls back",
        "HOST_CALLS(function() WAIT end)",
        false,
    ),
];

/// The yield boundaries of LuaJIT, against the rules of `lifecycle`: where LuaJIT suspends, the wait lends the turn
/// (the bumper gets in meanwhile); where it refuses, the host call fails with Lua's own error and the call
/// *keeps* the turn (the bumper does not get in), so whatever restriction goes with the turn stays with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wait_lends_the_turn_where_luajit_suspends_and_keeps_it_where_it_refuses() {
    let rig = Rig::new();
    let mut script = String::from(
        r#"local task = require("std:task")
           local out = {}
           local function try(name, f)
             local before = BUMPS()
             local ok, e = pcall(f)
             out[name] = { ok = ok, err = tostring(e), lent = BUMPS() ~= before }
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
        let (ok, err, lent): (bool, String, bool) = (
            r.get("ok").unwrap(),
            r.get("err").unwrap(),
            r.get("lent").unwrap(),
        );
        if *suspends {
            assert!(ok, "{name}: LuaJIT suspends here, got {err}");
            assert!(lent, "{name}: a wait that suspends lends the turn");
        } else {
            assert!(!ok, "{name}: LuaJIT refuses a yield here");
            assert!(err.contains("yield across"), "{name}: {err}");
            assert!(
                !lent,
                "{name}: a wait that did not suspend must not lend the turn"
            );
        }
    }
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
/// protected call of its own and then fails is called again by its own error until a stack overflow, which is past
/// a 2 MiB thread. The engine's `xpcall` runs the handler under a `pcall` (`guarded_xpcall.lua`).
#[test]
fn an_xpcall_handler_that_fails_cannot_overflow_the_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for lua in [make_engine().unwrap(), make_bare_engine().unwrap()] {
                let out: Vec<String> = lua
                    .load(
                        r#"local r = {}
                           local function show(...) r[#r + 1] = table.concat({ ... }, ",") end
                           local function fails(handler) return select(2, xpcall(function() error("x", 0) end, handler)) end
                           show("after a pcall", tostring(fails(function() pcall(error, "z") error("again") end)))
                           show("plain error", tostring(fails(function() error("again") end)))
                           show("a host error", tostring(fails(function() local _ = nil + 1 end)))
                           show("ok", tostring(fails(function(m) return "handled:" .. m end)))
                           show("args", tostring(select(2, xpcall(function(a, b) return a + b end, print, 1, 2))))
                           -- a handler that is not a function raises, as the builtin does
                           show("not a function", tostring(pcall(xpcall, function() end, nil)))
                           return r"#,
                    )
                    .eval()
                    .unwrap();
                assert_eq!(
                    out,
                    [
                        "after a pcall,error in error handling",
                        "plain error,error in error handling",
                        "a host error,error in error handling",
                        "ok,handled:x",
                        "args,3",
                        "not a function,false",
                    ]
                );
            }
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
/// and the shared string metatable hold.
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
    // a library cannot be written to, however it is spelled, and the engine's own tables are untouched
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
    ] {
        assert!(run_in(&lua, &env, write).is_err(), "{write} must fail");
    }
    for table in ["string", "table", "math", "bit", "coroutine"] {
        let leaked: Value = lua.load(format!("return {table}.evil")).eval().unwrap();
        assert!(matches!(leaked, Value::Nil), "{table} was written to");
    }
    // LuaJIT compiles hot loops: a sealed library still answers from inside one
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

/// Two environments of one engine do not share what a script writes, and neither writes into the engine's globals.
#[test]
fn environments_of_one_engine_are_apart() {
    let lua = make_bare_engine().expect("engine");
    let make = || {
        let env = script_env::new_env(&lua).unwrap();
        script_env::freeze(&lua, &env).unwrap();
        env
    };
    let (a, b) = (make(), make());
    run_in(&lua, &a, "shared = 1").unwrap();
    assert!(matches!(
        run_in(&lua, &b, "return shared").unwrap(),
        Value::Nil
    ));
    assert!(matches!(
        lua.globals().get::<Value>("shared").unwrap(),
        Value::Nil
    ));
}

// ---- the call model's other rules, on a real engine --------------------------------------------------------------

/// A script resumes only the coroutines it made; a thread it reaches through `coroutine.running()` is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_script_resumes_only_the_coroutines_it_made() {
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    let env = script_env::new_env(&lua).unwrap();
    script_env::freeze(&lua, &env).unwrap();
    let out: Table = lua
        .load(
            r#"local host = coroutine.running()
               local mine = coroutine.create(function() coroutine.yield("a") return "b" end)
               local wrapped = coroutine.wrap(function() coroutine.yield(1) return 2 end)
               local made = { coroutine.resume(mine) }
               local theirs = { pcall(coroutine.resume, host) }
               return { made = made[1] and made[2], theirs_ok = theirs[2], theirs_msg = tostring(theirs[3]),
                        wrapped = wrapped() }"#,
        )
        .set_environment(env)
        .eval_async()
        .await
        .unwrap();
    assert_eq!(out.get::<String>("made").unwrap(), "a");
    assert!(!out.get::<bool>("theirs_ok").unwrap());
    assert_eq!(
        out.get::<String>("theirs_msg").unwrap(),
        "cannot resume non-suspended coroutine"
    );
    assert_eq!(out.get::<i64>("wrapped").unwrap(), 1);
}

/// A host call inside a coroutine of the script's own suspends the whole call, in a script environment (whose
/// `coroutine` is `async_coroutines.lua`'s): the marker an async host function yields passes out through the
/// script's `coroutine.resume` / `wrap` to the executor, and the turn is lent at the wait as anywhere else. (On the
/// plain engine's own `coroutine` the marker would come back from `resume` as a yielded value.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_call_inside_the_scripts_own_coroutines_suspends_the_call_in_a_script_environment() {
    let lua = make_bare_engine().unwrap();
    install_runtime_state(&lua, Life::new());
    let entry = Arc::new(Gate {
        gate: Arc::new(Mutex::new(())),
    });
    set_entry(&lua, entry.clone());
    let bumps = Arc::new(AtomicUsize::new(0));
    let counter = bumps.clone();
    let env = script_env::new_env(&lua).unwrap();
    env.set(
        "BUMPS",
        lua.create_function(move |_, (): ()| Ok(counter.load(Ordering::SeqCst)))
            .unwrap(),
    )
    .unwrap();
    // the embedder's own `require`: a module is built in this engine on first use
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
    script_env::freeze(&lua, &env).unwrap();

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

// ---- lifetimes ----------------------------------------------------------------------------------------------------

/// What a script starts belongs to the engine's `Life`: shutting it down aborts the tasks and workers, ends a server
/// a script sits in `wait()` on, and frees the listener's port for whoever binds next.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutting_a_script_down_stops_what_it_started_and_frees_its_port() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).unwrap();
    let addr: String = lua
        .load(
            r#"local task = require("std:task")
               local net, hyper = require("std:net"), require("vnd:hyper")
               local thread = require("std:thread")
               _G.tick = 0
               task.spawn(function() while true do task.wait(0.005) _G.tick = _G.tick + 1 end end)
               task.delay(0.001, function() while true do task.wait(0.005) end end)
               local listener = net.TcpListener.bind("127.0.0.1:0")
               local server = hyper.serve(listener, function() return { status = 200, body = "ok" } end)
               _G.server = server
               _G.worker = thread.spawn { source = "while true do require('std:task').wait(0.005) end" }
               return server:addr()"#,
        )
        .eval_async()
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(life.live_tasks() >= 4, "tasks: {}", life.live_tasks());
    // a script sits in `server:wait()`: it must end when the script is shut down
    let waiting = {
        let lua = lua.clone();
        tokio::spawn(async move {
            lua.load("_G.server:wait() return 'ended'")
                .eval_async::<String>()
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!waiting.is_finished());
    life.shutdown();
    let ended = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("wait() ended with the shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(ended, "ended");
    // everything registered is gone, and nothing registers afterwards
    for _ in 0..100 {
        if life.live_tasks() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(life.live_tasks(), 0);
    let late: u32 = lua
        .load("return require('std:task').spawn(function() end) and 1")
        .eval_async()
        .await
        .unwrap();
    assert_eq!(late, 1);
    assert_eq!(
        life.live_tasks(),
        0,
        "a task started after the shutdown never runs"
    );
    // the port is free again (the listener was released by the shutdown, not left to the VM's collector)
    let rebound = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if tokio::net::TcpListener::bind(&addr).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(rebound.is_ok(), "{addr} was not freed");
}

/// `lib:http`'s `serve_static` ends with the script too, and installs no signal handler of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_static_server_ends_when_the_script_is_shut_down() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).unwrap();
    let dir = std::env::temp_dir().join(format!("embedding-static-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    let pattern = format!("{}/*.txt", dir.display());
    let served = tokio::spawn({
        let lua = lua.clone();
        async move {
            lua.load(format!(
                "require('lib:http').serve_static({pattern:?}, {{ host = '127.0.0.1', port = 0 }}) return 'ended'"
            ))
            .eval_async::<String>()
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!served.is_finished());
    life.shutdown();
    let ended = tokio::time::timeout(Duration::from_secs(5), served)
        .await
        .expect("serve_static ended with the shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(ended, "ended");
    let _ = std::fs::remove_dir_all(&dir);
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
    assert!(!neoc_vm::lua::lifecycle::has_entry(&lua));
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

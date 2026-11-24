//! `std:thread` with the modules a worker is offered, from other namespaces than `std:`: what the tests of the module
//! itself (in `neoc-vm-std`, which links no `lib:` module) cannot say.

use neoc_vm::Life;
use neoc_vm::mlua::{self, FromLuaMulti, Lua};

fn engine() -> Lua {
    engine_in(Life::new())
}

fn engine_in(life: Life) -> Lua {
    let lua = neoc_vm::make_engine_in(life).expect("engine");
    for (global, module) in [("thread", "std:thread"), ("collections", "std:collections")] {
        let table: mlua::Table = lua.load(format!("return require({module:?})")).eval().unwrap();
        lua.globals().set(global, table).unwrap();
    }
    lua
}

async fn eval<T: FromLuaMulti>(lua: &Lua, code: &str) -> T {
    lua.load(code).eval_async().await.unwrap_or_else(|e| panic!("{e}"))
}

async fn run<T: FromLuaMulti>(code: &str) -> T {
    eval(&engine(), code).await
}

/// A worker is offered what its parent was offered, and a script on an engine whose embedder did not say how a
/// worker is built cannot start one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_worker_is_not_offered_more_than_its_parent() {
    let narrow = neoc_vm::make_bare_engine().expect("engine");
    neoc_vm::install_runtime_state(&narrow, Life::new());
    neoc_vm::modules::install_allowed(&narrow, &["std:thread", "lib:base64"]).expect("install");
    let (offered, withheld): (String, bool) = eval(
        &narrow,
        r#"
        local thread = require("std:thread")
        local w = assert(thread.spawn([[
            return { require("lib:base64").encode("hi"), (pcall(require, "std:time")) }
        ]]))
        local seen = w:await()
        return seen[1], seen[2]
        "#,
    )
    .await;
    assert_eq!(offered, "aGk=");
    assert!(!withheld, "the worker's `require` knows what its parent's does");

    let bare = neoc_vm::make_bare_engine().expect("engine");
    neoc_vm::install_runtime_state(&bare, Life::new());
    let thread = neoc_vm::modules::build(&bare, "std:thread").expect("the module").expect("builds");
    bare.globals().set("thread", thread).unwrap();
    let (none, kind): (bool, String) =
        eval(&bare, r#"local w, err = thread.spawn("return 1") return w == nil, err.kind"#).await;
    assert!(none);
    assert_eq!(kind, "no_worker_engine");
}

/// A worker logs where its parent logs: the embedder's sink is handed to the worker's engine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_worker_logs_to_its_parents_sink() {
    let e = engine();
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = lines.clone();
    neoc_vm::modules::lib::log::set_log_sink(
        &e,
        std::sync::Arc::new(move |level, line| kept.lock().unwrap().push(format!("{level} {line}"))),
    );
    let done: String = eval(
        &e,
        r#"
        require("lib:log").info("from the parent")
        return (assert(thread.spawn([[
            require("lib:log").warn("from the worker", { n = 1 })
            return "done"
        ]])):await())
        "#,
    )
    .await;
    assert_eq!(done, "done");
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("info ") && lines[0].contains("from the parent"), "{lines:?}");
    assert!(lines[1].starts_with("warn ") && lines[1].contains(r#""msg":"from the worker""#), "{lines:?}");
}

/// A worker's handle on a shared map or counter is the parent's: what one writes the other reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_map_and_counter_across_spawn() {
    let (worker_saw, worker_wrote, counter_val, lib): (String, String, i64, String) = run(r#"
        local m, c = collections.Map.new(), collections.Counter.new()
        m:set("before", "parent")
        local w = assert(thread.spawn([[
            local shared = require("std:thread").shared()
            shared.m:set("from_worker", "hello")
            shared.c:increment()
            shared.c:increment()
            shared.c:add(5)
            -- a worker has the modules its parent has
            shared.m:set("lib", require("lib:base64").encode("hi"))
            return shared.m:get("before")
        ]], nil, { m = m, c = c }))
        local worker_saw = w:await()
        return worker_saw, m:get("from_worker"), c:get(), m:get("lib")
        "#)
    .await;
    assert_eq!(worker_saw, "parent", "worker should see parent's map value");
    assert_eq!(worker_wrote, "hello", "parent should see worker's map write");
    assert_eq!(counter_val, 7, "counter should reflect worker ops");
    assert_eq!(lib, "aGk=");
}

/// Only a handle on shared state can be shared: anything else is refused where it is passed, with its key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_cannot_be_shared_is_refused_with_its_key() {
    let refused: Vec<String> = run(r#"
        local out = {}
        for _, shared in {
            { bad = require("lib:json").object() },
            { t = {} },
            { n = 42 },
        } do
            local ok, err = pcall(thread.spawn, "return 1", nil, shared)
            assert(not ok)
            table.insert(out, tostring(err))
        end
        return out
        "#)
    .await;
    for (err, key) in refused.iter().zip(["bad", "t", "n"]) {
        assert!(err.contains("std:thread.spawn: argument #3 'shared'") && err.contains(key), "{err}");
    }
}

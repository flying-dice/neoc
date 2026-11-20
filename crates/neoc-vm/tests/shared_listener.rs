#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Several servers on one shared listener (the `std:thread` pattern: N workers each `hyper.serve` the same
//! `Sendable` listener) share the work, and one of them closing leaves the others answering.
//!
//! Tokio's `poll_accept` wakes only the task that polled last, where `accept()` queues every waiter; the
//! listener's slot restores the queueing (`ListenerSlot`), and these tests are what notices if it does not: with
//! only the last poller woken one worker takes every request, and the others hang when it closes.

mod common;

use std::time::Duration;

use common::{free_addr, http_get};
use neoc_vm::lua::sandbox::make_engine;

/// A worker's source: serve the shared listener counting hits in this worker's counter, and close the server
/// when this worker's stop counter is raised.
const WORKER: &str = r#"
    local idx = ...
    local th = require("std:thread")
    local task = require("std:task")
    local hits, stop = th.shared["hits" .. idx], th.shared["stop" .. idx]
    local server = require("vnd:hyper").serve(th.shared.listener, function()
      hits:increment()
      return { status = 200, body = "ok" }
    end)
    th.shared.ready:increment()
    task.spawn(function()
      while stop:get() == 0 do task.wait(0.01) end
      server:close()
    end)
    server:wait()
    return "closed"
"#;

/// Start `n` workers on one listener; the address, and the engine whose globals hold the counters.
async fn start(n: usize) -> (mlua::Lua, String) {
    let lua = make_engine().unwrap();
    let addr = free_addr();
    let mut shared = String::from("listener = L, ready = READY");
    let mut spawns = String::new();
    for i in 1..=n {
        shared.push_str(&format!(", hits{i} = H[{i}], stop{i} = S[{i}]"));
    }
    for i in 1..=n {
        spawns.push_str(&format!(
            "_G.workers[{i}] = thread.spawn {{ source = WORKER, args = {i}, shared = {{ {shared} }} }}\n"
        ));
    }
    lua.globals().set("WORKER", WORKER).unwrap();
    lua.load(format!(
        r#"local thread = require("std:thread")
           local Counter = require("std:collections").Counter
           local L = require("std:net").TcpListener.bind({addr:?})
           local READY = Counter.new(0)
           _G.READY = READY
           local H, S = {{}}, {{}}
           for i = 1, {n} do H[i] = Counter.new(0) S[i] = Counter.new(0) end
           _G.H, _G.S, _G.workers = H, S, {{}}
           {spawns}"#
    ))
    .exec_async()
    .await
    .unwrap();
    // every worker is serving: each says so, after `serve` returned
    for _ in 0..500 {
        let ready: f64 = lua
            .load("return _G.READY:get()")
            .eval_async()
            .await
            .unwrap();
        if ready as usize == n {
            return (lua, addr);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the {n} workers did not all start serving");
}

async fn hits(lua: &mlua::Lua, n: usize) -> Vec<i64> {
    let counts: Vec<f64> = lua
        .load(format!(
            "local r = {{}} for i = 1, {n} do r[i] = _G.H[i]:get() end return r"
        ))
        .eval_async()
        .await
        .unwrap();
    counts.into_iter().map(|c| c as i64).collect()
}

/// `n` requests, `parallel` at a time, each with a two-second limit: how many were answered.
async fn requests(addr: &str, n: usize, parallel: usize) -> usize {
    let mut answered = 0;
    let mut left = n;
    while left > 0 {
        let batch = left.min(parallel);
        let tasks: Vec<_> = (0..batch)
            .map(|_| {
                let addr = addr.to_owned();
                tokio::spawn(async move {
                    tokio::time::timeout(Duration::from_secs(2), http_get(&addr, "/"))
                        .await
                        .ok()
                        .flatten()
                        .is_some_and(|r| r.contains("200 OK"))
                })
            })
            .collect();
        for task in tasks {
            answered += usize::from(task.await.unwrap());
        }
        left -= batch;
    }
    answered
}

/// Four workers serve one listener and the requests are shared: at least two of them serve some of 800 requests
/// (on `main` they split roughly evenly; one worker taking every request, as the last-poller-only wake does,
/// fails this; the wake order may favour one, so no more than that is asked).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn several_servers_on_one_listener_share_the_requests() {
    let (lua, addr) = start(4).await;
    let answered = requests(&addr, 800, 20).await;
    assert_eq!(answered, 800, "every request is answered");
    let counts = hits(&lua, 4).await;
    assert_eq!(counts.iter().sum::<i64>(), 800, "{counts:?}");
    assert!(
        counts.iter().filter(|c| **c > 0).count() >= 2,
        "one worker took every request: {counts:?}"
    );
}

/// One of two servers closes, whichever polled last or first: the other still answers every request.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn the_server_that_stays_keeps_answering_after_the_other_closes() {
    for closing in [1, 2] {
        let (lua, addr) = start(2).await;
        assert_eq!(requests(&addr, 40, 8).await, 40, "control: both up");
        lua.load(format!("_G.S[{closing}]:increment()"))
            .exec_async()
            .await
            .unwrap();
        // the closing worker's `wait()` returns and the worker ends
        let ended: String = lua
            .load(format!("return _G.workers[{closing}]:await()"))
            .eval_async()
            .await
            .unwrap();
        assert_eq!(ended, "closed");
        let before = hits(&lua, 2).await;
        let answered = requests(&addr, 20, 4).await;
        assert_eq!(
            answered, 20,
            "worker {closing} closed, the other must still answer"
        );
        let after = hits(&lua, 2).await;
        let survivor = 2 - (closing - 1);
        assert_eq!(
            after[survivor - 1] - before[survivor - 1],
            20,
            "the survivor (worker {survivor}) took the requests: {before:?} -> {after:?}"
        );
        assert_eq!(
            after[closing - 1],
            before[closing - 1],
            "the closed server answered"
        );
    }
}

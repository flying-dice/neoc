#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
#![cfg(unix)]
//! The process-level signals `neoc run` reacts to, which a script's servers wait on. This binary is its own process:
//! it sends itself SIGINT and fires the process-wide `ShutdownSignal`, which latches.
//!
//! - `lib:http`'s `serve_static` ends at a fresh Ctrl-C (it listens for it itself, as ever), is not ended by the
//!   latched `ShutdownSignal` (which is what SIGTERM fires in the CLI), and a second `serve_static` after one
//!   Ctrl-C serves until the next one.
//! - `vnd:hyper`'s `server:wait()` ends at the `ShutdownSignal`.

use std::time::Duration;

mod common;
use common::{ends, free_addr, http_get, serve_ok, start_static, static_site};
use neoc_vm::lua::{sandbox::make_engine, std::workers::ShutdownSignal};

async fn answers(addr: &str, path: &str) -> bool {
    http_get(addr, path)
        .await
        .is_some_and(|out| out.contains("200 OK"))
}

async fn serving(addr: &str) {
    for _ in 0..100 {
        if answers(addr, "/a.txt").await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{addr} never started answering");
}

fn sigint_self() {
    let status = std::process::Command::new("kill")
        .args(["-INT", &std::process::id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// One test, in this order: the shutdown signal latches for the life of the process, so `server:wait()` is tried
/// while it is still unfired and `serve_static` after, when it is fired for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_process_signals_end_what_main_ended_them_for() {
    server_wait_ends_at_the_process_shutdown_signal().await;
    serve_static_ends_at_a_fresh_ctrl_c_and_not_at_the_latched_shutdown_signal().await;
}

async fn serve_static_ends_at_a_fresh_ctrl_c_and_not_at_the_latched_shutdown_signal() {
    let (dir, pattern) = static_site("signals-static");
    let lua = make_engine().unwrap();

    // what SIGTERM does in the CLI: the process-wide signal is fired, and stays fired (it was, above)
    assert!(ShutdownSignal::current().is_fired());

    let (first_addr, second_addr) = (free_addr(), free_addr());
    let first = start_static(&lua, &pattern, &first_addr);
    serving(&first_addr).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !first.is_finished(),
        "serve_static ended at the latched ShutdownSignal (SIGTERM in the CLI)"
    );

    sigint_self();
    assert_eq!(ends(first, "serve_static at Ctrl-C").await, "ended");

    // a second server, started after that Ctrl-C, waits for a fresh one
    let second = start_static(&lua, &pattern, &second_addr);
    serving(&second_addr).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !second.is_finished(),
        "a later serve_static returned at once (a Ctrl-C that came before it)"
    );
    assert!(answers(&second_addr, "/a.txt").await);
    sigint_self();
    ends(second, "the second serve_static at the second Ctrl-C").await;
    let _ = std::fs::remove_dir_all(&dir);
}

async fn server_wait_ends_at_the_process_shutdown_signal() {
    let lua = make_engine().unwrap();
    lua.load(serve_ok("127.0.0.1:0"))
        .exec_async()
        .await
        .unwrap();
    let waiting = {
        let lua = lua.clone();
        tokio::spawn(async move {
            lua.load("_G.server:wait() return 'ended'")
                .eval_async::<String>()
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiting.is_finished());
    ShutdownSignal::current().fire();
    assert_eq!(
        ends(waiting, "wait() at the shutdown signal").await,
        "ended"
    );
}

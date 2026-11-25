//! `neoc_vm::kill_processes`: what a program does before it ends by `std::process::exit`. It kills every process
//! any script of the program started, so it has a test binary of its own: beside the tests of `std:process` it
//! would end their processes too.

use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Is there a process `pid`? (`kill -0` asks without signalling; a process that was reaped is not there.)
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("kill")
        .success()
}

/// Is there a process `pid`? (`tasklist` lists it, or says that nothing matches.)
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let listed = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&listed.stdout).contains(&format!(",\"{pid}\","))
}

/// Two engines, each with a process that stays until it is killed: one kill ends both, at once, and their scripts
/// are told how they ended.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_process_is_killed_before_the_program_exits() {
    let start = r#"
        local process = require("std:process")
        local windows = require("std:env").family == "windows"
        held = assert(process.spawn(if windows
            then { program = "ping.exe", args = { "-n", "1000", "127.0.0.1" } }
            else { program = "/bin/sh", args = { "-c", "echo ready; exec sleep 1000" } }))
        assert(held:read())
        return held:id()
    "#;
    let engines = [neoc_vm::make_engine().expect("engine"), neoc_vm::make_engine().expect("engine")];
    let mut pids = Vec::new();
    for lua in &engines {
        let pid: u32 = lua.load(start).eval_async().await.expect("a process");
        assert!(alive(pid));
        pids.push(pid);
    }

    neoc_vm::kill_processes();

    let started = Instant::now();
    while pids.iter().any(|pid| alive(*pid)) {
        assert!(started.elapsed() < TIMEOUT, "a process of {pids:?} is still running");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for lua in &engines {
        let success: bool =
            lua.load("return assert(held:wait()).success").eval_async().await.expect("a status");
        assert!(!success, "it was killed");
    }
}

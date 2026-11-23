//! `std:process` against real processes: `sh` and what every Unix has beside it (`cat`, `sleep`, `head`). The tests
//! that start a process are Unix's: the commands are, and no test here was run on Windows. What is refused before
//! anything starts is tested on every platform.

// what only the Unix tests use
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]

use std::time::{Duration, Instant};

use mlua::{Lua, Value};

use crate::lua::lifecycle::Life;
use crate::lua::sandbox::{make_engine, make_engine_in};

const TIMEOUT: Duration = Duration::from_secs(10);

/// What every script here starts with: the module, a read that goes on until the output holds `needle`, and one
/// that goes on to the end.
const PRELUDE: &str = r#"
    local process = require("std:process")
    local task = require("std:task")
    local function read_until(child, needle)
        local seen = ""
        while not string.find(seen, needle, 1, true) do
            local output, err = child:read()
            assert(err == nil, err)
            assert(output, "the output ended before `" .. needle .. "`: " .. seen)
            seen ..= output
        end
        return seen
    end
    local function read_all(child, method)
        local seen = ""
        while true do
            local output, err = child[method or "read"](child)
            assert(err == nil, err)
            if not output then
                return seen
            end
            seen ..= output
        end
    end
    local function sh(script, opts)
        opts = opts or {}
        opts.program = "/bin/sh"
        opts.args = { "-c", script }
        return assert(process.spawn(opts))
    end
"#;

async fn run_in<R: mlua::FromLuaMulti>(lua: &Lua, script: &str) -> R {
    let run = lua.load(format!("{PRELUDE}\n{script}")).call_async(());
    tokio::time::timeout(TIMEOUT, run)
        .await
        .expect("the script did not end in time")
        .unwrap_or_else(|e| panic!("the script failed: {e}"))
}

/// Run `script` in a new engine.
async fn run<R: mlua::FromLuaMulti>(script: &str) -> R {
    run_in(&make_engine().expect("engine"), script).await
}

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

/// Wait for the process `pid` to be gone.
#[cfg(unix)]
async fn gone(pid: u32) {
    let started = Instant::now();
    while alive(pid) {
        assert!(
            started.elapsed() < TIMEOUT,
            "the process {pid} is still running"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---------------------------------------------------------------------------
// a process and its streams
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversation_of_lines_goes_both_ways_until_the_input_is_closed() {
    let (replies, code, success): (String, i32, bool) = run(r#"
        -- a helper that answers each line it reads, as a protocol over standard streams does
        local child = sh('while read -r line; do echo "re:$line"; done; echo bye')
        assert(type(child:id()) == "number")
        local replies = ""
        for turn = 1, 20 do
            assert(child:write("turn " .. turn .. "\n") == true)
            replies ..= read_until(child, "re:turn " .. turn .. "\n")
        end
        -- the end of its input is how it is asked to finish
        assert(child:close_stdin() == true)
        assert(child:close_stdin() == true, "closing again does nothing")
        replies ..= read_all(child)
        local written, err = child:write("late\n")
        assert(written == nil and process.error_kind(err) == "closed", tostring(err))
        local status = assert(child:wait())
        assert(child:id() == nil, "an ended process has no id")
        assert(status.signal == nil)
        child:close()
        return replies, status.code, status.success
    "#)
    .await;
    let expected: String = (1..=20).map(|turn| format!("re:turn {turn}\n")).collect();
    assert_eq!(replies, format!("{expected}bye\n"));
    assert_eq!((code, success), (0, true));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_two_outputs_are_apart_and_the_exit_code_is_the_programs_own() {
    let (out, err, code, success, again): (String, String, i32, bool, i32) = run(r#"
        local child = sh("printf out; printf err >&2; exit 7")
        local status = assert(child:wait())
        -- what it wrote is still there after it ended
        local out, err = read_all(child), read_all(child, "read_stderr")
        -- the answer stays, also after a close
        child:close()
        return out, err, status.code, status.success, assert(child:wait()).code
    "#)
    .await;
    assert_eq!((out.as_str(), err.as_str()), ("out", "err"));
    assert_eq!((code, success, again), (7, false, 7));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_pass_as_they_are_and_a_read_takes_no_more_than_it_was_told() {
    let lua = make_engine().expect("engine");
    let (first, rest): (mlua::LuaString, mlua::LuaString) = run_in(
        &lua,
        r#"
        local child = assert(process.spawn({ program = "cat" }))
        -- not text, and no terminal in between: nothing is echoed, joined or rewritten
        assert(child:write("\0\255\254\r\n\tend"))
        assert(child:close_stdin())
        local first = assert(child:read(3))
        return first, read_all(child)
        "#,
    )
    .await;
    assert_eq!(&*first.as_bytes(), b"\0\xff\xfe");
    assert_eq!(&*rest.as_bytes(), b"\r\n\tend");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_has_its_arguments_directory_and_environment() {
    let dir = tempfile::tempdir().expect("a directory");
    let here = dir.path().canonicalize().expect("its real path");
    let lua = make_engine().expect("engine");
    lua.globals()
        .set("dir", here.to_string_lossy().into_owned())
        .expect("a global");
    let (args, cwd, added, cleared): (String, String, String, String) = run_in(
        &lua,
        r#"
        local function output(opts)
            local child = assert(process.spawn(opts))
            local out = read_all(child)
            assert(assert(child:wait()).success, read_all(child, "read_stderr"))
            return out
        end
        -- no shell reads the arguments: each arrives whole, and nothing in it is expanded
        local args = output({ program = "printf", args = { "%s|", "a b", "$HOME", "*", "" } })
        local cwd = output({ program = "pwd", args = { "-P" }, cwd = dir })
        local added = output({ program = "/bin/sh", args = { "-c", 'printf "%s:%s" "$NEOC_ADDED" "${PATH:+path}"' },
            env = { NEOC_ADDED = "added" } })
        local cleared = output({ program = "/usr/bin/env", env = { ONLY = "this" }, clear_env = true })
        return args, cwd, added, cleared
        "#,
    )
    .await;
    assert_eq!(args, "a b|$HOME|*||");
    assert_eq!(cwd.trim_end(), here.to_string_lossy());
    assert_eq!(added, "added:path", "the engine's environment is kept");
    assert_eq!(cleared, "ONLY=this\n");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_that_is_not_a_pipe_is_not_read_or_written() {
    let (out, null_out): (String, Value) = run(r#"
        -- no input: the process reads the end at once
        local child = sh("cat; echo done", { stdin = "null", stderr = "null" })
        local written, err = child:write("x")
        assert(written == nil and process.error_kind(err) == "closed", tostring(err))
        assert(string.find(err, "process.write: closed: stdin is not a pipe", 1, true) == 1, err)
        local closed, close_err = child:close_stdin()
        assert(closed == nil and process.error_kind(close_err) == "closed", tostring(close_err))
        local errors, read_err = child:read_stderr()
        assert(errors == nil and process.error_kind(read_err) == "closed", tostring(read_err))
        local out = read_all(child)
        assert(assert(child:wait()).success)

        -- no output: what the process writes is thrown away, and it ends all the same
        local silent = sh("echo lost; echo lost >&2", { stdout = "null", stderr = "null" })
        local null_out, null_err = silent:read()
        assert(process.error_kind(null_err) == "closed")
        assert(assert(silent:wait()).success)
        return out, null_out
    "#)
    .await;
    assert_eq!(out, "done\n");
    assert!(null_out.is_nil());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_killed_process_was_ended_by_a_signal() {
    let (code, signal, success): (Value, i32, bool) = run(r#"
        local child = sh("echo ready; exec sleep 1000")
        read_until(child, "ready")
        child:kill()
        local status = assert(child:wait())
        assert(child:id() == nil)
        -- nothing to kill now, and nothing fails
        child:kill()
        return status.code, status.signal, status.success
    "#)
    .await;
    assert!(code.is_nil(), "it did not end by itself: {code:?}");
    assert_eq!((signal, success), (9, false));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_that_writes_faster_than_it_is_read_waits_and_loses_nothing() {
    let (early, total): (bool, usize) = run(r#"
        -- 4 MiB, far more than the buffer and the pipe hold together
        local child = sh("head -c 4194304 /dev/zero; echo done >&2")
        -- nobody reads: it cannot have finished
        task.wait(0.3)
        local early = child:id() == nil
        local total = 0
        while true do
            local output, err = child:read()
            assert(err == nil, err)
            if not output then break end
            assert(#output <= 65536)
            total += #output
        end
        assert(read_all(child, "read_stderr") == "done\n")
        assert(assert(child:wait()).success)
        return early, total
    "#)
    .await;
    assert!(
        !early,
        "the process finished although nobody read its output"
    );
    assert_eq!(total, 4 * 1024 * 1024);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_tasks_run_while_a_read_a_write_and_a_wait_are_waiting() {
    let ticks: u32 = run(r#"
        local child = sh("sleep 0.3; cat >/dev/null")
        local ticks = 0
        local ticking = task.spawn(function()
            while true do
                task.wait(0.01)
                ticks += 1
            end
        end)
        -- far more than the pipe holds: the write waits until the process starts reading
        assert(child:write(string.rep("x", 1024 * 1024)))
        assert(child:close_stdin())
        assert(read_all(child) == "")
        assert(assert(child:wait()).success)
        ticking:cancel()
        return ticks
    "#)
    .await;
    assert!(ticks >= 5, "the ticking task ran {ticks} times");
}

// ---------------------------------------------------------------------------
// what is refused
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_cannot_be_started_is_a_spawn_error() {
    let (): () = run(r#"
        local child, err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
        assert(child == nil and process.error_kind(err) == "spawn", tostring(err))
        assert(string.find(err, "process.spawn: spawn: ", 1, true) == 1, err)
        local in_dir, dir_err = process.spawn({ program = "neoc-no-such-program", cwd = "/nowhere/neoc-no-such-directory" })
        assert(in_dir == nil and process.error_kind(dir_err) == "spawn", tostring(dir_err))
        assert(string.find(dir_err, "does not exist or is not a directory", 1, true), dir_err)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn options_that_are_not_a_command_are_refused_before_anything_starts() {
    let (): () = run(r#"
        for _, opts in { false, {}, { args = { "x" } }, { program = "" }, { progam = "sh" }, { program = "sh", stdin = "pipe" },
            { program = "sh", args = "x" }, { program = "sh", env = { "A=1" } }, { program = "sh", clear_env = 1 },
            { program = "a\0b" }, { program = setmetatable({}, { __index = function() error("read") end }) } } do
            local child, err = process.spawn(opts or nil)
            assert(child == nil and process.error_kind(err) == "invalid_input", tostring(err))
        end
        -- what is not a table of options
        for _, opts in { 7, "sh", true } do
            local child, err = process.spawn(opts)
            assert(child == nil and process.error_kind(err) == "invalid_input", tostring(err))
        end
        assert(pcall(process.spawn, buffer.create(4)) == false, "a buffer is raised, as everywhere")

        assert(process.error_kind(nil) == nil)
        assert(process.error_kind("process.read: io: x") == "io")
        assert(process.error_kind("fs.read: not_found: x") == nil)
        assert(process.error_kind("\255\254") == nil)
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_panics_on_hostile_input() {
    let (): () = run(r#"
        local child = assert(process.spawn({ program = "cat" }))
        for _, method in { "read", "read_stderr" } do
            for _, max in { 0, -1, 1.5, 65537, 0 / 0, math.huge } do
                local output, err = child[method](child, max)
                assert(output == nil and process.error_kind(err) == "invalid_input", tostring(max))
            end
        end
        -- wrong types are raised, as everywhere
        assert(pcall(child.write, child, {}) == false)
        assert(pcall(child.write, child) == false)
        assert(pcall(child.read, {}) == false)
        assert(pcall(child.read, child, "many") == false)
        -- a write past the limit is refused whole, and nothing is a write of nothing
        local big, big_err = child:write(string.rep("x", 1024 * 1024 + 1))
        assert(big == nil and process.error_kind(big_err) == "invalid_input")
        assert(child:write("") == true)
        assert(child:write("kept\n"))
        assert(read_until(child, "kept\n") == "kept\n")
        child:close()
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// closing, collection and shutdown
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_process_ends_reads_refuses_writes_and_is_gone() {
    let pid: u32 = run(r#"
        -- it reads nothing, so a write of more than the pipe holds waits
        local child = sh("echo ready; exec sleep 1000")
        read_until(child, "ready")
        local pid = child:id()

        local reading = task.spawn(function()
            return child:read()
        end)
        local writing = task.spawn(function()
            return child:write(string.rep("x", 1024 * 1024))
        end)
        task.wait(0.05)
        child:close()
        local output, err = reading:await()
        assert(output == nil and err == nil, "the waiting read: " .. tostring(output) .. ", " .. tostring(err))
        local written, write_err = writing:await()
        assert(written == nil and process.error_kind(write_err) == "closed", tostring(write_err))

        for _, method in { "read", "read_stderr" } do
            local again, again_err = child[method](child)
            assert(again == nil and again_err == nil)
        end
        local late, late_err = child:write("x")
        assert(late == nil and process.error_kind(late_err) == "closed", tostring(late_err))
        assert(child:id() == nil)

        -- none of these does anything now, and none fails
        child:kill()
        child:close()
        assert(child:close_stdin())
        local status = assert(child:wait())
        assert(status.success == false)
        return pid
    "#)
    .await;
    assert!(!alive(pid), "wait() returned, so the process was reaped");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handle_that_is_collected_ends_its_process() {
    let lua = make_engine().expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        local child = sh("echo ready; exec sleep 1000")
        read_until(child, "ready")
        return child:id()
        "#,
    )
    .await;
    assert!(alive(pid));
    // the script is over and kept nothing: the handle is garbage (a script has no `collectgarbage` of its own)
    lua.gc_collect().expect("collect");
    lua.gc_collect().expect("collect");
    gone(pid).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_stops_with_the_script_that_started_it() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        -- kept in globals: the script still holds the process, and a task of it waits in a read
        held = sh("echo ready; exec sleep 1000")
        read_until(held, "ready")
        reading = task.spawn(function()
            return held:read()
        end)
        task.wait(0.05)
        return held:id()
        "#,
    )
    .await;
    assert!(alive(pid));

    life.shutdown();
    gone(pid).await;

    // what the script still holds is a closed process, and nothing is started after the shutdown
    let (closed, late): (String, String) = run_in(
        &lua,
        r#"
        local _, err = held:write("x")
        local late, late_err = process.spawn({ program = "/bin/sh", args = { "-c", "exit 0" } })
        assert(late == nil)
        return process.error_kind(err), process.error_kind(late_err)
        "#,
    )
    .await;
    assert_eq!((closed.as_str(), late.as_str()), ("closed", "closed"));
}

/// A process that is still running when the runtime is dropped (no close, no shutdown) is killed with it.
#[cfg(unix)]
#[test]
fn a_process_does_not_outlive_the_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let lua = make_engine().expect("engine");
    let pid: u32 = runtime.block_on(run_in(
        &lua,
        r#"
        held = sh("echo ready; exec sleep 1000")
        read_until(held, "ready")
        return held:id()
        "#,
    ));
    assert!(alive(pid));

    let started = Instant::now();
    drop(runtime);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the runtime waited for the process: {:?}",
        started.elapsed()
    );
    while alive(pid) {
        assert!(
            started.elapsed() < TIMEOUT,
            "the process {pid} is still running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(lua);
}

// ---------------------------------------------------------------------------
// where a call cannot wait
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_that_cannot_wait_fails_and_does_nothing() {
    let dir = tempfile::tempdir().expect("a directory");
    let marker = dir.path().join("started");
    let lua = make_engine().expect("engine");
    lua.globals()
        .set("marker", marker.to_string_lossy().into_owned())
        .expect("a global");
    let output: String = run_in(
        &lua,
        r#"
        local child = assert(process.spawn({ program = "cat" }))

        -- a callback of string.gsub and a table.sort comparator cannot yield: the write fails and sends nothing
        assert(pcall(string.gsub, "abc", "b", function() child:write("written in gsub\n") return "x" end) == false)
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) child:write("written in sort\n") return a < b end) == false)
        assert(pcall(string.gsub, "abc", "b", function() child:wait() return "x" end) == false)
        -- and no program is started
        assert(pcall(string.gsub, "abc", "b", function()
            process.spawn({ program = "/bin/sh", args = { "-c", 'echo started > "$1"', "sh", marker } })
            return "x"
        end) == false)
        -- the calls that do not wait are fine there
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) child:id() return a < b end) == true)

        assert(child:write("after\n"))
        local output = read_until(child, "after")
        child:close()
        return output
        "#,
    )
    .await;
    assert_eq!(output, "after\n");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !marker.exists(),
        "a program was started by a call that failed"
    );
}

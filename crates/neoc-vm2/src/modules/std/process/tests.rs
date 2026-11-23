//! `std:process` against real processes. On Unix: `sh` and what every Unix has beside it (`cat`, `sleep`, `head`).
//! On Windows: `cmd`, `ping` (a process that stays) and PowerShell (a process that reads its input as a script
//! says). A test whose commands exist on one platform only says so with its `cfg`.
//!
//! Ported from neoc-vm's `std/process/tests.rs`, which ran its processes on Unix alone.

use std::time::{Duration, Instant};

use mlua::{Lua, Value};

use crate::{Life, make_engine, make_engine_in};

const TIMEOUT: Duration = Duration::from_secs(30);

/// What every script here starts with: the module, a read that goes on until the output holds `needle`, one that
/// goes on to the end, and the processes the tests share, each in its platform's words.
const PRELUDE: &str = r#"
    local process = require("std:process")
    local task = require("std:task")
    local windows = require("std:env").family == "windows"
    local function read_until(child, needle)
        local seen = ""
        while not string.find(seen, needle, 1, true) do
            local output, err = child:read()
            assert(err == nil, tostring(err))
            assert(output, "the output ended before `" .. needle .. "`: " .. seen)
            seen ..= output
        end
        return seen
    end
    local function read_all(child, method)
        local seen = ""
        while true do
            local output, err = child[method or "read"](child)
            assert(err == nil, tostring(err))
            if not output then
                return seen
            end
            seen ..= output
        end
    end
    -- `assert` for a call that returns a value or an error: Luau's own takes no error that is not text
    local function check(value, err)
        assert(value, tostring(err))
        return value
    end
    local function spawn(opts)
        return check(process.spawn(opts))
    end
    -- a line for the platform's shell: `sh` or `cmd`
    local function sh(script, opts)
        opts = opts or {}
        opts.program = if windows then "cmd.exe" else "/bin/sh"
        opts.args = if windows then { "/D", "/C", script } else { "-c", script }
        return spawn(opts)
    end
    -- a script for PowerShell (Windows)
    local function ps(script, opts)
        opts = opts or {}
        opts.program = "powershell.exe"
        opts.args = { "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script }
        return spawn(opts)
    end
    -- a process that has started, reads nothing and stays until it is killed
    local function sleeper()
        if windows then
            local child = spawn({ program = "ping.exe", args = { "-n", "1000", "127.0.0.1" } })
            read_until(child, "127.0.0.1")
            return child
        end
        local child = sh("echo ready; exec sleep 1000")
        read_until(child, "ready")
        return child
    end
    -- a process that writes back each line it reads, until the end of its input
    local function echo()
        if windows then
            return ps("while (($l = [Console]::In.ReadLine()) -ne $null) { [Console]::Out.Write($l + [char]10) }")
        end
        return spawn({ program = "cat" })
    end
    -- what a shell's `echo` wrote, without the platform's line ending and `cmd`'s space before a redirection
    local function trimmed(text)
        return (string.gsub(text, "%s+$", ""))
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

/// Is there a process `pid`? (`tasklist` lists it, or says that nothing matches.)
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    let listed = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&listed.stdout).contains(&format!(",\"{pid}\","))
}

/// Wait for the process `pid` to be gone.
async fn gone(pid: u32) {
    let started = Instant::now();
    while alive(pid) {
        assert!(started.elapsed() < TIMEOUT, "the process {pid} is still running");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---------------------------------------------------------------------------
// a process and its streams
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversation_of_lines_goes_both_ways_until_the_input_is_closed() {
    let (replies, code, success): (String, i32, bool) = run(r#"
        -- a helper that answers each line it reads, as a protocol over standard streams does
        local child = if windows
            then ps("while (($l = [Console]::In.ReadLine()) -ne $null) { [Console]::Out.Write('re:' + $l + [char]10) }; [Console]::Out.Write('bye' + [char]10)")
            else sh('while read -r line; do echo "re:$line"; done; echo bye')
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
        assert(written == nil and err.kind == "closed" and err.what == "stdin", tostring(err))
        assert(tostring(err) == "std:process.Child:write: closed: stdin is closed", tostring(err))
        local status = check(child:wait())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_two_outputs_are_apart_and_the_exit_code_is_the_programs_own() {
    let (out, err, code, success, again): (String, String, i32, bool, i32) = run(r#"
        local child = sh(if windows then "echo out& echo err 1>&2& exit 7" else "printf out; printf err >&2; exit 7")
        local status = check(child:wait())
        assert(status.signal == nil)
        -- what it wrote is still there after it ended
        local out, err = read_all(child), read_all(child, "read_stderr")
        -- the answer stays, also after a close
        child:close()
        return trimmed(out), trimmed(err), status.code, status.success, check(child:wait()).code
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
        local child = spawn({ program = "cat" })
        -- not text, and no terminal in between: nothing is echoed, joined or rewritten
        check(child:write("\0\255\254\r\n\tend"))
        check(child:close_stdin())
        local first = check(child:read(3))
        return first, read_all(child)
        "#,
    )
    .await;
    assert_eq!(&*first.as_bytes(), b"\0\xff\xfe");
    assert_eq!(&*rest.as_bytes(), b"\r\n\tend");
}

/// What Windows has of the test above: a read takes no more than it was told, and the rest stays for the next.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_takes_no_more_than_it_was_told() {
    let (first, rest): (String, String) = run(r#"
        local child = sh("echo abcdef")
        assert(check(child:wait()).success)
        local first = check(child:read(3))
        return first, trimmed(read_all(child))
    "#)
    .await;
    assert_eq!((first.as_str(), rest.as_str()), ("abc", "def"));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_has_its_arguments_directory_and_environment() {
    let dir = tempfile::tempdir().expect("a directory");
    let here = dir.path().canonicalize().expect("its real path");
    let lua = make_engine().expect("engine");
    lua.globals().set("dir", here.to_string_lossy().into_owned()).expect("a global");
    let (args, cwd, added, cleared): (String, String, String, String) = run_in(
        &lua,
        r#"
        local function output(opts)
            local child = spawn(opts)
            local out = read_all(child)
            assert(check(child:wait()).success, read_all(child, "read_stderr"))
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

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_has_its_directory_and_environment() {
    let dir = tempfile::tempdir().expect("a directory");
    let here = dir.path().canonicalize().expect("its real path");
    let lua = make_engine().expect("engine");
    lua.globals().set("dir", dir.path().to_string_lossy().into_owned()).expect("a global");
    let (cwd, added, cleared): (String, String, String) = run_in(
        &lua,
        r#"
        local function output(script, opts)
            local child = sh(script, opts)
            local out = read_all(child)
            assert(check(child:wait()).success, read_all(child, "read_stderr"))
            return out
        end
        local cwd = output("cd", { cwd = dir })
        local added = output("echo %NEOC_ADDED%:%SystemRoot%", { env = { NEOC_ADDED = "added" } })
        local cleared = output("set", { env = { ONLY = "this" }, clear_env = true })
        return trimmed(cwd), trimmed(added), cleared
        "#,
    )
    .await;
    let real = std::path::Path::new(&cwd).canonicalize().expect("the directory it printed");
    assert_eq!(real, here);
    let (ours, engines) = added.split_once(':').expect("two variables");
    assert_eq!(ours, "added");
    assert!(!engines.is_empty() && engines != "%SystemRoot%", "the engine's environment is kept: {added}");
    // `cmd` adds a few variables of its own to an empty environment; the engine's are not there
    assert!(cleared.contains("ONLY=this"), "{cleared}");
    assert!(!cleared.to_ascii_uppercase().contains("SYSTEMROOT="), "{cleared}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_that_is_not_a_pipe_is_not_read_or_written() {
    let (out, null_out): (String, Value) = run(r#"
        -- no input: the process reads the end at once
        local child = sh(if windows then "sort& echo done" else "cat; echo done", { stdin = "null", stderr = "null" })
        local written, err = child:write("x")
        assert(written == nil and err.kind == "not_piped" and err.stream == "stdin", tostring(err))
        assert(tostring(err) == "std:process.Child:write: not_piped: stdin is not a pipe", tostring(err))
        local closed, close_err = child:close_stdin()
        assert(closed == nil and close_err.kind == "not_piped" and close_err.stream == "stdin", tostring(close_err))
        local errors, read_err = child:read_stderr()
        assert(errors == nil and read_err.kind == "not_piped" and read_err.stream == "stderr", tostring(read_err))
        local out = read_all(child)
        assert(check(child:wait()).success)

        -- no output: what the process writes is thrown away, and it ends all the same
        local silent = sh(if windows then "echo lost& echo lost 1>&2" else "echo lost; echo lost >&2",
            { stdout = "null", stderr = "null" })
        local null_out, null_err = silent:read()
        assert(null_err.kind == "not_piped" and null_err.stream == "stdout")
        assert(check(silent:wait()).success)
        return trimmed(out), null_out
    "#)
    .await;
    assert_eq!(out, "done");
    assert!(null_out.is_nil());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_killed_process_did_not_end_by_itself() {
    let (code, signal, success): (Value, Value, bool) = run(r#"
        local child = sleeper()
        child:kill()
        local status = check(child:wait())
        assert(child:id() == nil)
        -- nothing to kill now, and nothing fails
        child:kill()
        return status.code, status.signal, status.success
    "#)
    .await;
    assert!(!success);
    // Unix says which signal ended it, and has no exit code for it; Windows has an exit code alone
    if cfg!(unix) {
        assert!(code.is_nil(), "it did not end by itself: {code:?}");
        assert_eq!(signal.as_i32(), Some(9));
    } else {
        assert!(signal.is_nil(), "{signal:?}");
        assert_eq!(code.as_i32(), Some(1));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_that_writes_faster_than_it_is_read_waits_and_loses_nothing() {
    // 4 MiB, far more than the buffer and the pipe hold together
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(
        dir.path().join("lines"),
        "1234567
"
        .repeat(512 * 1024),
    )
    .expect("a file");
    let lua = make_engine().expect("engine");
    lua.globals().set("dir", dir.path().to_string_lossy().into_owned()).expect("a global");
    let (early, total): (bool, usize) = run_in(
        &lua,
        r#"
        local child = sh(if windows then "type lines& echo done 1>&2" else "cat lines; echo done >&2", { cwd = dir })
        -- nobody reads: it cannot have finished
        task.wait(0.3)
        local early = child:id() == nil
        local total = 0
        while true do
            local output, err = child:read()
            assert(err == nil, tostring(err))
            if not output then break end
            assert(#output <= 65536)
            total += #output
        end
        assert(trimmed(read_all(child, "read_stderr")) == "done")
        assert(check(child:wait()).success)
        return early, total
        "#,
    )
    .await;
    assert!(!early, "the process finished although nobody read its output");
    assert_eq!(total, 4 * 1024 * 1024);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_tasks_run_while_a_read_a_write_and_a_wait_are_waiting() {
    let ticks: u32 = run(r#"
        local child = if windows
            then ps("Start-Sleep -Milliseconds 300; [Console]::In.ReadToEnd() | Out-Null")
            else sh("sleep 0.3; cat >/dev/null")
        local ticks = 0
        local ticking = task.spawn(function()
            while true do
                task.wait(0.01)
                ticks += 1
            end
        end)
        -- far more than the pipe holds: the write waits until the process starts reading
        check(child:write(string.rep("x", 1024 * 1024)))
        check(child:close_stdin())
        assert(read_all(child) == "")
        assert(check(child:wait()).success)
        ticking:cancel()
        return ticks
    "#)
    .await;
    assert!(ticks >= 5, "the ticking task ran {ticks} times");
}

/// A megabyte on its way into a process that took a little of it and reads no more until the file `go` is there,
/// then counts the rest. The pipe holds far less than a megabyte, so once the process has said `started` the
/// writer is in the middle of that write, for as long as the test wants. `counted()` is what the process counted
/// and what it took first, together.
const A_WRITE_BEING_MADE: &str = r#"
        local megabyte = string.rep("x", 1024 * 1024)
        local child = if windows
            then ps("$first = [Console]::In.Read(); [Console]::Error.Write('started' + [char]10); "
                .. "while (-not (Test-Path $env:NEOC_GO)) { Start-Sleep -Milliseconds 20 }; "
                .. "[Console]::Out.Write([Console]::In.ReadToEnd().Length + 1)", { env = { NEOC_GO = go } })
            else spawn({ program = "/bin/sh", args = { "-c",
                'dd bs=1 count=1 >/dev/null 2>&1; echo started >&2; while [ ! -e "$1" ]; do sleep 0.02; done; echo $(($(wc -c) + 1))',
                "sh", go } })
        -- a task gives one value: the error of the write, or `false` for a write that was made
        local function write_megabyte()
            return task.spawn(function()
                local _, err = child:write(megabyte)
                return err or false
            end)
        end
        local being_made = write_megabyte()
        assert(child:read_stderr() == "started\n")
        local function go_on()
            assert(require("std:fs").write(go, ""))
        end
        local function counted()
            local count = tonumber(trimmed(read_all(child)))
            assert(check(child:wait()).success)
            return count
        end
"#;

/// An engine whose scripts have `go`, the path of a file that is not there yet.
fn engine_with_go(dir: &tempfile::TempDir) -> Lua {
    let lua = make_engine().expect("engine");
    lua.globals().set("go", dir.path().join("go").to_string_lossy().into_owned()).expect("a global");
    lua
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_the_input_ends_it_after_the_write_being_made_and_refuses_those_behind() {
    let dir = tempfile::tempdir().expect("a directory");
    let counted: u32 = run_in(
        &engine_with_go(&dir),
        &format!(
            r#"{A_WRITE_BEING_MADE}
        local behind = {{ write_megabyte(), write_megabyte() }}
        check(child:close_stdin())
        go_on()
        -- the write the pipe was taking is finished, and those behind it are refused
        assert(being_made:await() == false)
        for _, write in behind do
            local err = write:await()
            assert(err and err.kind == "closed" and err.what == "stdin", tostring(err))
        end
        -- then the process reads the end of its input
        return counted()
        "#
        ),
    )
    .await;
    assert_eq!(counted, 1024 * 1024, "the one write");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_write_is_still_made_and_does_not_keep_the_input_open() {
    let dir = tempfile::tempdir().expect("a directory");
    let counted: u32 = run_in(
        &engine_with_go(&dir),
        &format!(
            r#"{A_WRITE_BEING_MADE}
        being_made:cancel()
        go_on()
        -- the end of the input arrives, although the task that waited in the write was cancelled
        check(child:close_stdin())
        return counted()
        "#
        ),
    )
    .await;
    assert_eq!(counted, 1024 * 1024, "a write cannot be taken back");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_written_to_a_process_that_has_ended() {
    let (): () = run(r#"
        local child = sh("exit 0")
        assert(check(child:wait()).success)
        for _, data in { "late\n", "" } do
            local written, err = child:write(data)
            assert(written == nil and err.kind == "closed" and err.what == "stdin", tostring(err))
        end
    "#)
    .await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_input_of_a_process_that_has_ended_is_closed() {
    let seen: String = run(r#"
        -- the process ends at once and leaves a child that reads its input to the end
        -- (it reads a copy of the input: a shell gives what it starts in the background no input at all)
        local child = sh("exec 3<&0; (cat <&3 >/dev/null; echo end-of-input) & exit 0")
        assert(check(child:wait()).success)
        for _, data in { "late\n", "" } do
            local written, err = child:write(data)
            assert(written == nil and err.kind == "closed", tostring(err))
        end
        -- nobody closed the input: the writer let go of it when the process ended
        return read_all(child)
    "#)
    .await;
    assert_eq!(seen, "end-of-input\n");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kill_reaches_what_the_process_started() {
    let (): () = run(r#"
        -- a wrapper, as `sh -c` or `npx` is: the program that does the work is its child, and holds the output
        local child = sh("sleep 1000 & echo started; wait")
        read_until(child, "started\n")
        child:kill()
        assert(check(child:wait()).signal == 9)
        -- the output ends only when the last process that holds it is gone
        assert(read_all(child) == "")
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// what is refused
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_cannot_be_started_is_a_spawn_error() {
    let (): () = run(r#"
        local child, err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
        assert(child == nil and err.kind == "spawn" and err.cause == "not_found", tostring(err))
        assert(string.find(tostring(err), "std:process.spawn: spawn: ", 1, true) == 1, tostring(err))
        assert(err.op == "std:process.spawn" and err.message == err.reason)
        -- a name that is in no directory of `PATH`
        local named, name_err = process.spawn({ program = "neoc-no-such-program" })
        assert(named == nil and name_err.kind == "spawn" and name_err.cause == "not_found", tostring(name_err))
        local in_dir, dir_err = process.spawn({ program = "neoc-no-such-program", cwd = "/nowhere/neoc-no-such-directory" })
        assert(in_dir == nil and dir_err.kind == "spawn" and dir_err.cause == "no_directory", tostring(dir_err))
        assert(string.find(dir_err.reason, "does not exist or is not a directory", 1, true), dir_err.reason)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_the_system_would_not_take_is_refused_before_anything_starts() {
    let (): () = run(r#"
        local many = table.create(1025, "")
        local refused = {
            program = { program = "" },
            ["NUL"] = { program = "a\0b" },
            args = { program = "sh", args = many },
            cwd = { program = "sh", cwd = "" },
            env = { program = "sh", env = { ["A=B"] = "1" } },
        }
        for words, command in refused do
            local child, err = process.spawn(command)
            assert(child == nil and err.kind == "invalid_input", words .. ": " .. tostring(err))
            assert(string.find(err.reason, words, 1, true), err.reason)
            assert(string.find(tostring(err), "std:process.spawn: invalid_input: option \"", 1, true) == 1, tostring(err))
        end
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_command_is_raised_with_its_place() {
    let (): () = run(r#"
        local not_commands = {
            {}, { args = { "x" } }, { progam = "sh" }, { program = "sh", stdin = "pipe" }, { program = "sh", args = "x" },
            { program = "sh", args = { 1 } }, { program = "sh", args = { "a", nil, "c" } }, { program = "sh", env = { A = 1 } },
            { program = "sh", clear_env = 1 }, { program = 7 }, { program = "sh", cwd = {} },
            { program = setmetatable({}, { __index = function() error("read") end }) },
            7, "sh", true, buffer.create(4),
        }
        for index, command in not_commands do
            local ok, err = pcall(process.spawn, command)
            assert(not ok, "command " .. index .. " was taken")
            assert(string.find(tostring(err), "std:process.spawn: argument #1 'command'", 1, true), tostring(err))
        end
        assert(not pcall(process.spawn))
        local _, unknown = pcall(process.spawn, { progam = "sh" })
        assert(string.find(tostring(unknown), "progam", 1, true), tostring(unknown))
        local _, stream = pcall(process.spawn, { program = "sh", stdout = "pipe" })
        assert(string.find(tostring(stream), "piped", 1, true), tostring(stream))
        -- `error_kind` read a kind out of an error's text: the error has it now
        assert(process.error_kind == nil)
    "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_panics_on_hostile_input() {
    let (): () = run(r#"
        local child = echo()
        for _, method in { "read", "read_stderr" } do
            for _, max in { 0, 65537, 2 ^ 31 } do
                local output, err = child[method](child, max)
                assert(output == nil and err.kind == "invalid_input", tostring(max))
                assert(string.find(err.reason, "`max` must be a whole number from 1 to 65536", 1, true), err.reason)
            end
            -- what is not a count of bytes is a wrong argument
            for _, max in { -1, 1.5, 0 / 0, math.huge, "many" } do
                local ok, err = pcall(child[method], child, max)
                assert(not ok and string.find(tostring(err), "argument #1 'max'", 1, true), tostring(err))
            end
        end
        -- wrong types are raised, as everywhere
        assert(pcall(child.write, child, {}) == false)
        assert(pcall(child.write, child) == false)
        assert(pcall(child.read, {}) == false)
        -- a write past the limit is refused whole, and nothing is a write of nothing
        local big, big_err = child:write(string.rep("x", 1024 * 1024 + 1))
        assert(big == nil and big_err.kind == "invalid_input")
        assert(child:write("") == true)
        check(child:write("kept\n"))
        assert(read_until(child, "kept\n") == "kept\n")
        child:close()
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// closing, collection and shutdown
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_process_ends_reads_refuses_writes_and_is_gone() {
    let pid: u32 = run(r#"
        -- it reads nothing, so a write of more than the pipe holds waits
        local child = sleeper()
        local pid = child:id()

        local reading = task.spawn(function()
            local output, err = child:read_stderr()
            return output == nil and err == nil
        end)
        local writing = task.spawn(function()
            return select(2, child:write(string.rep("x", 1024 * 1024)))
        end)
        task.wait(0.05)
        child:close()
        assert(reading:await() == true, "the waiting read ended, with nothing")
        local write_err = writing:await()
        assert(write_err and write_err.kind == "closed", tostring(write_err))

        for _, method in { "read", "read_stderr" } do
            local again, again_err = child[method](child)
            assert(again == nil and again_err == nil)
        end
        local late, late_err = child:write("x")
        assert(late == nil and late_err.kind == "closed", tostring(late_err))
        assert(child:id() == nil)

        -- none of these does anything now, and none fails
        child:kill()
        child:close()
        check(child:close_stdin())
        local status = check(child:wait())
        assert(status.success == false)
        return pid
    "#)
    .await;
    assert!(!alive(pid), "wait() returned, so the process was reaped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handle_that_is_collected_ends_its_process() {
    let lua = make_engine().expect("engine");
    let pid: u32 = run_in(&lua, "return sleeper():id()").await;
    assert!(alive(pid));
    // the script is over and kept nothing: the handle is garbage (a script has no `collectgarbage` of its own)
    lua.gc_collect().expect("collect");
    lua.gc_collect().expect("collect");
    gone(pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_stops_with_the_script_that_started_it() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        -- kept in globals: the script still holds the process, and a task of it waits in a read
        held = sleeper()
        reading = task.spawn(function()
            return held:read_stderr()
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
    let (closed, late, what): (String, String, String) = run_in(
        &lua,
        r#"
        local _, err = held:write("x")
        local late, late_err = process.spawn({ program = if windows then "cmd.exe" else "/bin/sh" })
        assert(late == nil)
        return err.kind, late_err.kind, late_err.what
        "#,
    )
    .await;
    assert_eq!((closed.as_str(), late.as_str(), what.as_str()), ("closed", "closed", "engine"));
}

/// A process that is still running when the runtime is dropped (no close, no shutdown) is killed with it.
#[test]
fn a_process_does_not_outlive_the_runtime() {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
    let lua = make_engine().expect("engine");
    let pid: u32 = runtime.block_on(run_in(
        &lua,
        r#"
        held = sleeper()
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
        assert!(started.elapsed() < TIMEOUT, "the process {pid} is still running");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(lua);
}

// ---------------------------------------------------------------------------
// where a call cannot wait
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_that_cannot_wait_fails_and_does_nothing() {
    let dir = tempfile::tempdir().expect("a directory");
    let marker = dir.path().join("started");
    let lua = make_engine().expect("engine");
    lua.globals().set("dir", dir.path().to_string_lossy().into_owned()).expect("a global");
    let output: String = run_in(
        &lua,
        r#"
        local child = echo()

        -- a callback of string.gsub and a table.sort comparator cannot yield: the write fails and sends nothing
        assert(pcall(string.gsub, "abc", "b", function() child:write("written in gsub\n") return "x" end) == false)
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) child:write("written in sort\n") return a < b end) == false)
        assert(pcall(string.gsub, "abc", "b", function() child:wait() return "x" end) == false)
        assert(pcall(string.gsub, "abc", "b", function() child:read() return "x" end) == false)
        -- and no program is started
        assert(pcall(string.gsub, "abc", "b", function()
            sh(if windows then "echo started> started" else "echo started > started", { cwd = dir })
            return "x"
        end) == false)
        -- the calls that do not wait are fine there
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) child:id() return a < b end) == true)

        check(child:write("after\n"))
        local output = read_until(child, "after")
        child:close()
        return output
        "#,
    )
    .await;
    assert_eq!(output, "after\n");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!marker.exists(), "a program was started by a call that failed");
}

/// The command a script's table is read into: a stream is a pipe unless the script says otherwise, and a field
/// that was not given is not there.
#[test]
fn a_command_is_read_from_a_scripts_table() {
    use super::process::{Command, Stdio};
    use crate::modules::std::osstr::OsPath;

    let lua = Lua::new();
    let read = |source: &str| -> Result<Command, String> {
        let value: Value = lua.load(source).eval().expect("a value");
        mlua_ir::from_serde(value, &lua)
    };
    let cat = read("{ program = 'cat' }").expect("a command");
    assert_eq!(cat.program, OsPath::of("cat"));
    assert_eq!((cat.args.clone(), cat.cwd.clone(), cat.env.clone(), cat.clear_env), (None, None, None, None));
    assert_eq!((cat.stdin, cat.stdout, cat.stderr), (None, None, None));
    assert_eq!(Stdio::default(), Stdio::Piped);
    assert_eq!(cat.check(), Ok(()));

    let whole = read(
        r#"{
            program = "sh",
            args = { "-c", "echo hi" },
            cwd = "/tmp",
            env = { B = "2", A = "1" },
            clear_env = true,
            stdin = "null",
            stdout = "piped",
            stderr = "inherit",
        }"#,
    )
    .expect("a command");
    assert_eq!(whole.args.as_deref(), Some(&["-c".to_owned(), "echo hi".to_owned()][..]));
    assert_eq!(whole.cwd, Some(OsPath::of("/tmp")));
    // a table has no order of its own: the same environment is given in the same order
    let env: Vec<(String, String)> = whole.env.clone().expect("variables").into_iter().collect();
    assert_eq!(env, [("A".to_owned(), "1".to_owned()), ("B".to_owned(), "2".to_owned())]);
    assert_eq!(whole.clear_env, Some(true));
    assert_eq!(
        (whole.stdin, whole.stdout, whole.stderr),
        (Some(Stdio::Null), Some(Stdio::Piped), Some(Stdio::Inherit))
    );

    for (source, word) in [
        ("{}", "program"),
        ("{ progam = 'sh' }", "progam"),
        ("{ program = 'sh', stdin = 'pipe' }", "pipe"),
        ("{ program = 'sh', clear_env = 'yes' }", "boolean"),
    ] {
        let refused = read(source).expect_err("it is no command");
        assert!(refused.contains(word), "{source}: {refused}");
    }
}

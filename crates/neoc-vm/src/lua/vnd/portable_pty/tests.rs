//! `vnd:portable_pty` against real terminals and real processes: `sh` and what every Unix has beside it (`cat`,
//! `sleep`, `stty`). The tests that start a process are Unix's: the commands are, and no test here was run on
//! Windows. What is refused before anything starts is tested on every platform.

use std::time::{Duration, Instant};

use mlua::{Lua, Value};

use crate::lua::lifecycle::Life;
use crate::lua::sandbox::{make_engine, make_engine_in};

const TIMEOUT: Duration = Duration::from_secs(10);

/// What every script here starts with: the module, and a read that goes on until the output holds `needle`.
const PRELUDE: &str = r#"
    local portable_pty = require("vnd:portable_pty")
    local task = require("std:task")
    local function read_until(pty, needle, seen)
        seen = seen or ""
        while not string.find(seen, needle, 1, true) do
            local output, err = pty:read()
            assert(err == nil, err)
            assert(output, "the output ended before `" .. needle .. "`: " .. seen)
            seen ..= output
        end
        return seen
    end
    local function sh(script, opts)
        opts = opts or {}
        opts.program = "/bin/sh"
        opts.args = { "-c", script }
        return assert(portable_pty.spawn_command(opts))
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
// a terminal and its process
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_reads_writes_resizes_kills_waits_and_closes() {
    let (first, second, signalled, success): (String, String, bool, bool) = run(r#"
        local pty = sh('echo ready; read line; echo "got:$line"; stty size; read line; stty size; cat', { rows = 24, cols = 80 })
        assert(type(pty:process_id()) == "number")
        read_until(pty, "ready")

        -- what is written is the process's input, and the terminal echoes it as one would
        assert(pty:write("one\n") == true)
        local first = read_until(pty, "24 80")
        assert(string.find(first, "got:one", 1, true), first)

        assert(pty:resize(40, 100) == true)
        assert(pty:write("\n"))
        local second = read_until(pty, "40 100")

        -- `cat` now: it would run for ever
        pty:kill()
        local status = assert(pty:wait())
        assert(pty:process_id() == nil, "an ended process has no id")
        -- the end of the output, after whatever was still on its way
        repeat
            local output, err = pty:read()
            assert(err == nil, err)
        until output == nil
        pty:close()
        return first, second, status.signal ~= nil, status.success
    "#)
    .await;
    assert!(first.contains("24 80"), "{first:?}");
    assert!(second.contains("40 100"), "{second:?}");
    assert!(signalled, "a killed process was ended by a signal");
    assert!(!success);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_status_is_the_commands_own_and_the_output_ends_with_it() {
    let (code, signal, success, output, again): (u32, Value, bool, String, u32) = run(r#"
        local pty = sh("printf 'bye'; exit 7")
        local status = assert(pty:wait())
        local output = ""
        while true do
            local piece, err = pty:read()
            assert(err == nil, err)
            if not piece then break end
            output ..= piece
        end
        -- the answer stays, also after a close
        pty:close()
        return status.exit_code, status.signal, status.success, output, assert(pty:wait()).exit_code
    "#)
    .await;
    assert_eq!((code, success, again), (7, false, 7));
    assert!(signal.is_nil(), "it ended by itself: {signal:?}");
    assert_eq!(output, "bye");

    let (code, success): (u32, bool) = run(r#"
        local status = assert(sh("true"):wait())
        return status.exit_code, status.success
    "#)
    .await;
    assert_eq!((code, success), (0, true));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_command_gets_its_arguments_directory_and_environment() {
    let dir = tempfile::tempdir().expect("dir");
    let resolved = dir.path().canonicalize().expect("canonicalize");
    let output: String = run_in(
        &make_engine().expect("engine"),
        &format!(
            r#"
            local pty = assert(portable_pty.spawn_command({{
                program = "sh",
                args = {{ "-c", 'echo "$0|$1|$NEOC_PTY_TEST|$TERM|$(pwd -P)|${{HOME:+home}}|end"', "zero", "arg one" }},
                cwd = {dir:?},
                env = {{ NEOC_PTY_TEST = "set here", TERM = "xterm-256color" }},
            }}))
            return read_until(pty, "|end")
            "#,
            dir = dir.path().to_str().expect("utf-8"),
        ),
    )
    .await;
    // a bare name is found in PATH, the environment is the process's own plus `env`
    let expected = format!(
        "zero|arg one|set here|xterm-256color|{}|home|end",
        resolved.display()
    );
    assert!(output.contains(&expected), "{output:?} lacks {expected:?}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_program_it_is_the_users_shell_in_their_home() {
    // the shell reads the user's own start-up files, so the test asks it for nothing but to leave
    let exited: bool = run(r#"
        local pty = assert(portable_pty.spawn_command())
        assert(type(pty:process_id()) == "number")
        pty:kill()
        local status = assert(pty:wait())
        return status.exit_code ~= nil
    "#)
    .await;
    assert!(exited);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_that_ignores_the_hangup_is_killed_after_the_grace() {
    let started = Instant::now();
    let (pid, signal): (u32, String) = run(r#"
        local pty = sh('trap "" HUP; echo ready; exec sleep 1000')
        read_until(pty, "ready")
        local pid = pty:process_id()
        pty:kill()
        return pid, assert(pty:wait()).signal
    "#)
    .await;
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "the process was given no time to end by itself"
    );
    assert!(signal.to_lowercase().contains("kill"), "{signal:?}");
    assert!(!alive(pid));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_is_bounded_and_a_flood_waits_for_the_reader() {
    let (longest, total): (usize, usize) = run(r#"
        local pty = sh("yes 0123456789")
        -- left alone, the process fills what is kept for the script and then waits in its write (the bound itself
        -- is held by `terminal::tests`)
        task.wait(0.2)
        local longest, total = 0, 0
        for _ = 1, 200 do
            local output = assert(pty:read(1000))
            assert(#output >= 1, "a read returns something or the end")
            longest = math.max(longest, #output)
            total += #output
        end
        local whole = assert(pty:read())
        assert(#whole <= 65536)
        pty:close()
        return longest, total
    "#)
    .await;
    assert!(longest <= 1000, "a read returned {longest} bytes");
    assert!(total >= 200);
}

// ---------------------------------------------------------------------------
// what cannot be started
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_program_is_a_spawn_error() {
    let (by_path, by_name, kind): (String, String, String) = run(r#"
        local pty, err = portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })
        assert(pty == nil)
        local named, name_err = portable_pty.spawn_command({ program = "neoc-no-such-program", args = { "x" } })
        assert(named == nil)
        return err, name_err, portable_pty.error_kind(err)
    "#)
    .await;
    assert!(
        by_path.starts_with("portable_pty.spawn_command: spawn: "),
        "{by_path}"
    );
    assert!(by_path.contains("neoc-no-such-program"), "{by_path}");
    assert!(
        by_name.starts_with("portable_pty.spawn_command: spawn: "),
        "{by_name}"
    );
    assert_eq!(kind, "spawn");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_directory_that_is_not_there_is_a_spawn_error_not_the_home_directory() {
    let err: String = run(r#"
        local pty, err = portable_pty.spawn_command({ cwd = "/nowhere/neoc-no-such-directory" })
        assert(pty == nil)
        assert(portable_pty.error_kind(err) == "spawn")
        return err
    "#)
    .await;
    assert!(err.contains("/nowhere/neoc-no-such-directory"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_command_is_refused_before_anything_starts() {
    let (): () = run(r#"
        local function refused(opts, needle)
            local pty, err = portable_pty.spawn_command(opts)
            assert(pty == nil, "accepted")
            assert(portable_pty.error_kind(err) == "invalid_input", err)
            assert(string.find(err, "portable_pty.spawn_command: invalid_input: ", 1, true) == 1, err)
            assert(string.find(err, needle, 1, true), err)
        end
        refused({ progam = "sh" }, "unknown option `progam`")
        refused("sh", "table of named options")
        refused({ program = "" }, "`program`")
        refused({ program = "sh", args = "-c" }, "`args`")
        refused({ args = { "-l" } }, "needs `program`")
        refused({ program = "sh", env = { A = 1 } }, "`env`")
        refused({ program = "sh", rows = 0 }, "`rows`")
        refused({ program = "sh", cols = 1.5 }, "`cols`")
        refused({ program = "sh", cols = 40000 }, "`cols`")
        refused({ program = "a\0b" }, "NUL")
        refused({ program = function() end }, "")
        local cycle = {}
        cycle.env = cycle
        refused(cycle, "")

        -- a buffer is raised, as every conversion of a script value raises it
        local ok, raised = pcall(portable_pty.spawn_command, { program = buffer.create(2) })
        assert(ok == false and string.find(tostring(raised), "portable_pty.spawn_command", 1, true), tostring(raised))

        assert(portable_pty.error_kind(nil) == nil)
        assert(portable_pty.error_kind("fs.read: not_found: x") == nil)
        assert(portable_pty.error_kind("\255\254") == nil)

        local _, err = portable_pty.spawn_command({ rows = 0 })
        local message = assert(portable_pty.error_message(err), tostring(err))
        assert(message ~= "" and err == "portable_pty.spawn_command: invalid_input: " .. message, err)
        assert(portable_pty.error_message("portable_pty.write: closed") == "")
        assert(portable_pty.error_message("portable_pty.write: nonsense: x") == nil)
        assert(portable_pty.error_message(nil) == nil)
        assert(portable_pty.error_message("fs.read: not_found: x") == nil)
        assert(portable_pty.error_message("\255\254") == nil)
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// closing, collection and shutdown
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_terminal_ends_reads_refuses_writes_and_has_ended_its_process() {
    let pid: u32 = run(r#"
        local pty = sh("echo ready; cat")
        read_until(pty, "ready")
        local pid = pty:process_id()

        -- a read that is waiting when the terminal is closed ends too, and so does a write
        local waiting = task.spawn(function()
            return pty:read()
        end)
        task.wait(0.05)
        pty:close()
        local output, err = waiting:await()
        assert(output == nil and err == nil, "the waiting read: " .. tostring(output) .. ", " .. tostring(err))

        local again, again_err = pty:read()
        assert(again == nil and again_err == nil)
        local written, write_err = pty:write("x")
        assert(written == nil and portable_pty.error_kind(write_err) == "closed", tostring(write_err))
        assert(string.find(write_err, "portable_pty.write: closed: ", 1, true) == 1, write_err)
        local empty, empty_err = pty:write("")
        assert(empty == nil and portable_pty.error_kind(empty_err) == "closed")
        local resized, resize_err = pty:resize(10, 10)
        assert(resized == nil and portable_pty.error_kind(resize_err) == "closed", tostring(resize_err))
        assert(pty:process_id() == nil)

        -- none of these does anything now, and none fails
        pty:kill()
        pty:close()
        local status = assert(pty:wait())
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
        local pty = sh("echo ready; cat")
        read_until(pty, "ready")
        return pty:process_id()
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
async fn a_terminal_stops_with_the_script_that_opened_it() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        -- kept in globals: the script still holds the terminal, and a task of it waits in a read
        held = sh("echo ready; cat")
        read_until(held, "ready")
        reading = task.spawn(function()
            return held:read()
        end)
        task.wait(0.05)
        return held:process_id()
        "#,
    )
    .await;
    assert!(alive(pid));

    life.shutdown();
    gone(pid).await;

    // what the script still holds is a closed terminal, and nothing can be opened after the shutdown
    let (closed, late): (String, u32) = run_in(
        &lua,
        r#"
        local _, err = held:write("x")
        local late = sh("echo ready; cat")
        local late_pid = late:process_id()
        local output, read_err = late:read()
        assert(output == nil and read_err == nil, "a terminal opened after the shutdown is closed")
        return portable_pty.error_kind(err), late_pid or 0
        "#,
    )
    .await;
    assert_eq!(closed, "closed");
    assert_eq!(
        late, 0,
        "a terminal opened after the shutdown has no process"
    );
}

/// The reader of a silent process sits in a read the operating system does not interrupt. Neither the engine's
/// shutdown nor the runtime's own waits for it.
#[cfg(unix)]
#[test]
fn a_reader_blocked_in_the_operating_system_holds_neither_the_engine_nor_the_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = runtime.block_on(run_in(
        &lua,
        r#"
        -- it ignores the hangup and says nothing: the reader waits in its read until the process is killed
        held = sh('trap "" HUP; echo ready; exec sleep 1000')
        read_until(held, "ready")
        return held:process_id()
        "#,
    ));

    let started = Instant::now();
    life.shutdown();
    drop(lua);
    drop(runtime);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the shutdown waited for the terminal: {:?}",
        started.elapsed()
    );

    // and the process is ended all the same, by the thread that owns it
    let started = Instant::now();
    while alive(pid) {
        assert!(
            started.elapsed() < TIMEOUT,
            "the process {pid} is still running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------
// where a call cannot wait
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_that_cannot_wait_fails_and_does_nothing() {
    let output: String = run(r#"
        local pty = sh("stty -echo; echo ready; cat")
        read_until(pty, "ready")

        -- a callback of string.gsub and a table.sort comparator cannot yield: the write fails and types nothing
        assert(pcall(string.gsub, "abc", "b", function() pty:write("typed in gsub\n") return "x" end) == false)
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) pty:write("typed in sort\n") return a < b end) == false)
        assert(pcall(string.gsub, "abc", "b", function() pty:wait() return "x" end) == false)
        -- and no command is started
        assert(pcall(string.gsub, "abc", "b", function()
            portable_pty.spawn_command({ program = "/bin/sh", args = { "-c", "exit 0" } })
            return "x"
        end) == false)
        -- the calls that do not wait are fine there
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) pty:resize(30, 90) pty:process_id() return a < b end) == true)

        assert(pty:write("after\n"))
        local output = read_until(pty, "after")
        pty:close()
        return output
    "#)
    .await;
    assert!(!output.contains("typed"), "{output:?}");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_panics_on_hostile_input() {
    let (): () = run(r#"
        local pty = sh("echo ready; cat")
        read_until(pty, "ready")
        for _, max in { 0, -1, 1.5, 65537, 0 / 0, math.huge } do
            local output, err = pty:read(max)
            assert(output == nil and portable_pty.error_kind(err) == "invalid_input", tostring(max))
        end
        for _, size in { { 0, 80 }, { 24, 0 }, { -1, -1 }, { 0 / 0, 80 }, { 24, math.huge }, { 1.5, 80 }, { 40000, 80 } } do
            local resized, err = pty:resize(size[1], size[2])
            assert(resized == nil and portable_pty.error_kind(err) == "invalid_input")
        end
        -- wrong types are raised, as everywhere
        assert(pcall(pty.resize, pty, "a", {}) == false)
        assert(pcall(pty.write, pty, {}) == false)
        assert(pcall(pty.write, pty) == false)
        assert(pcall(pty.read, {}) == false)
        -- a write past the limit is refused whole, and input need not be text
        local big, big_err = pty:write(string.rep("x", 1024 * 1024 + 1))
        assert(big == nil and portable_pty.error_kind(big_err) == "invalid_input")
        assert(pty:write("\0\255\254 not text\n"))
        pty:close()
    "#)
    .await;
}

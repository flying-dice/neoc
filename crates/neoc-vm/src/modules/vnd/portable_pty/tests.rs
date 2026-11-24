//! `vnd:portable_pty` against real terminals and real processes. On Unix: `sh` and what every Unix has beside it
//! (`cat`, `sleep`, `stty`). On Windows: `cmd` and PowerShell on a ConPTY pseudo-console. A test whose commands
//! exist on one platform only says so with its `cfg`; what is refused before anything starts is the same everywhere.

use std::time::{Duration, Instant};

use mlua::{Lua, Value};

use crate::{Life, make_engine, make_engine_in};

const TIMEOUT: Duration = Duration::from_secs(30);

/// What every script here starts with: the module, a read that goes on until the output holds `needle`, one that
/// goes on to the end, and the processes the tests share, each in its platform's words.
const PRELUDE: &str = r#"
    local portable_pty = require("vnd:portable_pty")
    local task = require("std:task")
    local windows = require("std:env").family == "windows"
    -- what ends a line of input: a ConPTY takes keys, and the key is Enter
    local enter = if windows then "\r" else "\n"
    -- a terminal's answer to "where is the cursor?" (`ESC [ 6 n`): row 1, column 1
    local cursor_at_home = "\27[1;1R"
    local function read_until(pty, needle, seen)
        seen = seen or ""
        while not string.find(seen, needle, 1, true) do
            local output, err = pty:read()
            assert(err == nil, tostring(err))
            assert(output, "the output ended before `" .. needle .. "`: " .. seen)
            seen ..= output
        end
        return seen
    end
    local function read_all(pty)
        local seen = ""
        while true do
            local output, err = pty:read()
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
        local pty = check(portable_pty.spawn_command(opts))
        if windows then
            -- A pseudo-console asks where the cursor is before anything runs on it, and waits for the answer: the
            -- top left corner. (A terminal that is closed already takes none.)
            pty:write(cursor_at_home)
        end
        return pty
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
        opts.args = { "-NoLogo", "-NoProfile", "-Command", script }
        return spawn(opts)
    end
    -- a process that has said `ready` and then reads its input until it is ended
    local function stays()
        local pty
        if windows then
            pty = spawn({ program = "cmd.exe", args = { "/D", "/Q", "/K", "echo ready" } })
        else
            -- `exit 3`: the hangup may reach `cat` as the end of its input before the signal does, and `cat` then
            -- ends with 0; the shell does not
            pty = sh("echo ready; cat; exit 3")
        end
        read_until(pty, "ready")
        return pty
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
        local status = check(pty:wait())
        assert(pty:process_id() == nil, "an ended process has no id")
        -- the end of the output, after whatever was still on its way
        read_all(pty)
        pty:close()
        return first, second, status.signal ~= nil, status.success
    "#)
    .await;
    assert!(first.contains("24 80"), "{first:?}");
    assert!(second.contains("40 100"), "{second:?}");
    assert!(signalled, "a killed process was ended by a signal");
    assert!(!success);
}

/// The same on a ConPTY: PowerShell says what size its console has.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_reads_writes_resizes_kills_waits_and_closes() {
    let (first, second, code, signalled, success): (String, String, u32, bool, bool) = run(r#"
        local size = "Write-Host ('size:' + [Console]::WindowHeight + 'x' + [Console]::WindowWidth)"
        local pty = ps(
            "Write-Host ready; $l = [Console]::ReadLine(); Write-Host ('got:' + $l); " .. size
                .. "; $l = [Console]::ReadLine(); " .. size .. "; [Console]::ReadLine()",
            { rows = 24, cols = 80 }
        )
        assert(type(pty:process_id()) == "number")
        read_until(pty, "ready")

        -- what is written is the process's input
        assert(pty:write("one\r") == true)
        local first = read_until(pty, "size:24x80")
        assert(string.find(first, "got:one", 1, true), first)

        assert(pty:resize(40, 100) == true)
        assert(pty:write("\r"))
        local second = read_until(pty, "size:40x100")

        -- it waits for a third line now: it would stay
        pty:kill()
        local status = check(pty:wait())
        assert(pty:process_id() == nil, "an ended process has no id")
        -- the end of the output, after whatever was still on its way: on Windows it ends with the process
        read_all(pty)
        -- and the pseudo-console went with it
        local resized, err = pty:resize(24, 80)
        assert(resized == nil and err.kind == "closed", tostring(err))
        pty:close()
        return first, second, status.exit_code, status.signal ~= nil, status.success
    "#)
    .await;
    assert!(first.contains("size:24x80"), "{first:?}");
    assert!(second.contains("size:40x100"), "{second:?}");
    assert_eq!(code, 1, "a terminated process ends with 1");
    assert!(!signalled, "Windows has no signals");
    assert!(!success);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_exit_status_is_the_commands_own_and_the_output_ends_with_it() {
    let (code, signal, success, output, again): (u32, Value, bool, String, u32) = run(r#"
        local pty = sh(if windows then "echo bye& exit 7" else "printf 'bye'; exit 7")
        local status = check(pty:wait())
        local output = read_all(pty)
        -- the answer stays, also after a close
        pty:close()
        return status.exit_code, status.signal, status.success, output, check(pty:wait()).exit_code
    "#)
    .await;
    assert_eq!((code, success, again), (7, false, 7));
    assert!(signal.is_nil(), "it ended by itself: {signal:?}");
    if cfg!(windows) {
        // what a pseudo-console draws: the text, with its own escape sequences around it
        assert!(output.contains("bye"), "{output:?}");
    } else {
        assert_eq!(output, "bye");
    }

    let (code, success): (u32, bool) = run(r#"
        local status = check(sh(if windows then "exit 0" else "true"):wait())
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
    let lua = make_engine().expect("engine");
    lua.globals().set("DIR", dir.path().to_str().expect("utf-8")).expect("global");
    let output: String = run_in(
        &lua,
        r#"
        local pty = spawn({
            program = "sh",
            args = { "-c", 'echo "$0|$1|$NEOC_PTY_TEST|$TERM|$(pwd -P)|${HOME:+home}|end"', "zero", "arg one" },
            cwd = DIR,
            env = { NEOC_PTY_TEST = "set here", TERM = "xterm-256color" },
        })
        return read_until(pty, "|end")
        "#,
    )
    .await;
    // a bare name is found in PATH, the environment is the process's own plus `env`
    let expected = format!("zero|arg one|set here|xterm-256color|{}|home|end", resolved.display());
    assert!(output.contains(&expected), "{output:?} lacks {expected:?}");
}

/// The same on Windows, where PowerShell says them. The terminal is wide, so the line is not wrapped.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_command_gets_its_arguments_directory_and_environment() {
    let dir = tempfile::tempdir().expect("dir");
    let lua = make_engine().expect("engine");
    lua.globals().set("DIR", dir.path().to_str().expect("utf-8")).expect("global");
    let output: String = run_in(
        &lua,
        r#"
        local pty = spawn({
            program = "powershell",
            args = {
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Write-Host",
                "('one argument|' + $env:NEOC_PTY_TEST + '|' + (Get-Location).Path + '|' + ($env:USERPROFILE -ne $null) + '|end')",
            },
            cwd = DIR,
            env = { NEOC_PTY_TEST = "set here" },
            cols = 1000,
        })
        return read_until(pty, "|end")
        "#,
    )
    .await;
    // a bare name is found in PATH, the environment is the process's own plus `env`
    let expected = format!("one argument|set here|{}|True|end", dir.path().display());
    assert!(output.contains(&expected), "{output:?} lacks {expected:?}");
}

/// The environment is the engine's, also where the registry says otherwise: every Windows has `OS=Windows_NT` among
/// the machine's variables, and the crate reads those over the process's own. (The default shell is `ComSpec` of
/// that same environment, and a bare `program` is looked up in its `PATH`.)
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_variable_of_the_engine_is_not_replaced_by_the_registrys() {
    // SAFETY: nothing else in this test's process reads or writes `OS` while it is set (nextest runs a test in a
    // process of its own).
    unsafe { std::env::set_var("OS", "neoc-pty-test") };
    let output: String = run(r#"
        local pty = spawn({ program = "cmd.exe", args = { "/D", "/C", "echo os=%OS%=end" } })
        return read_all(pty)
    "#)
    .await;
    assert!(output.contains("os=neoc-pty-test=end"), "{output:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_program_it_is_the_users_shell_in_their_home() {
    // the shell reads the user's own start-up files, so the test asks it for nothing but to leave
    let exited: bool = run(r#"
        local pty = check(portable_pty.spawn_command())
        assert(type(pty:process_id()) == "number")
        pty:kill()
        local status = check(pty:wait())
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
        return pid, check(pty:wait()).signal
    "#)
    .await;
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "the process was given no time to end by itself"
    );
    assert!(signal.to_lowercase().contains("kill"), "{signal:?}");
    assert!(!alive(pid));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_is_bounded_and_a_flood_waits_for_the_reader() {
    let (longest, total): (usize, usize) = run(r#"
        local pty = sh(if windows then "for /L %i in () do @echo 0123456789" else "yes 0123456789")
        -- left alone, the process fills what is kept for the script and then waits in its write (the bound itself
        -- is held by `byte_stream::tests`)
        task.wait(0.2)
        local longest, total = 0, 0
        for _ = 1, 200 do
            local output = check(pty:read(1000))
            assert(#output >= 1, "a read returns something or the end")
            longest = math.max(longest, #output)
            total += #output
        end
        local whole = check(pty:read())
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
    let (by_path, by_name, kind, op): (String, String, String, String) = run(r#"
        local pty, err = portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })
        assert(pty == nil)
        local named, name_err = portable_pty.spawn_command({ program = "neoc-no-such-program", args = { "x" } })
        assert(named == nil and name_err.kind == "spawn")
        assert(err.reason == err.message)
        return tostring(err), tostring(name_err), err.kind, err.op
    "#)
    .await;
    assert!(by_path.starts_with("vnd:portable_pty.spawn_command: spawn: "), "{by_path}");
    assert!(by_path.contains("neoc-no-such-program"), "{by_path}");
    assert!(by_name.starts_with("vnd:portable_pty.spawn_command: spawn: "), "{by_name}");
    assert_eq!((kind.as_str(), op.as_str()), ("spawn", "vnd:portable_pty.spawn_command"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_directory_that_is_not_there_is_a_spawn_error_not_the_home_directory() {
    let reason: String = run(r#"
        local pty, err = portable_pty.spawn_command({ cwd = "/nowhere/neoc-no-such-directory" })
        assert(pty == nil)
        assert(err.kind == "spawn")
        return err.reason
    "#)
    .await;
    assert!(reason.contains("/nowhere/neoc-no-such-directory"), "{reason}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn what_is_not_a_command_is_refused_before_anything_starts() {
    let (): () = run(r#"
        -- what the shape of the options does not say is an `invalid_input` error
        local function refused(opts, needle)
            local pty, err = portable_pty.spawn_command(opts)
            assert(pty == nil, "accepted")
            assert(err.kind == "invalid_input", tostring(err))
            assert(string.find(tostring(err), "vnd:portable_pty.spawn_command: invalid_input: ", 1, true) == 1, tostring(err))
            assert(string.find(err.reason, needle, 1, true), err.reason)
        end
        refused({ program = "" }, "\"program\"")
        refused({ args = { "-l" } }, "needs \"program\"")
        refused({ program = "sh", args = { "a\0b" } }, "\"args\"")
        refused({ program = "sh", args = table.create(1025, "") }, "more than")
        refused({ cwd = "" }, "\"cwd\"")
        refused({ program = "sh", env = { ["A=B"] = "1" } }, "\"env\"")
        refused({ program = "sh", rows = 0 }, "\"rows\"")
        refused({ program = "sh", cols = 40000 }, "\"cols\"")
        refused({ program = "a\0b" }, "NUL")
        local _, err = portable_pty.spawn_command({ rows = 0 })
        assert(err.message ~= "" and tostring(err) == "vnd:portable_pty.spawn_command: invalid_input: " .. err.message)

        -- what is not of the shape is raised, with its place
        local function raised(opts, needle)
            local ok, message = pcall(portable_pty.spawn_command, opts)
            assert(ok == false, "accepted")
            message = tostring(message)
            assert(string.find(message, "vnd:portable_pty.spawn_command: argument #1 'opts'", 1, true), message)
            assert(string.find(message, needle, 1, true), message)
        end
        raised({ progam = "sh" }, "progam")
        raised("sh", "expected a table")
        raised({ program = 7 }, "program")
        -- (a list that is no table, and a map with a number for a name, are refused without their place)
        raised({ program = "sh", args = "-c" }, "")
        raised({ program = "sh", args = { 1 } }, "args")
        raised({ program = "sh", env = { A = 1 } }, "env")
        raised({ program = "sh", env = { "A=1" } }, "")
        raised({ program = "sh", cols = 1.5 }, "cols")
        raised({ program = "sh", cols = "80" }, "cols")
        raised({ program = "sh", rows = -1 }, "rows")
        raised({ program = "sh", rows = 1e9 }, "rows")
        raised({ program = function() end }, "program")
        raised({ program = buffer.create(2) }, "program")
        local cycle = {}
        cycle.env = cycle
        raised(cycle, "env")
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// closing, collection and shutdown
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_terminal_ends_reads_refuses_writes_and_has_ended_its_process() {
    let pid: u32 = run(r#"
        local pty = stays()
        local pid = pty:process_id()

        -- a read that is waiting when the terminal is closed ends too, and so does a write
        -- (it reads on past what the process still had to say: a `cmd` draws its prompt)
        local waiting = task.spawn(function()
            while true do
                local output, err = pty:read()
                if output == nil then
                    return { err = err }
                end
            end
        end)
        task.wait(0.05)
        assert(not waiting:is_finished(), "the read waits while the terminal is open")
        pty:close()
        local ended = check(waiting:await())
        assert(ended.err == nil, "the waiting read: " .. tostring(ended.err))

        local again, again_err = pty:read()
        assert(again == nil and again_err == nil)
        local written, write_err = pty:write("x")
        assert(written == nil and write_err.kind == "closed", tostring(write_err))
        assert(tostring(write_err) == "vnd:portable_pty.Pty:write: closed: the terminal is closed", tostring(write_err))
        local empty, empty_err = pty:write("")
        assert(empty == nil and empty_err.kind == "closed")
        local resized, resize_err = pty:resize(10, 10)
        assert(resized == nil and resize_err.kind == "closed", tostring(resize_err))
        assert(resize_err.op == "vnd:portable_pty.Pty:resize")
        assert(pty:process_id() == nil)

        -- none of these does anything now, and none fails
        pty:kill()
        pty:close()
        local status = check(pty:wait())
        assert(status.success == false)
        return pid
    "#)
    .await;
    assert!(!alive(pid), "wait() returned, so the process was reaped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handle_that_is_collected_ends_its_process() {
    let lua = make_engine().expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        local pty = stays()
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_stops_with_the_script_that_opened_it() {
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = run_in(
        &lua,
        r#"
        -- kept in globals: the script still holds the terminal, and a task of it waits in a read
        held = stays()
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

    // what the script still holds is a closed terminal, and one opened after the shutdown is closed too
    let (closed, late): (String, u32) = run_in(
        &lua,
        r#"
        local _, err = held:write("x")
        local late = sh(if windows then "pause" else "echo ready; cat")
        local late_pid = late:process_id()
        local output, read_err = late:read()
        assert(output == nil and read_err == nil, "a terminal opened after the shutdown is closed")
        return err.kind, late_pid or 0
        "#,
    )
    .await;
    assert_eq!(closed, "closed");
    assert_eq!(late, 0, "a terminal opened after the shutdown has no process");
}

/// The reader of a silent process sits in a read the operating system does not interrupt. Neither the engine's
/// shutdown nor the runtime's own waits for it.
#[test]
fn a_reader_blocked_in_the_operating_system_holds_neither_the_engine_nor_the_runtime() {
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
    let life = Life::new();
    let lua = make_engine_in(life.clone()).expect("engine");
    let pid: u32 = runtime.block_on(run_in(
        &lua,
        r#"
        if windows then
            -- it says nothing more: the reader waits in its read until the pseudo-console is let go of
            held = stays()
        else
            -- it ignores the hangup and says nothing: the reader waits in its read until the process is killed
            held = sh('trap "" HUP; echo ready; exec sleep 1000')
            read_until(held, "ready")
        end
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
        assert!(started.elapsed() < TIMEOUT, "the process {pid} is still running");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---------------------------------------------------------------------------
// where a call cannot wait
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_that_cannot_wait_fails_and_does_nothing() {
    let output: String = run(r#"
        -- what it is sent comes back: `cat` writes it again (the terminal's own echo is off), `cmd` echoes it
        local pty
        if windows then
            pty = stays()
        else
            pty = sh("stty -echo; echo ready; cat")
            read_until(pty, "ready")
        end

        -- a callback of string.gsub and a table.sort comparator cannot yield: the write fails and types nothing
        assert(pcall(string.gsub, "abc", "b", function() pty:write("typed in gsub" .. enter) return "x" end) == false)
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) pty:write("typed in sort" .. enter) return a < b end) == false)
        assert(pcall(string.gsub, "abc", "b", function() pty:wait() return "x" end) == false)
        -- and no command is started
        assert(pcall(string.gsub, "abc", "b", function()
            portable_pty.spawn_command({ program = if windows then "cmd.exe" else "/bin/sh" })
            return "x"
        end) == false)
        -- the calls that do not wait are fine there
        assert(pcall(table.sort, { 1, 2, 3 }, function(a, b) pty:resize(30, 90) pty:process_id() return a < b end) == true)

        assert(pty:write("after" .. enter))
        local output = read_until(pty, "after")
        pty:close()
        return output
    "#)
    .await;
    assert!(!output.contains("typed"), "{output:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_call_panics_on_hostile_input() {
    let (): () = run(r#"
        local pty = stays()
        -- a size out of range is an error, and what is not a whole number of that width is raised
        for _, max in { 0, 65537, 2 ^ 32 - 1 } do
            local output, err = pty:read(max)
            assert(output == nil and err.kind == "invalid_input", tostring(max))
        end
        for _, max in { -1, 1.5, 0 / 0, math.huge, 2 ^ 32 } do
            assert(pcall(pty.read, pty, max) == false, tostring(max))
        end
        for _, size in { { 0, 80 }, { 24, 0 }, { 40000, 80 }, { 24, 65535 } } do
            local resized, err = pty:resize(size[1], size[2])
            assert(resized == nil and err.kind == "invalid_input")
            assert(string.find(err.reason, "a whole number from 1 to 32767", 1, true), err.reason)
        end
        for _, size in { { -1, -1 }, { 0 / 0, 80 }, { 24, math.huge }, { 1.5, 80 }, { 24, 65536 } } do
            assert(pcall(pty.resize, pty, size[1], size[2]) == false)
        end
        -- wrong types are raised, as everywhere
        assert(pcall(pty.resize, pty, "a", {}) == false)
        assert(pcall(pty.resize, pty, 24) == false)
        assert(pcall(pty.write, pty, {}) == false)
        assert(pcall(pty.write, pty) == false)
        assert(pcall(pty.read, {}) == false)
        -- a write past the limit is refused whole, and input need not be text
        local big, big_err = pty:write(string.rep("x", 1024 * 1024 + 1))
        assert(big == nil and big_err.kind == "invalid_input")
        assert(pty:write("\0\255\254 not text" .. enter))
        pty:close()
    "#)
    .await;
}

// ---------------------------------------------------------------------------
// what a pseudo-console does that a Unix terminal does not
// ---------------------------------------------------------------------------

/// The bytes pass as they are, in both directions: the pseudo-console's question is the script's to answer (or the
/// terminal's it passes them to), and nothing runs before it is answered.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pseudo_console_asks_where_the_cursor_is_and_runs_nothing_before_the_answer() {
    let (first, waited, code, rest): (String, bool, u32, String) = run(r#"
        local pty = check(portable_pty.spawn_command({ program = "cmd.exe", args = { "/D", "/C", "echo bye& exit 7" } }))
        local first = check(pty:read())
        local waiting = task.spawn(function()
            return pty:wait()
        end)
        task.wait(0.3)
        local waited = waiting:is_finished()
        check(pty:write(cursor_at_home))
        local status = check(pty:wait())
        return first, waited, status.exit_code, read_all(pty)
    "#)
    .await;
    assert_eq!(first, "\x1b[6n");
    assert!(!waited, "the command ran before the pseudo-console had its answer");
    assert_eq!(code, 7);
    assert!(rest.contains("bye"), "{rest:?}");
}

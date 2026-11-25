//! `vnd:sysinfo` against the machine it runs on, and against processes it starts itself: the tests make a child that
//! sleeps and signal that child, never any other process. They run on Linux, macOS and Windows (CI runs them on all
//! three), and where a platform differs (`terminate` on Windows) the test says what that platform must answer.

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use mlua::{FromLuaMulti, IntoLuaMulti, Lua};
use neoc_vm_core::make_engine;
use parking_lot::Mutex;

use super::native;
use super::sysinfo::{Clock, Error, MIN_CPU_INTERVAL_MS, Process, State, disk_kind, meet, rate};

/// A child that is stopped when the test ends, however it ends.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

type Children = Arc<Mutex<Vec<Guard>>>;

/// A program that does nothing for a minute.
fn sleeper() -> Command {
    #[cfg(unix)]
    {
        let mut command = Command::new("sleep");
        command.arg("60");
        command
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("ping");
        command.args(["-n", "60", "127.0.0.1"]);
        command
    }
}

/// A program that ignores the request to stop.
#[cfg(unix)]
fn stubborn() -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", "trap '' TERM; while true; do sleep 1; done"]);
    command
}

fn start(mut command: Command, children: &Children) -> u32 {
    let child =
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("a child");
    let pid = child.id();
    children.lock().push(Guard(child));
    pid
}

/// An engine with `launch()` (a sleeper, its PID), `launch_stubborn()` (unix), `stop_child()` (the first child ends)
/// and `pause(ms)`.
fn engine() -> (Lua, Children) {
    let lua = make_engine().expect("engine");
    let children: Children = Arc::new(Mutex::new(Vec::new()));
    let launched = children.clone();
    let launch = lua.create_function(move |_, ()| Ok(start(sleeper(), &launched))).expect("launch");
    lua.globals().set("launch", launch).expect("global");
    #[cfg(unix)]
    {
        let launched = children.clone();
        let launch = lua.create_function(move |_, ()| Ok(start(stubborn(), &launched))).expect("launch");
        lua.globals().set("launch_stubborn", launch).expect("global");
    }
    let stopping = children.clone();
    let stop = lua
        .create_function(move |_, ()| {
            let mut all = stopping.lock();
            let child = &mut all[0].0;
            let _ = child.kill();
            let _ = child.wait();
            Ok(())
        })
        .expect("stop_child");
    lua.globals().set("stop_child", stop).expect("global");
    let pause = lua
        .create_async_function(|_, ms: u64| async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            Ok(())
        })
        .expect("pause");
    lua.globals().set("pause", pause).expect("global");
    (lua, children)
}

async fn run_with<R: FromLuaMulti>(lua: &Lua, script: &str, args: impl IntoLuaMulti) -> R {
    lua.load(script).call_async(args).await.unwrap_or_else(|e| panic!("the script failed: {e}"))
}

async fn run<R: FromLuaMulti>(script: &str) -> R {
    let (lua, _children) = engine();
    run_with(&lua, script, ()).await
}

/// Wait for a child to be gone from the process list of its own parent.
fn wait_exit(children: &Children, index: usize) -> std::process::ExitStatus {
    children.lock()[index].0.wait().expect("the child ends")
}

/// A mismatch of the observed start time with the live one is what "this PID was reused" is.
fn start_time_of(pid: u32) -> u64 {
    native::live_start_time(pid).expect("the child is running")
}

// ---------------------------------------------------------------------------
// the pure parts
// ---------------------------------------------------------------------------

#[test]
fn a_rate_is_bytes_over_the_time_that_passed() {
    assert_eq!(rate(1_000, 3_000, Duration::from_secs(2)), Some(1_000.0));
    assert_eq!(rate(0, 500, Duration::from_millis(250)), Some(2_000.0));
    assert_eq!(rate(7, 7, Duration::from_secs(1)), Some(0.0));
}

#[test]
fn a_counter_that_went_back_or_a_moment_that_did_not_pass_has_no_rate() {
    assert_eq!(rate(5_000, 100, Duration::from_secs(1)), None, "the counter was reset");
    assert_eq!(rate(0, 100, Duration::ZERO), None, "no time passed");
}

#[test]
fn the_clock_refuses_a_measurement_that_comes_too_soon_and_says_when_to_come_back() {
    let mut clock = Clock::default();
    let start = std::time::Instant::now();
    assert!(clock.check(start).is_ok(), "the first measurement is never too soon");
    clock.tick(start);
    assert!(!clock.sampled(), "one measurement is not a difference");
    let Err(Error::TooSoon { retry_after_ms }) = clock.check(start + Duration::from_millis(50)) else {
        panic!("50 ms is too soon");
    };
    assert!(retry_after_ms >= MIN_CPU_INTERVAL_MS - 50, "{retry_after_ms}");
    assert!(retry_after_ms <= MIN_CPU_INTERVAL_MS - 50 + 2, "{retry_after_ms}");
    let later = start + Duration::from_millis(u64::from(MIN_CPU_INTERVAL_MS));
    assert!(clock.check(later).is_ok(), "the interval has passed");
    clock.tick(later);
    assert!(clock.sampled(), "two measurements are");
}

#[cfg(unix)]
#[test]
fn an_error_number_is_only_what_it_says_it_is() {
    use rustix::io::Errno;
    assert!(matches!(native::errno_error(Errno::SRCH), Error::ProcessGone));
    assert!(matches!(native::errno_error(Errno::PERM), Error::PermissionDenied { .. }));
    // not a permission failure, not guessed to be one
    assert!(matches!(native::errno_error(Errno::INVAL), Error::Failed { .. }));
    assert!(matches!(native::errno_error(Errno::NOMEM), Error::Failed { .. }));
}

#[test]
fn a_disk_kind_is_mapped_and_an_unknown_one_is_unknown() {
    use ::sysinfo::DiskKind as K;
    assert!(matches!(disk_kind(K::HDD), super::sysinfo::DiskKind::Hdd));
    assert!(matches!(disk_kind(K::SSD), super::sysinfo::DiskKind::Ssd));
    assert!(matches!(disk_kind(K::Unknown(-1)), super::sysinfo::DiskKind::Unknown));
}

// ---------------------------------------------------------------------------
// the machine
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memory_disks_and_the_host_are_read_from_the_first_measurement() {
    let () = run(r#"
        local system = assert(require("vnd:sysinfo").system())
        local memory = system:memory()
        assert(memory.total > 0 and memory.used <= memory.total and memory.available <= memory.total)
        local swap = system:swap()
        assert(swap.used <= swap.total and swap.free <= swap.total)
        for _, disk in system:disks() do
            assert(disk.mount_point ~= "" and disk.available_bytes <= disk.total_bytes)
            assert(disk.read_rate == nil and disk.write_rate == nil, "a rate needs two measurements")
        end
        local host = system:host()
        assert(host.arch ~= "" and host.uptime_secs > 0 and host.boot_time > 0)
        -- what a platform lacks is named
        local lacks = system:unsupported()
        assert(type(lacks) == "table")
        "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_reading_has_no_load_and_a_second_refresh_does() {
    let () = run(r#"
        local system = assert(require("vnd:sysinfo").system())
        local first = system:cpu()
        assert(first.sampled == false and first.usage == nil, "a first reading is not a zero")
        assert(first.logical_cores > 0 and #first.cores == first.logical_cores and first.arch ~= "")
        for _, core in first.cores do
            assert(core.usage == nil)
        end
        -- reading does not measure: the same answer, however often it is asked
        assert(system:cpu().usage == nil)

        pause(300)
        assert(system:refresh_processes())
        local second = system:cpu()
        assert(second.sampled == true)
        local usage = assert(second.usage, "a second refresh has a load")
        assert(usage >= 0 and usage <= 100)
        for _, core in second.cores do
            local load = assert(core.usage)
            assert(load >= 0 and load <= 100)
        end
        "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refresh_that_comes_too_soon_is_refused_and_a_cheap_one_is_not() {
    let () = run(r#"
        local sysinfo = require("vnd:sysinfo")
        local system = assert(sysinfo.system())
        local ok, err = system:refresh_processes()
        assert(ok == nil and err ~= nil and err.kind == "too_soon")
        assert(err.retry_after_ms > 0 and err.retry_after_ms <= sysinfo.MIN_CPU_INTERVAL_MS + 1)
        local all_ok, all_err = system:refresh()
        assert(all_ok == nil and all_err ~= nil and all_err.kind == "too_soon", "refresh() measures CPU too")
        -- memory, disks and networks have no CPU clock
        assert(system:refresh_memory())
        assert(system:refresh_disks())
        assert(system:refresh_networks())
        -- and what was refused measured nothing: it is still the first reading
        assert(system:cpu().sampled == false)
        pause(sysinfo.MIN_CPU_INTERVAL_MS + 100)
        assert(system:refresh())
        assert(system:cpu().sampled == true)
        "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_network_rate_is_not_there_until_there_are_two_measurements() {
    let () = run(r#"
        local system = assert(require("vnd:sysinfo").system())
        for _, interface in system:networks() do
            assert(interface.receive_rate == nil and interface.transmit_rate == nil, interface.name)
        end
        pause(150)
        assert(system:refresh_networks())
        assert(system:refresh_disks())
        for _, interface in system:networks() do
            assert(interface.receive_rate == nil or interface.receive_rate >= 0, interface.name)
            assert(interface.transmit_rate == nil or interface.transmit_rate >= 0, interface.name)
        end
        for _, disk in system:disks() do
            assert(disk.read_rate == nil or disk.read_rate >= 0, disk.mount_point)
            assert(disk.write_rate == nil or disk.write_rate >= 0, disk.mount_point)
        end
        "#)
    .await;
}

// ---------------------------------------------------------------------------
// processes
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_does_not_refresh_so_a_new_process_appears_at_the_next_refresh() {
    let (lua, _children) = engine();
    let () = run_with(
        &lua,
        r#"
        local system = assert(require("vnd:sysinfo").system())
        local pid = launch()
        assert(system:process(pid) == nil, "the system has not measured since it started")
        local listed = 0
        for _, process in system:processes() do
            assert(process.pid ~= pid)
            listed += 1
        end
        assert(listed == #system:processes(), "reading again is the same list")

        pause(300)
        assert(system:refresh_processes())
        local found = assert(system:process(pid), "the refresh found it")
        assert(found.pid == pid and found.start_time > 0)
        assert(found:alive())
        "#,
        (),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_is_read_from_its_handle_and_its_load_needs_two_sightings() {
    let (lua, _children) = engine();
    let me = std::process::id();
    let () = run_with(
        &lua,
        r#"
        local me = ...
        local system = assert(require("vnd:sysinfo").system())
        local pid = launch()
        pause(300)
        assert(system:refresh_processes())
        local child = assert(system:process(pid))
        assert(child.name ~= "" and tostring(child):find("process:" .. pid, 1, true))
        assert(child:memory() ~= nil and child:memory() > 0 and child:run_time() ~= nil)
        assert(child:status() ~= nil and child:command() ~= nil and #child:command() >= 1)
        assert(child:cpu_usage() == nil, "seen once: it has no load yet, which is not a zero")
        -- it runs as the user this process runs as (and a machine that cannot name a user says nil for both)
        assert(child:user() == assert(system:process(me)):user())

        -- the parent is this process, which lists the child as one of its own
        assert(child:parent_pid() == me)
        local parent = assert(system:process(me))
        local mine = false
        for _, c in parent:children() do
            if c.pid == pid then mine = true end
        end
        assert(mine, "the child is in the parent's children")
        local back = assert(child:parent())
        assert(back.pid == me)

        pause(300)
        assert(system:refresh_processes())
        -- the second sighting confirms the process, which is a new observation: the handle to the first reads nothing
        assert(child:cpu_usage() == nil)
        child = assert(system:process(pid))
        local load = assert(child:cpu_usage(), "seen twice: it has a load")
        assert(load >= 0)
        "#,
        me,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminate_asks_and_kill_stops_and_a_platform_without_the_request_says_so() {
    let (lua, children) = engine();
    let () = run_with(
        &lua,
        r#"
        local sysinfo = require("vnd:sysinfo")
        local system = assert(sysinfo.system())
        local asked = launch()
        local forced = launch()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local can = sysinfo.capabilities()
        assert(can.kill)

        local first = assert(system:process(asked))
        local ok, err = first:terminate()
        if can.terminate then
            assert(ok == true and err == nil)
        else
            -- Windows: no request to stop, and none is made up
            assert(ok == nil and err ~= nil and err.kind == "unsupported")
            local _, again = first:terminate()
            assert(again ~= nil and again.kind == "unsupported")
            assert(first:kill())
        end
        assert(assert(system:process(forced)):kill())
        "#,
        (),
    )
    .await;
    let asked = wait_exit(&children, 0);
    let forced = wait_exit(&children, 1);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(asked.signal(), Some(15), "terminate is SIGTERM: {asked:?}");
        assert_eq!(forced.signal(), Some(9), "kill is SIGKILL: {forced:?}");
    }
    #[cfg(windows)]
    {
        assert_eq!(asked.code(), Some(1), "{asked:?}");
        assert_eq!(forced.code(), Some(1), "{forced:?}");
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminate_is_never_turned_into_kill() {
    let (lua, children) = engine();
    let () = run_with(
        &lua,
        r#"
        local system = assert(require("vnd:sysinfo").system())
        local pid = launch_stubborn()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local stubborn = assert(system:process(pid))
        assert(stubborn:terminate())
        pause(700)
        assert(system:refresh_processes())
        assert(stubborn:alive(), "a process that ignores the request is still there: nothing killed it")
        assert(stubborn:kill())
        "#,
        (),
    )
    .await;
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(wait_exit(&children, 0).signal(), Some(9));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handle_to_a_process_that_has_exited_is_gone() {
    let (lua, children) = engine();
    let () = run_with(
        &lua,
        r#"
        local system = assert(require("vnd:sysinfo").system())
        local pid = launch()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local stale = assert(system:process(pid))
        assert(stale:alive())
        stop_child()
        local _, err = stale:kill()
        assert(err ~= nil and err.kind == "process_gone", "signalled a process that is not there")
        local _, pin_err = stale:pin()
        assert(pin_err ~= nil and pin_err.kind == "process_gone")
        pause(300)
        assert(system:refresh_processes())
        assert(not stale:alive())
        assert(stale:memory() == nil and stale:cpu_usage() == nil and stale:command() == nil, "a gone process reads as nil")
        assert(stale.name ~= "", "what it was called is kept")
        assert(system:process(pid) == nil)
        "#,
        (),
    )
    .await;
    drop(children);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_that_is_not_the_one_observed_is_not_signalled() {
    let (lua, children) = engine();
    let pid = start(sleeper(), &children);
    let real = start_time_of(pid);
    let state = Arc::new(Mutex::new(State::new()));
    // a handle that observed some other process with this PID: what a reused PID looks like
    let forged = Process::forged(&state, pid, real + 1_000, "other");
    lua.globals().set("forged", forged).expect("a handle");
    let () = run_with(
        &lua,
        r#"
        local _, killed = forged:kill()
        assert(killed ~= nil and killed.kind == "pid_reused", "kill must be refused")
        local _, asked = forged:terminate()
        assert(asked ~= nil and (asked.kind == "pid_reused" or asked.kind == "unsupported"))
        local _, pinned = forged:pin()
        assert(pinned ~= nil and pinned.kind == "pid_reused", "pin must be refused")
        assert(not forged:pinned())
        "#,
        (),
    )
    .await;
    assert!(children.lock()[0].0.try_wait().expect("status").is_none(), "the real process was left alone");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_process_is_held_until_it_is_released_and_a_signal_goes_through_the_hold() {
    let (lua, children) = engine();
    let () = run_with(
        &lua,
        r#"
        local sysinfo = require("vnd:sysinfo")
        local system = assert(sysinfo.system())
        local pid = launch()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local process = assert(system:process(pid))
        assert(not process:pinned() and process:binding() == "start_time")

        local binding = assert(process:pin())
        assert(binding == "pidfd" or binding == "process_handle" or binding == "start_time")
        assert(process:binding() == binding)
        assert(process:pinned() == (binding ~= "start_time"))
        assert(assert(process:pin()) == binding, "pinning twice is the same hold")

        process:release()
        assert(not process:pinned() and process:binding() == "start_time")
        assert(process:released())

        -- a released handle is finished: it neither signals nor takes a new reference
        local _, late = process:kill()
        assert(late ~= nil and late.kind == "released", "a late signal must not reacquire")
        local _, again = process:pin()
        assert(again ~= nil and again.kind == "released", "a released handle is not pinned again")
        assert(process:alive(), "the process itself was left alone")

        local fresh = assert(system:process(pid))
        assert(not fresh:released())
        assert(assert(fresh:pin()))
        assert(fresh:kill())
        "#,
        (),
    )
    .await;
    wait_exit(&children, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_script_does_not_signal_the_process_it_runs_in() {
    let (lua, _children) = engine();
    let me = std::process::id();
    let () = run_with(
        &lua,
        r#"
        local me = ...
        local system = assert(require("vnd:sysinfo").system())
        local itself = assert(system:process(me))
        local _, killed = itself:kill()
        assert(killed ~= nil and killed.kind == "own_process")
        local _, asked = itself:terminate()
        assert(asked ~= nil and asked.kind == "own_process")
        "#,
        me,
    )
    .await;
}

#[test]
fn the_platform_says_what_it_can_do() {
    let can = native::capability();
    assert_eq!(can.terminate, cfg!(not(windows)));
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_linux_a_pinned_process_is_held_by_a_pidfd() {
    let () = run(r#"
        local system = assert(require("vnd:sysinfo").system())
        local pid = launch()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local process = assert(system:process(pid))
        assert(assert(process:pin()) == "pidfd", "this kernel has pidfd_open: the hold is a pidfd")
        assert(process:pinned())
        assert(process:kill())
        "#)
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_whose_start_time_is_not_known_is_neither_pinned_nor_signalled() {
    let (lua, children) = engine();
    let pid = start(sleeper(), &children);
    let state = Arc::new(Mutex::new(State::new()));
    // a start time of 0 is what the platform gives for some system processes
    let unknown = Process::forged(&state, pid, 0, "system");
    lua.globals().set("unknown", unknown).expect("a handle");
    let () = run_with(
        &lua,
        r#"
        local _, pinned = unknown:pin()
        assert(pinned ~= nil and pinned.kind == "unknown_identity", "pin must be refused")
        assert(not unknown:pinned())
        local _, killed = unknown:kill()
        assert(killed ~= nil and killed.kind == "unknown_identity", "kill must be refused")
        local _, asked = unknown:terminate()
        assert(asked ~= nil and (asked.kind == "unknown_identity" or asked.kind == "unsupported"))
        "#,
        (),
    )
    .await;
    assert!(children.lock()[0].0.try_wait().expect("status").is_none(), "the real process was left alone");
}

#[test]
fn identity_is_the_start_time_and_the_finer_one_where_there_is_one() {
    assert!(native::same_identity(10, None, Some(10), None).is_ok());
    assert!(matches!(native::same_identity(10, None, None, None), Err(Error::ProcessGone)));
    assert!(matches!(native::same_identity(10, None, Some(11), None), Err(Error::PidReused)));
    // the same second, and the finer time says it is another process
    assert!(native::same_identity(10, Some(500), Some(10), Some(500)).is_ok());
    assert!(matches!(native::same_identity(10, Some(500), Some(10), Some(501)), Err(Error::PidReused)));
    // a finer time that was taken and cannot be read now: not the process that was observed
    assert!(matches!(native::same_identity(10, Some(500), Some(10), None), Err(Error::ProcessGone)));
}

#[cfg(target_os = "linux")]
#[test]
fn on_linux_the_finer_start_time_is_read_from_the_process_itself() {
    let me = std::process::id();
    let first = native::fine_start(me).expect("this process has a start in ticks");
    assert_eq!(native::fine_start(me), Some(first), "it does not change while the process lives");
    assert_eq!(native::fine_start(u32::MAX), None, "no such process");
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_linux_a_pid_reused_in_the_same_second_is_told_apart() {
    let (lua, children) = engine();
    let pid = start(sleeper(), &children);
    let real = start_time_of(pid);
    let fine = native::fine_start(pid).expect("a running child has one");
    let state = Arc::new(Mutex::new(State::new()));
    // observed in the same second as the process that has the PID now, but not at the same tick
    let lookalike = Process::forged_fine(&state, pid, real, fine + 1, "lookalike");
    lua.globals().set("lookalike", lookalike).expect("a handle");
    let () = run_with(
        &lua,
        r#"
        local _, pinned = lookalike:pin()
        assert(pinned ~= nil and pinned.kind == "pid_reused", "pin must be refused")
        local _, killed = lookalike:kill()
        assert(killed ~= nil and killed.kind == "pid_reused", "kill must be refused")
        "#,
        (),
    )
    .await;
    assert!(children.lock()[0].0.try_wait().expect("status").is_none(), "the real process was left alone");
}

#[test]
fn a_pid_that_turns_out_to_be_another_process_is_a_new_sighting() {
    let mut issued = 0;
    let first = meet(None, 100, Some(5_000), &mut issued);
    assert_eq!((first.generation, first.samples), (1, 1));
    // met again with the identity it had: where there is a finer identity this confirms the sighting, and a
    // confirmed sighting has a number of its own (a reference to the ambiguous one never names the confirmed process)
    let again = meet(Some(&first), 100, Some(5_000), &mut issued);
    if native::FINE_START {
        assert!(!first.confirmed && again.confirmed);
        assert_eq!((again.generation, again.samples), (2, 2));
    } else {
        assert!(first.confirmed && again.confirmed, "nothing to confirm without a finer identity");
        assert_eq!((again.generation, again.samples), (1, 2));
    }
    // a confirmed process met once more is the same sighting
    let steady = meet(Some(&again), 100, Some(5_000), &mut issued);
    assert_eq!((steady.generation, steady.samples), (again.generation, again.samples + 1));
    // the same second, another finer time: another process has the PID, and the reference to the first is not it
    let same_second = meet(Some(&steady), 100, Some(5_001), &mut issued);
    assert!(same_second.generation > steady.generation && same_second.samples == 1);
    // another start time
    let later = meet(Some(&same_second), 101, Some(5_001), &mut issued);
    assert!(later.generation > same_second.generation && later.samples == 1);
    // a finer time that cannot be read now is not the one that was: it is not told apart, so it is not the same
    let unreadable = meet(Some(&later), 101, None, &mut issued);
    assert!(unreadable.generation > later.generation && unreadable.samples == 1);
    if native::FINE_START {
        assert!(!same_second.confirmed && !later.confirmed && !unreadable.confirmed);
    }
    // a process met for the first time on a platform with no finer time is one sighting, however often it is met
    if !native::FINE_START {
        let coarse = meet(None, 7, None, &mut issued);
        assert_eq!(meet(Some(&coarse), 7, None, &mut issued).generation, coarse.generation);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_keeps_its_observation_across_refreshes_and_each_process_has_its_own() {
    let () = run(r#"
        local system = assert(require("vnd:sysinfo").system())
        local first = launch()
        local second = launch()
        pause(300)
        assert(system:refresh_processes())
        -- a first sighting is read-only until a second refresh confirms it
        pause(300)
        assert(system:refresh_processes())
        local a = assert(system:process(first))
        local b = assert(system:process(second))
        assert(a.observation > 0 and b.observation > 0 and a.observation ~= b.observation, "each process has its own sighting")
        pause(300)
        assert(system:refresh_processes())
        local again = assert(system:process(first))
        assert(again.observation == a.observation, "the same process, met again, is the same sighting")
        assert(a:alive(), "a handle from before the refresh still reads it")
        "#)
    .await;
}

#[cfg(any(target_os = "linux", windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_the_platform_has_a_finer_start_time_a_process_without_one_is_not_signalled() {
    let (lua, children) = engine();
    let pid = start(sleeper(), &children);
    let real = start_time_of(pid);
    let state = Arc::new(Mutex::new(State::new()));
    // observed with the right second, and with the finer time not read: it cannot be told from a replacement
    let unread = Process::forged_without_fine(&state, pid, real, "unread");
    lua.globals().set("unread", unread).expect("a handle");
    let () = run_with(
        &lua,
        r#"
        local _, pinned = unread:pin()
        assert(pinned ~= nil and pinned.kind == "unknown_identity", "pin must be refused")
        local _, killed = unread:kill()
        assert(killed ~= nil and killed.kind == "unknown_identity", "kill must be refused")
        "#,
        (),
    )
    .await;
    assert!(children.lock()[0].0.try_wait().expect("status").is_none(), "the real process was left alone");
}

/// A process replaced by another with its PID between the sample of it and the read of its identity (the finer time
/// read is another than the one recorded) is not confirmed: it can be listed but nothing is sent to it.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_replaced_between_the_sample_and_the_identity_read_is_not_acted_on() {
    let (lua, children) = engine();
    let pid = start(sleeper(), &children);
    let mut state = State::new();
    state.system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    let real = |p: u32| native::fine_start(p);
    state.record(real);
    state.record(real);
    let key = &state.seen[&pid];
    assert!(key.confirmed, "met twice with one identity");
    let before = key.generation;
    // the sample described the sleeper; by the time the identity was read the PID had another process (a later tick)
    state.record(|p| real(p).map(|fine| fine + 1));
    let after = state.seen[&pid];
    assert!(after.generation > before && !after.confirmed, "a mixed sample is a new sighting, unconfirmed");
    // a row or selection from the ambiguous sample
    let state = Arc::new(Mutex::new(state));
    let observe = |state: &Arc<Mutex<State>>| {
        let locked = state.lock();
        let si_process = locked.system.process(sysinfo::Pid::from_u32(pid)).expect("sampled");
        Process::observed(state, &locked, si_process)
    };
    let old = observe(&state);
    // the next sample describes the process that has the PID now and its identity matches the one recorded: that
    // confirms it, with a generation of its own
    let ambiguous = state.lock().seen[&pid].generation;
    state.lock().record(|p| real(p).map(|fine| fine + 1));
    let (confirmed, start) = {
        let locked = state.lock();
        (locked.seen[&pid], locked.seen[&pid].start_time)
    };
    assert!(confirmed.confirmed && confirmed.generation > ambiguous, "confirmation issues a new observation");
    {
        let locked = state.lock();
        assert!(locked.find(pid, start, ambiguous).is_none(), "the ambiguous observation names nothing now");
        assert!(
            locked.find(pid, start, confirmed.generation).is_some(),
            "the confirmed one names the process"
        );
    }
    let fresh = observe(&state);
    lua.globals().set("old", old).expect("a handle");
    lua.globals().set("fresh", fresh).expect("a handle");
    let () = run_with(
        &lua,
        r#"
        assert(old.name ~= "")
        local _, killed = old:kill()
        assert(killed ~= nil and killed.kind == "unknown_identity", "kill must be refused")
        local _, pinned = old:pin()
        assert(pinned ~= nil and pinned.kind == "unknown_identity", "pin must be refused")
        assert(not old:alive(), "the ambiguous observation reads as nothing: it is not the confirmed process")
        assert(fresh:alive() and fresh.observation ~= old.observation, "the confirmed one is a new observation")
        "#,
        (),
    )
    .await;
    assert!(children.lock()[0].0.try_wait().expect("status").is_none(), "the process was left alone");
}

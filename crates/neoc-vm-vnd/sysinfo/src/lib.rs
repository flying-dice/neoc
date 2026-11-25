//! What the machine is doing, and the processes on it: `vnd:sysinfo` (the `sysinfo` crate, with the platform's own
//! process identity and signals).
//!
//! ```lua
//! local sysinfo = require("vnd:sysinfo")
//!
//! local system = assert(sysinfo.system())
//! -- a first reading has no CPU load yet: it is measured between two refreshes
//! print(system:cpu().usage)                --> nil
//! -- (wait at least `sysinfo.MIN_CPU_INTERVAL_MS`)
//! assert(system:refresh_processes())
//! print(system:cpu().usage)                --> 7.5
//!
//! local me = assert(system:process(1234))
//! print(me.name, me:cpu_usage(), me:memory())
//! assert(me:terminate())                   -- asks; `me:kill()` is the forced stop
//! ```
//!
//! **A `System` is a reference to what was last measured, and reading is not measuring.** `refresh_*` measures;
//! `cpu`, `memory`, `swap`, `disks`, `networks`, `host`, `process` and `processes` only read what the last refresh
//! left, and never refresh, however many times they are called. A script (or an app) owns its own clock and says when
//! to measure.
//!
//! **Sampling.** A load is a difference between two moments, so the first reading is not a zero: it is `nil` until a
//! second refresh makes a difference, per process (a process seen for the first time has no load yet, whichever
//! refresh found it), per core, per disk and per network interface. The CPU load of the machine and of each process
//! are one measurement over one interval, so `refresh_processes` measures both, and a refresh that comes before
//! `sysinfo.MIN_CPU_INTERVAL_MS` has passed since the last is refused as `too_soon` (with `retry_after_ms`) instead
//! of answering a reading over a few milliseconds. A network or disk rate is the bytes counted since the last
//! refresh over the time that really passed.
//!
//! **A process is a reference too.** `system:process(pid)` and `system:processes()` give `Process` handles that
//! name the process observed: its PID *and* its start time. A getter reads that process from the last refresh, or is
//! `nil` when it is gone (or the PID belongs to another process now), and a script builds whatever snapshot it needs
//! from them. `terminate()` and `kill()` act on the observed process and on no other, and the module does that
//! check, not the caller: see "Identity".
//!
//! **Identity.** A PID names a process only while it lives. A signal sent to a PID after a check is two steps with a
//! gap. So where the platform has one, the check is bound to a native reference: a `pidfd` on Linux, a process
//! handle on Windows, acquired and checked against the start time observed, after which a signal can only reach that
//! process. `process:pin()` holds that reference until `process:release()` (or the handle is dropped), so a
//! selection kept by a person is bound at the moment they made it; an unpinned process is bound for the one call. A
//! platform that has no such reference (macOS, or a Linux kernel older than 5.3) checks the start time again
//! immediately before the signal, and the gap that is left there is real: `process:binding()` says which applies
//! (`pidfd`, `process_handle` or `start_time`). The start time is compared in whole seconds, as `sysinfo` reads it;
//! where the platform gives a finer one (Linux: the start in clock ticks; Windows: the creation time) it is taken when
//! the process is *first observed* and compared again when the handle is acquired, so a PID reused in the same second
//! is told apart there, and not on macOS (the residual). The finer time is read at every refresh: a PID whose finer
//! time changed is another process, and `process.observation` (a number issued at each first sighting) says which
//! sighting a handle is, so a reference kept outside the module (a row a window drew) can be checked against the
//! process it is resolved to. A process whose start time is not known (0), or whose finer time was not read where the
//! platform gives one, has nothing to bind a signal to: `pin`, `terminate` and `kill` refuse it (`unknown_identity`). So does
//! a process seen for the first time (or as another one): the sample of it and the read of its finer time are two reads, and
//! a process may have been replaced between them, so a handle is acted on only once a later refresh has found the finer
//! time it recorded, and that refresh gives the process a new `observation`: a handle or row from the ambiguous sample
//! never becomes the confirmed process (take the new one). It can be read meanwhile. `process:release()` finishes the
//! handle (`process:released()`): it signals nothing afterwards and is not pinned again, so a late signal cannot take a
//! new reference. Nothing in this module signals a PID after an identity or permission failure.
//!
//! **Signals.** `terminate()` asks the process to stop (`SIGTERM`) and is never turned into `kill()`. Windows has no
//! such request for a process in general, so there it is `unsupported`, and `capabilities().terminate` is `false`.
//! `kill()` is `SIGKILL` and `TerminateProcess`. `permission_denied` is what the OS said it was (`EPERM`, or
//! `ERROR_ACCESS_DENIED` of a process that is still running); any other failure is `failed` with the OS's own words.
//! A script cannot signal the process it runs in (`own_process`).
//!
//! **What a platform does not report** is `nil`, and `system:unsupported()` lists what a whole kind of reading is
//! missing on this platform (Windows has no load average).
//!
//! Waiting: a refresh and a signal run on the blocking pool and the script waits for them.
//!
//! The API's rules:
//! - A failure is the typed `Error` (`too_soon`, `process_gone`, `pid_reused`, `own_process`, `permission_denied`,
//!   `unknown_identity`, `released`, `unsupported`, `failed`): a script reads `err.kind`, and for `too_soon` how long to wait.
//! - A reading that is not there (not sampled yet, not reported by the platform, a process that is gone) is `nil`,
//!   never `0` and never an error.

// What mlua-ir's macros imply, for the module below: a script's value is read before the call, so a function
// takes it owned (`needless_pass_by_value`); a module is named after what it exposes (`module_inception`); and the
// macro creates async functions itself, which the `host` runs as lifecycle leaves (`disallowed_methods`).
#![allow(
    clippy::needless_pass_by_value,
    clippy::module_inception,
    clippy::disallowed_methods,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

mod native;

#[mlua_ir::module("vnd:sysinfo")]
pub mod sysinfo {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use ::sysinfo as si;
    use mlua_ir::{LuaError, Object, Shape};
    use parking_lot::Mutex;
    use serde::Serialize;

    use crate::native::{self, Sig};

    /// The shortest time between two measurements of the CPU load, in milliseconds: a refresh sooner than this is
    /// `too_soon`. It is `sysinfo`'s own minimum (a load over less is noise).
    pub const MIN_CPU_INTERVAL_MS: u32 = 200;
    const MIN_CPU_INTERVAL: Duration = Duration::from_millis(MIN_CPU_INTERVAL_MS as u64);

    /// Why a call failed.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Error {
        /// The CPU load was measured less than `MIN_CPU_INTERVAL_MS` ago: refresh again after `retry_after_ms`.
        #[error("the CPU load was measured too recently: try again in {retry_after_ms} ms")]
        TooSoon {
            /// How long to wait, in milliseconds.
            retry_after_ms: u32,
        },
        /// The process has exited.
        #[error("the process has exited")]
        ProcessGone,
        /// The PID belongs to another process now: the one observed has exited and its PID was reused.
        #[error("the PID now belongs to another process")]
        PidReused,
        /// The process is the one this script runs in: it is not signalled from inside it.
        #[error("the process is the one this script runs in")]
        OwnProcess,
        /// The operating system refused the signal, and said so.
        #[error("permission denied: {reason}")]
        PermissionDenied {
            /// What the system said.
            reason: String,
        },
        /// The process's start time is not known (0: some system processes on macOS and Windows), so nothing tells it
        /// apart from another that is given its PID: no signal is sent to it, and it is not pinned.
        #[error("the process's start time is not known, so it cannot be told apart from another process")]
        UnknownIdentity,
        /// The native reference to the process was released: this handle signals nothing any more. Take a handle
        /// from the system again.
        #[error("the reference to the process was released")]
        Released,
        /// The platform does not do this (`terminate` on Windows).
        #[error("not supported: {reason}")]
        Unsupported {
            /// What is missing.
            reason: String,
        },
        /// The platform failed in a way it does not say more about, or in a way that is none of the above.
        #[error("{reason}")]
        Failed {
            /// What the system said, if it said.
            reason: String,
        },
    }

    /// What holds a process's identity when it is signalled.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Binding {
        /// A `pidfd` (Linux): it names one process for good.
        Pidfd,
        /// A process handle (Windows): it keeps the PID reserved for its process.
        ProcessHandle,
        /// The start time observed, checked again just before the signal: there is a gap between the check and the
        /// signal, in which a reused PID would be signalled.
        StartTime,
    }

    /// What this platform can do with a process.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Capabilities {
        /// `terminate()` (the request to stop) exists. It does not on Windows.
        pub terminate: bool,
        /// `kill()` (the forced stop) exists.
        pub kill: bool,
        /// How a signal is bound to the process that was observed, where the platform can (a kernel older than 5.3
        /// has no `pidfd`: `process:binding()` says what a particular handle got).
        pub binding: Binding,
    }

    /// How a process is doing.
    #[derive(Debug, Clone, Copy, Serialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Status {
        /// Being created (macOS, FreeBSD), or an idle kernel thread (Linux).
        Idle,
        /// Running.
        Run,
        /// Sleeping, until something wakes it.
        Sleep,
        /// Stopped, by a signal or a debugger.
        Stop,
        /// Exited, and not yet reaped by its parent.
        Zombie,
        /// Being traced.
        Tracing,
        /// Dead.
        Dead,
        /// Wakekill (Linux).
        Wakekill,
        /// Waking (Linux).
        Waking,
        /// Parked (Linux).
        Parked,
        /// Blocked on a lock (BSD).
        LockBlocked,
        /// Waiting on a disk, and not interruptible.
        UninterruptibleDiskSleep,
        /// Suspended (Windows).
        Suspended,
        /// The platform's state is none of these.
        Unknown,
    }

    /// What kind of disk.
    #[derive(Debug, Clone, Copy, Serialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum DiskKind {
        /// A hard disk.
        Hdd,
        /// A solid-state disk.
        Ssd,
        /// The platform does not say.
        Unknown,
    }

    /// The load average of the last 1, 5 and 15 minutes (not on Windows).
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct LoadAverage {
        /// The last minute.
        pub one: f64,
        /// The last five minutes.
        pub five: f64,
        /// The last fifteen minutes.
        pub fifteen: f64,
    }

    /// One logical core.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Core {
        /// The platform's name for it (`cpu0`, `CPU 0`).
        pub name: String,
        /// How busy it was between the last two refreshes, in percent of the core (0 to 100); `nil` until there
        /// are two.
        pub usage: Option<f64>,
        /// Its clock, in MHz, when the platform says.
        pub frequency_mhz: Option<u64>,
    }

    /// The processor, as last measured.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct CpuStatus {
        /// How busy the whole processor was between the last two refreshes, in percent (0 to 100); `nil` until there
        /// are two.
        pub usage: Option<f64>,
        /// `usage` is a measurement: there have been two refreshes.
        pub sampled: bool,
        /// Every logical core.
        pub cores: Vec<Core>,
        /// The number of logical cores.
        pub logical_cores: u32,
        /// The number of physical cores, when the platform says.
        pub physical_cores: Option<u32>,
        /// The processor's name, when the platform says.
        pub brand: Option<String>,
        /// The processor's architecture (`x86_64`, `arm64`).
        pub arch: String,
        /// The load average, where the platform has one.
        pub load_average: Option<LoadAverage>,
    }

    /// Memory, in bytes.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Memory {
        /// All of it.
        pub total: u64,
        /// What programs use.
        pub used: u64,
        /// What a program could still get (free, and what the system can take back).
        pub available: u64,
        /// What nothing holds at all.
        pub free: u64,
    }

    /// Swap, in bytes. A machine with none is all zeros.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Swap {
        /// All of it.
        pub total: u64,
        /// What is in use.
        pub used: u64,
        /// What is not.
        pub free: u64,
    }

    /// A mounted disk.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct DiskInfo {
        /// Its name.
        pub name: String,
        /// Where it is mounted.
        pub mount_point: String,
        /// Its file system (`ext4`, `NTFS`, `apfs`).
        pub file_system: String,
        /// What kind of disk.
        pub kind: DiskKind,
        /// Its size, in bytes.
        pub total_bytes: u64,
        /// What is free, in bytes.
        pub available_bytes: u64,
        /// It can be taken out.
        pub removable: bool,
        /// It is mounted read-only.
        pub read_only: bool,
        /// Bytes per second read from it between the last two refreshes, over the time that really passed; `nil`
        /// until there are two, and where the platform does not count them.
        pub read_rate: Option<f64>,
        /// Bytes per second written to it, the same way.
        pub write_rate: Option<f64>,
    }

    /// A network interface.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct NetworkInterface {
        /// Its name.
        pub name: String,
        /// Its hardware address, when it has one.
        pub mac_address: Option<String>,
        /// Bytes received since the machine started (or the counter was reset).
        pub received_bytes: u64,
        /// Bytes sent since the machine started (or the counter was reset).
        pub transmitted_bytes: u64,
        /// Bytes per second received between the last two refreshes, over the time that really passed between them;
        /// `nil` until there are two, and when the counter went back.
        pub receive_rate: Option<f64>,
        /// Bytes per second sent, the same way.
        pub transmit_rate: Option<f64>,
    }

    /// The machine.
    #[derive(Debug, Clone, Serialize, Shape)]
    pub struct Host {
        /// Its name on the network.
        pub name: Option<String>,
        /// The operating system (`Ubuntu`, `Windows`, `Darwin`).
        pub os_name: Option<String>,
        /// Its version.
        pub os_version: Option<String>,
        /// The kernel's version.
        pub kernel_version: Option<String>,
        /// The architecture.
        pub arch: String,
        /// Seconds since it started.
        pub uptime_secs: u64,
        /// When it started, in seconds since the Unix epoch.
        pub boot_time: u64,
    }

    // -----------------------------------------------------------------------
    // the measured state
    // -----------------------------------------------------------------------

    /// The shared clock of the CPU measurement: how long ago, and how many.
    #[derive(Default)]
    pub(super) struct Clock {
        last: Option<Instant>,
        ticks: u32,
    }

    impl Clock {
        /// The refusal for a measurement that comes too soon after the last.
        pub(super) fn check(&self, now: Instant) -> Result<(), Error> {
            match self.last {
                Some(last) if now.saturating_duration_since(last) < MIN_CPU_INTERVAL => {
                    let wait = MIN_CPU_INTERVAL - now.saturating_duration_since(last);
                    // rounded up: waiting this long is enough
                    let retry_after_ms = u32::try_from(wait.as_millis() + 1).unwrap_or(MIN_CPU_INTERVAL_MS);
                    Err(Error::TooSoon { retry_after_ms })
                }
                _ => Ok(()),
            }
        }

        pub(super) fn tick(&mut self, now: Instant) {
            self.last = Some(now);
            self.ticks = self.ticks.saturating_add(1);
        }

        /// Two measurements make a difference.
        pub(super) fn sampled(&self) -> bool {
            self.ticks >= 2
        }
    }

    /// A process, as the refreshes have met it: when it started, and in how many refreshes in a row.
    #[derive(Clone, Copy)]
    pub(super) struct Seen {
        pub(super) start_time: u64,
        /// The finer start time the platform gave when the process was first observed (see `native`).
        pub(super) fine: Option<u64>,
        /// Which sighting this is: a number issued when the process is first met, and again when the PID turns out to
        /// be another process (a different start time, or the same second and a different finer one). A reference to
        /// the process carries it, so that a reference to an earlier process with this PID is not this one.
        pub(super) generation: u64,
        pub(super) samples: u32,
        /// Whether the sample that described this process is known to have described one process: a process met
        /// before, with the same identity read before and after the sample (the earlier read brackets it on one side,
        /// this one on the other). A process met for the first time, or as another one, has no earlier read to compare
        /// with, so it can be inspected but not acted on. The refresh that confirms it issues a new `generation`: a
        /// reference from the ambiguous sample never names the confirmed process. Where the platform gives
        /// no finer identity (see `native::FINE_START`) there is nothing to confirm and it is always true.
        pub(super) confirmed: bool,
    }

    /// What a refresh makes of a process it has met before (or not): the same one, seen once more, or a new one.
    pub(super) fn meet(before: Option<&Seen>, start_time: u64, fine: Option<u64>, issued: &mut u64) -> Seen {
        match before {
            // confirmed by this sample: a reference to the sighting before it (a row drawn from the sample that was
            // ambiguous) is not this one, so it has a number of its own and the process is chosen again to be acted on
            Some(seen) if seen.start_time == start_time && seen.fine == fine && !seen.confirmed => {
                *issued += 1;
                Seen {
                    samples: seen.samples.saturating_add(1),
                    generation: *issued,
                    confirmed: true,
                    ..*seen
                }
            }
            Some(seen) if seen.start_time == start_time && seen.fine == fine => {
                Seen { samples: seen.samples.saturating_add(1), ..*seen }
            }
            // another process has the PID: it starts again, with no load, and is not the one a reference named
            _ => {
                *issued += 1;
                Seen { start_time, fine, generation: *issued, samples: 1, confirmed: !native::FINE_START }
            }
        }
    }

    /// A network interface's counters at one refresh.
    struct Meter {
        received: u64,
        transmitted: u64,
        at: Instant,
        rates: Option<(f64, f64)>,
    }

    /// Bytes per second between two counter values over `elapsed`, `None` when the counter went back (it was reset)
    /// or no time passed.
    pub(super) fn rate(before: u64, now: u64, elapsed: Duration) -> Option<f64> {
        let seconds = elapsed.as_secs_f64();
        if now < before || seconds <= 0.0 {
            return None;
        }
        // counters are far below 2^53: the conversion loses nothing
        #[allow(clippy::cast_precision_loss)]
        Some((now - before) as f64 / seconds)
    }

    pub(super) struct State {
        pub(super) system: si::System,
        disks: si::Disks,
        disk_meters: HashMap<String, Meter>,
        networks: si::Networks,
        users: si::Users,
        load: Clock,
        pub(super) seen: HashMap<u32, Seen>,
        /// The last generation issued (see `Seen`).
        issued: u64,
        meters: HashMap<String, Meter>,
    }

    fn process_kind() -> si::ProcessRefreshKind {
        si::ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_cmd(si::UpdateKind::OnlyIfNotSet)
            .with_exe(si::UpdateKind::OnlyIfNotSet)
            .with_user(si::UpdateKind::OnlyIfNotSet)
    }

    impl State {
        /// A `System`, measured once: what a script then reads is real, and what needs two measurements is not there
        /// yet.
        pub(super) fn new() -> Self {
            let mut state = Self {
                system: si::System::new(),
                disks: si::Disks::new_with_refreshed_list(),
                disk_meters: HashMap::new(),
                networks: si::Networks::new_with_refreshed_list(),
                users: si::Users::new_with_refreshed_list(),
                load: Clock::default(),
                seen: HashMap::new(),
                issued: 0,
                meters: HashMap::new(),
            };
            state.system.refresh_memory();
            state.refresh_load_unchecked();
            state.measure_networks(Instant::now());
            state.measure_disks(Instant::now());
            state
        }

        /// The CPU load and every process: one measurement over one interval.
        ///
        /// The interval is counted from when the measurement ended, as `sysinfo` counts it (its own minimum is from the
        /// last refresh of the CPU): on a platform where reading every process takes a while, counting from when it
        /// began would let the next measurement through at once.
        fn refresh_load_unchecked(&mut self) {
            self.system
                .refresh_cpu_specifics(si::CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
            self.system.refresh_processes_specifics(si::ProcessesToUpdate::All, true, process_kind());
            self.load.tick(Instant::now());
            self.record(native::fine_start);
        }

        /// What the last sample of the processes is taken to be. `fine` reads a process's finer identity, after the
        /// sample: a process whose identity is the one recorded at the refresh before has the same one at both ends
        /// of this sample, and is confirmed; a process met for the first time, or whose identity differs (it was
        /// replaced between the sample and the read, or since the refresh before), is a new sighting that cannot be
        /// acted on until a refresh confirms it.
        pub(super) fn record(&mut self, fine: impl Fn(u32) -> Option<u64>) {
            let processes = self.system.processes();
            self.seen.retain(|pid, _| processes.contains_key(&si::Pid::from_u32(*pid)));
            for (pid, process) in processes {
                let pid = pid.as_u32();
                let seen = meet(self.seen.get(&pid), process.start_time(), fine(pid), &mut self.issued);
                self.seen.insert(pid, seen);
            }
        }

        fn refresh_load(&mut self) -> Result<(), Error> {
            let now = Instant::now();
            self.load.check(now)?;
            self.refresh_load_unchecked();
            Ok(())
        }

        fn measure_networks(&mut self, now: Instant) {
            self.networks.refresh(true);
            let mut meters = HashMap::with_capacity(self.networks.len());
            for (name, data) in &self.networks {
                let (received, transmitted) = (data.total_received(), data.total_transmitted());
                let rates = self.meters.get(name).and_then(|before| {
                    let elapsed = now.saturating_duration_since(before.at);
                    Some((
                        rate(before.received, received, elapsed)?,
                        rate(before.transmitted, transmitted, elapsed)?,
                    ))
                });
                meters.insert(name.clone(), Meter { received, transmitted, at: now, rates });
            }
            self.meters = meters;
        }

        /// The disks' counters, and what they are per second since the last measurement. Disks are keyed by where
        /// they are mounted.
        fn measure_disks(&mut self, now: Instant) {
            self.disks.refresh(true);
            let mut meters = HashMap::with_capacity(self.disks.list().len());
            for disk in self.disks.list() {
                let key = disk.mount_point().to_string_lossy().into_owned();
                let usage = disk.usage();
                let (read, written) = (usage.total_read_bytes, usage.total_written_bytes);
                let rates = self.disk_meters.get(&key).and_then(|before| {
                    let elapsed = now.saturating_duration_since(before.at);
                    Some((rate(before.received, read, elapsed)?, rate(before.transmitted, written, elapsed)?))
                });
                meters.insert(key, Meter { received: read, transmitted: written, at: now, rates });
            }
            self.disk_meters = meters;
        }

        /// A live process by its identity: the one that was observed, not another that has its PID.
        pub(super) fn find(&self, pid: u32, start_time: u64, generation: u64) -> Option<&si::Process> {
            self.system
                .process(si::Pid::from_u32(pid))
                .filter(|process| process.start_time() == start_time)
                .filter(|_| self.seen.get(&pid).is_some_and(|seen| seen.generation == generation))
        }
    }

    /// Runs `job` on the blocking pool and waits for it: a refresh reads the platform's files and calls.
    async fn blocking<F, R>(job: F) -> Result<R, Error>
    where
        F: FnOnce() -> Result<R, Error> + Send + 'static,
        R: Send + 'static,
    {
        // Do nothing on the first poll. mlua polls a host call once before it tries to yield, and where Luau
        // refuses the yield (a `gsub` callback, a metamethod) it drops the call after that poll: a job started now
        // would signal a process although the script was told the call failed (`std:fs` has the same rule).
        tokio::task::yield_now().await;
        tokio::task::spawn_blocking(job)
            .await
            .unwrap_or_else(|e| Err(Error::Failed { reason: format!("the job failed: {e}") }))
    }

    fn whole(percent: f32) -> f64 {
        f64::from(percent.clamp(0.0, 100.0))
    }

    fn status_of(status: si::ProcessStatus) -> Status {
        match status {
            si::ProcessStatus::Idle => Status::Idle,
            si::ProcessStatus::Run => Status::Run,
            si::ProcessStatus::Sleep => Status::Sleep,
            si::ProcessStatus::Stop => Status::Stop,
            si::ProcessStatus::Zombie => Status::Zombie,
            si::ProcessStatus::Tracing => Status::Tracing,
            si::ProcessStatus::Dead => Status::Dead,
            si::ProcessStatus::Wakekill => Status::Wakekill,
            si::ProcessStatus::Waking => Status::Waking,
            si::ProcessStatus::Parked => Status::Parked,
            si::ProcessStatus::LockBlocked => Status::LockBlocked,
            si::ProcessStatus::UninterruptibleDiskSleep => Status::UninterruptibleDiskSleep,
            si::ProcessStatus::Suspended => Status::Suspended,
            _ => Status::Unknown,
        }
    }

    pub(super) fn disk_kind(kind: si::DiskKind) -> DiskKind {
        match kind {
            si::DiskKind::HDD => DiskKind::Hdd,
            si::DiskKind::SSD => DiskKind::Ssd,
            si::DiskKind::Unknown(_) => DiskKind::Unknown,
        }
    }

    fn text(value: &std::ffi::OsStr) -> String {
        value.to_string_lossy().into_owned()
    }

    /// What this platform can do with a process.
    ///
    /// ```luau
    /// local can = sysinfo.capabilities()
    /// assert(can.kill)
    /// -- `terminate` is `false` on Windows, which has no request to stop an arbitrary process
    /// assert(can.binding == "pidfd" or can.binding == "process_handle" or can.binding == "start_time")
    /// ```
    pub fn capabilities() -> Capabilities {
        let capability = native::capability();
        Capabilities { terminate: capability.terminate, kill: true, binding: capability.binding }
    }

    /// A new `System`: measured once, so memory, disks, networks and the process list are there to read and the loads
    /// are `nil` until the next refresh (after `MIN_CPU_INTERVAL_MS`).
    #[host("Wait")]
    pub async fn system() -> Result<System, Error> {
        blocking(|| Ok(System { state: Arc::new(Mutex::new(State::new())) })).await
    }

    // -----------------------------------------------------------------------
    // System
    // -----------------------------------------------------------------------

    /// The machine as last measured. Clones share what is measured.
    #[derive(Clone, Object)]
    pub struct System {
        state: Arc<Mutex<State>>,
    }

    #[mlua_ir::object]
    impl System {
        /// Measure the CPU load (of the machine, of each core and of each process) and the process list, together:
        /// they are one measurement over one interval. `too_soon` if the last was less than `MIN_CPU_INTERVAL_MS`
        /// ago; nothing is measured then.
        #[host("Wait")]
        pub async fn refresh_processes(&self) -> Result<(), Error> {
            let state = self.state.clone();
            blocking(move || state.lock().refresh_load()).await
        }

        /// Measure memory and swap.
        #[host("Wait")]
        pub async fn refresh_memory(&self) -> Result<(), Error> {
            let state = self.state.clone();
            blocking(move || {
                state.lock().system.refresh_memory();
                Ok(())
            })
            .await
        }

        /// Measure the disks: the list, the space on each, and what each read and wrote since the last refresh.
        #[host("Wait")]
        pub async fn refresh_disks(&self) -> Result<(), Error> {
            let state = self.state.clone();
            blocking(move || {
                state.lock().measure_disks(Instant::now());
                Ok(())
            })
            .await
        }

        /// Measure the network interfaces: their counters, and the rate since the last refresh over the time that
        /// really passed.
        #[host("Wait")]
        pub async fn refresh_networks(&self) -> Result<(), Error> {
            let state = self.state.clone();
            blocking(move || {
                state.lock().measure_networks(Instant::now());
                Ok(())
            })
            .await
        }

        /// Measure everything. `too_soon` is as for `refresh_processes`, and nothing is measured then.
        #[host("Wait")]
        pub async fn refresh(&self) -> Result<(), Error> {
            let state = self.state.clone();
            blocking(move || {
                let mut state = state.lock();
                state.load.check(Instant::now())?;
                state.system.refresh_memory();
                let now = Instant::now();
                state.measure_disks(now);
                state.measure_networks(now);
                state.refresh_load()
            })
            .await
        }

        /// The processor as last measured. `usage` (and each core's) is `nil` until two refreshes of
        /// `refresh_processes` make a difference.
        pub fn cpu(&self) -> CpuStatus {
            let state = self.state.lock();
            let sampled = state.load.sampled();
            let cores = state
                .system
                .cpus()
                .iter()
                .map(|cpu| Core {
                    name: cpu.name().to_owned(),
                    usage: sampled.then(|| whole(cpu.cpu_usage())),
                    frequency_mhz: Some(cpu.frequency()).filter(|mhz| *mhz > 0),
                })
                .collect::<Vec<_>>();
            CpuStatus {
                usage: sampled.then(|| whole(state.system.global_cpu_usage())),
                sampled,
                logical_cores: u32::try_from(cores.len()).unwrap_or(u32::MAX),
                cores,
                physical_cores: si::System::physical_core_count().and_then(|n| u32::try_from(n).ok()),
                brand: state
                    .system
                    .cpus()
                    .first()
                    .map(|cpu| cpu.brand().trim().to_owned())
                    .filter(|b| !b.is_empty()),
                arch: si::System::cpu_arch(),
                load_average: load_average(),
            }
        }

        /// Memory as last measured, in bytes.
        pub fn memory(&self) -> Memory {
            let state = self.state.lock();
            Memory {
                total: state.system.total_memory(),
                used: state.system.used_memory(),
                available: state.system.available_memory(),
                free: state.system.free_memory(),
            }
        }

        /// Swap as last measured, in bytes.
        pub fn swap(&self) -> Swap {
            let state = self.state.lock();
            Swap {
                total: state.system.total_swap(),
                used: state.system.used_swap(),
                free: state.system.free_swap(),
            }
        }

        /// The disks as last measured, by mount point.
        pub fn disks(&self) -> Vec<DiskInfo> {
            let state = self.state.lock();
            let mut disks = state
                .disks
                .list()
                .iter()
                .map(|disk| {
                    let mount_point = disk.mount_point().to_string_lossy().into_owned();
                    let rates = state.disk_meters.get(&mount_point).and_then(|meter| meter.rates);
                    DiskInfo {
                        name: text(disk.name()),
                        mount_point,
                        file_system: text(disk.file_system()),
                        kind: disk_kind(disk.kind()),
                        total_bytes: disk.total_space(),
                        available_bytes: disk.available_space(),
                        removable: disk.is_removable(),
                        read_only: disk.is_read_only(),
                        read_rate: rates.map(|(read, _)| read),
                        write_rate: rates.map(|(_, written)| written),
                    }
                })
                .collect::<Vec<_>>();
            disks.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
            disks
        }

        /// The network interfaces as last measured, by name. A rate is `nil` for an interface that has been seen in
        /// one refresh only.
        pub fn networks(&self) -> Vec<NetworkInterface> {
            let state = self.state.lock();
            let mut interfaces = state
                .networks
                .iter()
                .map(|(name, data)| {
                    let rates = state.meters.get(name).and_then(|meter| meter.rates);
                    let mac = data.mac_address().to_string();
                    NetworkInterface {
                        name: name.clone(),
                        mac_address: Some(mac).filter(|mac| mac != "00:00:00:00:00:00"),
                        received_bytes: data.total_received(),
                        transmitted_bytes: data.total_transmitted(),
                        receive_rate: rates.map(|(received, _)| received),
                        transmit_rate: rates.map(|(_, transmitted)| transmitted),
                    }
                })
                .collect::<Vec<_>>();
            interfaces.sort_by(|a, b| a.name.cmp(&b.name));
            interfaces
        }

        /// The machine: names, versions and how long it has been up. Uptime is read when asked.
        pub fn host(&self) -> Host {
            Host {
                name: si::System::host_name(),
                os_name: si::System::name(),
                os_version: si::System::long_os_version().or_else(si::System::os_version),
                kernel_version: si::System::kernel_version(),
                arch: si::System::cpu_arch(),
                uptime_secs: si::System::uptime(),
                boot_time: si::System::boot_time(),
            }
        }

        /// The readings this platform does not have at all, by name: they are `nil` (`load_average` is the one a
        /// platform may lack, on Windows).
        pub fn unsupported(&self) -> Vec<String> {
            unsupported_here()
        }

        /// The process with this PID, as last measured, or `nil` when there is none. The handle names the process
        /// that has the PID now, by its start time too.
        pub fn process(&self, pid: u32) -> Option<Process> {
            let state = self.state.lock();
            state
                .system
                .process(si::Pid::from_u32(pid))
                .map(|process| Process::observed(&self.state, &state, process))
        }

        /// Every process as last measured, in the order of their PIDs.
        pub fn processes(&self) -> Vec<Process> {
            let state = self.state.lock();
            let mut processes = state
                .system
                .processes()
                .values()
                .map(|process| Process::observed(&self.state, &state, process))
                .collect::<Vec<_>>();
            processes.sort_by_key(|process| process.pid);
            processes
        }
    }

    #[cfg(not(windows))]
    fn load_average() -> Option<LoadAverage> {
        let load = si::System::load_average();
        Some(LoadAverage { one: load.one, five: load.five, fifteen: load.fifteen })
    }

    #[cfg(windows)]
    fn load_average() -> Option<LoadAverage> {
        None
    }

    #[cfg(not(windows))]
    pub(super) fn unsupported_here() -> Vec<String> {
        Vec::new()
    }

    #[cfg(windows)]
    pub(super) fn unsupported_here() -> Vec<String> {
        vec!["load_average".to_owned()]
    }

    // -----------------------------------------------------------------------
    // Process
    // -----------------------------------------------------------------------

    /// A process as observed: its PID and its start time. It reads that process from the last refresh, and acts on
    /// it and on no other.
    #[derive(Clone, Object)]
    pub struct Process {
        state: Arc<Mutex<State>>,
        pid: u32,
        start_time: u64,
        /// The finer start time taken when it was first observed, where the platform gives one.
        fine: Option<u64>,
        /// Which sighting of this PID it is (see `Seen`): the process observed, not a later one with the same PID.
        generation: u64,
        /// Whether the sample that described it was confirmed as one process (see `Seen`).
        confirmed: bool,
        name: String,
        /// The native reference to it, once pinned, and whether it was let go. Clones share it.
        held: Arc<Mutex<Hold>>,
    }

    /// Where a process's native reference is: not asked for yet (a signal then binds for the one call), held, or
    /// released. A released handle is finished: it does not take a new reference (take another handle from the system).
    enum Hold {
        Never,
        Held(Option<native::Held>),
        Released,
    }

    impl Process {
        pub(super) fn observed(state: &Arc<Mutex<State>>, locked: &State, process: &si::Process) -> Self {
            let pid = process.pid().as_u32();
            let start_time = process.start_time();
            let seen = locked.seen.get(&pid).filter(|seen| seen.start_time == start_time);
            Self {
                state: state.clone(),
                pid,
                start_time,
                fine: seen.and_then(|seen| seen.fine),
                generation: seen.map_or(0, |seen| seen.generation),
                confirmed: seen.is_some_and(|seen| seen.confirmed),
                name: text(process.name()),
                held: Arc::new(Mutex::new(Hold::Never)),
            }
        }

        /// A handle for a PID with a start time of the caller's choosing: how a test makes "the PID was reused".
        #[cfg(test)]
        pub(super) fn forged(state: &Arc<Mutex<State>>, pid: u32, start_time: u64, name: &str) -> Self {
            Self {
                state: state.clone(),
                pid,
                start_time,
                // what a real observation of this PID would have taken
                fine: native::fine_start(pid),
                generation: 0,
                confirmed: true,
                name: name.to_owned(),
                held: Arc::new(Mutex::new(Hold::Never)),
            }
        }

        /// A handle that observed a finer start time than the process has: the same second, another process.
        #[cfg(all(test, target_os = "linux"))]
        pub(super) fn forged_fine(
            state: &Arc<Mutex<State>>,
            pid: u32,
            start_time: u64,
            fine: u64,
            name: &str,
        ) -> Self {
            Self { fine: Some(fine), ..Self::forged(state, pid, start_time, name) }
        }

        /// A handle whose finer start time was not read when it was observed.
        #[cfg(all(test, any(target_os = "linux", windows)))]
        pub(super) fn forged_without_fine(
            state: &Arc<Mutex<State>>,
            pid: u32,
            start_time: u64,
            name: &str,
        ) -> Self {
            Self { fine: None, ..Self::forged(state, pid, start_time, name) }
        }

        fn read<R>(&self, f: impl FnOnce(&State, &si::Process) -> R) -> Option<R> {
            let state = self.state.lock();
            let process = state.find(self.pid, self.start_time, self.generation)?;
            Some(f(&state, process))
        }

        /// Whether this handle names a process that can be told apart from another that is given its PID: its start
        /// time is known and, where the platform gives a finer one, it was read when the process was observed and the
        /// sample that described it was confirmed as one process (a first sighting is read-only until the next
        /// refresh, as the process may have been replaced between the sample and the identity read). Without
        /// that no signal is sent and no reference is taken (nothing falls back to the whole seconds).
        fn identified(&self) -> Result<(), Error> {
            if self.start_time == 0 || !self.confirmed || (native::FINE_START && self.fine.is_none()) {
                return Err(Error::UnknownIdentity);
            }
            Ok(())
        }

        /// Send the signal to the observed process (and nothing else), on the blocking pool.
        async fn signal(&self, sig: Sig) -> Result<(), Error> {
            let (pid, start_time, fine, held) = (self.pid, self.start_time, self.fine, self.held.clone());
            let identified = self.identified();
            blocking(move || {
                if pid == std::process::id() {
                    return Err(Error::OwnProcess);
                }
                identified?;
                if sig == Sig::Terminate && !native::capability().terminate {
                    return Err(Error::Unsupported {
                        reason: "this platform has no request to stop a process: only `kill` is available"
                            .to_owned(),
                    });
                }
                match &*held.lock() {
                    // a reference that was let go signals nothing, and is not taken again
                    Hold::Released => Err(Error::Released),
                    // bound when it was pinned
                    Hold::Held(held) => native::send(pid, start_time, fine, held.as_ref(), sig),
                    // bound for this one call
                    Hold::Never => {
                        let once = native::acquire(pid, start_time, fine)?;
                        native::send(pid, start_time, fine, once.as_ref(), sig)
                    }
                }
            })
            .await
        }
    }

    #[mlua_ir::object]
    impl Process {
        /// Its PID.
        #[get]
        pub fn pid(&self) -> u32 {
            self.pid
        }

        /// When it started, in seconds since the Unix epoch. With the PID, it is what makes it this process. It is 0 when
        /// the platform does not say (some system processes on macOS and Windows): such a process is not told apart from
        /// another that is given its PID, so a script should not signal it.
        #[get]
        pub fn start_time(&self) -> u64 {
            self.start_time
        }

        /// Which sighting of this PID it is: a number the module issues when it first meets a process, and again when
        /// the PID turns out to be another process (even one that started in the same second). A reference kept
        /// outside the module carries it with the PID and start time, and a process with another number is not it.
        #[get]
        pub fn observation(&self) -> u64 {
            self.generation
        }

        /// Its name when it was observed: still here when it has exited.
        #[get]
        pub fn name(&self) -> String {
            self.name.clone()
        }

        /// Is it still there as of the last refresh, with this PID and this start time.
        pub fn alive(&self) -> bool {
            self.read(|_, _| ()).is_some()
        }

        /// Its parent's PID, when it has one and the platform says; `nil` when it is gone.
        pub fn parent_pid(&self) -> Option<u32> {
            self.read(|_, process| process.parent().map(si::Pid::as_u32)).flatten()
        }

        /// Its parent, as last measured, or `nil`. A parent that started after it is not its parent (a PID that was
        /// reused).
        pub fn parent(&self) -> Option<Process> {
            let state = self.state.lock();
            let process = state.find(self.pid, self.start_time, self.generation)?;
            let parent = state.system.process(process.parent()?)?;
            (parent.start_time() <= process.start_time())
                .then(|| Process::observed(&self.state, &state, parent))
        }

        /// The processes whose parent it is, as last measured.
        pub fn children(&self) -> Vec<Process> {
            let state = self.state.lock();
            if state.find(self.pid, self.start_time, self.generation).is_none() {
                return Vec::new();
            }
            let me = si::Pid::from_u32(self.pid);
            let mut children = state
                .system
                .processes()
                .values()
                .filter(|child| child.parent() == Some(me) && child.start_time() >= self.start_time)
                .map(|child| Process::observed(&self.state, &state, child))
                .collect::<Vec<_>>();
            children.sort_by_key(|child| child.pid);
            children
        }

        /// Its executable's path, when the platform says (and the person running this may see it).
        pub fn exe(&self) -> Option<String> {
            self.read(|_, process| process.exe().map(|path| path.to_string_lossy().into_owned())).flatten()
        }

        /// Its command line, one entry per argument; empty when the platform does not say, `nil` when it is gone.
        pub fn command(&self) -> Option<Vec<String>> {
            self.read(|_, process| process.cmd().iter().map(|arg| text(arg)).collect())
        }

        /// The name of the user it runs as, when the platform says and the user is known to the machine.
        pub fn user(&self) -> Option<String> {
            self.read(|state, process| {
                let id = process.user_id()?;
                state.users.get_user_by_id(id).map(|user| user.name().to_owned())
            })
            .flatten()
        }

        /// What state it is in.
        pub fn status(&self) -> Option<Status> {
            self.read(|_, process| status_of(process.status()))
        }

        /// How much of a core it used between the last two refreshes, in percent: 100 is one whole core, and a
        /// process of several threads passes it. `nil` until it has been in two refreshes, and when it is gone.
        pub fn cpu_usage(&self) -> Option<f64> {
            self.read(|state, process| {
                let samples =
                    state.seen.get(&self.pid).filter(|seen| seen.generation == self.generation)?.samples;
                (samples >= 2).then(|| f64::from(process.cpu_usage().max(0.0)))
            })
            .flatten()
        }

        /// The memory it holds in RAM, in bytes.
        pub fn memory(&self) -> Option<u64> {
            self.read(|_, process| process.memory())
        }

        /// The memory it has mapped, in bytes.
        pub fn virtual_memory(&self) -> Option<u64> {
            self.read(|_, process| process.virtual_memory())
        }

        /// How long it has been running, in seconds, as of the last refresh.
        pub fn run_time(&self) -> Option<u64> {
            self.read(|_, process| process.run_time())
        }

        /// What binds a signal to this process: `pidfd`, `process_handle` or `start_time`. Pinned or not, it is the
        /// one that applies to this handle.
        pub fn binding(&self) -> Binding {
            match &*self.held.lock() {
                Hold::Held(held) => native::binding_of(held.as_ref()),
                Hold::Never | Hold::Released => Binding::StartTime,
            }
        }

        /// Is a native reference to the process held (`pin` was called, and `release` was not).
        pub fn pinned(&self) -> bool {
            matches!(&*self.held.lock(), Hold::Held(Some(_)))
        }

        /// Was its reference let go (`release` after `pin`). Such a handle signals nothing and cannot be pinned again:
        /// the process has to be taken from the system again.
        pub fn released(&self) -> bool {
            matches!(&*self.held.lock(), Hold::Released)
        }

        /// Hold a native reference to the process now, after checking it is the one observed, so that a later
        /// `terminate` or `kill` can only reach it, even if its PID is reused in between. It is the binding that
        /// applies, and `start_time` on a platform that has no native reference (then nothing is held). A process
        /// that is gone or whose PID was reused is `process_gone` or `pid_reused`. Hold one only for a process a
        /// person has selected: a reference is a descriptor or a handle, and it stays until `release`. A process whose
        /// start time is not known is `unknown_identity`; a handle that was released is `released`.
        #[host("Wait")]
        pub async fn pin(&self) -> Result<Binding, Error> {
            let (pid, start_time, fine, held) = (self.pid, self.start_time, self.fine, self.held.clone());
            let identified = self.identified();
            blocking(move || {
                identified?;
                let mut hold = held.lock();
                match &*hold {
                    Hold::Released => return Err(Error::Released),
                    Hold::Held(_) => {}
                    Hold::Never => *hold = Hold::Held(native::acquire(pid, start_time, fine)?),
                }
                Ok(match &*hold {
                    Hold::Held(held) => native::binding_of(held.as_ref()),
                    Hold::Never | Hold::Released => Binding::StartTime,
                })
            })
            .await
        }

        /// Let go of the native reference, if one is held. The handle is finished then: it signals nothing and is not
        /// pinned again (`released()` says so), so a signal that comes late cannot take a new reference.
        pub fn release(&self) {
            let mut hold = self.held.lock();
            if matches!(&*hold, Hold::Held(_)) {
                *hold = Hold::Released;
            }
        }

        /// Ask it to stop (`SIGTERM`): it may handle the request, or ignore it. Never `kill`. `unsupported` on
        /// Windows. `process_gone`, `pid_reused` and `permission_denied` as described in the module.
        #[host("Wait")]
        pub async fn terminate(&self) -> Result<(), Error> {
            self.signal(Sig::Terminate).await
        }

        /// Stop it, forced (`SIGKILL`, `TerminateProcess`): it is not asked.
        #[host("Wait")]
        pub async fn kill(&self) -> Result<(), Error> {
            self.signal(Sig::Kill).await
        }

        /// What `tostring` gives: `process:1234 name`.
        #[to_string]
        pub fn display(&self) -> String {
            format!("process:{} {}", self.pid, self.name)
        }
    }
}

#[cfg(test)]
mod tests;

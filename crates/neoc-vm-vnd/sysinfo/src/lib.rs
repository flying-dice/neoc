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
//! (`pidfd`, `process_handle` or `start_time`). The start time is compared in whole seconds, as `sysinfo` reads it, so
//! a PID reused by a process that started in the same second as the previous one is not told apart on any platform.
//! Nothing in this module signals a PID after an identity or permission failure.
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
//!   `unsupported`, `failed`): a script reads `err.kind`, and for `too_soon` how long to wait.
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
    struct Seen {
        start_time: u64,
        samples: u32,
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
        system: si::System,
        disks: si::Disks,
        disk_meters: HashMap<String, Meter>,
        networks: si::Networks,
        users: si::Users,
        load: Clock,
        seen: HashMap<u32, Seen>,
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
            let processes = self.system.processes();
            self.seen.retain(|pid, _| processes.contains_key(&si::Pid::from_u32(*pid)));
            for (pid, process) in processes {
                let start_time = process.start_time();
                self.seen
                    .entry(pid.as_u32())
                    .and_modify(|seen| {
                        if seen.start_time == start_time {
                            seen.samples = seen.samples.saturating_add(1);
                        } else {
                            // the PID is another process now: it starts again, with no load
                            *seen = Seen { start_time, samples: 1 };
                        }
                    })
                    .or_insert(Seen { start_time, samples: 1 });
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

        /// A live process by its identity.
        fn find(&self, pid: u32, start_time: u64) -> Option<&si::Process> {
            self.system.process(si::Pid::from_u32(pid)).filter(|process| process.start_time() == start_time)
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
                .map(|process| Process::observed(&self.state, process))
        }

        /// Every process as last measured, in the order of their PIDs.
        pub fn processes(&self) -> Vec<Process> {
            let state = self.state.lock();
            let mut processes = state
                .system
                .processes()
                .values()
                .map(|process| Process::observed(&self.state, process))
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
        name: String,
        /// The native reference to it, once pinned. Clones share it.
        held: Arc<Mutex<Option<native::Held>>>,
    }

    impl Process {
        pub(super) fn observed(state: &Arc<Mutex<State>>, process: &si::Process) -> Self {
            Self {
                state: state.clone(),
                pid: process.pid().as_u32(),
                start_time: process.start_time(),
                name: text(process.name()),
                held: Arc::new(Mutex::new(None)),
            }
        }

        /// A handle for a PID with a start time of the caller's choosing: how a test makes "the PID was reused".
        #[cfg(test)]
        pub(super) fn forged(state: &Arc<Mutex<State>>, pid: u32, start_time: u64, name: &str) -> Self {
            Self {
                state: state.clone(),
                pid,
                start_time,
                name: name.to_owned(),
                held: Arc::new(Mutex::new(None)),
            }
        }

        fn read<R>(&self, f: impl FnOnce(&State, &si::Process) -> R) -> Option<R> {
            let state = self.state.lock();
            let process = state.find(self.pid, self.start_time)?;
            Some(f(&state, process))
        }

        /// Send the signal to the observed process (and nothing else), on the blocking pool.
        async fn signal(&self, sig: Sig) -> Result<(), Error> {
            let (pid, start_time, held) = (self.pid, self.start_time, self.held.clone());
            blocking(move || {
                if pid == std::process::id() {
                    return Err(Error::OwnProcess);
                }
                if sig == Sig::Terminate && !native::capability().terminate {
                    return Err(Error::Unsupported {
                        reason: "this platform has no request to stop a process: only `kill` is available"
                            .to_owned(),
                    });
                }
                let held = held.lock();
                match held.as_ref() {
                    // bound when it was pinned
                    Some(held) => native::send(pid, start_time, Some(held), sig),
                    // bound for this one call
                    None => {
                        let once = native::acquire(pid, start_time)?;
                        native::send(pid, start_time, once.as_ref(), sig)
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

        /// When it started, in seconds since the Unix epoch. With the PID, it is what makes it this process.
        #[get]
        pub fn start_time(&self) -> u64 {
            self.start_time
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
            let process = state.find(self.pid, self.start_time)?;
            let parent = state.system.process(process.parent()?)?;
            (parent.start_time() <= process.start_time()).then(|| Process::observed(&self.state, parent))
        }

        /// The processes whose parent it is, as last measured.
        pub fn children(&self) -> Vec<Process> {
            let state = self.state.lock();
            if state.find(self.pid, self.start_time).is_none() {
                return Vec::new();
            }
            let me = si::Pid::from_u32(self.pid);
            let mut children = state
                .system
                .processes()
                .values()
                .filter(|child| child.parent() == Some(me) && child.start_time() >= self.start_time)
                .map(|child| Process::observed(&self.state, child))
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
                    state.seen.get(&self.pid).filter(|seen| seen.start_time == self.start_time)?.samples;
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
            native::binding_of(self.held.lock().as_ref())
        }

        /// Is a native reference to the process held (`pin` was called, and `release` was not).
        pub fn pinned(&self) -> bool {
            self.held.lock().is_some()
        }

        /// Hold a native reference to the process now, after checking it is the one observed, so that a later
        /// `terminate` or `kill` can only reach it, even if its PID is reused in between. It is the binding that
        /// applies, and `start_time` on a platform that has no native reference (then nothing is held). A process
        /// that is gone or whose PID was reused is `process_gone` or `pid_reused`. Hold one only for a process a
        /// person has selected: a reference is a descriptor or a handle, and it stays until `release`.
        #[host("Wait")]
        pub async fn pin(&self) -> Result<Binding, Error> {
            let (pid, start_time, held) = (self.pid, self.start_time, self.held.clone());
            blocking(move || {
                let mut held = held.lock();
                if held.is_none() {
                    *held = native::acquire(pid, start_time)?;
                }
                Ok(native::binding_of(held.as_ref()))
            })
            .await
        }

        /// Let go of the native reference, if one is held.
        pub fn release(&self) {
            *self.held.lock() = None;
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

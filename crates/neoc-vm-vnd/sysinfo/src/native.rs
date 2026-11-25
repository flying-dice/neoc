//! The platform's own process identity and signals, under `Process::terminate` and `Process::kill`.
//!
//! `sysinfo` monitors; it signals by PID alone, which is a race (a PID is reused once its process is gone) and which
//! says only "failed" when it fails. This layer binds a signal to the process that was *observed*, and reports what
//! the OS said:
//!
//! - **Linux**: a `pidfd`. `pidfd_open` names one process for good (it never turns into a process that took the PID
//!   afterwards), so after it is checked once against the start time observed, a signal sent through it can only reach
//!   the observed process. A kernel without `pidfd_open` (`ENOSYS`) is the one case that falls back to the start-time
//!   check, and `Binding::StartTime` says so.
//! - **Windows**: a process handle. A PID stays reserved while a handle to its process is open, so the same check
//!   holds for the handle.
//! - **other Unixes (macOS)**: no equivalent exists. The start time observed is checked again immediately before the
//!   signal, and that check and the signal are two steps: a PID that is reused between them is signalled. This is the
//!   residual race, and `Binding::StartTime` is how a script finds out it is the one that applies.
//!
//! "The same process" is "the process whose start time is the one observed" in every case, and the start time is
//! read the way `sysinfo` reads it (whole seconds). A PID reused by a process started in the same second as the
//! one before it is not told apart. That is the second residual, shared by every platform.
//!
//! A held handle is checked once, when it is acquired, and never again: acquiring is not a second look at a PID.
//! No path in here falls back to a PID-only signal after an identity or permission failure.

use ::sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::sysinfo::{Binding, Error};

/// Which signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sig {
    /// The platform's request to stop (`SIGTERM`): the process may handle it, or ignore it.
    Terminate,
    /// The platform's forced stop (`SIGKILL`, `TerminateProcess`): the process is not asked.
    Kill,
}

/// What a platform can do.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Capability {
    /// `Sig::Terminate` is a request this platform has.
    pub terminate: bool,
    /// The identity of a process can be held natively on this platform (a descriptor or a handle).
    pub binding: Binding,
}

pub(crate) use imp::Held;

/// The start time of the process that has this PID right now, read as `sysinfo` reads it, or `None` when no
/// process has it.
pub(crate) fn live_start_time(pid: u32) -> Option<u64> {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
    system.process(pid).map(::sysinfo::Process::start_time)
}

/// The start time of the process a native handle holds is `observed`, or why not. Windows binds by handle, so
/// only the unix backends use it.
#[cfg(unix)]
fn check_identity(pid: u32, observed: u64) -> Result<(), Error> {
    match live_start_time(pid) {
        None => Err(Error::ProcessGone),
        Some(live) if live == observed => Ok(()),
        Some(_) => Err(Error::PidReused),
    }
}

/// What an error number from a signal call means: only the numbers that say so are `process_gone` and
/// `permission_denied`; any other is reported with the system's own words, and never guessed at.
#[cfg(unix)]
pub(crate) fn errno_error(errno: rustix::io::Errno) -> Error {
    use rustix::io::Errno;
    match errno {
        Errno::SRCH => Error::ProcessGone,
        Errno::PERM => Error::PermissionDenied { reason: errno.to_string() },
        other => Error::Failed { reason: other.to_string() },
    }
}

/// What a platform can do.
pub(crate) fn capability() -> Capability {
    imp::capability()
}

/// Bind the process `pid`, observed to have started at `observed`, to what the platform can hold, after checking it is
/// that process. `Ok(None)` is a platform with nothing to hold (the binding is the start time).
pub(crate) fn acquire(pid: u32, observed: u64) -> Result<Option<Held>, Error> {
    imp::acquire(pid, observed)
}

/// Signal the process: through `held` if there is one, and otherwise after checking `observed` once more.
pub(crate) fn send(pid: u32, observed: u64, held: Option<&Held>, sig: Sig) -> Result<(), Error> {
    imp::send(pid, observed, held, sig)
}

/// The binding a `held` handle (or the lack of one) stands for.
pub(crate) fn binding_of(held: Option<&Held>) -> Binding {
    match held {
        Some(_) => imp::capability().binding,
        None => Binding::StartTime,
    }
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod imp {
    use std::os::fd::OwnedFd;

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use rustix::process::{PidfdFlags, Signal, pidfd_open, pidfd_send_signal};

    use super::{Capability, Sig, check_identity, errno_error as from_errno};
    use crate::sysinfo::{Binding, Error};

    /// A process, held by its `pidfd`. Dropping it closes the descriptor.
    pub(crate) struct Held(OwnedFd);

    pub(super) fn capability() -> Capability {
        Capability { terminate: true, binding: Binding::Pidfd }
    }

    pub(super) fn signal(sig: Sig) -> Signal {
        match sig {
            Sig::Terminate => Signal::TERM,
            Sig::Kill => Signal::KILL,
        }
    }

    pub(super) fn raw_pid(pid: u32) -> Result<rustix::process::Pid, Error> {
        i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw).ok_or_else(|| Error::Unsupported {
            reason: format!("{pid} is not a process id that can be signalled"),
        })
    }

    /// The process the descriptor holds has exited (it is a zombie or gone): a `pidfd` becomes readable then.
    fn exited(fd: &OwnedFd) -> Result<bool, Error> {
        let mut fds = [PollFd::new(fd, PollFlags::IN)];
        let none = Timespec { tv_sec: 0, tv_nsec: 0 };
        match poll(&mut fds, Some(&none)) {
            Ok(ready) => Ok(ready > 0),
            Err(why) => Err(from_errno(why)),
        }
    }

    pub(super) fn acquire(pid: u32, observed: u64) -> Result<Option<Held>, Error> {
        let raw = raw_pid(pid)?;
        match pidfd_open(raw, PidfdFlags::empty()) {
            Ok(fd) => {
                // The descriptor names one process. Which one it is: what has this PID now, and has that process
                // not gone since (a PID reused between the two steps would show as an exit of this one).
                let live = super::live_start_time(pid);
                if exited(&fd)? {
                    return Err(Error::ProcessGone);
                }
                match live {
                    None => Err(Error::ProcessGone),
                    Some(live) if live == observed => Ok(Some(Held(fd))),
                    Some(_) => Err(Error::PidReused),
                }
            }
            Err(Errno::SRCH) => Err(Error::ProcessGone),
            // A kernel older than 5.3: no descriptor to hold. The one fall back, and the binding says so.
            Err(Errno::NOSYS) => {
                check_identity(pid, observed)?;
                Ok(None)
            }
            Err(why) => Err(from_errno(why)),
        }
    }

    pub(super) fn send(pid: u32, observed: u64, held: Option<&Held>, sig: Sig) -> Result<(), Error> {
        match held {
            Some(Held(fd)) => pidfd_send_signal(fd, signal(sig)).map_err(from_errno),
            None => {
                check_identity(pid, observed)?;
                rustix::process::kill_process(raw_pid(pid)?, signal(sig)).map_err(from_errno)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// other Unixes
// ---------------------------------------------------------------------------

#[cfg(all(unix, not(target_os = "linux")))]
mod imp {
    use rustix::process::{Pid, Signal, kill_process};

    use super::{Capability, Sig, check_identity, errno_error};
    use crate::sysinfo::{Binding, Error};

    /// Nothing to hold on this platform: the type is there so the callers do not differ.
    pub(crate) struct Held;

    pub(super) fn capability() -> Capability {
        Capability { terminate: true, binding: Binding::StartTime }
    }

    pub(super) fn acquire(pid: u32, observed: u64) -> Result<Option<Held>, Error> {
        check_identity(pid, observed)?;
        Ok(None)
    }

    pub(super) fn send(pid: u32, observed: u64, _held: Option<&Held>, sig: Sig) -> Result<(), Error> {
        let raw = i32::try_from(pid).ok().and_then(Pid::from_raw).ok_or_else(|| Error::Unsupported {
            reason: format!("{pid} is not a process id that can be signalled"),
        })?;
        // The check and the signal are two steps here: the residual race, documented in this file's header.
        check_identity(pid, observed)?;
        let signal = match sig {
            Sig::Terminate => Signal::TERM,
            Sig::Kill => Signal::KILL,
        };
        kill_process(raw, signal).map_err(errno_error)
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, GetLastError, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        TerminateProcess, WaitForSingleObject,
    };

    use super::{Capability, Sig};
    use crate::sysinfo::{Binding, Error};

    /// A process, held by a handle with the rights to wait for it and to end it. Dropping it closes the handle.
    pub(crate) struct Held(HANDLE);

    // SAFETY: a process handle is a kernel object's reference, valid on any thread, and `Held` is only used by
    // one thread at a time (it is behind a mutex).
    unsafe impl Send for Held {}

    impl Drop for Held {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by `OpenProcess` and is closed here, once.
            unsafe { CloseHandle(self.0) };
        }
    }

    impl Held {
        /// The process has exited. A handle to an exited process stays valid while it is open, so this is
        /// answerable, and it is how an "access denied" from `TerminateProcess` of an exited process is told from
        /// a refusal.
        fn exited(&self) -> bool {
            // SAFETY: `self.0` is an open handle with `SYNCHRONIZE`.
            unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
        }
    }

    pub(super) fn capability() -> Capability {
        // No request to stop for a process in general: a console `CTRL_C_EVENT` reaches a process group that shares
        // this one's console, and `WM_CLOSE` a window. Neither is "terminate this process", so there is none.
        Capability { terminate: false, binding: Binding::ProcessHandle }
    }

    pub(super) fn acquire(pid: u32, observed: u64) -> Result<Option<Held>, Error> {
        if pid == 0 {
            return Err(Error::Unsupported { reason: "process 0 is the system idle process".to_owned() });
        }
        // SAFETY: plain call; the handle (if any) is owned by `Held` from here.
        let handle = unsafe {
            OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid)
        };
        if handle.is_null() {
            // SAFETY: reads the calling thread's last error.
            return Err(match unsafe { GetLastError() } {
                // `OpenProcess` says "invalid parameter" for a PID no process has.
                ERROR_INVALID_PARAMETER => Error::ProcessGone,
                ERROR_ACCESS_DENIED => Error::PermissionDenied {
                    reason: "the system would not open the process with the right to end it".to_owned(),
                },
                other => Error::Failed { reason: format!("OpenProcess failed with system error {other}") },
            });
        }
        let held = Held(handle);
        // The PID stays this process's while the handle is open, so what has the PID now is the process the handle
        // holds: unless it has exited, which is `exited`.
        let live = super::live_start_time(pid);
        if held.exited() {
            return Err(Error::ProcessGone);
        }
        match live {
            None => Err(Error::ProcessGone),
            Some(live) if live == observed => Ok(Some(held)),
            Some(_) => Err(Error::PidReused),
        }
    }

    pub(super) fn send(pid: u32, observed: u64, held: Option<&Held>, sig: Sig) -> Result<(), Error> {
        if sig == Sig::Terminate {
            return Err(Error::Unsupported {
                reason: "Windows has no request to stop an arbitrary process: only `kill` is available"
                    .to_owned(),
            });
        }
        // Without a handle held (an unpinned process) one is opened, and checked, for this call.
        let opened;
        let held = match held {
            Some(held) => held,
            None => {
                opened = acquire(pid, observed)?
                    .ok_or_else(|| Error::Failed { reason: "no process handle could be held".to_owned() })?;
                &opened
            }
        };
        // SAFETY: `held.0` is an open handle with `PROCESS_TERMINATE`.
        if unsafe { TerminateProcess(held.0, 1) } != 0 {
            return Ok(());
        }
        // SAFETY: reads the calling thread's last error.
        let last = unsafe { GetLastError() };
        // `TerminateProcess` answers "access denied" for a process that has already exited: that is not a refusal.
        if held.exited() {
            return Err(Error::ProcessGone);
        }
        Err(if last == ERROR_ACCESS_DENIED {
            Error::PermissionDenied { reason: "the system refused to end the process".to_owned() }
        } else {
            Error::Failed { reason: format!("TerminateProcess failed with system error {last}") }
        })
    }
}

// ---------------------------------------------------------------------------
// everything else
// ---------------------------------------------------------------------------

#[cfg(not(any(unix, windows)))]
mod imp {
    use super::{Capability, Sig};
    use crate::sysinfo::{Binding, Error};

    pub(crate) struct Held;

    pub(super) fn capability() -> Capability {
        Capability { terminate: false, binding: Binding::StartTime }
    }

    pub(super) fn acquire(_pid: u32, _observed: u64) -> Result<Option<Held>, Error> {
        Err(Error::Unsupported { reason: "this platform has no process signals".to_owned() })
    }

    pub(super) fn send(_pid: u32, _observed: u64, _held: Option<&Held>, _sig: Sig) -> Result<(), Error> {
        Err(Error::Unsupported { reason: "this platform has no process signals".to_owned() })
    }
}

//! The CPU budget, counted in Luau interrupts rather than wall-clock time.
//!
//! The VM's interrupt runs at every Lua function call and every loop back-edge, so the number of interrupts
//! since the app was last handed control is a deterministic measure of the work it has done: it does not
//! change when the machine is busy, when the process is descheduled or when the host sleeps. A loaded
//! machine makes a runaway script take longer to hit the budget, never an honest one hit it.
//!
//! A wall-clock backstop sits behind the count, checked only every few thousand interrupts so the check
//! stays cheap. It catches what the count cannot (a native call that polls the interrupt rarely) and is set
//! far above any honest slice.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// How often (in interrupts) the wall-clock backstop is read.
const WALL_CHECK_EVERY: u64 = 4096;

/// What one slice of app CPU may use.
#[derive(Clone, Copy, Debug)]
pub struct Allowance {
    /// Interrupts (Lua calls and loop iterations) between two host awaits.
    pub ticks: u64,
    /// Wall-clock time between two host awaits, whatever the count says.
    pub wall: Duration,
}

#[derive(Clone)]
pub struct Slice {
    base: Instant,
    ticks: Arc<AtomicU64>,
    started_ns: Arc<AtomicU64>,
    killed: Arc<AtomicBool>,
    /// The app is waiting on the host: its CPU clock is stopped.
    paused: Arc<AtomicBool>,
}

impl Slice {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            ticks: Arc::default(),
            started_ns: Arc::default(),
            killed: Arc::default(),
            paused: Arc::default(),
        }
    }
    fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    /// A host call returned (or a call started): the app gets a fresh slice.
    pub fn reset(&self) {
        self.ticks.store(0, Ordering::Relaxed);
        self.started_ns.store(self.now_ns(), Ordering::Relaxed);
    }
    /// The app is about to wait on the host: waiting is not app CPU, so the budget stops (the interrupt also
    /// runs when the coroutine is resumed to be polled, which must not count either).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    /// The host call returned: the app gets a fresh slice.
    pub fn resume(&self) {
        self.reset();
        self.paused.store(false, Ordering::Relaxed);
    }
    /// A new top-level call: fresh slice and the previous kill is forgotten.
    pub fn begin(&self) {
        self.killed.store(false, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        self.reset();
    }
    fn wall_elapsed(&self) -> Duration {
        Duration::from_nanos(self.now_ns().saturating_sub(self.started_ns.load(Ordering::Relaxed)))
    }
    /// One interrupt: has the budget run out? Once it has, it stays out until `begin`, so a `pcall` cannot
    /// swallow the abort.
    pub fn exceeded(&self, allowance: Allowance) -> bool {
        // while the app waits on the host nothing aborts it (that would drop the host call half done and
        // leak what it holds, such as an open transaction); the kill lands when the call returns
        if self.paused.load(Ordering::Relaxed) {
            return false;
        }
        if self.killed.load(Ordering::Relaxed) {
            return true;
        }
        let ticks = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
        let out_of_ticks = ticks > allowance.ticks;
        let out_of_time = ticks.is_multiple_of(WALL_CHECK_EVERY) && self.wall_elapsed() > allowance.wall;
        if out_of_ticks || out_of_time {
            self.killed.store(true, Ordering::Relaxed);
        }
        out_of_ticks || out_of_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowance(ticks: u64) -> Allowance {
        Allowance { ticks, wall: Duration::from_secs(3600) }
    }

    #[test]
    fn counts_interrupts_and_stays_exceeded_until_begin() {
        let slice = Slice::new();
        slice.begin();
        assert!((0..10).all(|_| !slice.exceeded(allowance(10))));
        assert!(slice.exceeded(allowance(10)));
        assert!(slice.exceeded(allowance(10)), "sticky");
        slice.begin();
        assert!(!slice.exceeded(allowance(10)));
    }

    #[test]
    fn waiting_on_the_host_costs_nothing_and_a_resume_refills_the_slice() {
        let slice = Slice::new();
        slice.begin();
        assert!((0..9).all(|_| !slice.exceeded(allowance(10))));
        slice.pause();
        assert!((0..1000).all(|_| !slice.exceeded(allowance(10))), "paused interrupts are free");
        slice.resume();
        assert!((0..10).all(|_| !slice.exceeded(allowance(10))));
    }

    #[test]
    fn real_time_passing_does_not_spend_the_budget() {
        let slice = Slice::new();
        slice.begin();
        std::thread::sleep(Duration::from_millis(50)); // a descheduled or busy host
        let tight = Allowance { ticks: 100, wall: Duration::from_secs(3600) };
        assert!((0..100).all(|_| !slice.exceeded(tight)));
    }

    #[test]
    fn the_wall_clock_backstop_still_ends_a_slice_that_runs_too_long() {
        let slice = Slice::new();
        slice.begin();
        std::thread::sleep(Duration::from_millis(20));
        let wall = Allowance { ticks: u64::MAX, wall: Duration::from_millis(5) };
        let killed = (0..WALL_CHECK_EVERY).any(|_| slice.exceeded(wall));
        assert!(killed);
    }
}

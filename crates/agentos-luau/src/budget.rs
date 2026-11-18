//! The CPU budget, counted in Luau interrupts rather than wall-clock time.
//!
//! The VM's interrupt runs at every Lua function call and every loop back-edge, so the number of interrupts
//! since the app was last handed control is a deterministic measure of the work it has done: it does not
//! change when the machine is busy, when the process is descheduled or when the host sleeps. A loaded
//! machine makes a runaway script take longer to hit the budget, never an honest one hit it.
//!
//! There are two budgets. A *slice* is the work between two host awaits and is refilled by every host call.
//! A *call* is the whole top-level call (a tool, an event, a render); host calls never refill it, so a loop
//! that does nothing but call the host cannot run forever. A wall-clock backstop sits behind each count,
//! checked only every few thousand interrupts so the check stays cheap (and at every host call).
//!
//! An unload retires the app: the interrupt, every host call and a host call in flight all stop, whatever is
//! left of the budgets.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// How often (in interrupts) the wall-clock backstops are read.
const WALL_CHECK_EVERY: u64 = 4096;

/// What a host call (a database statement, a transaction step) costs against the call's tick budget, on top
/// of the interrupts the app spent getting there. A host call is far dearer than a Lua function call, and
/// charging it keeps a loop of cheap host calls (`while true do app.kv:get("k") end`) from outliving the
/// budget just because each iteration is only a few interrupts.
pub const HOST_CALL_TICKS: u64 = 1_000;

/// What CPU an app may use: one slice (between two host awaits) and one whole top-level call.
#[derive(Clone, Copy, Debug)]
pub struct Allowance {
    /// Interrupts (Lua calls and loop iterations) between two host awaits.
    pub ticks: u64,
    /// Wall-clock time between two host awaits, whatever the count says.
    pub wall: Duration,
    /// Interrupts (plus [`HOST_CALL_TICKS`] per host call) in one top-level call. Host calls refill the slice
    /// but never this.
    pub call_ticks: u64,
    /// Wall-clock time of one top-level call, host waits included.
    pub call_wall: Duration,
}

/// Why the app must stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A budget ran out (a slice, or the whole call).
    Budget,
    /// The app was unloaded: nothing it is doing matters any more.
    Unloaded,
}

impl Stop {
    pub fn message(self) -> &'static str {
        match self {
            Self::Budget => "cpu budget exceeded",
            Self::Unloaded => "the app was unloaded",
        }
    }
}

#[derive(Clone)]
pub struct Slice {
    base: Instant,
    ticks: Arc<AtomicU64>,
    started_ns: Arc<AtomicU64>,
    call_ticks: Arc<AtomicU64>,
    call_started_ns: Arc<AtomicU64>,
    killed: Arc<AtomicBool>,
    /// The app is waiting on the host: its CPU clock is stopped.
    paused: Arc<AtomicBool>,
    /// Set once by unload and never cleared: the interrupt, `host()` and a waiting host call all stop.
    retired: Arc<AtomicBool>,
    retired_signal: Arc<Notify>,
}

impl Slice {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            ticks: Arc::default(),
            started_ns: Arc::default(),
            call_ticks: Arc::default(),
            call_started_ns: Arc::default(),
            killed: Arc::default(),
            paused: Arc::default(),
            retired: Arc::default(),
            retired_signal: Arc::default(),
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
    /// The app is about to wait on the host: waiting is not app CPU, so the slice stops (the interrupt also
    /// runs when the coroutine is resumed to be polled, which must not count either).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }
    /// The host call returned: the app gets a fresh slice (the call's own budget is not refilled).
    pub fn resume(&self) {
        self.reset();
        self.paused.store(false, Ordering::Relaxed);
    }
    /// A new top-level call: fresh slice, fresh call budget, and the previous kill is forgotten (an unload is
    /// not: a retired app stays retired).
    pub fn begin(&self) {
        self.killed.store(false, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        self.call_ticks.store(0, Ordering::Relaxed);
        self.call_started_ns.store(self.now_ns(), Ordering::Relaxed);
        self.reset();
    }
    /// The app is being unloaded: whatever it is running stops at its next interrupt or host call, and a
    /// host call it is waiting on is abandoned.
    pub fn retire(&self) {
        self.retired.store(true, Ordering::SeqCst);
        self.retired_signal.notify_waiters();
    }
    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }
    /// Resolves once the app is retired.
    pub async fn retired(&self) {
        loop {
            // created before the check: a retire between the two still wakes it
            let signalled = self.retired_signal.notified();
            if self.is_retired() {
                return;
            }
            signalled.await;
        }
    }
    fn wall_elapsed(&self) -> Duration {
        Duration::from_nanos(self.now_ns().saturating_sub(self.started_ns.load(Ordering::Relaxed)))
    }
    fn call_elapsed(&self) -> Duration {
        Duration::from_nanos(self.now_ns().saturating_sub(self.call_started_ns.load(Ordering::Relaxed)))
    }
    /// Time left of the call's wall-clock budget.
    pub fn call_wall_left(&self, allowance: Allowance) -> Duration {
        allowance.call_wall.saturating_sub(self.call_elapsed())
    }
    /// One interrupt: has a budget run out, or was the app unloaded? A budget stays out until `begin`, so a
    /// `pcall` cannot swallow the abort.
    pub fn exceeded(&self, allowance: Allowance) -> Option<Stop> {
        if self.is_retired() {
            return Some(Stop::Unloaded);
        }
        // while the app waits on the host nothing aborts it (that would drop the host call half done and
        // leak what it holds, such as an open transaction); the kill lands when the call returns
        if self.paused.load(Ordering::Relaxed) {
            return None;
        }
        if self.killed.load(Ordering::Relaxed) {
            return Some(Stop::Budget);
        }
        let ticks = self.ticks.fetch_add(1, Ordering::Relaxed) + 1;
        let call_ticks = self.call_ticks.fetch_add(1, Ordering::Relaxed) + 1;
        let out_of_ticks = ticks > allowance.ticks || call_ticks > allowance.call_ticks;
        let out_of_time = (ticks.is_multiple_of(WALL_CHECK_EVERY) && self.wall_elapsed() > allowance.wall)
            || (call_ticks.is_multiple_of(WALL_CHECK_EVERY) && self.call_elapsed() > allowance.call_wall);
        self.verdict(out_of_ticks || out_of_time)
    }
    /// A host call is about to start: charge it, and refuse it when the app was unloaded or the call's
    /// budget (ticks or wall-clock) is spent. The interrupt reads the wall clock only now and then, so a
    /// loop that does nothing but host calls is caught here.
    pub fn charge_host_call(&self, allowance: Allowance) -> Result<(), Stop> {
        if self.is_retired() {
            return Err(Stop::Unloaded);
        }
        if self.killed.load(Ordering::Relaxed) {
            return Err(Stop::Budget);
        }
        let call_ticks = self.call_ticks.fetch_add(HOST_CALL_TICKS, Ordering::Relaxed) + HOST_CALL_TICKS;
        let spent = call_ticks > allowance.call_ticks || self.call_elapsed() > allowance.call_wall;
        self.verdict(spent).map_or(Ok(()), Err)
    }
    /// A host call returned: did the unload or the call's wall-clock budget land while it ran?
    pub fn check_after_host_call(&self, allowance: Allowance) -> Result<(), Stop> {
        if self.is_retired() {
            return Err(Stop::Unloaded);
        }
        self.verdict(self.call_elapsed() > allowance.call_wall).map_or(Ok(()), Err)
    }
    fn verdict(&self, out: bool) -> Option<Stop> {
        if out {
            self.killed.store(true, Ordering::Relaxed);
            Some(Stop::Budget)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowance(ticks: u64) -> Allowance {
        Allowance {
            ticks,
            wall: Duration::from_secs(3600),
            call_ticks: u64::MAX / 2,
            call_wall: Duration::from_secs(3600),
        }
    }

    fn exceeded(slice: &Slice, a: Allowance) -> bool {
        slice.exceeded(a).is_some()
    }

    #[test]
    fn counts_interrupts_and_stays_exceeded_until_begin() {
        let slice = Slice::new();
        slice.begin();
        assert!((0..10).all(|_| !exceeded(&slice, allowance(10))));
        assert!(exceeded(&slice, allowance(10)));
        assert!(exceeded(&slice, allowance(10)), "sticky");
        slice.begin();
        assert!(!exceeded(&slice, allowance(10)));
    }

    #[test]
    fn waiting_on_the_host_costs_nothing_and_a_resume_refills_the_slice() {
        let slice = Slice::new();
        slice.begin();
        assert!((0..9).all(|_| !exceeded(&slice, allowance(10))));
        slice.pause();
        assert!((0..1000).all(|_| !exceeded(&slice, allowance(10))), "paused interrupts are free");
        slice.resume();
        assert!((0..10).all(|_| !exceeded(&slice, allowance(10))));
    }

    #[test]
    fn real_time_passing_does_not_spend_the_budget() {
        let slice = Slice::new();
        slice.begin();
        std::thread::sleep(Duration::from_millis(50)); // a descheduled or busy host
        let tight = Allowance { ticks: 100, ..allowance(100) };
        assert!((0..100).all(|_| !exceeded(&slice, tight)));
    }

    #[test]
    fn the_wall_clock_backstop_still_ends_a_slice_that_runs_too_long() {
        let slice = Slice::new();
        slice.begin();
        std::thread::sleep(Duration::from_millis(20));
        let wall = Allowance { ticks: u64::MAX, wall: Duration::from_millis(5), ..allowance(0) };
        let killed = (0..WALL_CHECK_EVERY).any(|_| exceeded(&slice, wall));
        assert!(killed);
    }

    #[test]
    fn host_calls_refill_the_slice_but_not_the_call_budget() {
        let a = Allowance { call_ticks: 100, ..allowance(1_000_000) };
        let slice = Slice::new();
        slice.begin();
        // 50 interrupts, then a host call refills the slice (not the call), 50 more: the call is spent
        assert!((0..50).all(|_| !exceeded(&slice, a)));
        slice.pause();
        slice.resume();
        assert!((0..50).all(|_| !exceeded(&slice, a)));
        assert!(
            exceeded(&slice, a),
            "the 101st interrupt of the call is over budget however often the slice refilled"
        );
        slice.begin();
        assert!(!exceeded(&slice, a), "a new call has a new budget");
    }

    #[test]
    fn a_host_call_is_charged_and_refused_once_the_call_is_spent() {
        let a = Allowance { call_ticks: 3 * HOST_CALL_TICKS, ..allowance(1_000_000) };
        let slice = Slice::new();
        slice.begin();
        assert!((0..3).all(|_| slice.charge_host_call(a).is_ok()));
        assert_eq!(slice.charge_host_call(a), Err(Stop::Budget));
        assert_eq!(slice.charge_host_call(a), Err(Stop::Budget), "sticky");
        assert!(exceeded(&slice, a), "and the interrupt agrees");
    }

    #[test]
    fn the_call_wall_clock_is_checked_at_every_host_call() {
        let a = Allowance { call_wall: Duration::from_millis(5), ..allowance(1_000_000) };
        let slice = Slice::new();
        slice.begin();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(slice.charge_host_call(a), Err(Stop::Budget));
        assert_eq!(slice.call_wall_left(a), Duration::ZERO);
    }

    #[tokio::test]
    async fn retiring_wakes_whoever_waits_for_it_and_a_late_waiter_does_not_wait() {
        let slice = Slice::new();
        let waiter = tokio::spawn({
            let slice = slice.clone();
            async move { slice.retired().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        slice.retire();
        tokio::time::timeout(Duration::from_secs(2), waiter).await.expect("woken").unwrap();
        tokio::time::timeout(Duration::from_millis(100), slice.retired()).await.expect("already retired");
    }

    #[test]
    fn a_retired_app_stays_stopped_through_begin_and_pause() {
        let slice = Slice::new();
        slice.begin();
        slice.retire();
        assert_eq!(slice.exceeded(allowance(10)), Some(Stop::Unloaded));
        slice.pause();
        assert_eq!(slice.exceeded(allowance(10)), Some(Stop::Unloaded), "even while waiting on the host");
        slice.begin();
        assert_eq!(slice.charge_host_call(allowance(10)), Err(Stop::Unloaded));
        assert_eq!(slice.check_after_host_call(allowance(10)), Err(Stop::Unloaded));
    }
}

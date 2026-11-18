//! The CPU budget, counted in VM instructions rather than wall-clock time.
//!
//! The VM's count hook runs every [`HOOK_INSTRUCTIONS`] Lua VM instructions (a *tick*), so the number of ticks
//! since the app was last handed control is a deterministic measure of the work it has done: it does not
//! change when the machine is busy, when the process is descheduled or when the host sleeps. A loaded
//! machine makes a runaway script take longer to hit the budget, never an honest one hit it. Work the VM
//! cannot count (the pattern matcher, `string.rep`, `table.concat`, ...) is charged by its own size through
//! [`CpuBudget::spend`].
//!
//! There are two budgets. A *slice* is the work between two host awaits and is refilled by every host call;
//! it is not counted while a host call is in flight. A *call* is the whole top-level call (a tool, an event,
//! a render); host calls never refill it and waiting never pauses it, so a loop that does nothing but call the
//! host, or one that spins behind a host call a coroutine left parked, cannot run forever. A wall-clock
//! backstop sits behind each count, read whenever a count crosses a multiple of [`WALL_CHECK_EVERY`] ticks (a
//! hundred microseconds or so of VM work) and at every host call.
//!
//! An unload retires the app (see [`Retirement`]): the hook, every host call and a host call in flight
//! all stop, whatever is left of the budgets.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// How often (in ticks) the wall-clock backstops are read: `Instant::now` is some tens of nanoseconds, a
/// tick is a hundred VM instructions, so this keeps a runaway inside one C library call's reach of the wall.
pub const WALL_CHECK_EVERY: u64 = 256;

/// VM instructions per tick: how often the count hook runs. Small enough that a runaway loop is caught
/// within microseconds, large enough that the hook costs a few percent of an honest script's time.
pub const HOOK_INSTRUCTIONS: u32 = 100;

/// What a host call (a database statement, a transaction step) costs against the call's tick budget, on top
/// of the ticks the app spent getting there. A host call is far dearer than a Lua function call, and
/// charging it keeps a loop of cheap host calls (`while true do app.kv:get("k") end`) from outliving the
/// budget just because each iteration is only a few ticks.
pub const HOST_CALL_TICKS: u64 = 100;

/// What CPU an app may use: one slice (between two host awaits) and one whole top-level call.
#[derive(Clone, Copy, Debug)]
pub struct Allowance {
    /// Ticks ([`HOOK_INSTRUCTIONS`] VM instructions each) between two host awaits.
    pub ticks: u64,
    /// Wall-clock time between two host awaits, whatever the count says.
    pub wall: Duration,
    /// Ticks (plus [`HOST_CALL_TICKS`] per host call) in one top-level call. Host calls refill the slice
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

/// The unload signal of one app: set once and never cleared. The interrupt, every host call and a host call in
/// flight all stop at it, whatever is left of the budgets.
#[derive(Clone, Default)]
pub struct Retirement {
    retired: Arc<AtomicBool>,
    signal: Arc<Notify>,
}

impl Retirement {
    /// The app is being unloaded: whatever it is running stops at its next tick or host call, and a
    /// host call it is waiting on is abandoned.
    pub fn retire(&self) {
        self.retired.store(true, Ordering::SeqCst);
        self.signal.notify_waiters();
    }
    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }
    /// Resolves once the app is retired.
    pub async fn retired(&self) {
        loop {
            // created before the check: a retire between the two still wakes it
            let signalled = self.signal.notified();
            if self.is_retired() {
                return;
            }
            signalled.await;
        }
    }
}

/// The CPU an app has used: the slice since it was last handed control, the whole top-level call, and whether
/// either ran out (the *spent latch*, which only [`CpuBudget::begin`] clears). Cheap to clone: every clone
/// counts into the same budget (the hook holds one, the host calls another).
#[derive(Clone)]
pub struct CpuBudget {
    base: Instant,
    ticks: Arc<AtomicU64>,
    started_ns: Arc<AtomicU64>,
    call_ticks: Arc<AtomicU64>,
    call_started_ns: Arc<AtomicU64>,
    /// A budget ran out and the call stays out of it until the next [`CpuBudget::begin`].
    spent: Arc<AtomicBool>,
    /// Host calls in flight: while one is, the app is waiting and its *slice* stops. The whole call's budget
    /// never does.
    waiting: Arc<AtomicUsize>,
    /// Counts the top-level calls, so a wait that outlives its call does not release the next call's.
    epoch: Arc<AtomicU64>,
    retirement: Retirement,
}

/// A host call is in flight. Dropping it (the call returned, or its future was dropped half done) ends the
/// wait and gives the app a fresh slice.
pub struct HostWait {
    budget: CpuBudget,
    epoch: u64,
}

impl Drop for HostWait {
    fn drop(&mut self) {
        if self.budget.epoch.load(Ordering::SeqCst) == self.epoch {
            // a count that `begin` already zeroed stays at zero
            let _ =
                self.budget.waiting.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        }
        self.budget.refill_slice();
    }
}

impl CpuBudget {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            ticks: Arc::default(),
            started_ns: Arc::default(),
            call_ticks: Arc::default(),
            call_started_ns: Arc::default(),
            spent: Arc::default(),
            waiting: Arc::default(),
            epoch: Arc::default(),
            retirement: Retirement::default(),
        }
    }
    pub fn retirement(&self) -> &Retirement {
        &self.retirement
    }
    fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    fn refill_slice(&self) {
        self.ticks.store(0, Ordering::Relaxed);
        self.started_ns.store(self.now_ns(), Ordering::Relaxed);
    }
    /// The app is about to wait on the host: waiting is not app CPU, so the slice stops until the returned
    /// guard is dropped (the hook also runs when the coroutine is resumed to be polled, which must not
    /// count against the slice either).
    pub fn wait_for_host(&self) -> HostWait {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        HostWait { budget: self.clone(), epoch: self.epoch.load(Ordering::SeqCst) }
    }
    /// A new top-level call: fresh slice, fresh call budget, and the previous spent latch and any wait are
    /// forgotten (an unload is not: a retired app stays retired).
    pub fn begin(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.waiting.store(0, Ordering::SeqCst);
        self.spent.store(false, Ordering::Relaxed);
        self.call_ticks.store(0, Ordering::Relaxed);
        self.call_started_ns.store(self.now_ns(), Ordering::Relaxed);
        self.refill_slice();
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
    /// Latch the budget as spent when `out` is true. Once latched, every later interrupt and host call of the
    /// call stops, so a `pcall` cannot swallow the abort.
    fn latch_if(&self, out: bool) -> Option<Stop> {
        if out {
            self.spent.store(true, Ordering::Relaxed);
            Some(Stop::Budget)
        } else {
            None
        }
    }
    /// One tick: has a budget run out, or was the app unloaded? A budget stays out until `begin`.
    pub fn exceeded(&self, allowance: Allowance) -> Option<Stop> {
        self.spend(allowance, 1)
    }
    /// `n` ticks of work were done (the hook's tick, or a library function's size-proportional cost): charge
    /// them, then say whether a budget ran out or the app was unloaded. A budget stays out until `begin`.
    ///
    /// While a host call is in flight the slice is not counted, but the call's budget (ticks and wall clock)
    /// is: an app that parks a host call and then spins still runs into it, and nothing here short-circuits
    /// on the wait.
    pub fn spend(&self, allowance: Allowance, n: u64) -> Option<Stop> {
        if self.retirement.is_retired() {
            return Some(Stop::Unloaded);
        }
        if self.spent.load(Ordering::Relaxed) {
            return Some(Stop::Budget);
        }
        // read the clock when a count crosses a multiple of WALL_CHECK_EVERY
        let crossed = |after: u64| after / WALL_CHECK_EVERY != after.saturating_sub(n) / WALL_CHECK_EVERY;
        let call_ticks = self.call_ticks.fetch_add(n, Ordering::Relaxed).saturating_add(n);
        let call_spent = call_ticks > allowance.call_ticks
            || (crossed(call_ticks) && self.call_elapsed() > allowance.call_wall);
        let slice_spent = self.waiting.load(Ordering::SeqCst) == 0 && {
            let ticks = self.ticks.fetch_add(n, Ordering::Relaxed).saturating_add(n);
            ticks > allowance.ticks || (crossed(ticks) && self.wall_elapsed() > allowance.wall)
        };
        self.latch_if(call_spent || slice_spent)
    }
    /// A host call is about to start: charge it, and refuse it when the app was unloaded or the call's
    /// budget (ticks or wall-clock) is spent. The hook reads the wall clock only now and then, so a
    /// loop that does nothing but host calls is caught here.
    pub fn charge_host_call(&self, allowance: Allowance) -> Result<(), Stop> {
        if self.retirement.is_retired() {
            return Err(Stop::Unloaded);
        }
        if self.spent.load(Ordering::Relaxed) {
            return Err(Stop::Budget);
        }
        let call_ticks = self.call_ticks.fetch_add(HOST_CALL_TICKS, Ordering::Relaxed) + HOST_CALL_TICKS;
        let call_spent = call_ticks > allowance.call_ticks || self.call_elapsed() > allowance.call_wall;
        self.latch_if(call_spent).map_or(Ok(()), Err)
    }
    /// A host call returned: did the unload or the call's wall-clock budget land while it ran?
    pub fn check_after_host_call(&self, allowance: Allowance) -> Result<(), Stop> {
        if self.retirement.is_retired() {
            return Err(Stop::Unloaded);
        }
        self.latch_if(self.call_elapsed() > allowance.call_wall).map_or(Ok(()), Err)
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

    fn exceeded(budget: &CpuBudget, a: Allowance) -> bool {
        budget.exceeded(a).is_some()
    }

    #[test]
    fn counts_interrupts_and_stays_exceeded_until_begin() {
        let budget = CpuBudget::new();
        budget.begin();
        assert!((0..10).all(|_| !exceeded(&budget, allowance(10))));
        assert!(exceeded(&budget, allowance(10)));
        assert!(exceeded(&budget, allowance(10)), "sticky");
        budget.begin();
        assert!(!exceeded(&budget, allowance(10)));
    }

    #[test]
    fn waiting_on_the_host_costs_nothing_and_a_resume_refills_the_slice() {
        let budget = CpuBudget::new();
        budget.begin();
        assert!((0..9).all(|_| !exceeded(&budget, allowance(10))));
        let wait = budget.wait_for_host();
        assert!((0..1000).all(|_| !exceeded(&budget, allowance(10))), "ticks while waiting are free");
        drop(wait);
        assert!((0..10).all(|_| !exceeded(&budget, allowance(10))));
    }

    #[test]
    fn waiting_on_the_host_never_exempts_the_calls_own_budget() {
        // a coroutine parked in a host call leaves a wait open for the rest of the call: spinning behind it
        // is still counted against the call
        let a = Allowance { call_ticks: 1_000, ..allowance(10) };
        let budget = CpuBudget::new();
        budget.begin();
        let _parked = budget.wait_for_host();
        assert!((0..1_000).all(|_| !exceeded(&budget, a)), "the slice (10) is not counted while waiting");
        assert_eq!(budget.exceeded(a), Some(Stop::Budget), "the call (1,000) is");
        assert_eq!(budget.exceeded(a), Some(Stop::Budget), "and stays spent");
    }

    #[test]
    fn several_host_calls_in_flight_keep_the_slice_stopped_until_the_last_returns() {
        let budget = CpuBudget::new();
        budget.begin();
        let (first, second) = (budget.wait_for_host(), budget.wait_for_host());
        drop(first);
        assert!((0..100).all(|_| !exceeded(&budget, allowance(10))), "one is still in flight");
        drop(second);
        assert!((0..10).all(|_| !exceeded(&budget, allowance(10))));
        assert!(exceeded(&budget, allowance(10)), "nothing is in flight: the slice counts again");
    }

    #[test]
    fn a_wait_dropped_half_done_does_not_leave_the_slice_stopped() {
        let budget = CpuBudget::new();
        budget.begin();
        drop(budget.wait_for_host()); // the host future was cancelled
        assert!((0..10).all(|_| !exceeded(&budget, allowance(10))));
        assert!(exceeded(&budget, allowance(10)));
    }

    #[test]
    fn a_wait_left_over_from_an_earlier_call_does_not_release_the_next_calls() {
        let budget = CpuBudget::new();
        budget.begin();
        let stale = budget.wait_for_host(); // parked in call 1, dropped during call 2
        budget.begin();
        let current = budget.wait_for_host();
        drop(stale);
        assert!((0..100).all(|_| !exceeded(&budget, allowance(10))), "call 2's own wait is still open");
        drop(current);
        assert!((0..10).all(|_| !exceeded(&budget, allowance(10))));
        assert!(exceeded(&budget, allowance(10)));
    }

    #[test]
    fn real_time_passing_does_not_spend_the_budget() {
        let budget = CpuBudget::new();
        budget.begin();
        std::thread::sleep(Duration::from_millis(50)); // a descheduled or busy host
        let tight = Allowance { ticks: 100, ..allowance(100) };
        assert!((0..100).all(|_| !exceeded(&budget, tight)));
    }

    #[test]
    fn the_wall_clock_backstop_still_ends_a_slice_that_runs_too_long() {
        let budget = CpuBudget::new();
        budget.begin();
        std::thread::sleep(Duration::from_millis(20));
        let wall = Allowance { ticks: u64::MAX, wall: Duration::from_millis(5), ..allowance(0) };
        let killed = (0..WALL_CHECK_EVERY).any(|_| exceeded(&budget, wall));
        assert!(killed);
    }

    #[test]
    fn host_calls_refill_the_slice_but_not_the_call_budget() {
        let a = Allowance { call_ticks: 100, ..allowance(1_000_000) };
        let budget = CpuBudget::new();
        budget.begin();
        // 50 interrupts, then a host call refills the slice (not the call), 50 more: the call is spent
        assert!((0..50).all(|_| !exceeded(&budget, a)));
        drop(budget.wait_for_host());
        assert!((0..50).all(|_| !exceeded(&budget, a)));
        assert!(
            exceeded(&budget, a),
            "the 101st tick of the call is over budget however often the slice refilled"
        );
        budget.begin();
        assert!(!exceeded(&budget, a), "a new call has a new budget");
    }

    #[test]
    fn a_host_call_is_charged_and_refused_once_the_call_is_spent() {
        let a = Allowance { call_ticks: 3 * HOST_CALL_TICKS, ..allowance(1_000_000) };
        let budget = CpuBudget::new();
        budget.begin();
        assert!((0..3).all(|_| budget.charge_host_call(a).is_ok()));
        assert_eq!(budget.charge_host_call(a), Err(Stop::Budget));
        assert_eq!(budget.charge_host_call(a), Err(Stop::Budget), "sticky");
        assert!(exceeded(&budget, a), "and the hook agrees");
    }

    #[test]
    fn the_call_wall_clock_is_checked_at_every_host_call() {
        let a = Allowance { call_wall: Duration::from_millis(5), ..allowance(1_000_000) };
        let budget = CpuBudget::new();
        budget.begin();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(budget.charge_host_call(a), Err(Stop::Budget));
        assert_eq!(budget.call_wall_left(a), Duration::ZERO);
    }

    #[tokio::test]
    async fn retiring_wakes_whoever_waits_for_it_and_a_late_waiter_does_not_wait() {
        let budget = CpuBudget::new();
        let retirement = budget.retirement().clone();
        let waiter = tokio::spawn({
            let retirement = retirement.clone();
            async move { retirement.retired().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        retirement.retire();
        tokio::time::timeout(Duration::from_secs(2), waiter).await.expect("woken").unwrap();
        tokio::time::timeout(Duration::from_millis(100), retirement.retired())
            .await
            .expect("already retired");
    }

    #[test]
    fn a_retired_app_stays_stopped_through_begin_and_pause() {
        let budget = CpuBudget::new();
        budget.begin();
        budget.retirement().retire();
        assert_eq!(budget.exceeded(allowance(10)), Some(Stop::Unloaded));
        let _wait = budget.wait_for_host();
        assert_eq!(budget.exceeded(allowance(10)), Some(Stop::Unloaded), "even while waiting on the host");
        budget.begin();
        assert_eq!(budget.charge_host_call(allowance(10)), Err(Stop::Unloaded));
        assert_eq!(budget.check_after_host_call(allowance(10)), Err(Stop::Unloaded));
    }
}

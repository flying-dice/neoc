//! The CPU budget: a monotonic "last time the VM was handed control" marker read by the Luau interrupt.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Slice {
    base: Instant,
    started_ns: Arc<AtomicU64>,
    killed: Arc<AtomicBool>,
    /// The app is waiting on the host: its CPU clock is stopped.
    paused: Arc<AtomicBool>,
}

impl Slice {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            started_ns: Arc::new(AtomicU64::new(0)),
            killed: Arc::default(),
            paused: Arc::default(),
        }
    }
    fn now_ns(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
    /// A host call returned (or a call started): the app gets a fresh slice.
    pub fn reset(&self) {
        self.started_ns.store(self.now_ns(), Ordering::Relaxed);
    }
    /// The app is about to wait on the host: waiting is not app CPU, so the clock stops (the interrupt also
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
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.now_ns().saturating_sub(self.started_ns.load(Ordering::Relaxed)))
    }
    /// Has the budget run out? Once it has, it stays out until `begin`, so a `pcall` cannot swallow the abort.
    pub fn exceeded(&self, max: Duration) -> bool {
        // while the app waits on the host nothing aborts it (that would drop the host call half done and
        // leak what it holds, such as an open transaction); the kill lands when the call returns
        if self.paused.load(Ordering::Relaxed) {
            return false;
        }
        if self.killed.load(Ordering::Relaxed) || self.elapsed() > max {
            self.killed.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

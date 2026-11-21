//! The operating-system implementations of the small infrastructure ports: wall clock and secure randomness.

use vos_application::ports::{Background, Clock, RandomSource, TaskHandle};
use std::{
    future::Future,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// The OS random number generator.
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill(&self, buf: &mut [u8]) {
        if let Err(e) = getrandom::fill(buf) {
            // Pairing codes and session tokens must never be predictable: if the OS cannot give us
            // randomness there is nothing safe to continue with.
            tracing::error!(error = %e, "the operating system random number generator failed");
            std::process::abort();
        }
    }
}

/// Runs background work (the live-source pumps) on the tokio runtime; dropping the handle aborts the task.
pub struct TokioBackground;

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl TaskHandle for AbortOnDrop {}
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Background for TokioBackground {
    fn spawn(
        &self,
        _name: &'static str,
        task: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Box<dyn TaskHandle> {
        Box::new(AbortOnDrop(tokio::spawn(task)))
    }
}

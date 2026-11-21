//! The operating-system implementations of the small infrastructure ports: wall clock and secure randomness.

use std::{
    future::Future,
    pin::Pin,
    time::{SystemTime, UNIX_EPOCH},
};
use vos_application::ports::{Background, Clock, RandomSource, TaskHandle};

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
        name: &'static str,
        task: Pin<Box<dyn Future<Output = ()> + Send>>,
    ) -> Box<dyn TaskHandle> {
        Box::new(AbortOnDrop(tokio::spawn(logged(name, task))))
    }
}

/// Run `work` (a background task, or one step of one of the host's loops); a panic in it is logged at error level
/// under `name` instead of unwinding further, so a loop goes on to its next step and a task does not end unseen.
/// The background tasks catch a panic in their own steps first (`pump_changes` goes on, an agent turn ends its run
/// with an error), so for them this is the last net.
pub(crate) async fn logged(name: &'static str, work: impl Future<Output = ()>) {
    if let Err(message) = vos_application::panics::caught(work).await {
        tracing::error!(task = name, %message, "host work panicked");
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn work_that_panics_ends_logged_without_unwinding_further() {
        // the join would be a panic error if the panic had escaped `logged`
        let joined = tokio::spawn(super::logged("probe", async { panic!("the task broke") })).await;
        assert!(joined.is_ok(), "{joined:?}");
    }
}

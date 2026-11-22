//! A panic in host code that runs for a long time (a pump, a connection's request loop, an agent turn), caught
//! where it starts so it ends one step of the work, not the work.
//!
//! The host unwinds on panic (NEOC-125): a panic ends only the tokio task it happens in. For a task that serves
//! for as long as the host or an app runs, that would leave something that looks alive and is not (a window that
//! no longer answers, notifications that stop, a chat that stays "running"), so each such task catches a panic in
//! one step, logs it at error level and carries on, or ends what it was doing with an error the person sees.
//! (A panic inside a call into an app's VM is caught by the Lua adapter; this is for the host's own code.)

use futures_util::FutureExt as _;
use std::{any::Any, future::Future, panic::AssertUnwindSafe};

/// The text of a panic's payload: the message of `panic!("...")`, or a placeholder for any other payload.
#[must_use]
pub fn message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// Poll `future` to its end; a panic in it ends it early with the panic's [`message`].
///
/// `AssertUnwindSafe`: what the future held is dropped once the panic is caught. The callers hold their shared
/// state in `parking_lot` locks (no poisoning) and treat the step that panicked as failed.
///
/// # Errors
/// The panic's message, when `future` panicked.
pub async fn caught<F: Future>(future: F) -> Result<F::Output, String> {
    AssertUnwindSafe(future).catch_unwind().await.map_err(|payload| message(&*payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_future_that_panics_ends_with_its_message_and_one_that_does_not_with_its_value() {
        let panicked: Result<(), String> = caught(async { panic!("broke") }).await;
        assert_eq!(panicked, Err("broke".to_owned()));
        let other: Result<(), String> = caught(async { std::panic::panic_any(7_u8) }).await;
        assert_eq!(other, Err("a panic with no message".to_owned()));
        assert_eq!(caught(async { 3 }).await, Ok(3));
    }
}

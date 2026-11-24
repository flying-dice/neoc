//! A panic in a host function, ended as an error of the call that reached it.
//!
//! mlua catches a panic in a host function and raises it in the script as an error, which `pcall` catches. When
//! that error reaches Rust instead (the call into the VM returns it, or a returned value carries it), mlua resumes
//! the panic on the thread that made the call. A place that drives script code from Rust with nobody above it to
//! report to (a `std:task` task, a `std:thread` worker, a `vnd:hyper` request handler, an embedder's own call)
//! ends the call with an error through [`caught`] instead, so one panicking function fails one call, not the task,
//! connection or program that polled it.
//!
//! Unwinding stops the call where script code called into Rust. That is a point where the call can already be
//! dropped (its caller gave up), so what it leaves behind is what a dropped call leaves: the guards of the call
//! (its turn, the leaves it was waiting at) run as they do then. What the panicking function changed before it
//! panicked is not undone.
//!
//! All of this needs `panic = "unwind"`, Rust's default: under `panic = "abort"` the first panic ends the process.
//! The panic hook runs before anything here sees the panic, so its message and place still reach stderr (or the
//! program's own hook).

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};

use futures::FutureExt as _;

/// The text of a panic's payload: the message of `panic!("...")`, or a placeholder for any other payload.
#[must_use]
pub fn message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

/// The error a call ends with when a host function it reached panicked with `message`.
#[must_use]
pub fn error(message: &str) -> mlua::Error {
    mlua::Error::runtime(format!("a host function panicked: {message}"))
}

/// Poll `future` to its end; a panic in it ends it early with the panic's [`message`].
///
/// `AssertUnwindSafe`: what the future held is dropped once the panic is caught, and the engine stays usable (see
/// the module docs for what a caught panic leaves behind).
///
/// # Errors
/// The panic's message, when `future` panicked.
pub async fn caught<F: Future>(future: F) -> Result<F::Output, String> {
    AssertUnwindSafe(future).catch_unwind().await.map_err(|payload| message(&*payload))
}

/// Run `f`; a panic in it ends it early with the panic's [`message`] (see [`caught`]).
///
/// # Errors
/// The panic's message, when `f` panicked.
pub fn caught_sync<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| message(&*payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_message_is_its_text_whatever_the_payload() {
        assert_eq!(caught_sync::<()>(|| panic!("plain")), Err("plain".to_owned()));
        let n = 7;
        assert_eq!(caught_sync::<()>(|| panic!("formatted {n}")), Err("formatted 7".to_owned()));
        assert_eq!(
            caught_sync::<()>(|| std::panic::panic_any(42_u8)),
            Err("a panic with no message".to_owned())
        );
        assert_eq!(caught_sync(|| 1), Ok(1));
    }

    #[test]
    fn a_future_that_panics_ends_with_its_message() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(async {
            let out: Result<(), String> = caught(async {
                tokio::task::yield_now().await;
                panic!("after a wait")
            })
            .await;
            assert_eq!(out, Err("after a wait".to_owned()));
            assert_eq!(caught(async { 2 }).await, Ok(2));
        });
    }
}

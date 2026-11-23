//! How a script learns that a process it started has ended: what `std:process` has, and what a terminal
//! (neoc-vm's `vnd:portable_pty`) will share with it.
//!
//! One watcher (a task, or a thread) owns the process and publishes how it ended, once, on a watch channel; every
//! `wait()` of the script reads that channel, so it may be called again and the answer is the same.
//!
//! Ported from neoc-vm's `child_exit.rs`, as it was.

use tokio::sync::watch;

/// What the watcher of a process publishes: `None` while the process runs, then how it ended, or why the operating
/// system did not say.
pub(crate) type Ended<T> = Option<Result<T, String>>;

/// Why a wait has no exit status to give.
pub(crate) enum WaitFailed {
    /// The operating system did not say how the process ended, in its words.
    Io(String),
    /// The watcher stopped without publishing anything.
    Lost,
}

/// Wait until the watcher behind `exit` says how the process ended.
pub(crate) async fn ended<T: Clone>(mut exit: watch::Receiver<Ended<T>>) -> Result<T, WaitFailed> {
    match exit.wait_for(Option::is_some).await {
        Ok(ended) => match ended.clone() {
            Some(Ok(exit)) => Ok(exit),
            Some(Err(message)) => Err(WaitFailed::Io(message)),
            None => Err(WaitFailed::Lost),
        },
        Err(_) => Err(WaitFailed::Lost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_wait_gives_what_the_watcher_published_as_often_as_it_is_asked() {
        let (published, exit) = watch::channel::<Ended<u8>>(None);
        let waiting = tokio::spawn(ended(exit.clone()));
        published.send_replace(Some(Ok(7)));
        assert!(matches!(waiting.await, Ok(Ok(7))));
        assert!(matches!(ended(exit).await, Ok(7)));

        let (published, exit) = watch::channel::<Ended<u8>>(None);
        published.send_replace(Some(Err("no such process".to_owned())));
        assert!(
            matches!(ended(exit).await, Err(WaitFailed::Io(message)) if message == "no such process")
        );
    }

    #[tokio::test]
    async fn a_watcher_that_is_gone_without_a_word_is_a_lost_wait() {
        let (published, exit) = watch::channel::<Ended<u8>>(None);
        drop(published);
        assert!(matches!(ended(exit).await, Err(WaitFailed::Lost)));
    }
}

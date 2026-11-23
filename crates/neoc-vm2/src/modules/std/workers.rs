//! Process-level coordination: `std:workers`.
//!
//! `workers.on_shutdown()` is the signal the program fires when it is asked to stop (Ctrl-C), so that what runs
//! for ever (a server's `wait`) can end cleanly.
//!
//! ```lua
//! local workers = require("std:workers")
//! workers.on_shutdown():wait() -- until Ctrl-C
//! ```
//!
//! The program that embeds the runtime fires it: `ShutdownSignal::current().fire()`.
//!
//! Ported from neoc-vm's `std:workers`. What changed on purpose: the deprecated alias `shutdown_signal()` is gone
//! (it was `on_shutdown()` under an older name).

#[mlua_ir::module("std:workers")]
pub mod workers {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};

    use mlua_ir::Object;
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Inner {
        notify: Notify,
        fired: AtomicBool,
    }

    /// A signal that fires once and stays fired. Copies of the handle are the same signal.
    #[derive(Clone, Default, Object)]
    pub struct ShutdownSignal {
        inner: Arc<Inner>,
    }

    impl ShutdownSignal {
        /// The signal of this process: the one `on_shutdown()` gives every script.
        pub fn current() -> Self {
            static SIGNAL: OnceLock<ShutdownSignal> = OnceLock::new();
            SIGNAL.get_or_init(Self::default).clone()
        }

        /// Fire the signal: everything waiting on it goes on, and every later wait returns at once.
        pub fn fire(&self) {
            if !self.inner.fired.swap(true, Ordering::SeqCst) {
                self.inner.notify.notify_waiters();
            }
        }
    }

    #[mlua_ir::object]
    impl ShutdownSignal {
        /// Whether the signal has fired.
        pub fn is_fired(&self) -> bool {
            self.inner.fired.load(Ordering::SeqCst)
        }

        /// Wait until the signal fires; at once when it already has.
        pub async fn wait(&self) {
            // the listener is enabled before the flag is read, so a `fire` between the two is not missed
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.is_fired() {
                notified.await;
            }
        }
    }

    /// The signal that fires when the program is asked to stop (Ctrl-C).
    ///
    /// ```luau
    /// local stopping = workers.on_shutdown()
    /// assert(not stopping:is_fired())
    /// ```
    pub fn on_shutdown() -> ShutdownSignal {
        ShutdownSignal::current()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::workers::ShutdownSignal;

    fn engine() -> mlua::Lua {
        let lua = crate::make_engine().expect("engine");
        let workers: mlua::Table = lua.load(r#"return require("std:workers")"#).eval().unwrap();
        lua.globals().set("workers", workers).unwrap();
        lua
    }

    /// The process's own signal is never fired here: other tests of this binary share it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn on_shutdown_gives_the_signal_of_the_process() {
        let lua = engine();
        let (kind, fired, alias): (String, bool, bool) = lua
            .load(
                r#"
                local signal = workers.on_shutdown()
                return type(signal), signal:is_fired() or workers.on_shutdown():is_fired(), workers.shutdown_signal == nil
                "#,
            )
            .eval_async()
            .await
            .unwrap();
        assert_eq!(kind, "userdata");
        assert!(!fired && !ShutdownSignal::current().is_fired());
        assert!(alias, "the deprecated alias is gone");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_wait_ends_when_the_signal_fires_and_at_once_after() {
        let lua = engine();
        let signal = ShutdownSignal::default();
        lua.globals().set("signal", signal.clone()).unwrap();

        let waiting = tokio::spawn(lua.load("signal:wait() return signal:is_fired()").eval_async::<bool>());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiting.is_finished() && !signal.is_fired(), "nothing fired it yet");

        signal.fire();
        let fired = tokio::time::timeout(Duration::from_secs(5), waiting).await.expect("the wait ended");
        assert!(fired.unwrap().unwrap());

        // fired for good: firing again changes nothing, and a later wait does not wait
        signal.fire();
        let again = lua.load("signal:wait() return signal:is_fired()").eval_async::<bool>();
        assert!(tokio::time::timeout(Duration::from_secs(5), again).await.expect("no wait").unwrap());
    }

    /// Every waiter is woken, whichever engine it waits in.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_fire_wakes_every_waiter() {
        let signal = ShutdownSignal::default();
        let waiters: Vec<_> = (0..3)
            .map(|_| {
                let lua = engine();
                lua.globals().set("signal", signal.clone()).unwrap();
                tokio::spawn(async move { lua.load("signal:wait() return true").eval_async::<bool>().await })
            })
            .collect();
        tokio::time::sleep(Duration::from_millis(50)).await;
        signal.fire();
        for waiter in waiters {
            let woken = tokio::time::timeout(Duration::from_secs(5), waiter).await.expect("woken");
            assert!(woken.unwrap().unwrap());
        }
    }
}

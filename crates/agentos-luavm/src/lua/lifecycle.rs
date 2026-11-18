//! What a script starts, and who may run its code.
//!
//! An app can start work that outlives the call that started it: `std:task` coroutines, a `vnd:hyper`
//! server's accept loop and connections, `std:thread` workers. Two things follow, and this module is both
//! of them:
//!
//! - **[`Life`]**, a per-VM registry of everything the script spawned. The embedder calls
//!   [`Life::shutdown`] when the app is unloaded or replaced: every registered task is aborted (which drops
//!   the listeners, pools and Lua threads it held), and nothing registers after that.
//! - **[`CallEntry`]**, the way a Lua callback that nobody called (an HTTP request arriving at a
//!   `vnd:hyper` server, a continuing `std:task` coroutine) takes its turn. The embedder serialises the
//!   calls into one VM; [`inbound`] makes those callbacks queue like any other call. A call is a *baton*:
//!   while it holds it, no other call runs in the VM. [`idle`] is the one place the baton is handed back: a
//!   wait that does not use the VM (a timer, the network, a worker) releases it and takes it again
//!   afterwards, so a `while true do task.wait(1) end` loop does not stop the app, and a script can call its
//!   own listener from a task. Between two waits a call is atomic.
//!
//! Without a [`CallEntry`] (an engine that is not an app's, like a `std:thread` worker's or a test's) the
//! callbacks run directly and [`idle`] is a plain `.await`.

use std::{
    cell::RefCell,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures::future::BoxFuture;
use mlua::Lua;
use parking_lot::Mutex;
use tokio::{sync::watch, task::AbortHandle};

/// The tasks a script spawned, abortable together. Cheap to clone: every clone is the same registry.
#[derive(Clone)]
pub struct Life {
    inner: Arc<LifeInner>,
}

struct LifeInner {
    closed: AtomicBool,
    tasks: Mutex<Vec<AbortHandle>>,
    closed_tx: watch::Sender<bool>,
}

impl Default for Life {
    fn default() -> Self {
        Self::new()
    }
}

impl Life {
    #[must_use]
    pub fn new() -> Self {
        let (closed_tx, _) = watch::channel(false);
        Self { inner: Arc::new(LifeInner { closed: AtomicBool::new(false), tasks: Mutex::new(vec![]), closed_tx }) }
    }

    /// The registry of this engine (made on first use). Workers a script starts share their parent's.
    #[must_use]
    pub fn of(lua: &Lua) -> Self {
        if let Some(life) = lua.app_data_ref::<Self>() {
            return life.clone();
        }
        let life = Self::new();
        lua.set_app_data(life.clone());
        life
    }

    /// Adopt `life` as this engine's registry (a worker joins the registry of the app that started it).
    pub fn adopt(lua: &Lua, life: Self) {
        lua.set_app_data(life);
    }

    /// Spawn on the current tokio runtime and register the task. After [`Life::shutdown`] the task is
    /// aborted at once, so a loop that keeps spawning cannot outlive the app.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let handle = tokio::spawn(future);
        let abort = handle.abort_handle();
        let mut tasks = self.inner.tasks.lock();
        tasks.retain(|t| !t.is_finished());
        if self.inner.closed.load(Ordering::SeqCst) {
            abort.abort();
        } else {
            tasks.push(abort);
        }
        handle
    }

    /// Abort everything registered and refuse anything new. Idempotent.
    pub fn shutdown(&self) {
        let tasks = {
            let mut tasks = self.inner.tasks.lock();
            self.inner.closed.store(true, Ordering::SeqCst);
            std::mem::take(&mut *tasks)
        };
        for task in tasks {
            task.abort();
        }
        self.inner.closed_tx.send_replace(true);
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Resolves when [`Life::shutdown`] has run.
    pub async fn closed(&self) {
        let mut rx = self.inner.closed_tx.subscribe();
        // an error means the sender is gone: this registry is being dropped, which is a close too
        let _ = rx.wait_for(|closed| *closed).await;
    }

    /// Tasks registered and not yet finished.
    #[must_use]
    pub fn live_tasks(&self) -> usize {
        self.inner.tasks.lock().iter().filter(|t| !t.is_finished()).count()
    }
}

/// A call's turn in the VM, held until it is dropped.
pub trait Baton: Send + 'static {
    /// The holder starts a wait that does not use the VM: set the call's own state aside and let others in.
    /// An error means the call cannot wait right now (it says why) and keeps the baton.
    ///
    /// # Errors
    /// The reason the call must not park.
    fn park(&mut self) -> Result<(), String>;

    /// The wait is over: queue for the VM again, and take the call's state back.
    fn unpark(&mut self) -> BoxFuture<'_, ()>;
}

/// The embedder's queue of calls into one VM.
pub trait CallEntry: Send + Sync + 'static {
    /// Wait for the VM to be free and take it.
    fn enter(&self) -> BoxFuture<'static, Box<dyn Baton>>;
}

struct Entry(Arc<dyn CallEntry>);

/// Install the embedder's [`CallEntry`]: from now on [`inbound`] callbacks wait their turn.
pub fn set_entry(lua: &Lua, entry: Arc<dyn CallEntry>) {
    lua.set_app_data(Entry(entry));
}

tokio::task_local! {
    /// The baton of the call running in this tokio task, put aside while it waits (see [`idle`]).
    static HELD: RefCell<Option<Box<dyn Baton>>>;
}

/// Run `future` as the call that holds `baton`; the baton is released when the call ends.
pub async fn scoped<F: Future>(baton: Box<dyn Baton>, future: F) -> F::Output {
    HELD.scope(RefCell::new(Some(baton)), future).await
}

/// Is there a [`CallEntry`] on this engine (an embedder that serialises calls)?
#[must_use]
pub fn has_entry(lua: &Lua) -> bool {
    lua.app_data_ref::<Entry>().is_some()
}

/// Run a Lua callback nobody called (an inbound request, a task's next slice) as a call of its own: it waits
/// for its turn first. Without a [`CallEntry`] it just runs.
pub async fn inbound<F: Future>(lua: &Lua, future: F) -> F::Output {
    let entry = lua.app_data_ref::<Entry>().map(|entry| entry.0.clone());
    match entry {
        Some(entry) => {
            let baton = entry.enter().await;
            scoped(baton, future).await
        }
        None => future.await,
    }
}

/// Wait for `future`, which does not use the VM, without holding the VM: the running call hands its baton
/// back for the wait and queues for it again afterwards. A plain `.await` when this is not a call.
///
/// # Errors
/// The call cannot park (it has an open `db:tx`): the wait is refused and `future` is not polled.
pub async fn idle<F: Future>(future: F) -> mlua::Result<F::Output> {
    let held = HELD.try_with(|slot| slot.borrow_mut().take()).ok().flatten();
    let Some(mut baton) = held else { return Ok(future.await) };
    if let Err(why) = baton.park() {
        let _ = HELD.try_with(|slot| *slot.borrow_mut() = Some(baton));
        return Err(mlua::Error::runtime(why));
    }
    let out = future.await;
    baton.unpark().await;
    let _ = HELD.try_with(|slot| *slot.borrow_mut() = Some(baton));
    Ok(out)
}

/// [`idle`] for a wait whose own result is an `mlua::Result` (every async host function's is).
///
/// # Errors
/// The wait's own error, or the reason the call cannot park.
pub async fn idle_result<T>(future: impl Future<Output = mlua::Result<T>>) -> mlua::Result<T> {
    idle(future).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// A call entry over one tokio mutex, counting how many calls hold it at once.
    struct Counting {
        gate: Arc<tokio::sync::Mutex<()>>,
        inside: Arc<AtomicUsize>,
        most: Arc<AtomicUsize>,
    }

    struct CountingBaton {
        gate: Arc<tokio::sync::Mutex<()>>,
        held: Option<tokio::sync::OwnedMutexGuard<()>>,
        inside: Arc<AtomicUsize>,
        most: Arc<AtomicUsize>,
    }

    impl CountingBaton {
        fn arrive(&self) {
            let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
        }
    }

    impl Baton for CountingBaton {
        fn park(&mut self) -> Result<(), String> {
            self.inside.fetch_sub(1, Ordering::SeqCst);
            self.held = None;
            Ok(())
        }
        fn unpark(&mut self) -> BoxFuture<'_, ()> {
            Box::pin(async move {
                self.held = Some(self.gate.clone().lock_owned().await);
                self.arrive();
            })
        }
    }

    impl Drop for CountingBaton {
        fn drop(&mut self) {
            if self.held.is_some() {
                self.inside.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    impl CallEntry for Counting {
        fn enter(&self) -> BoxFuture<'static, Box<dyn Baton>> {
            let (gate, inside, most) = (self.gate.clone(), self.inside.clone(), self.most.clone());
            Box::pin(async move {
                let held = gate.clone().lock_owned().await;
                let baton = CountingBaton { gate, held: Some(held), inside, most };
                baton.arrive();
                Box::new(baton) as Box<dyn Baton>
            })
        }
    }

    #[tokio::test]
    async fn spawned_tasks_are_aborted_at_shutdown_and_later_spawns_never_run() {
        let life = Life::new();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = life.spawn(async move {
            let _tx = tx; // dropped when the task is dropped
            std::future::pending::<()>().await;
        });
        assert_eq!(life.live_tasks(), 1);
        life.shutdown();
        assert!(task.await.expect_err("aborted").is_cancelled());
        assert!(rx.await.is_err(), "the aborted task's state was dropped");
        assert_eq!(life.live_tasks(), 0);

        let late = life.spawn(std::future::pending::<()>());
        assert!(late.await.expect_err("aborted at once").is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), life.closed()).await.expect("closed resolves");
    }

    #[tokio::test]
    async fn inbound_callbacks_take_turns_and_idle_hands_the_turn_back() {
        let lua = Lua::new();
        let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        set_entry(
            &lua,
            Arc::new(Counting {
                gate: Arc::new(tokio::sync::Mutex::new(())),
                inside: inside.clone(),
                most: most.clone(),
            }),
        );
        // four callbacks that each wait (idle) in the middle: never two inside at once, and the waits overlap
        let started = tokio::time::Instant::now();
        let mut calls = vec![];
        for _ in 0..4 {
            let lua = lua.clone();
            calls.push(tokio::spawn(async move {
                inbound(&lua, async {
                    idle(tokio::time::sleep(Duration::from_millis(100))).await.expect("idle");
                })
                .await;
            }));
        }
        for call in calls {
            call.await.expect("call");
        }
        assert_eq!(most.load(Ordering::SeqCst), 1, "one call at a time");
        assert!(started.elapsed() < Duration::from_millis(350), "the waits ran side by side");
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn without_an_entry_callbacks_run_and_idle_is_a_plain_wait() {
        let lua = Lua::new();
        let out = inbound(&lua, async { idle(async { 7 }).await.expect("idle") }).await;
        assert_eq!(out, 7);
    }
}

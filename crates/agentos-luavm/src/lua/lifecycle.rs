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
//!   **Who lends, and when.** A wait that cannot suspend the call (it was called from a `string.gsub` callback,
//!   a `table.sort` comparator, a metamethod or an iterator function: Lua 5.1 cannot yield across those, and the
//!   wait fails with an error the script may catch) must not lend the baton: the script goes on running, and so
//!   must the restrictions that go with the turn (a `readOnly` tool stays read-only). The VM cannot see whether
//!   Lua managed to yield, so the rule is structural and has three parts:
//!
//!   1. [`idle`] only *asks*: when its wait is `Pending` it marks the call "lend if suspended".
//!   2. [`scoped`] lends when the call's future returns `Pending` *and* the mark is still set. The mark is cleared
//!      at the start of every poll of the call and at the start of every poll of every *leaf*, so it survives
//!      only if the last leaf polled before the `Pending` was an idle wait: a failed wait is forgotten as soon as
//!      anything else runs, and a different leaf's `Pending` (a database query that was left in flight and is
//!      resumed later, say) can never be mistaken for it.
//!   3. Every async host function is a leaf, because it is made by [`async_function`] or [`async_method`] (the
//!      only constructors; clippy's `disallowed-methods` refuses `create_async_function` and `add_async_method`
//!      anywhere else). Its [`Role`] says how it relates to the turn, so no function has to remember to call
//!      [`hold`] or [`idle`] by hand: a [`Role::Vm`] function takes the turn before its body runs, a
//!      [`Role::Wait`] function is an idle wait.
//!
//!   A leaf never carries a baton: [`hold`] looks the baton up in the call that polls it. A host call that was
//!   left in flight (its `resume` failed at a C boundary) and is resumed later, by a different call, therefore
//!   runs in the turn of whoever resumed it, as Lua's own `coroutine.resume` would have it, instead of waiting
//!   for a turn that nobody can give it.
//!
//!   A `std:task` coroutine is the one call that does not start at its beginning: `task.spawn` runs its first
//!   slice inside the spawning call and the rest later, from whichever wait that slice stopped at. Its
//!   continuation therefore starts *detached* ([`resumed`]): it holds nothing until the wait it was stopped
//!   at ends (an [`idle`] wait, or a host call, which takes its turn first) and takes its turn then.
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
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua, UserDataMethods, UserDataRef};
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
    on_shutdown: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
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
        Self {
            inner: Arc::new(LifeInner {
                closed: AtomicBool::new(false),
                tasks: Mutex::new(vec![]),
                on_shutdown: Mutex::new(vec![]),
                closed_tx,
            }),
        }
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

    /// Run `release` when [`Life::shutdown`] runs (at once when it already has): for what a script holds that
    /// dropping the tasks does not free, because a Lua object holds it too (a listener's socket).
    pub fn on_shutdown(&self, release: impl FnOnce() + Send + 'static) {
        let mut releases = self.inner.on_shutdown.lock();
        if self.inner.closed.load(Ordering::SeqCst) {
            drop(releases);
            release();
        } else {
            releases.push(Box::new(release));
        }
    }

    /// Abort everything registered and run the releases, but stay open: the script keeps running and may start
    /// new work. A replacement version that wants the old one's port is loaded after this.
    pub fn stop_background(&self) {
        let (tasks, releases) = {
            let mut tasks = self.inner.tasks.lock();
            let mut releases = self.inner.on_shutdown.lock();
            (std::mem::take(&mut *tasks), std::mem::take(&mut *releases))
        };
        for task in tasks {
            task.abort();
        }
        for release in releases {
            release();
        }
    }

    /// Abort everything registered, run the releases and refuse anything new. Idempotent.
    pub fn shutdown(&self) {
        {
            let _tasks = self.inner.tasks.lock();
            let _releases = self.inner.on_shutdown.lock();
            self.inner.closed.store(true, Ordering::SeqCst);
        }
        self.stop_background();
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

/// What a piece of work a call starts (a `std:task` coroutine, a deferred or delayed function) takes from
/// the call that started it. The VM does not know what the restrictions mean; the embedder does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inherited {
    /// The starting call may not change anything (a tool declared `readOnly`): neither may what it starts,
    /// for as long as that lives.
    pub read_only: bool,
}

/// What a task started *now*, by the call running in this tokio task, inherits. [`Inherited::default`] outside
/// a call (a worker, a test).
#[must_use]
pub fn inherited() -> Inherited {
    HELD.try_with(|slot| slot.borrow().baton.as_ref().map(|baton| baton.inherited()))
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// A call's turn in the VM, held until it is dropped.
pub trait Baton: Send + 'static {
    /// Does this call hold the VM right now? (A parked or detached call does not.)
    fn holding(&self) -> bool;

    /// What the work this call starts inherits from it.
    fn inherited(&self) -> Inherited;

    /// May the holder wait without the VM right now? Asked before the wait starts.
    ///
    /// # Errors
    /// The reason the call must not wait (the wait is refused with it).
    fn can_park(&self) -> Result<(), String>;

    /// The holder is suspended at a wait that does not use the VM: set the call's own state aside and let
    /// others in. Called only after [`Baton::can_park`] agreed and the wait really suspended the call.
    fn park(&mut self);

    /// Queue for the VM (when this call does not hold it), and take the call's state back. [`hold`] polls this
    /// until it is ready; the place in the queue lives in the baton, not in whatever future is polling, so a
    /// host call that is polled by another call later asks that call's baton instead.
    fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}

/// The embedder's queue of calls into one VM.
pub trait CallEntry: Send + Sync + 'static {
    /// Wait for the VM to be free and take it, for work a call started (a deferred or delayed function):
    /// `inherited` is what it takes from the call that started it, and nothing else restricts it.
    fn enter(&self, inherited: Inherited) -> BoxFuture<'static, Box<dyn Baton>>;

    /// Wait for the VM to be free and take it, for a request that arrives from outside (to the app's own
    /// listener). It has no origin to inherit from, so the embedder decides how far to trust it.
    fn request(&self) -> BoxFuture<'static, Box<dyn Baton>>;

    /// A call that exists but holds nothing yet: it takes its turn at its first [`idle`] or [`hold`].
    fn detached(&self, inherited: Inherited) -> Box<dyn Baton>;
}

struct Entry(Arc<dyn CallEntry>);

/// Install the embedder's [`CallEntry`]: from now on [`inbound`] callbacks wait their turn.
pub fn set_entry(lua: &Lua, entry: Arc<dyn CallEntry>) {
    lua.set_app_data(Entry(entry));
}

/// A call's baton and whether its wait wants it lent (see [`idle`]).
struct Slot {
    baton: Option<Box<dyn Baton>>,
    /// The last leaf polled was an idle wait and is pending; the baton is lent if the call suspends. Every poll
    /// of a leaf clears it first, so only the last one counts.
    lend: bool,
}

impl Slot {
    fn new(baton: Option<Box<dyn Baton>>) -> RefCell<Self> {
        RefCell::new(Self { baton, lend: false })
    }
}

tokio::task_local! {
    /// The baton of the call running in this tokio task.
    static HELD: RefCell<Slot>;
}

/// Run `future` as the call that holds `baton`; the baton is released when the call ends. It is lent when the call
/// suspends at a wait that asked for it (see the module docs).
pub async fn scoped<F: Future>(baton: Box<dyn Baton>, future: F) -> F::Output {
    HELD.scope(Slot::new(Some(baton)), async move {
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(move |cx| {
            forget_lend();
            let polled = future.as_mut().poll(cx);
            if polled.is_pending() {
                lend_if_asked();
            }
            polled
        })
        .await
    })
    .await
}

/// Forget that a wait asked for the baton to be lent: called at the start of every poll of a leaf (see [`async_function`]).
fn forget_lend() {
    let _ = HELD.try_with(|slot| slot.borrow_mut().lend = false);
}

/// The call suspended (its future returned `Pending`): if the last leaf it polled was an idle wait, give the VM
/// to the next call.
fn lend_if_asked() {
    let _ = HELD.try_with(|slot| {
        let mut slot = slot.borrow_mut();
        if std::mem::take(&mut slot.lend) {
            if let Some(baton) = slot.baton.as_mut().filter(|b| b.holding()) {
                baton.park();
            }
        }
    });
}

/// Is there a [`CallEntry`] on this engine (an embedder that serialises calls)?
#[must_use]
pub fn has_entry(lua: &Lua) -> bool {
    lua.app_data_ref::<Entry>().is_some()
}

/// Run a Lua callback nobody called (a deferred or delayed function) as a call of its own, with what it takes
/// from the call that started it: it waits for its turn first. Without a [`CallEntry`] it just runs.
pub async fn inbound<F: Future>(lua: &Lua, inherited: Inherited, future: F) -> F::Output {
    let entry = lua.app_data_ref::<Entry>().map(|entry| entry.0.clone());
    match entry {
        Some(entry) => {
            let baton = entry.enter(inherited).await;
            scoped(baton, future).await
        }
        None => future.await,
    }
}

/// Run the handler of a request that arrived from outside (to the app's own listener) as a call of its own:
/// it waits for its turn first. It has no origin; see [`CallEntry::request`]. Without a [`CallEntry`] it
/// just runs.
pub async fn request<F: Future>(lua: &Lua, future: F) -> F::Output {
    let entry = lua.app_data_ref::<Entry>().map(|entry| entry.0.clone());
    match entry {
        Some(entry) => {
            let baton = entry.request().await;
            scoped(baton, future).await
        }
        None => future.await,
    }
}

/// A coroutine's continuation, which starts where its first slice stopped (see the module docs): it runs as
/// a call that holds nothing until the wait it stopped at ends. Without a [`CallEntry`] it just runs.
pub async fn resumed<F: Future>(lua: &Lua, inherited: Inherited, future: F) -> F::Output {
    let detached = lua.app_data_ref::<Entry>().map(|entry| entry.0.detached(inherited));
    match detached {
        Some(baton) => scoped(baton, future).await,
        None => future.await,
    }
}

/// Run the synchronous first slice of a coroutine (`task.spawn`) inside the spawning call, but *not as* that
/// call: a wait it starts keeps a detached baton of its own (see [`resumed`]), so the spawner keeps its turn.
pub fn first_slice<R>(lua: &Lua, inherited: Inherited, slice: impl FnOnce() -> R) -> R {
    let detached = lua.app_data_ref::<Entry>().map(|entry| entry.0.detached(inherited));
    HELD.sync_scope(Slot::new(detached), slice)
}

/// Wait for `future`, which does not use the VM, without holding the VM: when the wait suspends the running
/// call it hands its baton back (see [`scoped`]), and it queues for it again afterwards. A plain `.await` when
/// this is not a call.
///
/// Only for a [`Role::Mixed`] function, which waits for part of its work; a function that is a wait from end to
/// end is a [`Role::Wait`] and needs no call.
///
/// # Errors
/// The call cannot wait (it has an open `db:tx`): the wait is refused and `future` is not polled.
pub(crate) async fn idle<F: Future>(future: F) -> mlua::Result<F::Output> {
    let may_wait =
        HELD.try_with(|slot| slot.borrow().baton.as_ref().filter(|b| b.holding()).map(|b| b.can_park()));
    if let Ok(Some(Err(why))) = may_wait {
        return Err(mlua::Error::runtime(why));
    }
    let mut future = std::pin::pin!(future);
    let out = std::future::poll_fn(|cx| {
        // also cleared by the enclosing leaf, but a `Role::Mixed` body calls `idle` directly
        forget_lend();
        let polled = future.as_mut().poll(cx);
        if polled.is_pending() {
            // lent only if the call suspends: this wait may be one Lua cannot yield from
            let _ = HELD.try_with(|slot| slot.borrow_mut().lend = true);
        }
        polled
    })
    .await;
    hold().await;
    Ok(out)
}

/// Make sure the running call holds the VM before it goes on. A call holds it unless it is parked at a wait,
/// or is a detached coroutine continuation (see [`resumed`]). The baton asked is the one of the call that is
/// polling, which is not necessarily the one that started this future (see the module docs).
pub(crate) async fn hold() {
    std::future::poll_fn(|cx| {
        HELD.try_with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.lend = false;
            match slot.baton.as_mut() {
                Some(baton) if !baton.holding() => baton.poll_unpark(cx),
                _ => Poll::Ready(()),
            }
        })
        .unwrap_or(Poll::Ready(()))
    })
    .await;
}

/// How an async host function relates to the call's turn in the VM (see [`async_function`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// It uses the VM (the app's database, its state): it takes the turn before its body runs, so the body
    /// never has to ask.
    Vm,
    /// It waits for something that does not use the VM (a timer, a socket, a worker): the whole body is an
    /// [`idle`] wait, and the turn is taken again when it ends. Turning the result into Lua values is part of
    /// the body, so it happens as the wait ends, just before the turn is taken again.
    Wait,
    /// It does both in turns: the body calls [`idle`] for its waits (`Mutex:with_lock` waits for the lock, then
    /// runs Lua).
    Mixed,
}

/// Run an async host function's body as the leaf its `role` says.
async fn leaf<R: Send + 'static>(role: Role, body: impl Future<Output = mlua::Result<R>>) -> mlua::Result<R> {
    let mut run = std::pin::pin!(async move {
        match role {
            Role::Vm => {
                hold().await;
                body.await
            }
            Role::Wait => idle(body).await?,
            Role::Mixed => body.await,
        }
    });
    // every poll of the function, the first and each one after a wake, forgets a request to lend made by an
    // earlier leaf in the same poll: only the wait that is pending *now* may ask for it
    std::future::poll_fn(|cx| {
        forget_lend();
        run.as_mut().poll(cx)
    })
    .await
}

/// The one way to make an async host function (a Lua function whose work is a future). Every poll of the
/// function is a leaf poll (see the module docs), and `role` decides how it relates to the turn, so the body
/// does not call [`hold`] or [`idle`] for the function as a whole. Use this, not `Lua::create_async_function`.
///
/// `f` only builds the future; whatever it does before returning runs in the call that invoked the function.
///
/// # Errors
/// mlua could not create the function.
pub fn async_function<A, R, F, Fut>(lua: &Lua, role: Role, f: F) -> mlua::Result<Function>
where
    A: FromLuaMulti,
    R: IntoLuaMulti + Send + 'static,
    F: Fn(Lua, A) -> Fut + Send + 'static,
    Fut: Future<Output = mlua::Result<R>> + Send + 'static,
{
    #[allow(clippy::disallowed_methods)] // the one place
    lua.create_async_function(move |lua, args| leaf(role, f(lua, args)))
}

/// [`async_function`] for a method of a userdata. Use this, not `UserDataMethods::add_async_method`.
pub fn async_method<T, M, A, R, F, Fut>(methods: &mut M, name: &str, role: Role, f: F)
where
    T: 'static,
    M: UserDataMethods<T>,
    A: FromLuaMulti,
    R: IntoLuaMulti + Send + 'static,
    F: Fn(Lua, UserDataRef<T>, A) -> Fut + Send + 'static,
    Fut: Future<Output = mlua::Result<R>> + Send + 'static,
{
    #[allow(clippy::disallowed_methods)] // the one place
    methods.add_async_method(name, move |lua, this, args| leaf(role, f(lua, this, args)));
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
        waiting: Option<BoxFuture<'static, tokio::sync::OwnedMutexGuard<()>>>,
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
        fn holding(&self) -> bool {
            self.held.is_some()
        }
        fn inherited(&self) -> Inherited {
            Inherited::default()
        }
        fn can_park(&self) -> Result<(), String> {
            Ok(())
        }
        fn park(&mut self) {
            self.inside.fetch_sub(1, Ordering::SeqCst);
            self.held = None;
        }
        fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()> {
            if self.held.is_none() {
                let gate = self.gate.clone();
                let waiting = self.waiting.get_or_insert_with(|| Box::pin(gate.lock_owned()));
                let Poll::Ready(guard) = waiting.as_mut().poll(cx) else { return Poll::Pending };
                self.waiting = None;
                self.held = Some(guard);
                self.arrive();
            }
            Poll::Ready(())
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
        fn detached(&self, _: Inherited) -> Box<dyn Baton> {
            Box::new(CountingBaton {
                gate: self.gate.clone(),
                held: None,
                waiting: None,
                inside: self.inside.clone(),
                most: self.most.clone(),
            })
        }
        fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
            self.enter(Inherited::default())
        }
        fn enter(&self, _: Inherited) -> BoxFuture<'static, Box<dyn Baton>> {
            let (gate, inside, most) = (self.gate.clone(), self.inside.clone(), self.most.clone());
            Box::pin(async move {
                let held = gate.clone().lock_owned().await;
                let baton = CountingBaton { gate, held: Some(held), waiting: None, inside, most };
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
        let released = Arc::new(AtomicBool::new(false));
        let flag = released.clone();
        life.on_shutdown(move || flag.store(true, Ordering::SeqCst));
        assert!(released.load(Ordering::SeqCst), "a release registered after shutdown runs at once");
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
                inbound(&lua, Inherited::default(), async {
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
    async fn a_detached_continuation_holds_nothing_until_its_wait_ends_and_the_spawner_keeps_its_turn() {
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
        // the spawner's call holds the VM; the slice it runs starts a wait (the future is created and polled
        // once inside the slice) and the continuation finishes it later, outside the spawner's call
        let mut continuation = None;
        inbound(&lua, Inherited::default(), async {
            let mut wait = Box::pin(idle(tokio::time::sleep(Duration::from_millis(80))));
            first_slice(&lua, Inherited::default(), || {
                let waker = futures::task::noop_waker();
                let mut cx = std::task::Context::from_waker(&waker);
                assert!(std::future::Future::poll(wait.as_mut(), &mut cx).is_pending());
            });
            assert_eq!(inside.load(Ordering::SeqCst), 1, "the spawner still holds its turn");
            let lua = lua.clone();
            continuation = Some(tokio::spawn(async move {
                resumed(&lua, Inherited::default(), async move {
                    wait.await.expect("idle");
                    hold().await;
                })
                .await;
            }));
        })
        .await;
        // the spawner's call is over; the continuation takes its turn only when its wait ended, and gives it
        // back when it is dropped
        continuation.expect("spawned").await.expect("continuation");
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two inside at once");
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    /// A wait that Lua then fails to yield from (a `string.gsub` callback, say) is polled once and abandoned: the
    /// call goes on in the same poll. It must still hold the VM, so nothing else runs inside it.
    #[tokio::test]
    async fn a_wait_that_was_started_but_never_suspended_the_call_does_not_lend_the_turn() {
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
        let other_ran = Arc::new(AtomicBool::new(false));
        let first = {
            let (lua, inside, other_ran) = (lua.clone(), inside.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async {
                    // started, found pending, and abandoned inside this poll: the yield failed
                    {
                        let mut wait = Box::pin(idle(tokio::time::sleep(Duration::from_secs(60))));
                        let waker = futures::task::noop_waker();
                        let mut cx = std::task::Context::from_waker(&waker);
                        assert!(std::future::Future::poll(wait.as_mut(), &mut cx).is_pending());
                    }
                    // the call goes on, and a host call (which yields to the executor) lets others try
                    hold().await;
                    for _ in 0..5 {
                        tokio::task::yield_now().await;
                    }
                    assert_eq!(inside.load(Ordering::SeqCst), 1, "this call still holds the VM");
                    assert!(!other_ran.load(Ordering::SeqCst), "nothing else ran inside it");
                })
                .await;
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = {
            let (lua, other_ran) = (lua.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async { other_ran.store(true, Ordering::SeqCst) }).await;
            })
        };
        first.await.expect("first call");
        second.await.expect("second call");
        assert!(other_ran.load(Ordering::SeqCst), "it ran once the first call ended");
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two inside at once");
    }

    /// A wait that failed to suspend the call leaves its request to lend behind; the next thing the call waits for
    /// is not that wait, so the call must not give up the VM at it (it is still running script code).
    #[tokio::test]
    async fn a_failed_wait_does_not_make_the_next_leafs_pending_lend_the_turn() {
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
        let other_ran = Arc::new(AtomicBool::new(false));
        let first = {
            let (lua, inside, other_ran) = (lua.clone(), inside.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async {
                    // a wait that is pending and abandoned (the yield failed): it asked to lend
                    let mut failed = Box::pin(idle(tokio::time::sleep(Duration::from_secs(60))));
                    {
                        let waker = futures::task::noop_waker();
                        let mut cx = Context::from_waker(&waker);
                        assert!(failed.as_mut().poll(&mut cx).is_pending());
                    }
                    // another function is then pending (a host call left in flight, resumed now) and the call
                    // really suspends at it
                    leaf(Role::Mixed, async {
                        tokio::task::yield_now().await;
                        tokio::task::yield_now().await;
                        assert_eq!(inside.load(Ordering::SeqCst), 1, "still holding while it waits");
                        assert!(!other_ran.load(Ordering::SeqCst), "nothing else ran inside the call");
                        Ok(())
                    })
                    .await
                    .expect("leaf");
                    for _ in 0..5 {
                        tokio::task::yield_now().await;
                    }
                    assert!(!other_ran.load(Ordering::SeqCst), "nothing else ran inside the call");
                })
                .await;
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = {
            let (lua, other_ran) = (lua.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async { other_ran.store(true, Ordering::SeqCst) }).await;
            })
        };
        first.await.expect("first call");
        second.await.expect("second call");
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two inside at once");
    }

    /// A host call started by a detached call (a task's first slice) that is still waiting for its turn when it
    /// is resumed by another call: it asks the baton of the call that resumes it, which holds the VM already.
    #[tokio::test]
    async fn a_hold_left_in_flight_by_a_detached_call_is_finished_by_the_call_that_resumes_it() {
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
        let done = inbound(&lua, Inherited::default(), async {
            let mut waiting = Box::pin(hold());
            first_slice(&lua, Inherited::default(), || {
                let waker = futures::task::noop_waker();
                let mut cx = Context::from_waker(&waker);
                assert!(waiting.as_mut().poll(&mut cx).is_pending(), "the spawner holds the VM");
            });
            tokio::time::timeout(Duration::from_secs(2), waiting).await.is_ok()
        })
        .await;
        assert!(done, "the call that holds the VM finished it instead of waiting for its own turn");
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn without_an_entry_callbacks_run_and_idle_is_a_plain_wait() {
        let lua = Lua::new();
        let out = inbound(&lua, Inherited::default(), async { idle(async { 7 }).await.expect("idle") }).await;
        assert_eq!(out, 7);
    }
}

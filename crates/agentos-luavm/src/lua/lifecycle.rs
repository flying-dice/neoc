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
//!   while it holds it, no other call runs in the VM. [`scoped`] is the one place the baton is handed back: when
//!   the call suspends at a wait that does not use the VM (a timer, the network, a worker: an [`idle`] wait) it
//!   releases the baton and takes it again afterwards, so a `while true do task.wait(1) end` loop does not stop the app, and a script can call its
//!   own listener from a task. Between two waits a call is atomic.
//!
//!   **Who lends, and when.** A wait that cannot suspend the call (it was called from a `string.gsub` callback,
//!   a `table.sort` comparator, a metamethod or an iterator function: Lua 5.1 cannot yield across those, and the
//!   wait fails with an error the script may catch) must not lend the baton: the script goes on running, and so
//!   must the restrictions that go with the turn (a `readOnly` tool stays read-only). The VM cannot see whether
//!   Lua managed to yield, so the rules are structural, and they are checked by a generated test
//!   (`crates/agentos-lua/tests/chaos.rs`):
//!
//!   1. **Every async host function is a leaf.** It is made by [`async_function`] or [`async_method`] (the only
//!      constructors; clippy's `disallowed-methods` refuses `create_async_function` and `add_async_method`
//!      anywhere else). Its [`Role`] says how it relates to the turn, so no function has to remember to call
//!      [`hold`] or [`idle`] by hand: a [`Role::Vm`] function takes the turn before its body runs, a
//!      [`Role::Wait`] function is an idle wait.
//!   2. **A leaf that returns `Pending` is *suspended*,** and the call records it (innermost first, with whether
//!      its wait is idle). mlua's wrapper yields to the executor on the very next instruction, and the executor
//!      resumes into that same leaf: when the call goes on, the next thing it polls is the leaf that suspended
//!      (an outer leaf first, then the one inside it).
//!   3. **A leaf the script ran past is *abandoned*.** If anything else happens instead (another leaf starts
//!      polling, the call ends, the first slice of a task stops somewhere else) the yield failed: Lua refused it at
//!      a C boundary. The leaf's future is dropped on the spot, so whatever it held (a database gate, a mutex
//!      guard, a transaction) is released, and a coroutine the script resumes later finds the leaf gone: its host
//!      call raises "abandoned". A host call that Lua could not suspend is therefore over, not left half-done.
//!   4. **A call lends the baton only when it really suspended at an idle wait:** its future returned `Pending`,
//!      the innermost suspended leaf is an idle wait, the call's waker was *not* woken during that poll (mlua
//!      wakes it itself when the script calls a plain `coroutine.yield()`, which waits for nothing), and the baton
//!      still agrees (no open `db:tx`, not rendering: [`Baton::can_park`], asked at the moment of lending, not
//!      only before the wait began).
//!
//!   A leaf never carries a baton: [`hold`] looks the baton up in the call that polls it. A host call that was
//!   suspended by one call and is resumed by another therefore runs in the turn of whoever resumed it, as Lua's
//!   own `coroutine.resume` would have it, instead of waiting for a turn that nobody can give it.
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
        Arc, Weak,
    },
    task::{Context, Poll},
};

pub use futures::future::BoxFuture;
use futures::task::ArcWake;
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
    /// Which call this is: unique among the calls of the embedder, stable for the call's life.
    fn id(&self) -> u64;

    /// Does this call hold the VM right now? (A parked or detached call does not.)
    fn holding(&self) -> bool;

    /// What the work this call starts inherits from it.
    fn inherited(&self) -> Inherited;

    /// May the holder wait without the VM right now? Asked before an idle wait starts, and again when the call
    /// is about to lend its turn: a wait that was started where it was allowed and finished being polled where
    /// it is not (a `db:tx` was opened meanwhile) must not lend.
    ///
    /// # Errors
    /// The reason the call must not wait (the wait is refused with it).
    fn can_park(&self) -> Result<(), String>;

    /// The holder is suspended at a wait that does not use the VM: set the call's own state aside and let
    /// others in. Called only after [`Baton::can_park`] agreed and the call really suspended at the wait.
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

/// A leaf that returned `Pending` and has not been polled since (see the module docs, rules 2 and 3).
struct Suspended {
    /// Weak: a leaf whose future is dropped (the script's reference to it is collected) is gone at once.
    leaf: Weak<dyn Abandon>,
    /// Its wait does not use the VM: the call may lend its turn while it lasts.
    idle: bool,
}

/// What a suspended leaf lets its caller do when the suspension turns out not to have happened.
trait Abandon: Send + Sync {
    /// Drop the leaf's future now, whatever it holds with it.
    fn abandon(&self);
}

/// A call's baton, and what it is waiting at.
struct Slot {
    baton: Option<Box<dyn Baton>>,
    /// An idle wait is pending in the leaf that is being polled; the leaf records that when it returns `Pending`.
    idle_pending: bool,
    /// The leaves that returned `Pending` since the call last ran script code, innermost first.
    suspended: Vec<Suspended>,
}

impl Slot {
    fn new(baton: Option<Box<dyn Baton>>) -> RefCell<Self> {
        RefCell::new(Self { baton, idle_pending: false, suspended: vec![] })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // the call ends (or its future is dropped): nothing it was waiting at will be polled again. This runs
        // before the baton is dropped, so what the leaves held is free by the time the turn is.
        abandon_all(std::mem::take(&mut self.suspended));
    }
}

fn abandon_all(leaves: Vec<Suspended>) {
    for suspended in leaves {
        if let Some(leaf) = suspended.leaf.upgrade() {
            leaf.abandon();
        }
    }
}

tokio::task_local! {
    /// The baton of the call running in this tokio task.
    static HELD: RefCell<Slot>;
}

/// Run `future` as the call that holds `baton`; the baton is released when the call ends. It is lent when the call
/// suspends at an idle wait (see the module docs, rule 4).
pub async fn scoped<F: Future>(baton: Box<dyn Baton>, future: F) -> F::Output {
    HELD.scope(Slot::new(Some(baton)), async move {
        let mut future = std::pin::pin!(future);
        let proxy = Arc::new(WakeProxy::default());
        std::future::poll_fn(move |cx| {
            proxy.arm(cx.waker());
            let waker = futures::task::waker_ref(&proxy);
            let polled = future.as_mut().poll(&mut Context::from_waker(&waker));
            match &polled {
                Poll::Ready(_) => abandon_all(take_suspended()),
                Poll::Pending => lend_if_suspended_at_idle_wait(proxy.woken()),
            }
            polled
        })
        .await
    })
    .await
}

/// The waker a call is polled with: it forwards to the executor's and remembers that it was woken during a poll.
/// mlua wakes it when the script yields something that is not a wait (`coroutine.yield()` at the top of the
/// call), which is how a plain yield is told from a leaf's suspension.
#[derive(Default)]
struct WakeProxy {
    woken: AtomicBool,
    executor: Mutex<Option<std::task::Waker>>,
}

impl WakeProxy {
    fn arm(&self, executor: &std::task::Waker) {
        self.woken.store(false, Ordering::SeqCst);
        let mut slot = self.executor.lock();
        if !slot.as_ref().is_some_and(|known| known.will_wake(executor)) {
            *slot = Some(executor.clone());
        }
    }

    fn woken(&self) -> bool {
        self.woken.load(Ordering::SeqCst)
    }
}

impl ArcWake for WakeProxy {
    fn wake_by_ref(arc: &Arc<Self>) {
        arc.woken.store(true, Ordering::SeqCst);
        if let Some(executor) = arc.executor.lock().as_ref() {
            executor.wake_by_ref();
        }
    }
}

fn take_suspended() -> Vec<Suspended> {
    HELD.try_with(|slot| std::mem::take(&mut slot.borrow_mut().suspended)).unwrap_or_default()
}

/// A leaf starts a poll (the first, or one after a wake). If it is the one that suspended last, it is being
/// resumed as it should: it leaves the record. Anything else suspended is stale (the script ran on after it):
/// those leaves are abandoned.
fn leaf_polling(me: &Weak<dyn Abandon>) {
    let stale = HELD
        .try_with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.idle_pending = false;
            match slot.suspended.last() {
                Some(last) if Weak::ptr_eq(&last.leaf, me) => {
                    slot.suspended.pop();
                    vec![]
                }
                Some(_) => std::mem::take(&mut slot.suspended),
                None => vec![],
            }
        })
        .unwrap_or_default();
    abandon_all(stale);
}

/// A leaf's poll returned `Pending`: record it as suspended.
fn leaf_suspended(leaf: Weak<dyn Abandon>) {
    let _ = HELD.try_with(|slot| {
        let mut slot = slot.borrow_mut();
        let idle = std::mem::take(&mut slot.idle_pending);
        slot.suspended.push(Suspended { leaf, idle });
    });
}

/// The call's future returned `Pending`. Give the VM to the next call only if the call is waiting at an idle wait
/// (module docs, rule 4); anything else (a database call, a plain yield, a leaf waiting for the VM) keeps it.
fn lend_if_suspended_at_idle_wait(woken: bool) {
    let _ = HELD.try_with(|slot| {
        let mut slot = slot.borrow_mut();
        let waits = !woken && slot.suspended.first().is_some_and(|innermost| innermost.idle);
        if waits {
            if let Some(baton) = slot.baton.as_mut().filter(|b| b.holding() && b.can_park().is_ok()) {
                baton.park();
            }
        }
    });
}

/// The identity of the call running in this tokio task (see [`Baton::id`]); `None` outside a call.
#[must_use]
pub fn turn_id() -> Option<u64> {
    HELD.try_with(|slot| slot.borrow().baton.as_ref().map(|baton| baton.id())).ok().flatten()
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
///
/// `suspended` says whether the slice stopped at a wait, which its continuation finishes later: the leaf it is
/// suspended at stays alive for that. When the slice stopped anywhere else (it ended, failed or yielded plainly)
/// the leaves it ran past are abandoned, as they would be at the end of a call.
pub fn first_slice<R>(
    lua: &Lua,
    inherited: Inherited,
    slice: impl FnOnce() -> R,
    suspended: impl FnOnce(&R) -> bool,
) -> R {
    let detached = lua.app_data_ref::<Entry>().map(|entry| entry.0.detached(inherited));
    let (out, kept, left) = HELD.sync_scope(Slot::new(detached), || {
        let out = slice();
        let kept = suspended(&out);
        (out, kept, take_suspended())
    });
    if !kept {
        abandon_all(left);
    }
    out
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
        let polled = future.as_mut().poll(cx);
        if polled.is_pending() {
            // the leaf records it when its poll ends; the call lends only if it really suspends here
            let _ = HELD.try_with(|slot| slot.borrow_mut().idle_pending = true);
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
            slot.idle_pending = false;
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
///
/// One rule holds for every role: **a body may create Lua values (tables, strings, userdata) at any time, with the
/// turn or without it, but it may only run script code or touch the app's state while the call holds the turn.**
/// Making a value reads and changes nothing the app can see; a `Role::Wait` body therefore builds its result as
/// the wait ends, while the VM may still be lent to another call, and a `Role::Mixed` body that needs the wait's
/// result first builds it after [`idle`] (which has taken the turn back). The conversion of the finished result
/// into Lua values, by mlua, always happens with the turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// It uses the VM (the app's database, its state): it takes the turn before its body runs, so the body
    /// never has to ask.
    Vm,
    /// It waits for something that does not use the VM (a timer, a socket, a worker): the whole body is an
    /// [`idle`] wait, and the turn is taken again when it ends.
    Wait,
    /// It does both in turns, and the body starts *without* the turn: nothing takes it first (a detached
    /// continuation runs the start of the body without it). The body calls [`idle`] for its waits, which take the
    /// turn back when they end, and [`hold`] before it runs script code or touches the app's state before its
    /// first wait (`Mutex:with_lock` waits for the lock, which takes the turn, then runs Lua).
    Mixed,
}

/// Where the Lua registry keeps the table of coroutines whose driver is somewhere else (`async_coroutines.lua`).
pub(crate) const DRIVEN: &str = "agentos.driven_coroutines";

/// A coroutine the task scheduler took over (`task.spawn`, `task.defer`, `task.delay` of a coroutine the script made):
/// the script may no longer resume it itself, as with a coroutine that is running; two drivers of one coroutine would
/// fight over the host call it is waiting at. Without the app's environment (a worker, a test) it does nothing.
///
/// # Errors
/// The coroutine is already being driven (a thread is waiting at its host call, or the scheduler has it).
pub fn mark_driven(lua: &Lua, thread: &mlua::Thread) -> mlua::Result<()> {
    let Ok(driven) = lua.named_registry_value::<mlua::Table>(DRIVEN) else { return Ok(()) };
    let live = match driven.raw_get::<mlua::Value>(thread.clone())? {
        mlua::Value::Boolean(true) => true,
        mlua::Value::Thread(driver) => driver.status() == mlua::thread::ThreadStatus::Resumable,
        _ => false,
    };
    if live {
        return Err(mlua::Error::runtime("cannot schedule a coroutine that is already running"));
    }
    driven.raw_set(thread.clone(), true)
}

/// A leaf's future, in a cell the call can empty from outside when the leaf is abandoned.
struct LeafCell<R> {
    body: Mutex<Option<BoxFuture<'static, mlua::Result<R>>>>,
    abandoned: AtomicBool,
}

impl<R: Send> Abandon for LeafCell<R> {
    fn abandon(&self) {
        self.abandoned.store(true, Ordering::SeqCst);
        // not while the leaf is being polled (that is the next poll's business: it sees the flag)
        let body = self.body.try_lock().and_then(|mut body| body.take());
        drop(body);
    }
}

/// Run an async host function's body as the leaf its `role` says.
fn leaf<R: Send + 'static>(
    role: Role,
    body: impl Future<Output = mlua::Result<R>> + Send + 'static,
) -> impl Future<Output = mlua::Result<R>> + Send + 'static {
    let run = async move {
        match role {
            Role::Vm => {
                hold().await;
                body.await
            }
            Role::Wait => idle(body).await?,
            Role::Mixed => body.await,
        }
    };
    let cell =
        Arc::new(LeafCell { body: Mutex::new(Some(Box::pin(run))), abandoned: AtomicBool::new(false) });
    // every poll of the function, the first and each one after a wake, is a leaf poll (module docs, rules 2-3)
    std::future::poll_fn(move |cx| {
        let me: Weak<dyn Abandon> = Arc::downgrade(&cell) as Weak<dyn Abandon>;
        leaf_polling(&me);
        let polled = {
            let mut body = cell.body.lock();
            if cell.abandoned.load(Ordering::SeqCst) {
                *body = None;
            }
            let polled = match body.as_mut() {
                Some(future) => future.as_mut().poll(cx),
                None => Poll::Ready(Err(mlua::Error::runtime(
                    "this host call was abandoned: Lua could not suspend at it (it was called from a string.gsub \
                     callback, a table.sort comparator, a metamethod or an iterator) and the script went on",
                ))),
            };
            if polled.is_ready() {
                // finished: a second poll (the script resumed a coroutine it should not have) must not resume
                // a completed future
                *body = None;
            }
            polled
        };
        if polled.is_pending() {
            leaf_suspended(me);
        }
        polled
    })
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
    use std::pin::Pin;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// A call entry over one tokio mutex, counting how many calls hold it at once.
    struct Counting {
        gate: Arc<tokio::sync::Mutex<()>>,
        inside: Arc<AtomicUsize>,
        most: Arc<AtomicUsize>,
        refuse_park: Arc<AtomicBool>,
    }

    struct CountingBaton {
        gate: Arc<tokio::sync::Mutex<()>>,
        held: Option<tokio::sync::OwnedMutexGuard<()>>,
        waiting: Option<BoxFuture<'static, tokio::sync::OwnedMutexGuard<()>>>,
        inside: Arc<AtomicUsize>,
        most: Arc<AtomicUsize>,
        /// Refuse to park (an open transaction, say).
        refuse_park: Arc<AtomicBool>,
    }

    impl CountingBaton {
        fn arrive(&self) {
            let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
        }
    }

    impl Baton for CountingBaton {
        fn id(&self) -> u64 {
            1
        }
        fn holding(&self) -> bool {
            self.held.is_some()
        }
        fn inherited(&self) -> Inherited {
            Inherited::default()
        }
        fn can_park(&self) -> Result<(), String> {
            if self.refuse_park.load(Ordering::SeqCst) {
                Err("no".to_owned())
            } else {
                Ok(())
            }
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
                refuse_park: self.refuse_park.clone(),
            })
        }
        fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
            self.enter(Inherited::default())
        }
        fn enter(&self, _: Inherited) -> BoxFuture<'static, Box<dyn Baton>> {
            let (gate, inside, most) = (self.gate.clone(), self.inside.clone(), self.most.clone());
            let refuse_park = self.refuse_park.clone();
            Box::pin(async move {
                let held = gate.clone().lock_owned().await;
                let baton =
                    CountingBaton { gate, held: Some(held), waiting: None, inside, most, refuse_park };
                baton.arrive();
                Box::new(baton) as Box<dyn Baton>
            })
        }
    }

    /// An idle wait, as every real one is made: by a leaf.
    fn wait_for(time: Duration) -> impl Future<Output = mlua::Result<()>> + Send + 'static {
        leaf(Role::Wait, async move {
            tokio::time::sleep(time).await;
            Ok(())
        })
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
                refuse_park: Arc::default(),
            }),
        );
        // four callbacks that each wait (idle) in the middle: never two inside at once, and the waits overlap
        let started = tokio::time::Instant::now();
        let mut calls = vec![];
        for _ in 0..4 {
            let lua = lua.clone();
            calls.push(tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async {
                    wait_for(Duration::from_millis(100)).await.expect("wait");
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
                refuse_park: Arc::default(),
            }),
        );
        // the spawner's call holds the VM; the slice it runs starts a wait (the future is created and polled
        // once inside the slice) and the continuation finishes it later, outside the spawner's call
        let mut continuation = None;
        inbound(&lua, Inherited::default(), async {
            let mut wait = Box::pin(idle(tokio::time::sleep(Duration::from_millis(80))));
            first_slice(
                &lua,
                Inherited::default(),
                || {
                    let waker = futures::task::noop_waker();
                    let mut cx = std::task::Context::from_waker(&waker);
                    assert!(std::future::Future::poll(wait.as_mut(), &mut cx).is_pending());
                },
                |()| true,
            );
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
                refuse_park: Arc::default(),
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
                refuse_park: Arc::default(),
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
                    let (inside_now, other_now) = (inside.clone(), other_ran.clone());
                    leaf(Role::Mixed, async move {
                        tokio::task::yield_now().await;
                        tokio::task::yield_now().await;
                        assert_eq!(inside_now.load(Ordering::SeqCst), 1, "still holding while it waits");
                        assert!(!other_now.load(Ordering::SeqCst), "nothing else ran inside the call");
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
                refuse_park: Arc::default(),
            }),
        );
        let done = inbound(&lua, Inherited::default(), async {
            let mut waiting = Box::pin(hold());
            first_slice(
                &lua,
                Inherited::default(),
                || {
                    let waker = futures::task::noop_waker();
                    let mut cx = Context::from_waker(&waker);
                    assert!(waiting.as_mut().poll(&mut cx).is_pending(), "the spawner holds the VM");
                },
                |()| true,
            );
            tokio::time::timeout(Duration::from_secs(2), waiting).await.is_ok()
        })
        .await;
        assert!(done, "the call that holds the VM finished it instead of waiting for its own turn");
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    /// Sets a flag when it is dropped: a leaf's body that holds something.
    struct Holds(Arc<AtomicBool>);
    impl Drop for Holds {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// A leaf that holds something and never finishes.
    fn stuck(
        role: Role,
        dropped: &Arc<AtomicBool>,
    ) -> impl Future<Output = mlua::Result<()>> + Send + 'static {
        let holds = Holds(dropped.clone());
        leaf(role, async move {
            let _holds = holds;
            std::future::pending::<()>().await;
            Ok(())
        })
    }

    fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
        let waker = futures::task::noop_waker();
        Pin::new(future).poll(&mut Context::from_waker(&waker))
    }

    fn counting(lua: &Lua) -> (Arc<AtomicUsize>, Arc<AtomicBool>) {
        let (inside, refuse_park) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        set_entry(
            lua,
            Arc::new(Counting {
                gate: Arc::new(tokio::sync::Mutex::new(())),
                inside: inside.clone(),
                most: Arc::default(),
                refuse_park: refuse_park.clone(),
            }),
        );
        (inside, refuse_park)
    }

    /// The yield at a leaf failed (Lua could not suspend there) and the script ran on: the next leaf to start, and
    /// the end of the call, drop what the first one held; and polling it later is an error, not a stale suspension.
    #[tokio::test]
    async fn a_leaf_the_script_ran_past_is_abandoned_by_the_next_leaf_and_by_the_end_of_the_call() {
        let lua = Lua::new();
        counting(&lua);
        let (first, second, third) = (Arc::default(), Arc::default(), Arc::default());
        inbound(&lua, Inherited::default(), async {
            let mut a = Box::pin(stuck(Role::Mixed, &first));
            assert!(poll_once(&mut a).is_pending());
            assert!(!first.load(Ordering::SeqCst), "suspended, not abandoned");
            // the script ran on and called something else
            assert!(leaf(Role::Mixed, async { Ok(()) }).await.is_ok());
            assert!(first.load(Ordering::SeqCst), "the next leaf abandoned it");
            let Poll::Ready(late) = poll_once(&mut a) else { panic!("an abandoned leaf is over") };
            assert!(late.unwrap_err().to_string().contains("abandoned"));

            let mut b = Box::pin(stuck(Role::Mixed, &second));
            assert!(poll_once(&mut b).is_pending());
            std::mem::forget(b);
            let mut c = Box::pin(stuck(Role::Wait, &third));
            assert!(poll_once(&mut c).is_pending());
            assert!(second.load(Ordering::SeqCst), "a leaf started after it abandoned it as well");
            std::mem::forget(c);
        })
        .await;
        assert!(third.load(Ordering::SeqCst), "the end of the call abandoned what it was still waiting at");
    }

    /// A leaf that has finished is over: polling it again (a script resumed a coroutine it should not have) is an
    /// error, not a resumed completed future.
    #[tokio::test]
    async fn a_finished_leaf_polled_again_is_an_error_not_a_panic() {
        let mut once = Box::pin(leaf(Role::Mixed, async { Ok(7) }));
        assert!(matches!(poll_once(&mut once), Poll::Ready(Ok(7))));
        let Poll::Ready(again) = poll_once(&mut once) else { panic!("over") };
        assert!(again.is_err());
    }

    /// A leaf that suspended for real is polled again next: it is not abandoned, however many times it is polled.
    #[tokio::test]
    async fn a_leaf_that_is_polled_again_after_suspending_is_not_abandoned() {
        let lua = Lua::new();
        counting(&lua);
        let dropped = Arc::default();
        inbound(&lua, Inherited::default(), async {
            let mut a = Box::pin(stuck(Role::Mixed, &dropped));
            for _ in 0..3 {
                assert!(poll_once(&mut a).is_pending());
            }
            assert!(!dropped.load(Ordering::SeqCst));
            drop(a);
            assert!(dropped.load(Ordering::SeqCst), "dropping it drops its body, as always");
        })
        .await;
    }

    /// A first slice that stopped at a wait keeps the leaf alive for its continuation; one that stopped anywhere else
    /// abandons it.
    #[tokio::test]
    async fn the_first_slice_of_a_task_keeps_a_leaf_only_when_it_stopped_at_it() {
        let lua = Lua::new();
        counting(&lua);
        let (kept, left) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        inbound(&lua, Inherited::default(), async {
            let (mut a, mut b) = (Box::pin(stuck(Role::Wait, &kept)), Box::pin(stuck(Role::Wait, &left)));
            first_slice(&lua, Inherited::default(), || assert!(poll_once(&mut a).is_pending()), |()| true);
            first_slice(&lua, Inherited::default(), || assert!(poll_once(&mut b).is_pending()), |()| false);
            assert!(!kept.load(Ordering::SeqCst), "the continuation finishes it");
            assert!(left.load(Ordering::SeqCst), "the slice went on past it");
        })
        .await;
    }

    /// The call lends the VM at an idle wait, and only there.
    #[tokio::test]
    async fn a_call_lends_the_vm_at_an_idle_wait_and_only_when_nothing_says_otherwise() {
        // an idle wait: lent
        let lua = Lua::new();
        let (inside, _) = counting(&lua);
        let call = tokio::spawn({
            let lua = lua.clone();
            async move {
                inbound(&lua, Inherited::default(), async {
                    let _ = wait_for(Duration::from_secs(60)).await;
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        call.abort();
        assert_eq!(inside.load(Ordering::SeqCst), 0, "suspended at an idle wait: lent");

        // a leaf that is not an idle wait: kept
        let lua = Lua::new();
        let (inside, _) = counting(&lua);
        let started = Arc::new(AtomicBool::new(false));
        let flag = started.clone();
        let call = tokio::spawn({
            let lua = lua.clone();
            async move {
                inbound(&lua, Inherited::default(), async {
                    flag.store(true, Ordering::SeqCst);
                    let _ = leaf(Role::Mixed, std::future::pending::<mlua::Result<()>>()).await;
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(started.load(Ordering::SeqCst));
        assert_eq!(inside.load(Ordering::SeqCst), 1, "suspended at something that is not an idle wait: kept");
        call.abort();

        // an idle wait where the baton says no (a `db:tx` was opened meanwhile): kept
        let lua = Lua::new();
        let (inside, refuse_park) = counting(&lua);
        let call = tokio::spawn({
            let lua = lua.clone();
            async move {
                inbound(&lua, Inherited::default(), async {
                    // the wait is allowed when it starts, and by the time it is pending the call may not wait
                    // (a transaction was opened where the wait was left in flight)
                    let _ = leaf(Role::Wait, async move {
                        refuse_park.store(true, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        Ok(())
                    })
                    .await;
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(inside.load(Ordering::SeqCst), 1, "the baton refused at the moment of lending: kept");
        call.abort();
    }

    /// A call that wakes itself while an idle wait is pending (mlua does that when a script yields plainly) is not
    /// waiting for anything: it keeps the VM.
    #[tokio::test]
    async fn a_call_that_wakes_itself_at_a_pending_idle_wait_does_not_lend_the_vm() {
        let lua = Lua::new();
        let (inside, _) = counting(&lua);
        let call = tokio::spawn({
            let lua = lua.clone();
            async move {
                inbound(&lua, Inherited::default(), async {
                    let mut wait = Box::pin(wait_for(Duration::from_secs(60)));
                    std::future::poll_fn(|cx| {
                        let _ = wait.as_mut().poll(cx);
                        cx.waker().wake_by_ref();
                        Poll::<()>::Pending
                    })
                    .await;
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(inside.load(Ordering::SeqCst), 1, "a plain yield after a wait is not a wait");
        call.abort();
    }

    #[tokio::test]
    async fn without_an_entry_callbacks_run_and_idle_is_a_plain_wait() {
        let lua = Lua::new();
        let out = inbound(&lua, Inherited::default(), async { idle(async { 7 }).await.expect("idle") }).await;
        assert_eq!(out, 7);
    }
}

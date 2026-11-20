//! What a script starts, and who may run its code.
//!
//! The embedding mechanism of the VM. `neoc run` uses none of it: without a [`CallEntry`] every hook below is
//! a no-op and a call is a plain `.await`. A program that embeds the VM and runs several scripts (one engine
//! each) supplies the two things this module defines, and the VM keeps the rules.
//!
//! A script can start work that outlives the call that started it: `std:task` coroutines, a `vnd:hyper`
//! server's accept loop and connections, `std:thread` workers. Two things follow, and this module is both
//! of them:
//!
//! - **[`Life`]**, a per-VM registry of everything the script spawned. The embedder calls
//!   [`Life::shutdown`] when the script is unloaded or replaced: every registered task is aborted (which drops
//!   the listeners, pools and Lua threads it held), and nothing registers after that.
//! - **[`CallEntry`]**, the way a Lua callback that nobody called takes its turn. The embedder serialises the
//!   calls into one VM, and every callback joins the queue like any other call: [`request`] for an HTTP request
//!   arriving at a `vnd:hyper` server, [`inbound`] for a deferred or delayed `std:task` function, [`resumed`] for
//!   the continuation of a `std:task` coroutine (after [`first_slice`], its first slice). A call is a *baton*:
//!   while it holds it, no other call runs in the VM. [`scoped`] runs a call that holds a baton; the baton is handed
//!   back in one place, the gate every call goes through (`run_call`, which [`scoped`] and [`resumed`] share): when
//!   the call suspends at a wait that does not use the VM (a timer, the network, a worker: an [`idle`] wait) it
//!   releases the baton and takes it again afterwards, so a `while true do task.wait(1) end` loop does not stop the
//!   script's host, and a script can call its own listener from a task. Between two waits a call is atomic.
//!
//!   **Threading rule for a [`Baton`].** [`Baton::poll_unpark`], [`Baton::can_park`], [`Baton::park`] and
//!   [`Baton::idle_at`] are called while the call's task-local slot is mutably borrowed. A `Baton` that calls
//!   [`turn_id`] or [`inherited`] from one of them panics with a `RefCell` borrow error: keep what those need in
//!   the baton itself.
//!
//!   **Who lends, and when.** A wait that cannot suspend the call must not lend the baton: the script goes on
//!   running, and so must whatever the embedder attaches to the turn (a limit on what the call may do, say). The wait fails with an error the
//!   script may catch. On Luau (the engine of this VM) the places that *refuse* a yield are every metamethod
//!   (`__index`, `__newindex`, the arithmetic, comparison, `__concat`, `__len` and `__iter` ones; `__call` is
//!   the exception, it is an ordinary call), `tostring()` of a `__tostring`, the C functions that call back into Lua
//!   (a `string.gsub` replacement function or table, a `table.sort` comparator), the message handler of `xpcall` (it
//!   runs on the C stack of the error), and any host function that calls Lua synchronously
//!   (`mlua::Function::call`): Luau says "attempt to yield across metamethod/C-call boundary". Luau *does* yield
//!   across `pcall`, `xpcall` (its protected function), a `__call` and `for ... in` iterator functions, so those
//!   are ordinary suspensions that lend. (LuaJIT also yielded across every metamethod and `tostring()`; PUC Lua 5.1
//!   refused all of them. `tests/embedding.rs` holds the table to a real engine.) The VM cannot see whether Lua
//!   managed to yield, so the rules are structural, and an embedder is expected to check them with generated
//!   programs over every boundary (`tests/embedding.rs` lists the boundaries, and a generated-program test
//!   generates programs out of them):
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
//!      guard, an open section of the embedder's) is released, and a coroutine the script resumes later finds the leaf gone: its host
//!      call raises "abandoned". A host call that Lua could not suspend is therefore over, not left half-done.
//!   4. **A call lends the baton only when it really suspended at an idle wait:** its future returned `Pending`,
//!      the innermost suspended leaf is an idle wait, the call's waker was *not* woken during that poll (mlua
//!      wakes it itself when the script calls a plain `coroutine.yield()`, which waits for nothing), and the baton
//!      still agrees ([`Baton::can_park`], asked at the moment of lending, not only before the wait began: an
//!      embedder refuses it while it is in a state that must not lend the turn).
//!
//!   5. **Script code runs only in a poll that holds the turn.** Every poll of a call goes through one gate
//!      (`run_call`, behind [`scoped`] and [`resumed`]): a poll that is not resuming into a suspended leaf (the leaf takes the turn itself when it ends)
//!      first takes the turn, and does not run script code until it has it. A call resumed anywhere else (at a plain
//!      yield, or into a leaf that is gone) therefore queues for the turn instead of running in someone else's.
//!   6. **A script resumes (and closes) only the coroutines it made, and only while nobody else drives them.** This
//!      rule is kept by the [script environment](crate::lua::script_env) (its `coroutine` library); a script that runs on
//!      the engine's own `coroutine` (`make_engine_in` and no environment) is not held to it, and [`mark_driven`](crate::lua::script_env::mark_driven)
//!      accepts every coroutine there.
//!      `coroutine.create` and `coroutine.wrap` record their coroutine in a weak-keyed *owned* set
//!      (`async_coroutines.lua`); `coroutine.resume` of any other thread is refused with "cannot resume non-suspended
//!      coroutine", as for a coroutine that is running, and `coroutine.close` of one is refused with "cannot close a coroutine
//!      that is not yours" (Luau's `close` kills a suspended thread: a task's thread would end with its `await` returning
//!      nothing; the engine of `neoc run`, which has no environment, has no `close` at all). A script can hold other threads (`coroutine.running()` returns
//!      the top-level thread of a call, a task's thread), but the refusal is the default, so there is no list of host
//!      threads to keep complete. (On Luau a `pcall` or `xpcall` runs in the thread that called it, so inside a
//!      coroutine the script made `coroutine.running()` is that coroutine, which it may resume.) A coroutine of its own is
//!      refused too while someone else drives it: a thread is waiting at its host call, or [`mark_driven`](crate::lua::script_env::mark_driven) gave it to
//!      the task scheduler (`task.spawn`, `defer` and `delay` take only coroutines the script made).
//!      A coroutine whose resumer's call was dropped while it waited is released when the collector clears the dead
//!      thread from `driven` (a weak-valued table): the call is never resumed, so nothing can drive the coroutine
//!      meanwhile, and a later resume finds its host call abandoned (rule 3).
//!
//!      Rule 5 is the second line of defence, and no script in a script environment reaches it today: the one way a call finds its leaf gone
//!      was another call resuming a thread parked at it, which rule 6 refuses for every thread the script did not
//!      make and for every one that is waiting at a host call. It stays because it costs one gate and guards the
//!      invariant itself (script code runs only with the turn) rather than the ways to break it; the unit test
//!      `a_call_resumed_without_its_leaf_takes_the_turn_before_it_runs_script_code` covers the branch directly.
//!
//!   **Quiet calls.** A wait made by [`Role::Timer`] (`task.wait`, a `std:time` sleep) tells the baton so
//!   ([`Baton::idle_at`] with [`IdleKind::Timer`]): a call asleep there has nothing in flight and runs no script
//!   code until it asks for its turn back. An embedder uses that to tell a call that may still be acting on the world
//!   from one that is asleep (to decide how to treat a request that reaches the script's own server, say).
//!
//!   A leaf never carries a baton: [`hold`] looks the baton up in the call that polls it. A host call that was
//!   suspended by one call and is resumed by another therefore runs in the turn of whoever resumed it, as Lua's
//!   own `coroutine.resume` would have it, instead of waiting for a turn that nobody can give it.
//!
//!   A `std:task` coroutine is the one call that does not start at its beginning: `task.spawn` runs its first
//!   slice inside the spawning call and the rest later, from whichever wait that slice stopped at. Its
//!   continuation therefore starts *detached* ([`resumed`]): it holds nothing until the wait it was stopped
//!   at ends (an [`idle`] wait, or a host call, which takes its turn first) and takes its turn then. The leaf it
//!   stopped at travels with it as [`Carried`], so a continuation that is dropped before it ever runs abandons it.
//!
//! Without a [`CallEntry`] (an engine that is not an embedder's script, like a `std:thread` worker's, `neoc run`'s or a test's) the
//! callbacks run directly and [`idle`] is a plain `.await`.

/// The stack size of every thread that polls a VM: the runtime's workers and its blocking pool (build the runtime
/// with `.thread_stack_size(VM_THREAD_STACK)`), the thread that `block_on`s the top-level future (run that future on a
/// worker instead, with `tokio::spawn`), and any thread of the embedder's own that calls into Lua. `std:thread`
/// workers need nothing of their own: they are tasks on the embedder's runtime.
///
/// Luau stops a recursion that goes through a C function or a metamethod at 200 C levels with an ordinary error
/// (`C stack overflow`), so a script cannot recurse past that. What the stack must hold is those 200 levels with the
/// host functions in them, and a level through a Rust host function costs more than one through Lua's own. Measured
/// at `tests/abort_regressions.rs` (the smallest thread, in KiB, the child survives on; mlua-sys at `opt-level = 2` in
/// both builds, Apple silicon):
///
/// | recursion through | debug | release |
/// |---|---|---|
/// | `Mutex:with_lock` nested | 4608 | 640 |
/// | `lib:test` `describe` nested | 4096 | 640 |
/// | `lib:cache` `on_evict` re-entering the cache | 3072 | 768 |
/// | a `vnd:hyper` request whose options table recurses | 1536 | 512 |
/// | a `std:thread` spawn, or a `lib:http` `serve_static`, whose options table recurses | 1024 | 640 |
/// | a `__tostring` that resumes a failing coroutine through mlua | 768 | under 256 |
/// | a `string.gsub` callback | 384 | 384 |
///
/// tokio's default, 2 MiB, holds every release figure (the suite holds a release build to it) and none of the three
/// largest debug ones. 8 MiB holds the worst debug figure with a margin of 1.7. (An async host function that reads
/// a script table inside its future, as `vnd:hyper` did until its options were read first, is a path of this kind:
/// each level stacks a polled future, about 20 KiB in release and 150 KiB in debug.) The stack is reserved address
/// space and committed only as it is touched, so a bigger constant costs a thread nothing it does not use. `neoc run`
/// builds its runtime with it.
pub const VM_THREAD_STACK: usize = 8 * 1024 * 1024;

use std::{
    any::Any,
    cell::RefCell,
    future::Future,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

pub use futures::future::BoxFuture;
use futures::task::ArcWake;
use mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua, UserDataMethods, UserDataRef};
use parking_lot::Mutex;
use tokio::{sync::watch, task::AbortHandle};

/// The tasks a script spawned, abortable together. Cheap to clone: every clone is the same registry.
///
/// Aborting is cooperative: a task is stopped at its next `.await`, so a `std:thread` worker in a loop that never
/// yields to the executor (`while true do end`) keeps its VM and a runtime thread after [`Life::shutdown`].
#[derive(Clone)]
pub struct Life {
    inner: Arc<LifeInner>,
}

/// The least a registry holds before it looks for finished entries to drop.
const MIN_PRUNE: usize = 64;

/// A list that drops its dead entries when it has doubled since the last time it did, so adding is O(1) amortised.
pub(crate) struct Pruned<T> {
    pub(crate) items: Vec<T>,
    prune_at: usize,
}

impl<T> Pruned<T> {
    pub(crate) const fn new() -> Self {
        Self {
            items: Vec::new(),
            prune_at: MIN_PRUNE,
        }
    }

    pub(crate) fn push(&mut self, item: T, alive: impl Fn(&T) -> bool) {
        self.items.push(item);
        if self.items.len() >= self.prune_at {
            self.items.retain(alive);
            self.prune_at = (self.items.len() * 2).max(MIN_PRUNE);
        }
    }
}

/// What runs when a registry stops its background work or shuts down.
struct Release {
    /// Is there still something to release? (A release for an object the script has dropped is not kept.)
    alive: Box<dyn Fn() -> bool + Send>,
    run: Box<dyn FnOnce() + Send>,
}

struct LifeInner {
    closed: AtomicBool,
    tasks: Mutex<Pruned<AbortHandle>>,
    releases: Mutex<Pruned<Release>>,
    closed_tx: watch::Sender<bool>,
    /// Counts [`Life::stop_background`] calls (a shutdown is one).
    stopped_tx: watch::Sender<u64>,
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
        let (stopped_tx, _) = watch::channel(0);
        Self {
            inner: Arc::new(LifeInner {
                closed: AtomicBool::new(false),
                tasks: Mutex::new(Pruned::new()),
                releases: Mutex::new(Pruned::new()),
                closed_tx,
                stopped_tx,
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

    /// Adopt `life` as this engine's registry (a worker joins the registry of the script that started it).
    pub fn adopt(lua: &Lua, life: Self) {
        lua.set_app_data(life);
    }

    /// Spawn on the current tokio runtime and register the task. After [`Life::shutdown`] the task is
    /// aborted at once, so a loop that keeps spawning cannot outlive the script.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let handle = tokio::spawn(future);
        let abort = handle.abort_handle();
        let mut tasks = self.inner.tasks.lock();
        if self.inner.closed.load(Ordering::SeqCst) {
            abort.abort();
        } else {
            tasks.push(abort, |t| !t.is_finished());
        }
        handle
    }

    /// Run `release` when [`Life::shutdown`] or [`Life::stop_background`] runs (at once when the registry is
    /// already shut down): for what a script holds that dropping the tasks does not free. It is kept until then.
    /// For something the script can drop, use [`Life::on_shutdown_with`], which does not keep it.
    pub fn on_shutdown(&self, release: impl FnOnce() + Send + 'static) {
        self.register(Release {
            alive: Box::new(|| true),
            run: Box::new(release),
        });
    }

    /// [`Life::on_shutdown`] for something the script owns through `owner` (a listener's socket): `release` gets
    /// the owner if it is still alive when the registry shuts down, and the registry keeps no more than a weak
    /// reference, so a script that binds and drops listeners in a loop does not grow it.
    pub fn on_shutdown_with<T: Send + Sync + 'static>(
        &self,
        owner: &Arc<T>,
        release: impl FnOnce(Arc<T>) + Send + 'static,
    ) {
        let weak = Arc::downgrade(owner);
        let alive = weak.clone();
        self.register(Release {
            alive: Box::new(move || alive.strong_count() > 0),
            run: Box::new(move || {
                if let Some(owner) = weak.upgrade() {
                    release(owner);
                }
            }),
        });
    }

    fn register(&self, release: Release) {
        let mut releases = self.inner.releases.lock();
        if self.inner.closed.load(Ordering::SeqCst) {
            drop(releases);
            (release.run)();
        } else {
            releases.push(release, |r| (r.alive)());
        }
    }

    /// Abort everything registered and run the releases, but stay open: the script keeps running and may start
    /// new work. A replacement version that wants the old one's port is loaded after this. That covers what the
    /// script started (tasks, workers, `vnd:hyper` servers, `std:net` listeners) and a `lib:http` static server
    /// it is blocked in (see [`Life::stopped`]).
    pub fn stop_background(&self) {
        let (tasks, releases) = {
            let mut tasks = self.inner.tasks.lock();
            let mut releases = self.inner.releases.lock();
            (
                std::mem::take(&mut tasks.items),
                std::mem::take(&mut releases.items),
            )
        };
        for task in tasks {
            task.abort();
        }
        for release in releases {
            (release.run)();
        }
        self.inner.stopped_tx.send_modify(|stops| *stops += 1);
    }

    /// Abort everything registered, run the releases and refuse anything new. Idempotent.
    pub fn shutdown(&self) {
        {
            let _tasks = self.inner.tasks.lock();
            let _releases = self.inner.releases.lock();
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

    /// A future that resolves at the next [`Life::stop_background`] (or [`Life::shutdown`]), counted from now:
    /// for a call that blocks in work the registry cannot abort because it is the call itself (`serve_static`).
    pub fn stopped(&self) -> impl Future<Output = ()> + Send + 'static {
        let mut rx = self.inner.stopped_tx.subscribe();
        async move {
            let _ = rx.changed().await;
        }
    }
}

/// What a piece of work a call starts (a `std:task` coroutine, a deferred or delayed function) takes from the
/// call that started it: a value the embedder defines (a limit the starting call runs under, say) and the
/// VM carries without looking inside. [`Baton::inherited`] hands it over when work starts, and the embedder
/// reads it back with [`Inherited::get`] when that work asks for its own turn ([`CallEntry::enter`],
/// [`CallEntry::detached`]). The default is empty: nothing is inherited.
#[derive(Clone, Default)]
pub struct Inherited(Option<Arc<dyn Any + Send + Sync>>);

impl Inherited {
    /// Inherit `value`.
    #[must_use]
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Some(Arc::new(value)))
    }

    /// The inherited value, when there is one and it is a `T`.
    #[must_use]
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.0.as_deref().and_then(|value| value.downcast_ref())
    }
}

impl std::fmt::Debug for Inherited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Inherited(..)"
        } else {
            "Inherited(empty)"
        })
    }
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

/// What an idle wait is, for an embedder that wants to know what a call that does not hold the VM may be doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleKind {
    /// A timer and nothing else (`task.wait`, a `std:time` sleep): the call has nothing in flight, and runs no
    /// script code until the timer ends and it has taken its turn back.
    Timer,
    /// Anything else that is waited for (the network, a worker, a lock, a database): something the call started may
    /// be acting while it waits (a request on its way to somewhere, a worker running).
    Other,
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
    /// it is not (the embedder opened a section that must not lend the turn meanwhile) must not lend.
    ///
    /// # Errors
    /// The reason the call must not wait (the wait is refused with it).
    fn can_park(&self) -> Result<(), String>;

    /// The holder is suspended at a wait that does not use the VM: set the call's own state aside and let
    /// others in. Called only after [`Baton::can_park`] agreed and the call really suspended at the wait.
    fn park(&mut self);

    /// The call does not hold the VM and is waiting at `at` (told right after [`Baton::park`], and to a
    /// continuation that starts out detached at a wait its first slice stopped at). Until [`Baton::poll_unpark`]
    /// is asked, the call is not running script code, which lets an embedder tell a call that is *quiet* (asleep
    /// at a [`IdleKind::Timer`]: it sends nothing and starts nothing) from one that may still be acting on the
    /// world through a wait that is in flight.
    fn idle_at(&mut self, at: IdleKind);

    /// Queue for the VM (when this call does not hold it), and take the call's state back. [`hold`] polls this
    /// until it is ready; the place in the queue lives in the baton, not in whatever future is polling, so a
    /// host call that is polled by another call later asks that call's baton instead.
    fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}

/// The embedder's queue of calls into one VM.
pub trait CallEntry: Send + Sync + 'static {
    /// Wait for the VM to be free and take it, for work a call started (a deferred or delayed function):
    /// `inherited` is what it takes from the call that started it, and nothing else restricts it.
    ///
    /// Callers (this and [`CallEntry::request`]) must run inside a [`Life`] task: once the embedder's VM is closed
    /// it never grants their turn, so the future stays pending until that task is aborted.
    fn enter(&self, inherited: Inherited) -> BoxFuture<'static, Box<dyn Baton>>;

    /// Wait for the VM to be free and take it, for a request that arrives from outside (to the script's own
    /// listener). It has no origin to inherit from, so the embedder decides what it may do. Like
    /// [`CallEntry::enter`], it must be awaited inside a [`Life`] task (a closed VM never grants the turn).
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
    /// Its wait does not use the VM (and what kind it is): the call may lend its turn while it lasts.
    idle: Option<IdleKind>,
}

impl Suspended {
    /// Will a poll of the call resume into this leaf (it is neither gone nor abandoned)?
    fn alive(&self) -> bool {
        self.leaf.upgrade().is_some_and(|leaf| leaf.pending())
    }
}

/// What a suspended leaf lets its caller do when the suspension turns out not to have happened.
trait Abandon: Send + Sync {
    /// Drop the leaf's future now, whatever it holds with it.
    fn abandon(&self);

    /// Is the leaf's future still there to be polled?
    fn pending(&self) -> bool;
}

/// A call's baton, and what it is waiting at.
struct Slot {
    baton: Option<Box<dyn Baton>>,
    /// An idle wait is pending in the leaf that is being polled; the leaf records that when it returns `Pending`.
    idle_pending: Option<IdleKind>,
    /// The leaves that returned `Pending` since the call last ran script code, innermost first.
    suspended: Vec<Suspended>,
}

impl Slot {
    fn new(baton: Option<Box<dyn Baton>>) -> RefCell<Self> {
        RefCell::new(Self {
            baton,
            idle_pending: None,
            suspended: vec![],
        })
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
    run_call(Slot::new(Some(baton)), future).await
}

async fn run_call<F: Future>(slot: RefCell<Slot>, future: F) -> F::Output {
    HELD.scope(slot, async move {
        let mut future = std::pin::pin!(future);
        let proxy = Arc::new(WakeProxy::default());
        std::future::poll_fn(move |cx| {
            // rule 5: the one gate between the executor and script code
            if turn_before_script(cx).is_pending() {
                return Poll::Pending;
            }
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

/// Rule 5 of the module docs. A poll of the call that resumes into a leaf that is still suspended lets the leaf take
/// the turn when its wait ends (a wait that does not use the VM may end without it). Any other poll resumes script
/// code, and the call must hold the turn first: a call that was lent its turn and finds its leaf gone (the thread was
/// resumed and moved on by someone else) queues for it here, so it never runs in the turn of another call.
fn turn_before_script(cx: &mut Context<'_>) -> Poll<()> {
    HELD.try_with(|slot| {
        let mut slot = slot.borrow_mut();
        // the leaf the call resumes into first is the outermost one
        if slot.suspended.last().is_some_and(Suspended::alive) {
            return Poll::Ready(());
        }
        take_turn(&mut slot, cx)
    })
    .unwrap_or(Poll::Ready(()))
}

/// Ready when the call holds the turn (or has no baton to hold); otherwise queue for it. The one place a call takes
/// its turn, for [`hold`] and for [`turn_before_script`].
fn take_turn(slot: &mut Slot, cx: &mut Context<'_>) -> Poll<()> {
    match slot.baton.as_mut() {
        Some(baton) if !baton.holding() => baton.poll_unpark(cx),
        _ => Poll::Ready(()),
    }
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
    HELD.try_with(|slot| std::mem::take(&mut slot.borrow_mut().suspended))
        .unwrap_or_default()
}

/// A leaf starts a poll (the first, or one after a wake). If it is the one that suspended last, it is being
/// resumed as it should: it leaves the record. Anything else suspended is stale (the script ran on after it):
/// those leaves are abandoned.
fn leaf_polling(me: &Weak<dyn Abandon>) {
    let stale = HELD
        .try_with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.idle_pending = None;
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
        let idle = slot.idle_pending.take();
        slot.suspended.push(Suspended { leaf, idle });
    });
}

/// The call's future returned `Pending`. Give the VM to the next call only if the call is waiting at an idle wait
/// (module docs, rule 4); anything else (a database call, a plain yield, a leaf waiting for the VM) keeps it.
fn lend_if_suspended_at_idle_wait(woken: bool) {
    let _ = HELD.try_with(|slot| {
        let mut slot = slot.borrow_mut();
        let at = if woken {
            None
        } else {
            slot.suspended.first().and_then(|innermost| innermost.idle)
        };
        if let Some(at) = at
            && let Some(baton) = slot
                .baton
                .as_mut()
                .filter(|b| b.holding() && b.can_park().is_ok())
        {
            baton.park();
            baton.idle_at(at);
        }
    });
}

/// The identity of the call running in this tokio task (see [`Baton::id`]); `None` outside a call.
#[must_use]
pub fn turn_id() -> Option<u64> {
    HELD.try_with(|slot| slot.borrow().baton.as_ref().map(|baton| baton.id()))
        .ok()
        .flatten()
}

/// The engine's [`CallEntry`], taken out of the engine. Work a script starts (a `std:task` task) keeps this, not
/// the [`Lua`]: a task must not keep its engine alive, or a worker's tasks would outlive the worker.
#[derive(Clone)]
pub struct Entrance(Option<Arc<dyn CallEntry>>);

/// The [`Entrance`] of `lua`: empty without a [`CallEntry`].
#[must_use]
pub fn entrance(lua: &Lua) -> Entrance {
    Entrance(lua.app_data_ref::<Entry>().map(|entry| entry.0.clone()))
}

impl Entrance {
    /// Is there an embedder that serialises calls?
    #[must_use]
    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    /// [`inbound`] without the engine.
    pub async fn inbound<F: Future>(&self, inherited: Inherited, future: F) -> F::Output {
        match &self.0 {
            Some(entry) => {
                let baton = entry.enter(inherited).await;
                scoped(baton, future).await
            }
            None => future.await,
        }
    }

    /// [`resumed`] without the engine.
    pub async fn resumed<F: Future>(
        &self,
        inherited: Inherited,
        mut carried: Carried,
        future: F,
    ) -> F::Output {
        let detached = carried
            .baton
            .take()
            .or_else(|| self.0.as_ref().map(|entry| entry.detached(inherited)));
        match detached {
            Some(baton) => {
                let slot = Slot::new(Some(baton));
                slot.borrow_mut().suspended = std::mem::take(&mut carried.leaves);
                run_call(slot, future).await
            }
            None => future.await,
        }
    }
}

/// Run a Lua callback nobody called (a deferred or delayed function) as a call of its own, with what it takes
/// from the call that started it: it waits for its turn first. Without a [`CallEntry`] it just runs.
pub async fn inbound<F: Future>(lua: &Lua, inherited: Inherited, future: F) -> F::Output {
    entrance(lua).inbound(inherited, future).await
}

/// Run the handler of a request that arrived from outside (to the script's own listener) as a call of its own:
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

/// The leaves a task's first slice stopped at, which the continuation finishes ([`first_slice`], [`resumed`]). A
/// continuation that never runs (the task is aborted before its first poll, the script unloaded) drops this, which
/// abandons the leaves: what they hold is freed with the task instead of staying in the Lua thread that is
/// suspended at them.
///
/// It also holds the continuation's baton, made when the first slice ends: the call exists from that moment, not
/// from whenever the executor first polls the continuation, so an embedder that counts its calls never sees a gap in
/// which a task that is waiting at something in flight is nobody's.
#[derive(Default)]
pub struct Carried {
    leaves: Vec<Suspended>,
    baton: Option<Box<dyn Baton>>,
}

impl Drop for Carried {
    fn drop(&mut self) {
        abandon_all(std::mem::take(&mut self.leaves));
    }
}

/// A coroutine's continuation, which starts where its first slice stopped (see the module docs): it runs as a
/// call that holds nothing until the wait it stopped at ends, resuming into the leaf `carried` from that slice
/// (rule 5 makes a continuation that stopped anywhere else, at a plain yield, take its turn before it runs).
/// Without a [`CallEntry`] it just runs.
pub async fn resumed<F: Future>(
    lua: &Lua,
    inherited: Inherited,
    carried: Carried,
    future: F,
) -> F::Output {
    entrance(lua).resumed(inherited, carried, future).await
}

/// Run the synchronous first slice of a coroutine (`task.spawn`) inside the spawning call, but *not as* that
/// call: a wait it starts keeps a detached baton of its own (see [`resumed`]), so the spawner keeps its turn.
///
/// `suspended` says whether the slice stopped at a wait, which its continuation finishes later: the leaves it is
/// suspended at stay alive for that, and come back as [`Carried`]. When the slice stopped anywhere else (it ended,
/// failed or yielded plainly) the leaves it ran past are abandoned, as they would be at the end of a call.
pub fn first_slice<R>(
    lua: &Lua,
    inherited: Inherited,
    slice: impl FnOnce() -> R,
    suspended: impl FnOnce(&R) -> bool,
) -> (R, Carried) {
    // without an embedder there are no leaves to keep or abandon: the slice is just a resume, as it always was
    let Some(entry) = lua.app_data_ref::<Entry>().map(|entry| entry.0.clone()) else {
        return (slice(), Carried::default());
    };
    let detached = Some(entry.detached(inherited.clone()));
    let entry = Some(entry);
    let (out, kept, left, continuation) = HELD.sync_scope(Slot::new(detached), || {
        let out = slice();
        let kept = suspended(&out);
        let left = take_suspended();
        // made before this slice's own baton goes, so a counting embedder never sees the task as nobody's
        let continuation = entry.filter(|_| kept).map(|entry| {
            let mut baton = entry.detached(inherited);
            if let Some(at) = left.first().and_then(|innermost| innermost.idle) {
                baton.idle_at(at);
            }
            baton
        });
        (out, kept, left, continuation)
    });
    if kept {
        (
            out,
            Carried {
                leaves: left,
                baton: continuation,
            },
        )
    } else {
        abandon_all(left);
        (out, Carried::default())
    }
}

/// Wait for `future`, which does not use the VM, without holding the VM: when the wait suspends the running
/// call it hands its baton back (see [`scoped`]), and it queues for it again afterwards. A plain `.await` when
/// this is not a call.
///
/// Only for a [`Role::Mixed`] function, which waits for part of its work; a function that is a wait from end to
/// end is a [`Role::Wait`] and needs no call.
///
/// # Errors
/// The call cannot wait ([`Baton::can_park`] said no): the wait is refused and `future` is not polled.
pub async fn idle<F: Future>(future: F) -> mlua::Result<F::Output> {
    idle_kind(IdleKind::Other, future).await
}

/// [`idle`], saying what the wait is.
async fn idle_kind<F: Future>(kind: IdleKind, future: F) -> mlua::Result<F::Output> {
    let may_wait = HELD.try_with(|slot| {
        slot.borrow()
            .baton
            .as_ref()
            .filter(|b| b.holding())
            .map(|b| b.can_park())
    });
    if let Ok(Some(Err(why))) = may_wait {
        return Err(mlua::Error::runtime(why));
    }
    let mut future = std::pin::pin!(future);
    let out = std::future::poll_fn(|cx| {
        let polled = future.as_mut().poll(cx);
        if polled.is_pending() {
            // the leaf records it when its poll ends; the call lends only if it really suspends here
            let _ = HELD.try_with(|slot| slot.borrow_mut().idle_pending = Some(kind));
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
pub async fn hold() {
    std::future::poll_fn(|cx| {
        HELD.try_with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.idle_pending = None;
            take_turn(&mut slot, cx)
        })
        .unwrap_or(Poll::Ready(()))
    })
    .await;
}

/// How an async host function relates to the call's turn in the VM (see [`async_function`]).
///
/// One rule holds for every role: **a body may create Lua values (tables, strings, userdata) at any time, with the
/// turn or without it, but it may only run script code or touch the embedder's state while the call holds the turn.**
/// Making a value reads and changes nothing the script can see; a `Role::Wait` body therefore builds its result as
/// the wait ends, while the VM may still be lent to another call, and a `Role::Mixed` body that needs the wait's
/// result first builds it after [`idle`] (which has taken the turn back). The conversion of the finished result
/// into Lua values, by mlua, always happens with the turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// It uses the VM (state the embedder protects with its turn): it takes the turn before its body runs, so the body
    /// never has to ask.
    Vm,
    /// It waits for something that does not use the VM (a timer, a socket, a worker): the whole body is an
    /// [`idle`] wait, and the turn is taken again when it ends.
    Wait,
    /// A [`Role::Wait`] whose whole body is a timer and nothing else (`task.wait`, a `std:time` sleep): while the
    /// call sleeps it has nothing in flight (no request, no worker), so an embedder may treat it as quiet
    /// ([`IdleKind::Timer`]).
    Timer,
    /// It does both in turns, and the body starts *without* the turn: nothing takes it first (a detached
    /// continuation runs the start of the body without it). The body calls [`idle`] for its waits, which take the
    /// turn back when they end, and [`hold`] before it runs script code or touches the embedder's state before its
    /// first wait (`Mutex:with_lock` waits for the lock, which takes the turn, then runs Lua).
    Mixed,
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

    fn pending(&self) -> bool {
        // locked means it is being polled right now
        !self.abandoned.load(Ordering::SeqCst)
            && self.body.try_lock().is_none_or(|body| body.is_some())
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
            Role::Timer => idle_kind(IdleKind::Timer, body).await?,
            Role::Mixed => body.await,
        }
    };
    let cell = Arc::new(LeafCell {
        body: Mutex::new(Some(Box::pin(run))),
        abandoned: AtomicBool::new(false),
    });
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
                     callback, a table.sort comparator or an xpcall handler, say) and the script went on",
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
        /// What every baton was told it waits at ([`Baton::idle_at`]).
        idle: Arc<Mutex<Vec<IdleKind>>>,
    }

    struct CountingBaton {
        gate: Arc<tokio::sync::Mutex<()>>,
        held: Option<tokio::sync::OwnedMutexGuard<()>>,
        waiting: Option<BoxFuture<'static, tokio::sync::OwnedMutexGuard<()>>>,
        inside: Arc<AtomicUsize>,
        most: Arc<AtomicUsize>,
        /// Refuse to park (the embedder is in a state that must not lend the turn).
        refuse_park: Arc<AtomicBool>,
        idle: Arc<Mutex<Vec<IdleKind>>>,
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
        fn idle_at(&mut self, at: IdleKind) {
            self.idle.lock().push(at);
        }
        fn poll_unpark(&mut self, cx: &mut Context<'_>) -> Poll<()> {
            if self.held.is_none() {
                let gate = self.gate.clone();
                let waiting = self
                    .waiting
                    .get_or_insert_with(|| Box::pin(gate.lock_owned()));
                let Poll::Ready(guard) = waiting.as_mut().poll(cx) else {
                    return Poll::Pending;
                };
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
                idle: self.idle.clone(),
            })
        }
        fn request(&self) -> BoxFuture<'static, Box<dyn Baton>> {
            self.enter(Inherited::default())
        }
        fn enter(&self, _: Inherited) -> BoxFuture<'static, Box<dyn Baton>> {
            let (gate, inside, most) = (self.gate.clone(), self.inside.clone(), self.most.clone());
            let (refuse_park, idle) = (self.refuse_park.clone(), self.idle.clone());
            Box::pin(async move {
                let held = gate.clone().lock_owned().await;
                let baton = CountingBaton {
                    gate,
                    held: Some(held),
                    waiting: None,
                    inside,
                    most,
                    refuse_park,
                    idle,
                };
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

    /// The registry drops its finished tasks and the releases of what the script dropped as it goes, in amortised
    /// O(1) per spawn: a script that spawns and finishes tasks (or binds and drops listeners) for ever does not
    /// grow it, and spawning is not a walk over every live task.
    #[tokio::test]
    async fn the_registry_forgets_what_finished_and_what_was_dropped() {
        let life = Life::new();
        for _ in 0..5000 {
            life.spawn(async {}).await.unwrap();
        }
        let held = life.inner.tasks.lock().items.len();
        assert!(
            held <= 2 * MIN_PRUNE,
            "{held} finished tasks are still held"
        );

        for _ in 0..5000 {
            let owner = Arc::new(());
            life.on_shutdown_with(&owner, |_| {});
        }
        let kept = life.inner.releases.lock().items.len();
        assert!(
            kept <= 2 * MIN_PRUNE,
            "{kept} releases of dropped owners are still held"
        );

        // an owner that is alive when the registry shuts down is released with it; one that is gone is not
        let released = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(());
        let counter = released.clone();
        life.on_shutdown_with(&alive, move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let gone = Arc::new(());
        let counter = released.clone();
        life.on_shutdown_with(&gone, move |_| {
            counter.fetch_add(10, Ordering::SeqCst);
        });
        drop(gone);
        life.shutdown();
        assert_eq!(released.load(Ordering::SeqCst), 1);
        // after the shutdown a release for a live owner runs at once
        let counter = released.clone();
        life.on_shutdown_with(&alive, move |_| {
            counter.fetch_add(100, Ordering::SeqCst);
        });
        assert_eq!(released.load(Ordering::SeqCst), 101);
    }

    /// Spawning does not slow down with the number of live tasks (it once scanned them all each time).
    #[tokio::test]
    async fn spawning_many_live_tasks_is_linear() {
        let time = |n: usize| {
            let life = Life::new();
            let start = std::time::Instant::now();
            for _ in 0..n {
                life.spawn(std::future::pending::<()>());
            }
            let took = start.elapsed();
            life.shutdown();
            took
        };
        let _warm = time(2000);
        let (small, large) = (time(4000), time(32000));
        // 8 times the tasks: linear is about 8 times the time, a scan per spawn is about 64
        assert!(
            large < small * 24 + Duration::from_millis(50),
            "4000 spawns took {small:?}, 32000 took {large:?}"
        );
    }

    #[tokio::test]
    async fn spawned_tasks_are_aborted_at_shutdown_and_later_spawns_never_run() {
        let life = Life::new();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = life.spawn(async move {
            let _tx = tx; // dropped when the task is dropped
            std::future::pending::<()>().await;
        });
        life.shutdown();
        assert!(task.await.expect_err("aborted").is_cancelled());
        assert!(rx.await.is_err(), "the aborted task's state was dropped");

        let late = life.spawn(std::future::pending::<()>());
        assert!(late.await.expect_err("aborted at once").is_cancelled());
        let released = Arc::new(AtomicBool::new(false));
        let flag = released.clone();
        life.on_shutdown(move || flag.store(true, Ordering::SeqCst));
        assert!(
            released.load(Ordering::SeqCst),
            "a release registered after shutdown runs at once"
        );
        tokio::time::timeout(Duration::from_secs(1), life.closed())
            .await
            .expect("closed resolves");
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
                idle: Arc::default(),
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
        assert!(
            started.elapsed() < Duration::from_millis(350),
            "the waits ran side by side"
        );
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_detached_continuation_holds_nothing_until_its_wait_ends_and_the_spawner_keeps_its_turn()
     {
        let lua = Lua::new();
        let (inside, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        set_entry(
            &lua,
            Arc::new(Counting {
                gate: Arc::new(tokio::sync::Mutex::new(())),
                inside: inside.clone(),
                most: most.clone(),
                refuse_park: Arc::default(),
                idle: Arc::default(),
            }),
        );
        // the spawner's call holds the VM; the slice it runs starts a wait (the future is created and polled
        // once inside the slice) and the continuation finishes it later, outside the spawner's call
        let mut continuation = None;
        inbound(&lua, Inherited::default(), async {
            let mut wait = Box::pin(idle(tokio::time::sleep(Duration::from_millis(80))));
            let ((), carried) = first_slice(
                &lua,
                Inherited::default(),
                || {
                    let waker = futures::task::noop_waker();
                    let mut cx = std::task::Context::from_waker(&waker);
                    assert!(std::future::Future::poll(wait.as_mut(), &mut cx).is_pending());
                },
                |()| true,
            );
            assert_eq!(
                inside.load(Ordering::SeqCst),
                1,
                "the spawner still holds its turn"
            );
            let lua = lua.clone();
            continuation = Some(tokio::spawn(async move {
                resumed(&lua, Inherited::default(), carried, async move {
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

    fn counting_with(idle: &Arc<Mutex<Vec<IdleKind>>>) -> Arc<Counting> {
        Arc::new(Counting {
            gate: Arc::new(tokio::sync::Mutex::new(())),
            inside: Arc::default(),
            most: Arc::default(),
            refuse_park: Arc::default(),
            idle: idle.clone(),
        })
    }

    /// A timer wait, as `task.wait` makes it.
    fn sleep_for(time: Duration) -> impl Future<Output = mlua::Result<()>> + Send + 'static {
        leaf(Role::Timer, async move {
            tokio::time::sleep(time).await;
            Ok(())
        })
    }

    #[tokio::test]
    async fn a_parked_call_is_told_whether_it_sleeps_at_a_timer_or_waits_for_something_else() {
        let lua = Lua::new();
        let told = Arc::new(Mutex::new(vec![]));
        set_entry(&lua, counting_with(&told));
        inbound(&lua, Inherited::default(), async {
            sleep_for(Duration::from_millis(5)).await.expect("timer");
            wait_for(Duration::from_millis(5)).await.expect("wait");
        })
        .await;
        assert_eq!(*told.lock(), [IdleKind::Timer, IdleKind::Other]);
    }

    #[tokio::test]
    async fn the_continuation_of_a_task_exists_when_its_first_slice_ends_and_knows_what_it_waits_at()
     {
        let lua = Lua::new();
        let told = Arc::new(Mutex::new(vec![]));
        set_entry(&lua, counting_with(&told));
        // a slice that stops at a timer, one that stops at another wait, and one that does not stop at a wait
        let waits: [(BoxFuture<'static, mlua::Result<()>>, bool, Vec<IdleKind>); 3] = [
            (
                Box::pin(sleep_for(Duration::from_secs(60))),
                true,
                vec![IdleKind::Timer],
            ),
            (
                Box::pin(wait_for(Duration::from_secs(60))),
                true,
                vec![IdleKind::Other],
            ),
            (Box::pin(wait_for(Duration::from_secs(60))), false, vec![]),
        ];
        for (mut wait, kept, expected) in waits {
            told.lock().clear();
            let ((), carried) = first_slice(
                &lua,
                Inherited::default(),
                || {
                    let waker = futures::task::noop_waker();
                    let mut cx = std::task::Context::from_waker(&waker);
                    assert!(std::future::Future::poll(wait.as_mut(), &mut cx).is_pending());
                },
                |()| kept,
            );
            // told before anything polls the continuation: the call exists from the end of the slice
            assert_eq!(*told.lock(), expected);
            assert_eq!(carried.baton.is_some(), kept);
        }
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
                idle: Arc::default(),
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
                    assert_eq!(
                        inside.load(Ordering::SeqCst),
                        1,
                        "this call still holds the VM"
                    );
                    assert!(
                        !other_ran.load(Ordering::SeqCst),
                        "nothing else ran inside it"
                    );
                })
                .await;
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = {
            let (lua, other_ran) = (lua.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async {
                    other_ran.store(true, Ordering::SeqCst)
                })
                .await;
            })
        };
        first.await.expect("first call");
        second.await.expect("second call");
        assert!(
            other_ran.load(Ordering::SeqCst),
            "it ran once the first call ended"
        );
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
                idle: Arc::default(),
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
                        assert_eq!(
                            inside_now.load(Ordering::SeqCst),
                            1,
                            "still holding while it waits"
                        );
                        assert!(
                            !other_now.load(Ordering::SeqCst),
                            "nothing else ran inside the call"
                        );
                        Ok(())
                    })
                    .await
                    .expect("leaf");
                    for _ in 0..5 {
                        tokio::task::yield_now().await;
                    }
                    assert!(
                        !other_ran.load(Ordering::SeqCst),
                        "nothing else ran inside the call"
                    );
                })
                .await;
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        let second = {
            let (lua, other_ran) = (lua.clone(), other_ran.clone());
            tokio::spawn(async move {
                inbound(&lua, Inherited::default(), async {
                    other_ran.store(true, Ordering::SeqCst)
                })
                .await;
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
                idle: Arc::default(),
            }),
        );
        let done = inbound(&lua, Inherited::default(), async {
            let mut waiting = Box::pin(hold());
            let _ = first_slice(
                &lua,
                Inherited::default(),
                || {
                    let waker = futures::task::noop_waker();
                    let mut cx = Context::from_waker(&waker);
                    assert!(
                        waiting.as_mut().poll(&mut cx).is_pending(),
                        "the spawner holds the VM"
                    );
                },
                |()| true,
            );
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .is_ok()
        })
        .await;
        assert!(
            done,
            "the call that holds the VM finished it instead of waiting for its own turn"
        );
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
        let (inside, refuse_park) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        set_entry(
            lua,
            Arc::new(Counting {
                gate: Arc::new(tokio::sync::Mutex::new(())),
                inside: inside.clone(),
                most: Arc::default(),
                refuse_park: refuse_park.clone(),
                idle: Arc::default(),
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
            let Poll::Ready(late) = poll_once(&mut a) else {
                panic!("an abandoned leaf is over")
            };
            assert!(late.unwrap_err().to_string().contains("abandoned"));

            let mut b = Box::pin(stuck(Role::Mixed, &second));
            assert!(poll_once(&mut b).is_pending());
            std::mem::forget(b);
            let mut c = Box::pin(stuck(Role::Wait, &third));
            assert!(poll_once(&mut c).is_pending());
            assert!(
                second.load(Ordering::SeqCst),
                "a leaf started after it abandoned it as well"
            );
            std::mem::forget(c);
        })
        .await;
        assert!(
            third.load(Ordering::SeqCst),
            "the end of the call abandoned what it was still waiting at"
        );
    }

    /// A leaf that has finished is over: polling it again (a script resumed a coroutine it should not have) is an
    /// error, not a resumed completed future.
    #[tokio::test]
    async fn a_finished_leaf_polled_again_is_an_error_not_a_panic() {
        let mut once = Box::pin(leaf(Role::Mixed, async { Ok(7) }));
        assert!(matches!(poll_once(&mut once), Poll::Ready(Ok(7))));
        let Poll::Ready(again) = poll_once(&mut once) else {
            panic!("over")
        };
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
            assert!(
                dropped.load(Ordering::SeqCst),
                "dropping it drops its body, as always"
            );
        })
        .await;
    }

    /// A first slice that stopped at a wait keeps the leaf alive for its continuation; one that stopped anywhere else
    /// abandons it. A continuation that is dropped before it ever runs abandons the leaf it was carrying.
    #[tokio::test]
    async fn the_first_slice_of_a_task_keeps_a_leaf_only_when_it_stopped_at_it() {
        let lua = Lua::new();
        counting(&lua);
        let (kept, left) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        inbound(&lua, Inherited::default(), async {
            let (mut a, mut b) = (
                Box::pin(stuck(Role::Wait, &kept)),
                Box::pin(stuck(Role::Wait, &left)),
            );
            let ((), carried) = first_slice(
                &lua,
                Inherited::default(),
                || assert!(poll_once(&mut a).is_pending()),
                |()| true,
            );
            let _ = first_slice(
                &lua,
                Inherited::default(),
                || assert!(poll_once(&mut b).is_pending()),
                |()| false,
            );
            assert!(!kept.load(Ordering::SeqCst), "the continuation finishes it");
            assert!(left.load(Ordering::SeqCst), "the slice went on past it");
            // the continuation is aborted before its first poll: nobody will ever finish the wait
            drop(carried);
            assert!(
                kept.load(Ordering::SeqCst),
                "a continuation that never ran abandoned the leaf it carried"
            );
        })
        .await;
    }

    /// A wait that the test polls by hand from more than one place.
    type SharedWait = Arc<Mutex<Pin<Box<dyn Future<Output = mlua::Result<()>> + Send>>>>;

    /// Rule 5: a call that is resumed anywhere but into a live leaf (here: the leaf it was suspended at was finished
    /// by someone else while the call was lent) queues for the turn, and runs script code only after it has it.
    #[tokio::test]
    async fn a_call_resumed_without_its_leaf_takes_the_turn_before_it_runs_script_code() {
        let lua = Lua::new();
        counting(&lua);
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        // the leaf is polled by hand with a waker that wakes nobody, so only the test resumes the call
        let wait: SharedWait = Arc::new(Mutex::new(Box::pin(wait_for(Duration::from_millis(10)))));
        let call_waker = Arc::new(Mutex::new(None::<std::task::Waker>));
        let first = tokio::spawn({
            let (lua, wait, call_waker, order) =
                (lua.clone(), wait.clone(), call_waker.clone(), order.clone());
            async move {
                inbound(&lua, Inherited::default(), async {
                    let mut suspended = false;
                    std::future::poll_fn(|cx| {
                        if suspended {
                            return Poll::Ready(());
                        }
                        suspended = true;
                        *call_waker.lock() = Some(cx.waker().clone());
                        let noop = futures::task::noop_waker();
                        assert!(
                            wait.lock()
                                .as_mut()
                                .poll(&mut Context::from_waker(&noop))
                                .is_pending()
                        );
                        // the call is suspended at an idle wait: it lends the turn
                        Poll::Pending
                    })
                    .await;
                    // script code
                    order.lock().push("first runs script code");
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        // another call takes the lent turn and keeps it for a while
        let second = tokio::spawn({
            let (lua, order) = (lua.clone(), order.clone());
            async move {
                inbound(&lua, Inherited::default(), async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    order.lock().push("second ends");
                })
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        // someone else finishes the leaf the first call was suspended at, and the first call is resumed: its leaf
        // is gone, so it must not run script code in the turn of the second
        let finished = wait
            .lock()
            .as_mut()
            .poll(&mut Context::from_waker(&futures::task::noop_waker()));
        assert!(matches!(finished, Poll::Ready(Ok(()))));
        call_waker
            .lock()
            .take()
            .expect("the first call suspended")
            .wake();
        first.await.expect("first call");
        second.await.expect("second call");
        assert_eq!(*order.lock(), ["second ends", "first runs script code"]);
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
        assert_eq!(
            inside.load(Ordering::SeqCst),
            0,
            "suspended at an idle wait: lent"
        );

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
        assert_eq!(
            inside.load(Ordering::SeqCst),
            1,
            "suspended at something that is not an idle wait: kept"
        );
        call.abort();

        // an idle wait where the baton says no (the embedder's section was opened meanwhile): kept
        let lua = Lua::new();
        let (inside, refuse_park) = counting(&lua);
        let call = tokio::spawn({
            let lua = lua.clone();
            async move {
                inbound(&lua, Inherited::default(), async {
                    // the wait is allowed when it starts, and by the time it is pending the call may not wait
                    // (the embedder's state changed where the wait was left in flight)
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
        assert_eq!(
            inside.load(Ordering::SeqCst),
            1,
            "the baton refused at the moment of lending: kept"
        );
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
        assert_eq!(
            inside.load(Ordering::SeqCst),
            1,
            "a plain yield after a wait is not a wait"
        );
        call.abort();
    }

    #[tokio::test]
    async fn without_an_entry_callbacks_run_and_idle_is_a_plain_wait() {
        let lua = Lua::new();
        let out = inbound(&lua, Inherited::default(), async {
            idle(async { 7 }).await.expect("idle")
        })
        .await;
        assert_eq!(out, 7);
    }
}

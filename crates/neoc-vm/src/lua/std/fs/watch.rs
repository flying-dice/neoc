//! `fs.watch`: the changes inside a directory, as the operating system reports them, without rescanning it.
//!
//! The watching is the `notify` crate's (inotify on Linux, FSEvents on macOS, `ReadDirectoryChangesW` on Windows):
//! it calls back on a thread of its own, and the callback puts each change in a bounded [`Queue`] that
//! `Watcher:receive` takes from. Nothing here holds the engine: the callback holds the queue only, the engine's
//! [`Life`] holds the watch weakly, and closing it (`close`, the engine's shutdown or `stop_background`, or the
//! handle being collected) stops the operating system's watch and ends every wait on it.
//!
//! **Overflow.** At most [`CAPACITY`] changes wait in the queue. When it is full the next change is dropped and an
//! `overflow` event is put after the ones already waiting; until the script has received it, further changes are
//! dropped too (the script rescans once it sees `overflow`, which covers them). The operating system's own overflow
//! (inotify's queue, an FSEvents "must scan subdirectories") is reported the same way, so a busy directory costs at
//! most [`CAPACITY`] queued paths.
//!
//! **Platforms.**
//!
//! - *Coalescing.* Nothing here deduplicates or debounces. FSEvents (macOS) reports per path with flags that can
//!   accumulate, so one change may arrive as several kinds (a new file as `create` and `modify`); inotify and
//!   Windows report each operation.
//! - *Renames.* One `rename` per path, never paired: inotify's paired report (`RenameMode::Both`, sent after the two
//!   halves) is dropped so Linux matches Windows, which reports the halves only; FSEvents cannot say which path is
//!   the old one and which the new.
//! - *Recursion.* Native on macOS and Windows. On Linux each directory is one inotify watch, added for the tree at
//!   start and for new subdirectories as they appear (bounded by `fs.inotify.max_user_watches`), and each watcher is
//!   one inotify instance (`fs.inotify.max_user_instances`, 128 by default: inotify(7)).
//! - *Paths.* FSEvents reports resolved paths; they are reported under the path the script gave ([`Roots`]). A name
//!   that is not UTF-8 (a FAT32 volume on macOS reports one) is reported as its bytes: notify 9 takes such paths,
//!   where notify 8's FSEvents callback panicked on them.
//! - *Start.* FSEvents may also report a change made just before the watch started.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mlua::{Lua, UserData, UserDataMethods};
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use parking_lot::Mutex;
use tokio::sync::Notify;

use super::error::{FsError, FsResult, Kind};
use crate::lua::facade::DeclType;
use crate::lua::lifecycle::{Life, Role, async_method, hold, idle};
use crate::lua::sandbox::{from_err, ok};
use crate::lua::std::osstr;

/// How many changes wait for `receive` before the rest are reported as one `overflow` (the `fs.watch` doc says 4096).
pub(super) const CAPACITY: usize = 4096;

/// The error prefix of `Watcher:receive` (as `thread.receive` is the prefix of a worker handle's method).
const RECEIVE: &str = "fs.receive";

/// What happened, as a script sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    Create,
    Modify,
    Remove,
    Rename,
    /// Changes were lost (the queue or the operating system's buffer was full): rescan.
    Overflow,
}

impl Change {
    fn as_str(self) -> &'static str {
        match self {
            Change::Create => "create",
            Change::Modify => "modify",
            Change::Remove => "remove",
            Change::Rename => "rename",
            Change::Overflow => "overflow",
        }
    }
}

/// One entry of the queue: a change at a path, or an error the watch reported.
#[derive(Debug)]
pub(super) enum Item {
    Change(Change, PathBuf),
    Failed(FsError),
}

/// What [`Queue::next`] hands out.
#[derive(Debug)]
pub(super) enum Next {
    Item(Item),
    /// Changes were dropped since the last item: the `overflow` event.
    Overflow,
    /// The watch is closed: no more events.
    Closed,
}

struct State {
    items: VecDeque<Item>,
    /// Changes were dropped after `items`; new ones are dropped until a receiver takes the overflow.
    overflowed: bool,
    closed: bool,
}

/// The bounded queue between the watching thread and `receive`.
pub(super) struct Queue {
    state: Mutex<State>,
    capacity: usize,
    ready: Notify,
}

impl Queue {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State {
                items: VecDeque::new(),
                overflowed: false,
                closed: false,
            }),
            capacity,
            ready: Notify::new(),
        }
    }

    /// Queue `item` (from the watching thread). Dropped once the queue is closed, and while an overflow is pending.
    pub(super) fn push(&self, item: Item) {
        {
            let mut state = self.state.lock();
            if state.closed || state.overflowed {
                return;
            }
            if state.items.len() >= self.capacity {
                state.overflowed = true;
            } else {
                state.items.push_back(item);
            }
        }
        self.ready.notify_waiters();
    }

    /// Queue an overflow: the operating system lost changes.
    fn push_overflow(&self) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.overflowed = true;
        }
        self.ready.notify_waiters();
    }

    fn try_next(&self) -> Option<Next> {
        let mut state = self.state.lock();
        if let Some(item) = state.items.pop_front() {
            return Some(Next::Item(item));
        }
        if state.overflowed {
            state.overflowed = false;
            return Some(Next::Overflow);
        }
        state.closed.then_some(Next::Closed)
    }

    fn has_next(&self) -> bool {
        let state = self.state.lock();
        !state.items.is_empty() || state.overflowed || state.closed
    }

    /// Resolves once [`Queue::try_next`] has something to hand out, without taking it: the caller takes it with
    /// [`Queue::try_next`] in the poll that returns it, so a wait that is dropped (abandoned, timed out, cancelled)
    /// takes nothing. Another taker may get there first; the caller then waits again.
    async fn wait_ready(&self) {
        loop {
            // registered before looking, so a push between the look and the wait still wakes it
            let notified = self.ready.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.has_next() {
                return;
            }
            notified.await;
        }
    }

    /// The next item, waiting for one. Any number of waits may be pending; each item goes to exactly one.
    #[cfg(test)]
    pub(super) async fn next(&self) -> Next {
        loop {
            self.wait_ready().await;
            if let Some(next) = self.try_next() {
                return next;
            }
        }
    }

    /// End the queue because the watch ended on its own (its thread died): what waits is still handed out, then
    /// `item` (the reason), then the end. Refuses what comes after, as a closed queue does. Nothing happens to a
    /// queue that is already closed (the script closed the watch: the end it asked for is no failure).
    pub(super) fn fail(&self, item: Item) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            // an overflow still pending is dropped: once the watch has failed the script starts a new one and
            // rescans, which covers what it would have rescanned for
            state.overflowed = false;
            state.items.push_back(item);
            state.closed = true;
        }
        self.ready.notify_waiters();
    }

    /// Close the queue: drop what waits, refuse what comes, and end every wait.
    pub(super) fn close(&self) {
        let dropped = {
            let mut state = self.state.lock();
            state.closed = true;
            state.overflowed = false;
            std::mem::take(&mut state.items)
        };
        drop(dropped);
        self.ready.notify_waiters();
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.state.lock().items.len()
    }
}

/// The path the script gave and the same directory resolved, to report every change under the path the script gave
/// (FSEvents reports resolved paths: `/private/var/...` for `/var/...` on macOS).
struct Roots {
    given: PathBuf,
    resolved: Option<PathBuf>,
}

impl Roots {
    fn relocate(&self, path: PathBuf) -> PathBuf {
        let Some(resolved) = &self.resolved else {
            return path;
        };
        match path.strip_prefix(resolved) {
            Ok(rest) if rest.as_os_str().is_empty() => self.given.clone(),
            Ok(rest) => self.given.join(rest),
            Err(_) => path,
        }
    }
}

/// What a `notify` event is to a script, or `None` for one that changes nothing.
fn classify(event: &notify::Event) -> Option<Change> {
    if event.need_rescan() {
        return Some(Change::Overflow);
    }
    match event.kind {
        EventKind::Create(_) => Some(Change::Create),
        EventKind::Remove(_) => Some(Change::Remove),
        // inotify pairs a rename after reporting both halves (`From`, then `To`): a third report of the same rename
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => None,
        EventKind::Modify(ModifyKind::Name(_)) => Some(Change::Rename),
        EventKind::Modify(_) | EventKind::Any => Some(Change::Modify),
        // an open, a read or a close changes nothing
        EventKind::Access(_) => None,
        // a catch-all notify 8.2 sends only with the rescan flag (handled above); one without it says nothing usable
        // about a path, so the script is told to rescan rather than told nothing
        EventKind::Other => Some(Change::Overflow),
    }
}

/// What the watching thread's callback holds: the queue it fills and the roots it reports under.
///
/// A panic in the callback loses that event and nothing else: it is reported as an `overflow` (the script rescans)
/// and the watch goes on, on every platform. If the thread itself dies (a panic in notify's own code, on a backend
/// whose thread owns the callback: inotify, kqueue), the callback is dropped as the thread unwinds and the watch
/// fails: `receive` hands out what was queued, then an error saying the watch stopped, instead of waiting for
/// changes that will never come.
struct Sink {
    queue: Arc<Queue>,
    roots: Roots,
}

impl Sink {
    /// Classify, relocate and queue. A panic doing so is an overflow: the event is lost, so the script must rescan.
    fn deliver(&self, received: notify::Result<notify::Event>) {
        if crate::lua::panics::caught_sync(|| self.queue_event(received)).is_err() {
            self.queue.push_overflow();
        }
    }

    fn queue_event(&self, received: notify::Result<notify::Event>) {
        // a way for the tests to make the callback panic
        #[cfg(test)]
        if received.as_ref().is_ok_and(|event| {
            event
                .paths
                .iter()
                .any(|path| path.ends_with("panic-in-the-callback"))
        }) {
            panic!("the callback broke");
        }
        match received {
            Ok(event) => match classify(&event) {
                None => {}
                Some(Change::Overflow) => self.queue.push_overflow(),
                Some(change) => {
                    for path in event.paths {
                        self.queue
                            .push(Item::Change(change, self.roots.relocate(path)));
                    }
                }
            },
            Err(e) => self
                .queue
                .push(Item::Failed(watch_error(&e, &self.roots.given))),
        }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        // dropped without a panic: the watch was closed, and closing it closed the queue
        if std::thread::panicking() {
            self.queue.fail(Item::Failed(FsError::at(
                Kind::Other,
                &self.roots.given,
                "the watch stopped: the thread that watched the directory panicked",
            )));
        }
    }
}

/// A `notify` error as an `std:fs` error about `root` (or the path the error names).
fn watch_error(e: &notify::Error, root: &Path) -> FsError {
    let path = e.paths.first().map_or(root, PathBuf::as_path);
    match &e.kind {
        notify::ErrorKind::Io(io) => FsError::io(io, path),
        notify::ErrorKind::PathNotFound => {
            FsError::at(Kind::NotFound, path, "no such file or directory")
        }
        notify::ErrorKind::InvalidConfig(_) => FsError::at(Kind::InvalidInput, path, e.to_string()),
        notify::ErrorKind::MaxFilesWatch => FsError::at(
            Kind::Other,
            path,
            "the operating system's limit on watches is reached (fs.inotify.max_user_watches on Linux)",
        ),
        // `Generic`, `WatchNotFound`, `WatchStartFailed` and whatever a later notify adds (the enum is non-exhaustive)
        _ => FsError::at(Kind::Other, path, e.to_string()),
    }
}

/// A started watch: the operating system's watcher and the queue it fills.
struct WatchInner {
    watcher: Mutex<Option<RecommendedWatcher>>,
    queue: Arc<Queue>,
    root: PathBuf,
}

impl WatchInner {
    /// Stop watching and end every wait. Idempotent, and safe while the watching thread is delivering: the watcher is
    /// dropped (which waits for that thread on macOS) with no lock held, and the queue refuses what still arrives.
    ///
    /// The queue is closed first, so every wait ends whatever the drop does. A watcher whose thread has died cannot
    /// hang the drop (notify 9 stops FSEvents without waiting for its run loop, and the other backends only send a
    /// message), but notify unwraps that thread's result there (`join().expect`, `send().unwrap()`): the panic is
    /// caught here, so a close, a collection or the engine's shutdown cannot unwind out of it. (The thread's own
    /// panic has already failed the queue: see [`Sink`]. Under `panic = "abort"` it ended the process instead.)
    fn close(&self) {
        let watcher = self.watcher.lock().take();
        self.queue.close();
        if let Some(watcher) = watcher {
            let dropped =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(watcher)));
            if dropped.is_err() {
                tracing::warn!(target: "lua::lib::fs", root = %self.root.display(), "the watcher's thread had died");
            }
        }
    }
}

impl Drop for WatchInner {
    fn drop(&mut self) {
        self.close();
    }
}

/// Start watching the directory `root`, the whole tree under it when `recursive`. Blocking (a recursive inotify
/// watch adds one watch per directory of the tree): it runs on the blocking pool, and [`WatchHandle::register`]
/// ties the result to the engine.
pub(super) fn start(root: PathBuf, recursive: bool) -> FsResult<WatchHandle> {
    let meta = std::fs::metadata(&root).map_err(|e| FsError::io(&e, &root))?;
    if !meta.is_dir() {
        return Err(FsError::at(Kind::NotADirectory, &root, "not a directory"));
    }
    let queue = Arc::new(Queue::new(CAPACITY));
    let roots = Roots {
        resolved: std::fs::canonicalize(&root).ok(),
        given: root.clone(),
    };
    let sink = Sink {
        queue: Arc::clone(&queue),
        roots,
    };
    let mut watcher = notify::recommended_watcher(move |received| sink.deliver(received))
        .map_err(|e| watch_error(&e, &root))?;
    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher
        .watch(&root, mode)
        .map_err(|e| watch_error(&e, &root))?;
    Ok(WatchHandle(Arc::new(WatchInner {
        watcher: Mutex::new(Some(watcher)),
        queue,
        root,
    })))
}

/// `receive`'s timeout: `nil` or `math.huge` waits for ever, a negative or NaN one is `invalid_input`.
fn timeout_of(secs: Option<f64>) -> FsResult<Option<Duration>> {
    match secs {
        None => Ok(None),
        Some(s) if s.is_nan() || s < 0.0 => Err(FsError::new(
            Kind::InvalidInput,
            "the timeout must be a non-negative number of seconds",
        )),
        // past what a Duration holds (hundreds of billions of years) is no limit
        Some(s) => Ok(Duration::try_from_secs_f64(s).ok()),
    }
}

/// The `{ kind, path }` table of one event.
fn event(lua: &Lua, change: Change, path: &Path) -> mlua::Result<mlua::Table> {
    let table = lua.create_table_with_capacity(0, 2)?;
    table.raw_set("kind", change.as_str())?;
    table.raw_set("path", osstr::to_lua(lua, path.as_os_str())?)?;
    Ok(table)
}

/// The handle `fs.watch` returns.
pub(super) struct WatchHandle(Arc<WatchInner>);

impl WatchHandle {
    /// Register the watch with the engine's [`Life`], weakly (a script that starts and drops watchers in a loop
    /// leaves nothing registered), so its shutdown or `stop_background` closes it; closed at once when the engine is
    /// already shut down.
    pub(super) fn register(self, lua: &Lua) -> Self {
        Life::of(lua).on_shutdown_with(&self.0, |inner| inner.close());
        self
    }
}

/// Wait for the next event of `queue` and take it, `None` once `deadline` passes. The wait lends the call's turn
/// (an [`idle`] wait); the event is taken only after the turn is back, in the poll that returns it, so a call dropped
/// while it queues for its turn loses no event.
async fn receive(
    queue: &Queue,
    deadline: Option<tokio::time::Instant>,
) -> mlua::Result<Option<Next>> {
    loop {
        let ready = match deadline {
            None => {
                idle(queue.wait_ready()).await?;
                true
            }
            Some(deadline) => idle(tokio::time::timeout_at(deadline, queue.wait_ready()))
                .await?
                .is_ok(),
        };
        if let Some(next) = queue.try_next() {
            return Ok(Some(next));
        }
        if !ready {
            return Ok(None);
        }
    }
}

#[neoc_macros::lua_userdata]
impl UserData for WatchHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `Role::Mixed`, not `Role::Wait`: a `Wait` body takes the event when its wait ends and only then queues for
        // the turn, and a call dropped there would lose it. `receive` waits for the operating system through `idle`
        // (an idle wait, as `Wait` is: never `Timer`, a timeout is not all it waits for) and takes the event with the
        // turn held.
        /// Wait for the next change and return it as `{ kind, path }` (see `fs.watch`). Once the watcher is closed it returns `(nil, nil)`, also to a call already waiting. `timeout` (seconds; `nil` or `math.huge` waits for ever, `0` only takes what is already there) ends the wait with `timed_out`; a negative or NaN one is `invalid_input`. An error the watch reports (`fs.receive: <kind>: <path>: <message>`) is returned as `(nil, err)` and the watch goes on. Other tasks, and an embedder's other calls, run while it waits; where Luau cannot yield (a metamethod, a `table.sort` comparator, a `string.gsub` callback) a call that would have to wait fails with the yield error.
        #[lua(
            "Result<{ kind: \"create\" | \"modify\" | \"remove\" | \"rename\" | \"overflow\", path: string }, string>",
            names(secs = "timeout")
        )]
        async_method(
            methods,
            "receive",
            Role::Mixed,
            |lua, this, secs: Option<f64>| {
                let queue = Arc::clone(&this.0.queue);
                let root = this.0.root.clone();
                let limit = timeout_of(secs);
                async move {
                    let limit = match limit {
                        Ok(limit) => limit,
                        Err(e) => {
                            hold().await;
                            return from_err(RECEIVE, e);
                        }
                    };
                    // a deadline past what an Instant holds is no deadline
                    let deadline =
                        limit.and_then(|limit| tokio::time::Instant::now().checked_add(limit));
                    let Some(next) = receive(&queue, deadline).await? else {
                        let secs = limit.map_or(0.0, |limit| limit.as_secs_f64());
                        let e = FsError::at(
                            Kind::TimedOut,
                            &root,
                            format!("no change within {secs} seconds"),
                        );
                        return from_err(RECEIVE, e);
                    };
                    match next {
                        Next::Item(Item::Change(change, path)) => {
                            ok(Some(event(&lua, change, &path)?))
                        }
                        Next::Item(Item::Failed(e)) => from_err(RECEIVE, e),
                        Next::Overflow => ok(Some(event(&lua, Change::Overflow, &root)?)),
                        Next::Closed => ok(None),
                    }
                }
            },
        );
        /// Stop watching: the operating system's watch is released, events not yet received are dropped, and every `receive`, waiting or later, returns `(nil, nil)`. Closing again does nothing.
        methods.add_method("close", |_, this, ()| {
            this.0.close();
            Ok(())
        });
    }
}

impl DeclType for WatchHandle {
    fn decl_type() -> String {
        "Watcher".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str) -> Item {
        Item::Change(Change::Create, PathBuf::from(path))
    }

    fn path_of(next: Next) -> String {
        match next {
            Next::Item(Item::Change(_, path)) => path.display().to_string(),
            other => panic!("expected a change, got {other:?}"),
        }
    }

    /// A full queue drops what comes, keeps what waits, and reports one overflow after it; changes after the overflow
    /// was received are queued again.
    #[tokio::test]
    async fn a_full_queue_reports_one_overflow_after_what_waits() {
        let queue = Queue::new(2);
        for name in ["a", "b", "c", "d", "e"] {
            queue.push(change(name));
        }
        assert_eq!(queue.len(), 2, "bounded");
        assert_eq!(path_of(queue.next().await), "a");
        // room again, but the overflow is still pending: this one is covered by the rescan
        queue.push(change("f"));
        assert_eq!(path_of(queue.next().await), "b");
        assert!(matches!(queue.next().await, Next::Overflow));
        queue.push(change("g"));
        assert_eq!(path_of(queue.next().await), "g");
        assert!(queue.try_next().is_none(), "nothing else");
    }

    /// The operating system's own overflow comes after what already waits, once.
    #[tokio::test]
    async fn an_overflow_from_the_system_comes_after_what_waits() {
        let queue = Queue::new(8);
        queue.push(change("a"));
        queue.push_overflow();
        queue.push_overflow();
        queue.push(change("dropped"));
        assert_eq!(path_of(queue.next().await), "a");
        assert!(matches!(queue.next().await, Next::Overflow));
        assert!(queue.try_next().is_none());
    }

    /// Closing drops what waits and refuses what comes.
    #[tokio::test]
    async fn closing_drops_what_waits_and_refuses_what_comes() {
        let queue = Queue::new(8);
        queue.push(change("a"));
        queue.close();
        queue.push(change("b"));
        assert_eq!(queue.len(), 0);
        assert!(matches!(queue.next().await, Next::Closed));
        assert!(
            matches!(queue.next().await, Next::Closed),
            "and stays closed"
        );
    }

    /// A wait that is pending when the queue closes ends, with `Closed`.
    #[tokio::test]
    async fn closing_ends_a_pending_wait() {
        use std::future::Future;
        let queue = Queue::new(8);
        let mut wait = Box::pin(queue.next());
        let pending =
            std::future::poll_fn(|cx| std::task::Poll::Ready(wait.as_mut().poll(cx).is_pending()))
                .await;
        assert!(pending, "nothing to take yet");
        queue.close();
        let ended = tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .expect("the close woke the wait");
        assert!(matches!(ended, Next::Closed));
    }

    /// A rename is reported once per path: inotify's paired report is dropped, access events too, and the system's
    /// "rescan" is an overflow.
    #[test]
    fn notify_events_map_to_the_five_kinds() {
        use notify::event::{AccessKind, CreateKind, DataChange, Flag, RemoveKind};
        let kind = |k| classify(&notify::Event::new(k));
        assert_eq!(
            kind(EventKind::Create(CreateKind::File)),
            Some(Change::Create)
        );
        assert_eq!(
            kind(EventKind::Remove(RemoveKind::Any)),
            Some(Change::Remove)
        );
        assert_eq!(
            kind(EventKind::Modify(ModifyKind::Data(DataChange::Any))),
            Some(Change::Modify)
        );
        assert_eq!(kind(EventKind::Any), Some(Change::Modify));
        assert_eq!(
            kind(EventKind::Modify(ModifyKind::Name(RenameMode::From))),
            Some(Change::Rename)
        );
        assert_eq!(
            kind(EventKind::Modify(ModifyKind::Name(RenameMode::To))),
            Some(Change::Rename)
        );
        assert_eq!(
            kind(EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
            Some(Change::Rename)
        );
        assert_eq!(
            kind(EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
            None
        );
        assert_eq!(kind(EventKind::Access(AccessKind::Any)), None);
        assert_eq!(kind(EventKind::Other), Some(Change::Overflow));
        let rescan = notify::Event::new(EventKind::Create(CreateKind::Any)).set_flag(Flag::Rescan);
        assert_eq!(classify(&rescan), Some(Change::Overflow));
    }

    /// A resolved path is reported under the path the script gave; any other path is left alone.
    #[test]
    fn paths_are_reported_under_the_path_the_script_gave() {
        let roots = Roots {
            given: PathBuf::from("/var/x"),
            resolved: Some(PathBuf::from("/private/var/x")),
        };
        assert_eq!(
            roots.relocate(PathBuf::from("/private/var/x/a/b")),
            PathBuf::from("/var/x/a/b")
        );
        assert_eq!(
            roots.relocate(PathBuf::from("/private/var/x")),
            PathBuf::from("/var/x")
        );
        assert_eq!(
            roots.relocate(PathBuf::from("/var/x/a")),
            PathBuf::from("/var/x/a")
        );
        let unresolved = Roots {
            given: PathBuf::from("x"),
            resolved: None,
        };
        assert_eq!(
            unresolved.relocate(PathBuf::from("x/a")),
            PathBuf::from("x/a")
        );
    }

    /// The engine's registry holds a watch weakly: dropping the handle frees it and closes the watch, and the watch
    /// does not keep the engine alive.
    #[tokio::test]
    async fn a_watch_holds_neither_itself_nor_its_engine() {
        let dir = tempfile::tempdir().unwrap();
        let lua = crate::lua::sandbox::make_engine().unwrap();
        let handle = start(dir.path().to_path_buf(), false)
            .unwrap()
            .register(&lua);
        let weak_lua = lua.weak();
        drop(lua);
        assert!(
            weak_lua.try_upgrade().is_none(),
            "the engine is gone although its watch is alive"
        );

        let queue = Arc::clone(&handle.0.queue);
        let inner = Arc::downgrade(&handle.0);
        drop(handle);
        assert!(inner.upgrade().is_none(), "nothing else holds the watch");
        assert!(
            matches!(queue.try_next(), Some(Next::Closed)),
            "dropping it closed it"
        );
    }

    /// Shutting the engine down closes its watches (and drops the system watcher), and a watch started afterwards is
    /// closed at once; `stop_background` closes them too.
    #[tokio::test]
    async fn the_engines_shutdown_and_stop_background_close_its_watches() {
        let dir = tempfile::tempdir().unwrap();
        let life = Life::new();
        let lua = crate::lua::sandbox::make_engine_in(life.clone()).unwrap();
        let stopped = start(dir.path().to_path_buf(), false)
            .unwrap()
            .register(&lua);
        life.stop_background();
        assert!(
            stopped.0.watcher.lock().is_none(),
            "the system watcher is released"
        );
        assert!(matches!(stopped.0.queue.try_next(), Some(Next::Closed)));

        let open = start(dir.path().to_path_buf(), true)
            .unwrap()
            .register(&lua);
        assert!(
            open.0.watcher.lock().is_some(),
            "a watch after stop_background works"
        );
        life.shutdown();
        assert!(open.0.watcher.lock().is_none());
        assert!(matches!(open.0.queue.try_next(), Some(Next::Closed)));

        let late = start(dir.path().to_path_buf(), false)
            .unwrap()
            .register(&lua);
        assert!(
            late.0.watcher.lock().is_none(),
            "closed at once after shutdown"
        );
    }

    /// An overflow after the close is refused like any change.
    #[test]
    fn a_closed_queue_refuses_an_overflow() {
        let queue = Queue::new(8);
        queue.close();
        queue.push_overflow();
        assert!(matches!(queue.try_next(), Some(Next::Closed)));
    }

    /// The watching thread's callback: a change per path, relocated; an access is dropped; a rescan is one overflow;
    /// an error is queued as an error.
    #[test]
    fn the_callback_queues_changes_overflows_and_errors() {
        use notify::event::{AccessKind, CreateKind, Flag};
        let sink = sink();
        let queue = &sink.queue;
        let created = notify::Event::new(EventKind::Create(CreateKind::File))
            .add_path(PathBuf::from("/resolved/a"))
            .add_path(PathBuf::from("/resolved/b"));
        sink.deliver(Ok(created));
        let access =
            notify::Event::new(EventKind::Access(AccessKind::Any)).add_path("/resolved/c".into());
        sink.deliver(Ok(access));
        sink.deliver(Err(notify::Error::generic("boom")));
        assert_eq!(path_of(queue.try_next().unwrap()), "/given/a");
        assert_eq!(path_of(queue.try_next().unwrap()), "/given/b");
        match queue.try_next() {
            Some(Next::Item(Item::Failed(e))) => {
                assert_eq!(e.kind(), Kind::Other);
                assert!(e.to_string().contains("/given"), "{e}");
            }
            other => panic!("expected the error, got {other:?}"),
        }
        assert!(queue.try_next().is_none(), "the access was dropped");
        let rescan = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
        sink.deliver(Ok(rescan));
        assert!(matches!(queue.try_next(), Some(Next::Overflow)));
    }

    /// The callback of a watch whose roots are `/given`, reported for `/resolved`.
    fn sink() -> Sink {
        Sink {
            queue: Arc::new(Queue::new(8)),
            roots: Roots {
                given: PathBuf::from("/given"),
                resolved: Some(PathBuf::from("/resolved")),
            },
        }
    }

    /// The thread that holds the callback dies: `receive` hands out what was queued, then says the watch stopped,
    /// then the end, instead of waiting for changes that will never come.
    #[test]
    fn a_watch_whose_thread_panics_hands_out_what_was_queued_then_fails() {
        let sink = sink();
        let queue = Arc::clone(&sink.queue);
        let died = std::thread::spawn(move || {
            let created = notify::Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path(PathBuf::from("/resolved/a"));
            sink.deliver(Ok(created));
            panic!("the watching thread broke");
        })
        .join();
        assert!(died.is_err(), "the thread panicked");
        assert_eq!(path_of(queue.try_next().unwrap()), "/given/a");
        match queue.try_next() {
            Some(Next::Item(Item::Failed(e))) => {
                assert_eq!(e.kind(), Kind::Other);
                assert!(e.to_string().contains("/given"), "{e}");
                assert!(e.to_string().contains("the watch stopped"), "{e}");
            }
            other => panic!("expected the failure, got {other:?}"),
        }
        assert!(matches!(queue.try_next(), Some(Next::Closed)));
        queue.push(change("/late"));
        assert!(
            matches!(queue.try_next(), Some(Next::Closed)),
            "nothing comes after"
        );
    }

    /// A panic in the callback loses that event only: it is an overflow (rescan), and the watch goes on.
    #[test]
    fn a_panic_in_the_callback_is_an_overflow_and_the_watch_goes_on() {
        use notify::event::CreateKind;
        let sink = sink();
        let created = |path: &str| {
            notify::Event::new(EventKind::Create(CreateKind::File)).add_path(path.into())
        };
        sink.deliver(Ok(created("/resolved/a")));
        sink.deliver(Ok(created("/resolved/panic-in-the-callback")));
        assert_eq!(path_of(sink.queue.try_next().unwrap()), "/given/a");
        assert!(matches!(sink.queue.try_next(), Some(Next::Overflow)));
        assert!(sink.queue.try_next().is_none(), "still open");
        sink.deliver(Ok(created("/resolved/b")));
        assert_eq!(
            path_of(sink.queue.try_next().unwrap()),
            "/given/b",
            "the watch goes on"
        );
    }

    /// A callback dropped without a panic (the watch was closed, which closed the queue) fails nothing, and a queue
    /// the script closed stays an ordinary end.
    #[test]
    fn a_callback_dropped_without_a_panic_or_after_the_close_fails_nothing() {
        let sink = sink();
        let queue = Arc::clone(&sink.queue);
        drop(sink);
        assert!(queue.try_next().is_none(), "still open, nothing queued");
        queue.close();
        queue.fail(change("/reason"));
        assert!(matches!(queue.try_next(), Some(Next::Closed)));
    }

    /// Each `notify` error maps to a stable kind about the path it names, or the watched directory.
    #[test]
    fn notify_errors_map_to_stable_kinds() {
        let root = Path::new("/root");
        let kind = |e: notify::Error| watch_error(&e, root);
        let io = kind(notify::Error::io(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert_eq!(io.kind(), Kind::PermissionDenied);
        assert!(
            io.to_string().starts_with("permission_denied: /root: "),
            "{io}"
        );
        let missing = kind(notify::Error::path_not_found().add_path(PathBuf::from("/root/sub")));
        assert_eq!(missing.kind(), Kind::NotFound);
        assert!(
            missing.to_string().starts_with("not_found: /root/sub: "),
            "{missing}"
        );
        assert_eq!(
            kind(notify::Error::new(notify::ErrorKind::MaxFilesWatch)).kind(),
            Kind::Other
        );
        assert_eq!(kind(notify::Error::watch_not_found()).kind(), Kind::Other);
        assert_eq!(kind(notify::Error::generic("x")).kind(), Kind::Other);
        let config = notify::Error::invalid_config(&notify::Config::default());
        assert_eq!(kind(config).kind(), Kind::InvalidInput);
    }

    /// Through Lua: an overflow is `{ kind = "overflow", path = <the watched directory> }`, an error the watch reports
    /// is `(nil, "fs.receive: ...")` and the watch goes on, and a timeout that passes is `timed_out` with the path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn receive_hands_out_overflows_errors_and_timeouts() {
        let dir = tempfile::tempdir().unwrap();
        let lua = crate::lua::sandbox::make_engine().unwrap();
        // no system watcher: a real one could report a change of its own first (FSEvents may report the temp
        // directory's creation), and this test is about what `receive` makes of each kind of queued item
        let queue = Arc::new(Queue::new(CAPACITY));
        let handle = WatchHandle(Arc::new(WatchInner {
            watcher: Mutex::new(None),
            queue: Arc::clone(&queue),
            root: dir.path().to_path_buf(),
        }));
        queue.push(Item::Failed(FsError::at(
            Kind::PermissionDenied,
            dir.path(),
            "denied",
        )));
        queue.push_overflow();
        lua.globals().set("w", handle).unwrap();
        lua.globals()
            .set("dir", dir.path().to_str().unwrap())
            .unwrap();
        lua.load(
            r#"
            local fs = require("std:fs")
            local ev, err = w:receive(5)
            assert(ev == nil and err:find("^fs%.receive: permission_denied: "), tostring(err))
            assert(fs.error_kind(err) == "permission_denied")
            ev, err = w:receive(5)
            assert(err == nil, err)
            assert(ev.kind == "overflow" and ev.path == dir, ev.kind .. " " .. ev.path)
            ev, err = w:receive(0.05)
            assert(ev == nil and fs.error_kind(err) == "timed_out", tostring(err))
            assert(err:find(dir, 1, true) and err:find("no change within 0.05 seconds", 1, true), err)
            w:close()
            "#,
        )
        .exec_async()
        .await
        .unwrap();
    }

    /// notify 8's FSEvents callback panicked on a path that is not UTF-8 (a FAT32 volume reports one) and its
    /// `stop()` then spun for ever; 9.0 fixed both. Held here so a downgrade says why it is wrong.
    #[test]
    fn notify_is_a_version_that_takes_paths_that_are_not_utf8() {
        let lock =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
                .unwrap();
        let versions: Vec<u64> = lock
            .split("[[package]]")
            .filter(|package| package.contains("\nname = \"notify\"\n"))
            .filter_map(|package| package.split("version = \"").nth(1))
            .filter_map(|version| version.split('.').next()?.parse().ok())
            .collect();
        assert!(!versions.is_empty(), "notify is not in Cargo.lock");
        assert!(
            versions.iter().all(|major| *major >= 9),
            "notify {versions:?}: 9.0 at least"
        );
    }

    /// A name that is not UTF-8, made while the directory is watched, is reported as its bytes and the watch closes
    /// cleanly. Skipped where the filesystem refuses such a name (APFS does; Linux filesystems do not).
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_name_that_is_not_utf8_is_reported_as_its_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let handle = start(dir.path().to_path_buf(), false).unwrap();
        let name = dir.path().join(std::ffi::OsStr::from_bytes(b"new\xffname"));
        if let Err(e) = std::fs::write(&name, "x") {
            eprintln!("skipped: this filesystem refuses a name that is not UTF-8 ({e})");
            return;
        }
        let queue = Arc::clone(&handle.0.queue);
        let seen = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match queue.next().await {
                    Next::Item(Item::Change(_, path)) if path == name => return true,
                    Next::Closed => return false,
                    _ => {}
                }
            }
        })
        .await
        .expect("an event for the name within 10 seconds");
        assert!(seen, "the watch ended before reporting the name");
        handle.0.close();
        assert!(handle.0.watcher.lock().is_none());
        assert!(matches!(queue.try_next(), Some(Next::Closed)));
    }

    #[test]
    fn a_timeout_is_a_non_negative_number_of_seconds() {
        assert_eq!(timeout_of(None).unwrap(), None);
        assert_eq!(
            timeout_of(Some(0.5)).unwrap(),
            Some(Duration::from_millis(500))
        );
        assert_eq!(timeout_of(Some(f64::INFINITY)).unwrap(), None);
        assert_eq!(timeout_of(Some(1e300)).unwrap(), None);
        assert_eq!(
            timeout_of(Some(-1.0)).unwrap_err().kind(),
            Kind::InvalidInput
        );
        assert_eq!(
            timeout_of(Some(f64::NAN)).unwrap_err().kind(),
            Kind::InvalidInput
        );
    }
}

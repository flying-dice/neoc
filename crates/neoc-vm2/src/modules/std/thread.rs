//! Worker VMs: `std:thread`.
//!
//! `spawn` starts a fresh Lua engine that runs on the same tokio runtime as its parent and is offered the modules
//! its parent was (see [`set_worker_engine`](crate::engine::set_worker_engine)). Modeled after Node's
//! `worker_threads`: a worker starts from *source* (or a file), takes plain data as its argument, and talks to its
//! parent through a two-way channel, `thread.parent()`.
//!
//! ```lua
//! local thread = require("std:thread")
//! local collections = require("std:collections")
//!
//! local hits = collections.Counter.new()
//! local worker = assert(thread.spawn([[
//!     local thread = require("std:thread")
//!     thread.shared().hits:increment()
//!     return ... * 2
//! ]], 21, { hits = hits }))
//! assert(worker:await() == 42 and hits:get() == 1)
//! ```
//!
//! Closures cannot move between VMs (a Lua function is tied to its VM's bytecode and upvalues), so a worker always
//! starts from source. Its argument, the messages and its return value travel as plain data (a copy, as
//! `serde_json::Value`); what is not plain data (a buffer, a function) is refused, never turned into something else.
//! State is *shared* by handing over handles on it ([`thread::Shared`]): the worker's handle and its parent's are
//! the same map, counter or socket.
//!
//! A worker is registered with the [`Life`](crate::Life) of the script that started it: shutting the script down
//! aborts the worker, whose VM and tasks go with it. A worker whose handle is dropped is aborted too.
//!
//! Ported from neoc-vm's `std:thread`. What changed on purpose:
//! - `spawn(source, args?, shared?, options?)` and `spawn_file(path, ...)` replace `spawn({ source | file, name,
//!   args, shared })`: the source is not an option, and each part has its type.
//! - `thread.parent()` and `thread.shared()` are functions (they were fields set on the worker's module table, which
//!   is read-only now). `shared()` is an empty table, not `nil`, when nothing was shared.
//! - What may be shared is a declared union ([`thread::Shared`]) checked as an argument: neoc-vm's `Sendable` trait,
//!   its registry of types and the type-erased `Arc` it rebuilt handles from are gone (a handle is `Clone + Send`
//!   and is given to the worker as it is). A value that cannot be shared is refused with its place (it was
//!   `nil, "... not Sendable"`), and so is an argument that is not plain data.
//! - Every failure is a typed error: `SpawnError`, `ChannelError`, `AwaitError`. `send` still raises when the other
//!   side has exited.
//! - `WorkerHandle` is `Worker`.

use std::collections::BTreeMap;

use mlua::Value;

use crate::engine::{WorkerEngineFn, load_source};
use crate::lifecycle::Life;
use crate::modules::lib::log::{LogFn, log_sink, set_log_sink};
use crate::panics;

/// What a worker script was started with: its engine's app data.
struct WorkerContext {
    parent: thread::Channel,
    shared: BTreeMap<String, thread::Shared>,
    /// Where the parent's `lib:log` lines go, when its embedder took them: the worker logs there too.
    log_sink: Option<LogFn>,
}

/// Build the worker's engine and run `source` in it with `args`; what it returned, as plain data.
async fn run_worker(
    build_engine: WorkerEngineFn,
    life: Life,
    name: String,
    source: String,
    args: serde_json::Value,
    context: WorkerContext,
) -> mlua::Result<serde_json::Value> {
    // the worker gets the same module set as its parent and spawns into the parent's registry
    let lua = build_engine(life)?;
    if let Some(sink) = context.log_sink.clone() {
        set_log_sink(&lua, sink);
    }
    lua.set_app_data(context);
    // one value (any plain data, a table for "named" arguments) that arrives as the chunk's first vararg, as
    // Node's `workerData` does
    let args = mlua_ir::to_serde(&args, &lua)?;
    // a host function that panics in the worker fails it like an error, which `await` returns (see `panics`)
    let result: Value = panics::caught(load_source(&lua, &source).set_name(&name).call_async(args))
        .await
        .unwrap_or_else(|message| Err(panics::error(&message)))?;
    mlua_ir::from_serde(result, &lua)
        .map_err(|reason| mlua::Error::runtime(format!("the worker's result is not plain data: {reason}")))
}

#[mlua_ir::module("std:thread")]
pub mod thread {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{LuaError, Object, OneOf, Options, Raise, Unknown};
    use serde::{Deserialize, Serialize};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};

    use super::{WorkerContext, log_sink, run_worker};
    use crate::engine::worker_engine;
    use crate::lifecycle::Life;
    use crate::modules::std::collections::collections::{Counter, Map};
    use crate::modules::std::net::net::TcpListener;
    use crate::modules::std::osstr::OsPath;

    mlua_ir::unknown_reason!(
        /// Why what scripts pass each other has no declared shape.
        Passed = "plain data one script passes another (an argument, a message, a result): its shape is the two scripts' own agreement"
    );

    /// One side's messages to the other.
    type Sender = mpsc::UnboundedSender<serde_json::Value>;
    type Receiver = AsyncMutex<mpsc::UnboundedReceiver<serde_json::Value>>;

    /// State a worker shares with the script that started it: a handle on the same map, counter or socket.
    #[derive(Clone, OneOf)]
    pub enum Shared {
        /// A `std:collections` map.
        Map(Map),
        /// A `std:collections` counter.
        Counter(Counter),
        /// A bound socket, for several workers to serve on.
        TcpListener(TcpListener),
    }

    /// How a worker is started.
    #[derive(Debug, Default, Serialize, Deserialize, Options)]
    pub struct SpawnOptions {
        /// What the worker's script is called in its errors and stack traces. When not given: the path for
        /// `spawn_file`, `thread:worker` for `spawn`.
        pub name: Option<String>,
    }

    /// Why a worker could not be started.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum SpawnError {
        /// The worker's script could not be read.
        #[error("read {}: {reason}", .path.0.display())]
        Unreadable {
            /// The path asked for.
            path: OsPath,
            /// What the operating system said.
            reason: String,
        },
        /// This engine's embedder has not said how a worker's engine is built, so its scripts cannot start one.
        #[error(
            "this engine has no worker engine: the embedder must call `set_worker_engine` (or `install_allowed`) \
             before a script can start a worker"
        )]
        NoWorkerEngine,
        /// The call was made outside the async runtime that would run the worker.
        #[error("no tokio runtime in scope")]
        NoRuntime,
    }

    /// Why a message could not be sent or received.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ChannelError {
        /// The other side has exited: nothing reads what is sent, and nothing more will arrive.
        #[error("the other side has exited")]
        Closed,
    }

    /// Why a worker has no result.
    #[derive(Debug, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum AwaitError {
        /// The worker raised an error, a host function it called panicked, its engine could not be built, or it
        /// returned what is not plain data.
        #[error("{reason}")]
        Failed {
            /// What went wrong, as text.
            reason: String,
        },
        /// The worker was terminated, or its script was shut down.
        #[error("terminated")]
        Terminated,
        /// The result was already taken by an earlier `await`.
        #[error("already awaited")]
        AlreadyAwaited,
    }

    type Join = tokio::task::JoinHandle<mlua::Result<serde_json::Value>>;

    struct WorkerInner {
        tx: Sender,
        rx: Receiver,
        join: parking_lot::Mutex<Option<Join>>,
        abort: tokio::task::AbortHandle,
    }

    impl Drop for WorkerInner {
        fn drop(&mut self) {
            // a worker whose handle went out of scope without an `await` or a `terminate` does not run for ever
            self.abort.abort();
        }
    }

    /// A worker, as the script that started it sees it.
    #[derive(Clone, Object)]
    pub struct Worker {
        inner: Arc<WorkerInner>,
    }

    fn send(tx: &Sender, value: Unknown<Passed>) -> Result<(), Raise<ChannelError>> {
        tx.send(value.value).map_err(|_| Raise(ChannelError::Closed))
    }

    async fn receive(rx: &Receiver) -> Result<Unknown<Passed>, ChannelError> {
        rx.lock().await.recv().await.map(Unknown::new).ok_or(ChannelError::Closed)
    }

    #[mlua_ir::object]
    impl Worker {
        /// Send plain data to the worker, which reads it with `thread.parent():receive()`. Raises a `ChannelError`
        /// when the worker has exited.
        pub fn send(&self, value: Unknown<Passed>) -> Result<(), Raise<ChannelError>> {
            send(&self.inner.tx, value)
        }

        /// Wait for a message from the worker. Once it has exited and every message was read: `nil` and a `closed`
        /// error.
        pub async fn receive(&self) -> Result<Unknown<Passed>, ChannelError> {
            receive(&self.inner.rx).await
        }

        /// Wait for the worker to end; what its script returned.
        pub async fn r#await(&self) -> Result<Unknown<Passed>, AwaitError> {
            let join = self.inner.join.lock().take().ok_or(AwaitError::AlreadyAwaited)?;
            match join.await {
                Ok(Ok(value)) => Ok(Unknown::new(value)),
                Ok(Err(e)) => Err(AwaitError::Failed { reason: e.to_string() }),
                Err(e) if e.is_cancelled() => Err(AwaitError::Terminated),
                Err(e) => Err(AwaitError::Failed { reason: e.to_string() }),
            }
        }

        /// Abort the worker at once. An `await` after it gives a `terminated` error.
        pub fn terminate(&self) {
            self.inner.abort.abort();
        }
    }

    /// A worker's line to the script that started it.
    #[derive(Clone, Object)]
    pub struct Channel {
        tx: Sender,
        rx: Arc<Receiver>,
    }

    #[mlua_ir::object]
    impl Channel {
        /// Send plain data to the parent, which reads it with `worker:receive()`. Raises a `ChannelError` when the
        /// parent has let go of the worker.
        pub fn send(&self, value: Unknown<Passed>) -> Result<(), Raise<ChannelError>> {
            send(&self.tx, value)
        }

        /// Wait for a message from the parent. Once the parent has let go of the worker: `nil` and a `closed`
        /// error.
        pub async fn receive(&self) -> Result<Unknown<Passed>, ChannelError> {
            receive(&self.rx).await
        }
    }

    fn start(
        lua: &Lua,
        name: String,
        source: String,
        args: Option<Unknown<Passed>>,
        shared: Option<BTreeMap<String, Shared>>,
    ) -> Result<Worker, SpawnError> {
        // a worker is built like its parent's engine, or not at all
        let build_engine = worker_engine(lua).ok_or(SpawnError::NoWorkerEngine)?;
        tokio::runtime::Handle::try_current().map_err(|_| SpawnError::NoRuntime)?;

        // two unbounded queues: parent to worker and worker to parent. The worker's channel is the other end of
        // each.
        let (to_worker, from_parent) = mpsc::unbounded_channel();
        let (to_parent, from_worker) = mpsc::unbounded_channel();
        let context = WorkerContext {
            parent: Channel { tx: to_parent, rx: Arc::new(AsyncMutex::new(from_parent)) },
            shared: shared.unwrap_or_default(),
            log_sink: log_sink(lua),
        };
        let args = args.map_or(serde_json::Value::Null, |args| args.value);

        // registered with the script that started it: shutting the script down aborts the worker, whose VM goes
        // with it. A worker's own tasks and workers join the same registry.
        let life = Life::of(lua);
        let join = life.spawn(run_worker(build_engine, life.clone(), name, source, args, context));
        let abort = join.abort_handle();
        Ok(Worker {
            inner: Arc::new(WorkerInner {
                tx: to_worker,
                rx: AsyncMutex::new(from_worker),
                join: parking_lot::Mutex::new(Some(join)),
                abort,
            }),
        })
    }

    /// Start a worker: a new Lua VM that runs `source` alongside this one. `args` (plain data) is the script's
    /// `...`; each handle in `shared` is the worker's under the same name in `thread.shared()`.
    ///
    /// ```luau
    /// local worker = assert(thread.spawn("local n = ... return n * 2", 21))
    /// local doubled, err = worker:await()
    /// assert(doubled == 42 and err == nil)
    /// ```
    pub fn spawn(
        lua: &Lua,
        source: String,
        args: Option<Unknown<Passed>>,
        shared: Option<BTreeMap<String, Shared>>,
        options: SpawnOptions,
    ) -> Result<Worker, SpawnError> {
        let name = options.name.unwrap_or_else(|| "thread:worker".to_owned());
        start(lua, name, source, args, shared)
    }

    /// `spawn`, with the script read from the file at `path`.
    pub fn spawn_file(
        lua: &Lua,
        path: OsPath,
        args: Option<Unknown<Passed>>,
        shared: Option<BTreeMap<String, Shared>>,
        options: SpawnOptions,
    ) -> Result<Worker, SpawnError> {
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(e) => return Err(SpawnError::Unreadable { path, reason: e.to_string() }),
        };
        // named after the file, so that stack traces point at it
        let name = options.name.unwrap_or_else(|| path.0.to_string_lossy().into_owned());
        start(lua, name, source, args, shared)
    }

    /// In a worker: the channel to the script that started it. `nil` in a script that is not a worker.
    pub fn parent(lua: &Lua) -> Option<Channel> {
        lua.app_data_ref::<WorkerContext>().map(|context| context.parent.clone())
    }

    /// In a worker: the handles it was started with, by name. Empty when none were, and in a script that is not a
    /// worker.
    pub fn shared(lua: &Lua) -> BTreeMap<String, Shared> {
        lua.app_data_ref::<WorkerContext>().map(|context| context.shared.clone()).unwrap_or_default()
    }

    /// How many threads the process can usefully run at once (the logical cores it may use). At least 1.
    ///
    /// ```luau
    /// assert(thread.available_parallelism() >= 1)
    /// ```
    pub fn available_parallelism() -> u32 {
        std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use mlua::{FromLuaMulti, Lua};

    use crate::Life;

    fn engine() -> Lua {
        engine_in(Life::new())
    }

    fn engine_in(life: Life) -> Lua {
        let lua = crate::make_engine_in(life).expect("engine");
        for (global, module) in [("thread", "std:thread"), ("collections", "std:collections")] {
            let table: mlua::Table = lua.load(format!("return require({module:?})")).eval().unwrap();
            lua.globals().set(global, table).unwrap();
        }
        lua
    }

    async fn eval<T: FromLuaMulti>(lua: &Lua, code: &str) -> T {
        lua.load(code).eval_async().await.unwrap_or_else(|e| panic!("{e}"))
    }

    async fn run<T: FromLuaMulti>(code: &str) -> T {
        eval(&engine(), code).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn available_parallelism_returns_positive() {
        let n: i64 = run("return thread.available_parallelism()").await;
        assert!(n >= 1, "should return at least 1, got: {n}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_takes_its_argument_and_returns_a_value() {
        let (doubled, plain, named, nothing): (i64, String, String, bool) = run(r#"
            local w, err = thread.spawn("local args = ...; return args.v * 2", { v = 21 })
            assert(err == nil)
            local doubled, await_err = w:await()
            assert(await_err == nil)
            local plain = assert(thread.spawn("return 'ok'")):await()
            -- a table comes back as a copy of its data
            local t = assert(thread.spawn("return { name = ..., tags = { 'a', 'b' } }", "n")):await()
            local nothing = assert(thread.spawn("return ... == nil")):await()
            return doubled, plain, t.name .. t.tags[1] .. t.tags[2], nothing
            "#)
        .await;
        assert_eq!((doubled, plain.as_str(), named.as_str(), nothing), (42, "ok", "nab", true));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_that_fails_gives_a_typed_error_naming_its_script() {
        let (kind, reason, text, named): (String, String, String, String) = run(r#"
            local value, err = assert(thread.spawn("error('boom')")):await()
            assert(value == nil)
            local _, named = assert(thread.spawn("error('boom')", nil, nil, { name = "sums" })):await()
            return err.kind, err.reason, tostring(err), named.reason
            "#)
        .await;
        assert_eq!(kind, "failed");
        assert!(reason.contains("boom") && reason.contains("thread:worker"), "{reason}");
        assert!(text.starts_with("std:thread.Worker:await: failed: "), "{text}");
        assert!(named.contains("sums") && !named.contains("thread:worker"), "{named}");
    }

    /// A worker logs where its parent logs: the embedder's sink is handed to the worker's engine.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_logs_to_its_parents_sink() {
        let e = engine();
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = lines.clone();
        crate::modules::lib::log::set_log_sink(
            &e,
            std::sync::Arc::new(move |level, line| kept.lock().unwrap().push(format!("{level} {line}"))),
        );
        let done: String = eval(
            &e,
            r#"
            require("lib:log").info("from the parent")
            return (assert(thread.spawn([[
                require("lib:log").warn("from the worker", { n = 1 })
                return "done"
            ]])):await())
            "#,
        )
        .await;
        assert_eq!(done, "done");
        let lines = lines.lock().unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("info ") && lines[0].contains("from the parent"), "{lines:?}");
        assert!(lines[1].starts_with("warn ") && lines[1].contains(r#""msg":"from the worker""#), "{lines:?}");
    }

    /// A host function that panics in a worker fails the worker: `await` returns the error, and the parent runs on.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_host_function_that_panics_in_a_worker_fails_its_await() {
        let e = engine();
        // the worker's engine is a full one with a global `explode()` that panics
        crate::set_worker_engine(
            &e,
            std::sync::Arc::new(|life| {
                let worker = crate::make_engine_in(life)?;
                let explode =
                    worker.create_function(|_, ()| -> mlua::Result<()> { panic!("the module broke") })?;
                worker.globals().set("explode", explode)?;
                Ok(worker)
            }),
        );
        let (value, kind, reason, after): (Option<String>, String, String, String) = eval(
            &e,
            r#"
            local value, err = assert(thread.spawn("explode()")):await()
            return value, err.kind, err.reason, (assert(thread.spawn("return 'still runs'")):await())
            "#,
        )
        .await;
        assert_eq!(value, None);
        assert_eq!(kind, "failed");
        assert!(reason.contains("a host function panicked: the module broke"), "{reason}");
        assert_eq!(after, "still runs");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn messages_go_both_ways_in_order_as_plain_data() {
        let (first, second, done, echoed, nested): (String, String, String, String, String) = run(r#"
            local talker = assert(thread.spawn([[
                local parent = require("std:thread").parent()
                parent:send("hello")
                parent:send("world")
                return "done"
            ]]))
            local first, second = talker:receive(), talker:receive()
            local done = talker:await()

            local listener = assert(thread.spawn([[
                local parent = require("std:thread").parent()
                parent:send("ping")
                return parent:receive() .. "!"
            ]]))
            assert(listener:receive() == "ping")
            listener:send("pong")
            local echoed = listener:await()

            local shaper = assert(thread.spawn([[
                local msg = require("std:thread").parent():receive()
                return { received = msg.text, double = msg.n * 2, tags = msg.tags }
            ]]))
            shaper:send({ text = "hi", n = 21, tags = { "a", "b" } })
            local r = shaper:await()
            return first, second, done, echoed, r.received .. r.double .. r.tags[1] .. r.tags[2]
            "#)
        .await;
        assert_eq!((first.as_str(), second.as_str(), done.as_str()), ("hello", "world", "done"));
        assert_eq!((echoed.as_str(), nested.as_str()), ("pong!", "hi42ab"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_can_be_terminated_and_awaited_once() {
        let kinds: Vec<String> = run(r#"
            local stuck = assert(thread.spawn([[
                require("std:thread").parent():receive() -- never sent
                return "should-not-reach"
            ]]))
            stuck:terminate()
            local value, terminated = stuck:await()
            assert(value == nil)

            local once = assert(thread.spawn("return 1"))
            assert(once:await() == 1)
            local again, already = once:await()
            assert(again == nil)

            -- every message was read and the worker is gone
            local nothing, closed = once:receive()
            assert(nothing == nil)
            local sent, raised = pcall(once.send, once, "late")
            assert(not sent)
            return { terminated.kind, already.kind, closed.kind, tostring(raised) }
            "#)
        .await;
        assert_eq!(kinds[..3], ["terminated", "already_awaited", "closed"]);
        assert!(
            kinds[3].starts_with("std:thread.Worker:send: closed: the other side has exited"),
            "{}",
            kinds[3]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_script_that_is_not_a_worker_has_no_parent_and_nothing_shared() {
        let (parent_nil, shared_empty, in_worker): (bool, bool, String) = run(r#"
            local worker = assert(thread.spawn([[
                local thread = require("std:thread")
                return type(thread.parent()) .. tostring(next(thread.shared()))
            ]], nil, {}))
            return thread.parent() == nil, next(thread.shared()) == nil, (worker:await())
            "#)
        .await;
        assert!(parent_nil && shared_empty);
        assert_eq!(in_worker, "userdatanil");
    }

    /// A worker's handle on a shared map or counter is the parent's: what one writes the other reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_map_and_counter_across_spawn() {
        let (worker_saw, worker_wrote, counter_val, lib): (String, String, i64, String) = run(r#"
            local m, c = collections.Map.new(), collections.Counter.new()
            m:set("before", "parent")
            local w = assert(thread.spawn([[
                local shared = require("std:thread").shared()
                shared.m:set("from_worker", "hello")
                shared.c:increment()
                shared.c:increment()
                shared.c:add(5)
                -- a worker has the modules its parent has
                shared.m:set("lib", require("lib:base64").encode("hi"))
                return shared.m:get("before")
            ]], nil, { m = m, c = c }))
            local worker_saw = w:await()
            return worker_saw, m:get("from_worker"), c:get(), m:get("lib")
            "#)
        .await;
        assert_eq!(worker_saw, "parent", "worker should see parent's map value");
        assert_eq!(worker_wrote, "hello", "parent should see worker's map write");
        assert_eq!(counter_val, 7, "counter should reflect worker ops");
        assert_eq!(lib, "aGk=");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_to_multiple_workers() {
        let result: i64 = run(r#"
            local c = collections.Counter.new()
            local workers = {}
            for i = 1, 4 do
                workers[i] = assert(thread.spawn([[require("std:thread").shared().c:increment()]], nil, { c = c }))
            end
            for _, w in ipairs(workers) do
                w:await()
            end
            return c:get()
            "#)
        .await;
        assert_eq!(result, 4, "every worker increments the same counter");
    }

    /// A listener handed to a worker is the same socket: the worker reads the address its parent bound.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_listener_across_spawn() {
        let (addr, worker_addr): (String, String) = run(r#"
            local listener = assert(require("std:net").TcpListener.bind("127.0.0.1:0"))
            local w = assert(thread.spawn(
                [[return (assert(require("std:thread").shared().listener:addr()))]],
                nil,
                { listener = listener }
            ))
            return assert(listener:addr()), (w:await())
            "#)
        .await;
        assert!(addr.starts_with("127.0.0.1:"), "{addr}");
        assert_eq!(worker_addr, addr, "worker should see the listener's address");
    }

    /// A worker can hand what it was given on to a worker of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dag_forwarding() {
        let result: i64 = run(r#"
            local c = collections.Counter.new()
            local w = assert(thread.spawn([[
                local thread = require("std:thread")
                local shared = thread.shared()
                local child = assert(thread.spawn(thread.parent():receive(), nil, { c = shared.c }))
                child:await()
                shared.c:add(1)
            ]], nil, { c = c }))
            w:send("require('std:thread').shared().c:add(10)")
            w:await()
            return c:get()
            "#)
        .await;
        assert_eq!(result, 11, "child and parent worker both modify the same counter");
    }

    /// Only a handle on shared state can be shared: anything else is refused where it is passed, with its key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_cannot_be_shared_is_refused_with_its_key() {
        let refused: Vec<String> = run(r#"
            local out = {}
            for _, shared in {
                { bad = require("lib:json").object() },
                { t = {} },
                { n = 42 },
            } do
                local ok, err = pcall(thread.spawn, "return 1", nil, shared)
                assert(not ok)
                table.insert(out, tostring(err))
            end
            return out
            "#)
        .await;
        for (err, key) in refused.iter().zip(["bad", "t", "n"]) {
            assert!(err.contains("std:thread.spawn: argument #3 'shared'") && err.contains(key), "{err}");
        }
    }

    /// A buffer is refused wherever it would cross to another VM, however deep, never sent as an array of numbers.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_is_not_plain_data_does_not_cross() {
        let (spawned, deep, sent, from_worker, kind, reason): (
            String,
            String,
            String,
            String,
            String,
            String,
        ) = run(r#"
            local function deep()
                local t = { buffer.create(2) }
                for _ = 2, 100 do
                    t = { inner = t }
                end
                return t
            end
            local _, spawned = pcall(thread.spawn, "return 1", { buffer.create(2) })
            local _, deep_err = pcall(thread.spawn, "return 1", deep())
            local echo = assert(thread.spawn([[
                local parent = require("std:thread").parent()
                local ok, err = pcall(parent.send, parent, { buffer.create(2) })
                parent:send(tostring(err))
            ]]))
            local _, sent = pcall(echo.send, echo, buffer.create(2))
            local from_worker = echo:receive()
            echo:await()
            local value, err = assert(thread.spawn("return { buffer.create(2) }")):await()
            assert(value == nil)
            return tostring(spawned), tostring(deep_err), tostring(sent), from_worker, err.kind, err.reason
            "#)
        .await;
        assert!(spawned.contains("std:thread.spawn: argument #2 'args'"), "{spawned}");
        assert!(deep.contains("std:thread.spawn: argument #2 'args'"), "{deep}");
        assert!(sent.contains("std:thread.Worker:send: argument #1 'value'"), "{sent}");
        assert!(from_worker.contains("std:thread.Channel:send: argument #1 'value'"), "{from_worker}");
        assert_eq!(kind, "failed");
        assert!(reason.contains("the worker's result is not plain data"), "{reason}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_runs_a_file_and_is_named_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker.lua");
        std::fs::write(
            &path,
            r#"
            local thread = require("std:thread")
            local args = ...
            local parent = thread.parent()
            parent:send("hello from file worker")
            if parent:receive() == "ack" then
                parent:send("acknowledged")
            end
            if args.fail then error("from the file") end
            return args.n * args.n
            "#,
        )
        .unwrap();
        let e = engine();
        e.globals().set("path", path.to_str().unwrap()).unwrap();
        e.globals().set("missing", dir.path().join("nope.lua").to_str().unwrap()).unwrap();
        let (squared, failed, kind, missing_path): (i64, String, String, String) = eval(
            &e,
            r#"
            local w = assert(thread.spawn_file(path, { n = 7 }))
            assert(w:receive() == "hello from file worker")
            w:send("ack")
            assert(w:receive() == "acknowledged")
            local squared = w:await()

            local failing = assert(thread.spawn_file(path, { n = 1, fail = true }))
            failing:receive()
            failing:send("no")
            local _, failed = failing:await()

            local none, err = thread.spawn_file(missing)
            assert(none == nil and tostring(err):find("std:thread.spawn_file: unreadable: read ", 1, true))
            return squared, failed.reason, err.kind, err.path
            "#,
        )
        .await;
        assert_eq!(squared, 49);
        assert!(failed.contains("worker.lua") && failed.contains("from the file"), "{failed}");
        assert_eq!(kind, "unreadable");
        assert!(missing_path.ends_with("nope.lua"), "{missing_path}");
    }

    /// A worker's script is text: Luau bytecode is refused inline and from a file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_rejects_bytecode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worker.lua");
        let bytecode = crate::engine::luau_bytecode();
        std::fs::write(&path, &bytecode).unwrap();
        let e = engine();
        e.globals().set("bytecode", bytecode).unwrap();
        e.globals().set("path", path.to_str().unwrap()).unwrap();
        for spawn in ["thread.spawn(bytecode)", "thread.spawn_file(path)"] {
            let (returned, reason): (bool, String) = eval(
                &e,
                &format!("local value, err = assert({spawn}):await() return value ~= nil, err.reason"),
            )
            .await;
            assert!(!returned, "{spawn}: bytecode must not execute");
            assert!(reason.contains("binary"), "{spawn}: got {reason}");
        }
    }

    /// A worker is offered what its parent was offered, and a script on an engine whose embedder did not say how a
    /// worker is built cannot start one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_worker_is_not_offered_more_than_its_parent() {
        let narrow = crate::make_bare_engine().expect("engine");
        crate::install_runtime_state(&narrow, Life::new());
        crate::modules::install_allowed(&narrow, &["std:thread", "lib:base64"]).expect("install");
        let (offered, withheld): (String, bool) = eval(
            &narrow,
            r#"
            local thread = require("std:thread")
            local w = assert(thread.spawn([[
                return { require("lib:base64").encode("hi"), (pcall(require, "std:time")) }
            ]]))
            local seen = w:await()
            return seen[1], seen[2]
            "#,
        )
        .await;
        assert_eq!(offered, "aGk=");
        assert!(!withheld, "the worker's `require` knows what its parent's does");

        let bare = crate::make_bare_engine().expect("engine");
        crate::install_runtime_state(&bare, Life::new());
        let thread = crate::modules::build(&bare, "std:thread").expect("the module").expect("builds");
        bare.globals().set("thread", thread).unwrap();
        let (none, kind): (bool, String) =
            eval(&bare, r#"local w, err = thread.spawn("return 1") return w == nil, err.kind"#).await;
        assert!(none);
        assert_eq!(kind, "no_worker_engine");
    }

    /// Shutting the script down aborts its workers, and the workers of its workers.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_shutdown_aborts_the_workers_of_the_script() {
        let life = Life::new();
        let e = engine_in(life.clone());
        let hits: mlua::AnyUserData = eval(
            &e,
            r#"
            hits = collections.Counter.new()
            worker = assert(thread.spawn([=[
                local thread = require("std:thread")
                local hits = thread.shared().hits
                child = assert(thread.spawn([[
                    local thread = require("std:thread")
                    thread.shared().hits:increment()
                    thread.parent():receive()
                    thread.shared().hits:add(100)
                ]], nil, { hits = hits }))
                hits:increment()
                thread.parent():receive()
                hits:add(100)
            ]=], nil, { hits = hits }))
            while hits:get() < 2 do
                require("std:time").sleep(require("std:time").Duration.from_millis(5))
            end
            return hits
            "#,
        )
        .await;
        drop(hits);
        life.shutdown();
        let (kind, count): (String, i64) =
            eval(&e, "local _, err = worker:await() return err.kind, hits:get()").await;
        assert_eq!(kind, "terminated");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(count, 2, "neither worker ran past its wait");
    }

    /// A worker nobody holds any more is aborted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_worker_is_aborted() {
        let e = engine();
        eval::<()>(
            &e,
            r#"
            flag = collections.Map.new()
            do
                local worker = assert(thread.spawn([[
                    local thread = require("std:thread")
                    thread.shared().flag:set("started", true)
                    local time = require("std:time")
                    time.sleep(time.Duration.from_millis(300))
                    thread.shared().flag:set("finished", true)
                ]], nil, { flag = flag }))
                while not flag:has("started") do
                    require("std:time").sleep(require("std:time").Duration.from_millis(5))
                end
            end
            "#,
        )
        .await;
        e.gc_collect().unwrap();
        e.gc_collect().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        let finished: bool = eval(&e, r#"return flag:has("finished")"#).await;
        assert!(!finished, "the worker was aborted with its handle");
    }
}

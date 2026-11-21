# Rust host: spike results and decision memo

> Historical record: the spike that preceded the build (since folded into `crates/`; the throwaway `rust-spike/` directory is gone). The "Running the host" and later sections describe the host as built.

Everything below was built and run in this workspace (`rust-spike/`, rustc 1.98.1, macOS arm64). `cargo nextest run --workspace`:
**42 tests, 42 passed, 0.5 s.** Clippy: 0 errors, 21 pedantic warnings (spike code). `scripts/check-deps.py` and `cargo deny --exclude-dev check bans`: OK.
The spikes were restructured into the proposed hexagonal layout (section "Workspace layout"), so the layout is proven by compiling code, not only drawn.
~3.5k lines of code in 11 crates; the host binary is 15.1 MiB (`release-host`, stripped, measured on NEOC-113).

**Verdict: the stack works end to end. Recommend it as decided.** One test (`crates/vos-host/tests/e2e.rs`) installs a Luau app from a `file://` app ref over `/rpc`, calls its tool from an `rmcp` client with a per-agent bearer token, shows `app.notify` reaching the UI socket, round-trips a UI event to a new view tree, uninstalls, and sees `tools/list_changed`.

## Answers per question

### 1. Apps: typed Luau source on `mlua 0.12` (Luau) — `crates/vos-lua` on `crates/neoc-vm`

> **Engine (NEOC-113, NEOC-137):** the VM is `neoc-vm` on Luau (mlua `luau`, the engine built from source), which replaced the former PUC-Lua VM crate (PUC Lua 5.1; on the way through, LuaJIT). Luau's yield boundaries are close to PUC 5.1's, so most of this section still reads true: a host call suspends in `pcall`, an `xpcall` body, `__call` and the iterator function of a `for ... in`, and is refused ("attempt to yield across metamethod/C-call boundary") in every other metamethod, in `tostring` of an object whose `__tostring` waits, in `string.gsub` and `table.sort` callbacks and in the handler of an `xpcall`. What is not true any more is called out where it occurs: neoc-vm has no yield-safe `pcall` (Luau yields across `pcall` itself), the engine's own `script_env` installs the per-app coroutine functions and seals the libraries with Luau's `readonly` flag (no proxies), and the adapter keeps the `NOT_FOR_APPS` module list and the worker module set (`set_worker_engine`). The measured baseline is the PUC VM's, due a re-measure with the docs pass (NEOC-115).
Apps are written in typed Luau, `--!strict` (checked by luau-lsp against the generated Luau declarations in `types/generated/vos.d.luau`, see below) and **run as written, less their types**: `bun run build:apps` runs darklua (`.darklua.json`, pinned 0.18.0) over each app, which **bundles the app's relative `require`s into one `app.luau`** and strips the comments and the type annotations. An app ref lists that one file (plus its SQL migrations); `manifest.main` is `app.luau`, and the host refuses a `main` that does not end in `.luau` or `.lua` (a plain-Lua bundle published earlier still loads). `vos:app` and `vos:ui` stay runtime `require`s. The two members of Luau's standard library that the app environment does not copy from the engine, `typeof` and `utf8`, are provided by `lua/prelude.lua` (copied to `types/generated/prelude.lua`), which the host runs in each app's environment before the app (`table.pack`/`clone`/`move`/`unpack` are Luau's own and need no polyfill). `types/generated/ui.lua` is the generated `vos:ui` module (plain Lua source, written by `vos-codegen`).

`neoc-vm` is the VM library (mlua with `luau, async, serialize, send, macros`; no dependency on any other vos crate; a support library, not an adapter) and `vos-lua` is the adapter that implements `AppRuntime` on it, reaching mlua only through neoc-vm's re-export. The VM crate also holds the neoc module registry (`std:`, `lib:`, `vnd:`, `lua::modules`): **apps get all of it** through `require("std:fs")`, `require("lib:json")`, `require("vnd:hyper")` (the adapter's `require` builds a module in the app's VM on first use and caches it, so an app has its own table of each and two apps never share one), next to `vos:app` and `vos:ui`. Left out on purpose: `lib:test` (a test harness that prints to stdout), `lib:compile` (builds standalone executables) and `std:workers` (a process-wide shutdown signal); `vnd:testcontainers_modules` is part of neoc-vm and is offered like the rest. The adapter builds the engine with `make_bare_engine` plus `install_runtime_state` (the `std:task` scheduler and the app's `Life`, below) and sends `lib:log` lines to the host's tracing with the app id (`lib::log::set_log_sink`); no module writes to the daemon's stdout.

- **One VM per app** (a Luau state), and **one environment table per app** (`neoc_vm::lua::script_env`): the base functions are an allow-list (no `load`, `loadstring`, `dofile`, `loadfile`, `getfenv`, `setfenv`, `newproxy`, `module`, `collectgarbage`, `gcinfo`, `print`, `string.dump`, `string.gfind`; no `io`/`os`/`debug`/`package`); `string`, `table`, `math`, `coroutine` and `bit32` are private copies (and the prelude's `utf8`), sealed with Luau's `readonly` table flag, so writing to a library, `setmetatable` on one, `rawset` on one and `table.insert` into one all fail ("attempt to modify a readonly table") and the engine's own tables are unreachable; `getmetatable("")` answers a string. The shared **string metatable is locked** (`__metatable`), so `getmetatable("").__index` is not a way to the real `string`. Chunks load in text mode only (Luau checks a bytecode chunk's version and does not verify its instructions). Two VMs have independent globals. `math.random`/`randomseed` are per-environment generators seeded from the OS (the C library's is one state shared by every VM in the process). `pairs(string)` sees the library's functions.
- **No permissions, no budgets.** An app is the person's own code on their own host (a homelab: "people should be able to build all sorts of apps"). There is no manifest `permissions` (a `permissions` key in an older `app.json`, app ref or `app.manifest` is parsed and ignored), no install-time approval of capabilities, no `Capabilities` check in the adapter; and no CPU, memory or time limit on a VM: no count hook, no memory limit, no wall clocks, no host-call timeout, no cap on effects, no charged library calls. Apps get Lua's own C `string` and `table` functions (including `table.sort` and the pattern matcher). A loop that never ends spins one worker thread of the host. Unloading ends an app's work only where the app yields to the host (an idle wait, a host call, a task slice boundary: those end with `the app was unloaded`); a loop that never yields, or a `std:thread` worker stuck in one, is not interrupted: it can keep the host busy or hung and can outlive the app's uninstall until the host restarts.
- **What still protects the host.** Values crossing out of Lua are walked once with a node, byte and cycle guard (`convert::MAX_NODES` 100 000, `MAX_BYTES` 4 MiB; view trees use the domain's `MAX_NODES` 10 000 and `MAX_DEPTH` 64), so a cyclic table or one shared at every level is an error rather than a stack overflow or an allocation of gigabytes; `math.random` validates its bounds (a bound with no integer representation is a Lua error, never a panic); chunks load in text mode only (bytecode is unverified); SQL `ATTACH` is denied through `app.db` and `app.kv` (not through `vnd:rusqlite`, which opens any file); a tool declared `readOnly` (an MCP annotation for agents, not an app permission) cannot write through the **`app.*` API**: no database or `app.kv` write, no opening windows, notifying, changing the shell, assigning `app.state`, or registering tools, windows, handlers or threads. The restriction belongs to the call and is inherited by everything the call starts: a `std:task` task, deferred or delayed function it spawns stays read-only for as long as it lives, and a request that reaches the app's own `vnd:hyper` server (the one call that has no origin to inherit from) while any read-only call or task is *acting* runs read-only. The adapter counts the acting ones (`Shared::read_only_active`): a `readOnly` tool, a task it started (`task.spawn`, and a `task.delay`/`task.defer` from the moment it begins) and a request made read-only by this very rule, while each is running, queued for the VM, or waiting at something in flight (the network, a worker, a lock); a task's continuation counts from the end of its first slice, not from when the executor first polls it. A call that is asleep at a timer (`task.wait`, a `std:time` sleep: `Role::Timer`, which tells the baton `IdleKind::Timer`) is *quiet* and is not counted: it runs no script code until its timer has ended and it asks for its turn back, and it counts again from that moment, before it can send or start anything. The taint therefore lasts as long as some read-only call is acting, not as long as one is alive: a `readOnly` tool that leaves behind a poller that waits on timers alone (`task.wait` between cycles, nothing in flight) does not make the app's webhooks read-only for the poller's life. The limit, plainly: a poller that also makes requests (`while true do hyper.get(ext) task.wait(60) end`) counts as acting while each request is in flight (an `IdleKind::Other` wait) and in the moment between its timer firing and its parking again (it counts the moment it asks for its turn back), so a webhook that takes the VM inside those windows runs read-only, intermittently. Long-lived work whose requests or whose webhooks must write belongs in a writable call, not in a `readOnly` tool. The routes a read-only call has to its own server stay closed, while the routes a read-only call has to its own server stay closed (waiting at the network for the answer, a handler that asks again, a task that wakes from a timer and asks, a task whose first slice already has the request in flight while its tool sleeps). What stays open, and is accepted like the neoc modules: a request a read-only call sent and abandoned before the handler took its turn (its task was cancelled, then the tool returned or went to sleep at a timer, with the VM held busy meanwhile; or the host cancelled or timed out a `readOnly` call that was waiting on its own server while the VM was busy) and a request that reached the server by a route the call does not wait at (a `std:thread` worker, which has no origin and can outlive the tool, or a third party that calls the app back); in both the handler runs as an ordinary request. It applies to nothing else: a task or a tool carries its own origin, so a long-lived read-only poller does not make an unrelated writable tool's `task.delay` read-only. The guarantee does **not** cover the neoc modules (`std:fs`, `vnd:rusqlite`, `std:net`, ...), which every tool can use. A host panic, stack overflow or allocation failure reachable from app input is a bug (the `release-host` profile aborts on a panic, which takes every app down), and the guards are: Lua numbers that size an allocation, a sleep or an index are bounded (`lua::limits`), and a table's `#t` is never trusted to presize a buffer (a forged table reports 2^29); third-party calls that `expect` are made fallible (`lib:glob` compiles through `GlobSetBuilder::build`, so a pattern globset cannot compile is a Lua error), and globset's parser recurses per `{`, so `lib:glob`, `lib:archive`'s `include` and `lib:http`'s `serve_static` compile every glob through one helper (`lib::glob::compile`) that refuses a pattern over 4096 bytes, nested more than 32 `{` deep or with more than 256 `{` before globset sees it; Luau's pattern matcher bounds its own recursion ("pattern too complex", "too many captures"), so there is no guard of ours any more (the former `lua::patterns`, which allowed 512, is gone); and Tera recurses over the template with no limit, so `lib:template` refuses, before Tera sees the source, `include`/`import`/`extends`, blocks nested more than 24 deep, a tag with more than 48 operators (`+ - * / % == != < > <= >=`, `and`, `or`, `in`) or more than 24 open parentheses or brackets, and a template whose blocks and macro calls stack more than 48 deep when rendered (a macro call counts as two); a macro may call itself, directly or through others, and is stopped by that depth at run time (`macro recursion is more than N levels deep`: `crates/neoc-vm/src/lua/lib/template.rs`). Each limit is a third or less of where an unoptimised build overflows a 2 MiB thread, and is tested at the limit on a 2 MiB thread. What is not guarded: time and memory (a pattern or loop can run for ever, `vnd:jsonschema` loops on a cyclic schema), and a stack overflow in a code path nobody has swept; `hostile_inputs.rs` calls every module function with hostile values, including text and tables nested 100 000 deep for every bracket kind, on a 2 MiB thread. What the guards leave is stack: Luau stops a recursion through a Rust host function that calls back into Lua at 200 C levels with an ordinary error ("C stack overflow"), but the 200 levels must fit the thread, so every thread that polls a VM has `neoc_vm::VM_THREAD_STACK` (8 MiB): `vos_host::runtime` builds the runtime with it (workers and blocking pool) and runs the top-level future on a worker; `crates/vos-host/tests/stack.rs` holds it in a debug build.
- **What an app starts belongs to the app** (`lua::lifecycle`). `Life` is a per-VM registry (in the engine's app data) of everything the app spawns: `std:task` tasks, `vnd:hyper` and `lib:http` accept loops and connections, `std:thread` workers (which run in the *parent's* `Life` and are built with the same module set), and `std:net` listener sockets. `AppVm::shutdown` (called when the app is unloaded or replaced, and on `Drop`) aborts all of it and releases the sockets, so an unloaded app's port is free again; a task that is spawned after the shutdown is aborted at once; a host call a running call is waiting on is abandoned. On a reload the old version's background work is stopped first (`Life::stop_background`: tasks, workers and sockets, so a fixed port can be reused) and the old VM is shut down once the new one has replaced it; a version that fails to load leaves the old one answering its tools and windows, without its background work. Tested: an unloaded app's listener port is bindable again, its task and its worker stop writing, and a replacement binds the old version's port.
- **One call at a time per VM, including inbound callbacks** (`lua::lifecycle::{CallEntry, Baton}`; the adapter's `Turn`). Tool calls, events and view events take the VM's gate. So do the Lua callbacks nobody called: a request reaching a `vnd:hyper` server (`inbound`) and a `std:task` coroutine's next slice. A call is a *baton*: while it holds it, nothing else runs in the VM, so `db:tx`, the `readOnly` flag (the holder's own, set when it takes the gate) and state tracking stay unambiguous. The baton is handed back at **idle waits** (`lifecycle::idle`: `task.wait`, `std:time` sleep, `vnd:hyper` and `lib:http`, `std:thread` `await`/`receive`, sqlx, `Mutex:with_lock`'s wait): the call sets its own state aside (the `readOnly` flag, the tracking scope), lets the next call in, and takes the gate and its state back afterwards, so `while true do task.wait(1) ... end` does not stop the app and a task can call its own listener. Between two idle waits a call is atomic. A call **cannot wait inside `db:tx`** (the wait raises: a transaction holds the database) or while rendering. The VM cannot see whether Lua managed to yield at a host call, so what the call may do is decided by six structural rules (`lifecycle`'s module docs; `crates/vos-lua/tests/chaos.rs` generates scripts and checks them). (1) Every async host function is a *leaf*, because it is made by `lifecycle::async_function` / `async_method` (the only constructors: clippy `disallowed-methods` refuses `create_async_function` and `add_async_method` elsewhere), and its `Role` says how it relates to the turn: `Vm` (the database and key-value store: the function takes the turn before its body runs, so it checks the transaction and the `readOnly` flag as the holder), `Wait` (the whole body is an idle wait) or `Mixed` (the body starts without the turn and calls `idle` for part of its work, as `Mutex:with_lock` does). A body may build Lua values with or without the turn, but runs script code and touches the app's state only with it. (2) A leaf that returns `Pending` is *suspended*, and the call records it; mlua yields on the next instruction and resumes into that same leaf, so a call that goes on polls the suspended leaf first. (3) A leaf the script ran past is *abandoned*: Lua refused the yield at a C boundary (a `string.gsub` callback, a `table.sort` comparator, a metamethod other than `__call`, `tostring` of a waiting `__tostring`, the handler of an `xpcall`; Luau suspends only in `pcall`, an `xpcall` body, `__call` and a `for ... in` iterator function), the script caught the error and went on, so when another leaf starts or the call ends the first leaf's future is dropped on the spot (a database lock, a mutex guard, a transaction go with it), and a coroutine that is resumed later gets "this host call was abandoned" from it. (4) A call lends the baton only when its future returned `Pending` with an idle wait innermost, its waker was *not* woken during that poll (mlua wakes it for a plain `coroutine.yield()`, which waits for nothing) and the baton still agrees (no open `db:tx`, not rendering: asked again at the moment of lending). (5) Script code runs only in a poll that holds the turn: every poll of a call passes one gate (`lifecycle::scoped`), and a poll that does not resume into a suspended leaf takes the turn first, so a call that was lent its turn and is resumed anywhere else (at a plain yield, into a leaf that is gone) queues for it instead of running in someone else's. (6) A script resumes only the coroutines it made: `coroutine.create` and `coroutine.wrap` record their coroutine in a weak-keyed *owned* set (`async_coroutines.lua`), and `coroutine.resume` of any other thread is refused with "cannot resume non-suspended coroutine" and changes nothing. A script can hold other threads (`coroutine.running()` returns the top-level thread of a call or a task's thread; on Luau a `pcall` runs in its caller's thread and has none of its own), but the refusal is the default, so no thread the host or a module makes has to be found and marked; the host's own machinery (the task scheduler) uses the raw primitives and is not blocked. So a `readOnly` tool stays read-only and nothing runs inside a call between its waits, whatever the script does. (Rule 5 is the second line of defence for the invariant itself; no script reaches its take-the-turn branch today, and a unit test covers it directly.) A coroutine of the script's own whose host call is in flight is *running* as far as the script can tell: resuming it from anywhere but the thread waiting for it is "cannot resume non-suspended coroutine", so two threads never drive one host call. (`std:thread` workers run on their own engine, with plain coroutines, and nothing of the app's turn.) A leaf never carries a baton: `hold` asks the baton of the call that is polling it, so a host call left waiting for its turn by a detached call (a task's first slice) runs, and is checked, in the turn of whoever finishes it (never a hang); a task started inside `db:tx` waits for its turn, and so for the transaction to end, instead of reaching into it. A `db:tx` is an actor (`vos-lua/src/tx.rs`): its statements are queued at the moment the script calls them and run in that order before the commit, a transaction belongs to the turn that opened it (another call's statement is refused), `db:tx` raises "transaction lost" instead of reporting success for one that was rolled back, and letting go of it (the call is cut off) rolls it back at once, a running statement included. The transaction's task lives in the app's `Life`, so unloading the app or a reload's `stop_background` rolls an open transaction back (its call gets "the transaction has ended"); the sqlite adapter begins a transaction in a task of its own that runs to the end whether or not the caller still waits, and rolls back what nobody took, so a cut-off `begin` never leaves the connection inside a transaction. A `Vm` function does not hand the baton back while it runs. `db:migrate` inside `db:tx` is refused (the transaction holds the write gate a migration needs). `task.spawn` runs a coroutine's first slice inside the spawning call; its continuation is *detached* (it holds nothing until the wait it stopped at ends, or a host call: `hold`; a coroutine that stopped at a plain `coroutine.yield()` takes its turn before it resumes, so it never runs inside the call that spawned it). Effects a call that came from inside the VM queued (`app.notify`, shell declarations) are forwarded, and open windows are told to follow, by a pump task the runtime registers in the app's `Life`: it wakes when such a call **ends or pauses at an idle wait** (`Turn::park` signals for an inbound turn as `Turn::drop` does, so a `while true do ... task.wait() end` loop reports at every pause, not only when it is over), forwards the effects, and, when an `app.state` field was assigned or a window asked for a toast or client value (`AppVm::has_unsettled_views`), calls the application's `AppActivity::background_settled` port (late-bound with `LuaRuntime::bind_activity`, the way `RpcHub::bind` is; `AppService` implements it by telling its `AppObserver`s, so the view service settles the app: it re-renders the dirty views, sends their `view.ops` and delivers the windows' requests through the path a tool call uses, but does not re-query the live sources, which the change feed already pushes). Signals are coalesced (one pending wake), and the view service settles one app at a time (`ViewService::settle_turns`), so a settle a background change started finishes, its ops sent, before an event's own settle can return and the event be acknowledged ("ops before the answer" holds whoever rendered them). Effects queued by an app that has been unloaded or replaced meanwhile are dropped, and so is what such an app still had queued for the VM: a request to its own server or a task that asks for its turn after `Life::shutdown` never takes it (`Turn::take`), because a task being polled when it is aborted would otherwise run on, and a request whose read-only caller the shutdown cut off would run as an ordinary, writable one. For the same reason, once an app's background work is stopped (a reload begins: `AppVm::stop_background`, or the app is unloaded) every request that still reaches its server runs read-only (`Shared::background_stopped`, set before anything is aborted and read after the acting count): the listeners are gone, so what arrives is a straggler of work that was cut off, and its sender may have been a read-only call that the stop removed from the count. The pump takes the effects out of the VM and lets go of it before it awaits anything, so it never keeps an unloaded VM alive. A failed hot reload is the one place the flag stays set: when a reload's new version fails to load, the old VM keeps serving its tools and windows with `background_stopped` set, and if it starts a server again, requests to that server run read-only until the next successful reload or a host restart. That fails safe (a request can read but not write) and is the accepted behaviour (Jira AGOS-64, decided). A full restart of the previous version is the option to revisit if failed reloads become common, for example with AppStudio live deploys. Long-running work must be **started** at load, not run there: the entry chunk has to return (a top-level loop or wait that never ends holds up the load, and nothing is stopped by a timeout, as apps have no budgets), so start loops and servers with `task.spawn`; `vnd:hyper` and `lib:http` servers already start without blocking.
- **HTTPS in `vnd:hyper`.** The client speaks `https://` through `tokio-rustls` (aws-lc-rs, the provider reqwest uses elsewhere in the host) with the platform certificate verifier (`rustls-platform-verifier`, the operating system's trust store). A service with its own CA or a self-signed certificate passes `ca_pem = "<PEM>"` in the options of `get`, `post` or `request`; exactly those certificates are then trusted for that request. Tested against a local TLS server with a generated certificate (accepted with its `ca_pem`; refused without it, and with another server's).
- **Async host calls on Lua 5.1.** mlua turns an async host function into a Lua function that yields to the executor while its future is pending; PUC Lua 5.1 could not yield across `pcall`, a metamethod or a C function, which is why the former PUC-Lua VM crate installed a **yield-safe `pcall`/`xpcall`** (a coroutine per protected call); Luau suspends across `pcall` (and `xpcall`, `__call`, `for ... in` iterators), so neoc-vm has no such wrapper. In each app environment neoc-vm's `script_env` installs `coroutine.create`/`resume`/`wrap` that pass an async host call made inside a coroutine of the app's own out to the executor (a call that is cancelled while it waits is never resumed, so no app code runs again; mlua drops the thread, the call's end abandons the host call the inner coroutine waits at ("this host call was abandoned"), and the coroutine it left behind is refused while its dead resumer is still listed in `driven`, a weak-keyed *and weak-valued* table, and the script's own again once the collector has cleared that entry, never driven by two threads at once). The `app.state`, window `state`/`client`/`env`, source-handle proxies and the agent roster are plain synchronous Rust functions, so they never yield. What still cannot yield: a `string.gsub` callback, a metamethod other than `__call`, `tostring` of an object whose `__tostring` waits, a `table.sort` comparator, the handler of an `xpcall` (a `for ... in` iterator function and `__call` can): a host call there fails with Lua's own "attempt to yield across metamethod/C-call boundary", and the call is *abandoned* (the rules above): its future is dropped as soon as the script goes on, so it holds no database lock, mutex or transaction, and resuming the coroutine that made it raises an error. A script resumes only the coroutines it made (`coroutine.create`, `coroutine.wrap`; the *owned* set of `async_coroutines.lua`): `coroutine.resume` of any other thread (a `coroutine.running()` it stashed from the top of a call, a task, or inside a `pcall`) is "cannot resume non-suspended coroutine" and changes nothing, as is a coroutine of its own that another thread is waiting on at a host call or that was handed to the task scheduler (`neoc_vm::lua::script_env::mark_driven`; `task.spawn`, `defer` and `delay` take only coroutines the script made). Nothing is left that can lose a call's wake-up.
- **Capability table** `app.db`, `app.kv`, `app.tools.register{}`, `app.notify{}`, `app.state{}`, `app.on(event, fn)` (see `apps/README.md`). Rust→Lua event call with a JSON table works (`emit()`/`call_tool()`).
- **Source from bytes** (`lua.load(&[u8]).set_name("=app/app.lua")`; errors report `app/app.lua:12`). Line numbers refer to the bundle; the generator is `retain_lines`, which keeps each statement on its source line: line for line for an app with no relative `require`, and shifted by a constant (the size of the bundled modules before it) for an app with modules.
- **Unload frees**: abort what the app spawned and close its sockets (`AppVm::shutdown`), drop Rust-held `Function`s, then the `Lua`.
- **Gotchas found:** (a) the polyfills run in the app's environment, so the prelude installs into the chunk's `...` argument, never `getfenv()`. (b) `to_value` maps JSON null to a truthy lightuserdata: use `SerializeOptions::serialize_none_to_null(false)`. (c) `send` makes `Lua: Send+Sync` (mutex inside), which is what lets tokio multi-thread run VMs; one VM executes one call at a time (an app is single-threaded by design). (d) mlua's Lua-version features cannot coexist in one build (Cargo unifies features): neoc-vm and neoc both use `luau`, so every VM in the workspace is Luau (the former `lua51` of PUC-Lua VM crate, and the LuaJIT in between, are gone).
- **Dev loop and hot reload:** the host cannot run darklua. `bun run dev` watches `apps/**` (`*.lua`, `app.json`, migrations) and re-runs `build:apps`; the host's `notify` watcher watches the built app-ref folders under `dist/apps` (`.lua`/`.json`) → debounce → build a new VM → swap → old VM dropped (`crates/vos-lua/src/reload.rs`; the DB is kept; only the VM is rebuilt).
- **Measured baseline (2026-11-19, plain Lua 5.1 on the former PUC-Lua VM crate, now `neoc-vm`; replaces the Luau spike's 0.14 ms / 524 KiB per VM).** Machine: Apple M5, 10 cores, 24 GiB, macOS (Darwin 27.0.0, arm64), Rust 1.98.1; other work was running on the machine, so treat the build times as +/- 20%. The figures were taken under vos's former `[profile.release]` (`opt-level = "z"`, fat LTO, one codegen unit, stripped, `panic = "abort"`), which is now `[profile.release-host]`: the commands below name it, but the numbers are the PUC VM's and have not been re-measured on neoc-vm (measured once on neoc-vm, on NEOC-113: a 15.1 MiB host binary, on an Apple-silicon Mac; no build time is claimed, a re-measure under load took 3 min 15 s). Rerun the commands to see whether a change moved a number; do not edit the table by hand without them.

  | What | Measured | How |
  |---|---|---|
  | Release binary (`vos`) | 13 418 960 bytes (12.8 MiB; PUC VM, 15.1 MiB on neoc-vm with Luau) | `cargo build --profile release-host -p vos-host`, then `ls -l target/release-host/vos` |
  | Clean release build | 1 min 58 s (378 s of CPU) | `CARGO_TARGET_DIR=<empty dir> cargo build --profile release-host -p vos-host` |
  | Incremental release build (touched `crates/vos-lua/src/api/misc.rs`) | 1 min 06 s: the adapter and the host are recompiled and relinked with LTO | `touch crates/vos-lua/src/api/misc.rs`, then the same build |
  | Incremental dev build (same file) | 1.7 s | `cargo build -p vos-host` after a first build, then `touch` the file and build again |
  | VM create (the built Calculator, 14 662 bytes: engine, host API, manifest, window and tools; 50 VMs after one warm-up) | first 1.64 ms; then median 1.26 ms, mean 1.30 ms, p95 1.42 ms, max 1.51 ms | `cargo test --release -p vos-lua --test baseline -- --ignored --nocapture` (`VOS_BASELINE_VMS=<n>` for another count) |
  | Lua heap per VM (`AppVm::used_memory`) | 512.9 KiB, the same for every VM | the same test |
  | Process memory per VM (resident set after 50 VMs minus before, `ps`) | 822 KiB (12 208 KiB before, 53 296 KiB after): the Lua heap plus what Rust allocates for the VM (database handle, tables of the host API) | the same test |

  The VM numbers are for a VM that requires nothing from `std:`/`lib:`/`vnd:`: those module tables are built per VM on first `require`, so a VM that requires nothing pays nothing, and one that does pays for what it requires. Event dispatch (the Luau spike's 1.8 us per event) is not remeasured; `crates/vos-lua/tests/baseline.rs` is the place to add it. In a debug build the same test gives about 2.5 ms and the same heap.

### 2. SQLite — recommend **`rusqlite 0.43.0` (bundled SQLite 3.54.1)** — `crates/vos-sqlite`
- Design proven (7 tests): one `Connection` per app behind a mutex, all work in `spawn_blocking`; migrations numbered 1..N, each in its own transaction, recorded in `_migrations(version,name,checksum,applied_at)`, idempotent, incremental on app upgrade, **checksum-tamper detected**, failing migration leaves nothing; **authorizer denies `ATTACH`/`DETACH`** (an app's SQL can't escape its file).
- **Change notifications:** `update_hook` buffers `(table, op, rowid)`, `commit_hook` publishes one batch to a `tokio::broadcast`, `rollback_hook` discards. Verified: a multi-statement txn gives one batch; a rolled-back txn gives none. This beats calling `changes()` (which can't see triggers, cascades, or rollbacks).
- **sqlx 0.9.1 probe** (`spikes/sqlx-probe`) also works (dynamic `Migrator` from manifest data, `lock_handle().set_update_hook`). Why not: async-native but still a worker thread per connection; the hook needs the raw handle; **0.9 forces `AssertSqlSafe(..)` on every dynamic SQL string** (all app SQL is dynamic); `migrate!()`/compile-time checking don't apply to runtime-loaded apps; heavier compile. It shares `libsqlite3-sys 0.40` with rusqlite 0.43, so both link in one binary, but the versions must move together.
- **Gotcha:** rusqlite's `serde_json` feature stores a JSON *string* param as its quoted JSON text. Use own conversion (`to_sql_params`), covered by a test.

### 3. MCP — **`rmcp 3.11.0`** (`server, macros, transport-streamable-http-server`; client features for tests) — `crates/vos-mcp`
- `ServerHandler` implemented by hand (`list_tools` from a runtime catalogue, `call_tool`), mounted via `StreamableHttpService` + `nest_service("/mcp")` on axum. The `ToolRegistry` port is implemented by `McpState`: `publish/retract` → `notify_tool_list_changed()` to every live peer.
- **Both protocol generations proven** (3 tests): legacy `2025-11-25` session (`list_changed` on the GET stream, `with_legacy_session_mode(true)`) **and** `2026-07-28`, which has *no sessions* and uses `subscriptions/listen` (implemented via `listen()` + a broadcast). Install/uninstall during an open session produced `tools/list_changed` and the new `tools/list`.
- **Auth + identity:** axum middleware → `Authorization: Bearer` → `AgentDirectory` port (constant-time compare) → `AgentIdentity` into request extensions → `ctx.extensions.get::<http::request::Parts>()` inside `call_tool`. 401 + `WWW-Authenticate` otherwise (tested for none/wrong/Basic). Per-agent permission test: "Claude" (granted `notes`) succeeds; "Other" gets an in-band `isError` result `permission denied: Other may not call notes__add_note`, and the VM never ran.
- Tool names are `<app>__<tool>`. `StreamableHttpServerConfig` has built-in `Host` allow-list (loopback by default) and optional `Origin` validation: keep both on.
- Risk: rmcp is moving fast (3.x with a new spec version); pin `=3.11.0` and re-run these 3 tests on every bump.

### 4. ACP — **`agent-client-protocol 5.3.0`** (+ `-schema 1.18.0`; features `process`, `stdio`) — `crates/vos-acp`
- Client = `Client.v1().connect_with(AcpAgent::new(AcpAgentConfig::new(prog).args(..)), async |cx| { cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await })`. It spawns the child, speaks stdio, shuts it down. Runs fine under tokio.
- **Proven (3 tests):** handshake against a 25-line fake agent binary (`src/bin/fake_agent.rs`); `AgentConnector::start_run` = initialize + `session/new`; and the **real adapter**: `node …/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js` → `initialize` returns agent_info `@agentclientprotocol/claude-agent-acp 0.108.0`, protocol v1, in 0.27 s. No prompts, no model calls. No processes left behind.
- Not spiked: `session/prompt` streaming → AG-UI mapping (the next milestone), permission-request round trips, and the crate's MCP-over-ACP feature (`unstable_mcp_over_acp`) for handing agents our `/mcp` URL + token in `session/new`.

### 5. AG-UI — **hand-rolled**, `crates/vos-agui` (163 lines incl. endpoint)
- Crates.io: `ag-ui 0.5.0-alpha.3` (third party, "working toward becoming official", 600 downloads), `ag-ui-core 0.1.0` (the community SDK inside `ag-ui-protocol/ag-ui/sdks/community/rust`, Aug 2025, client-oriented, no server), `agui-sdk 0.9.1`, `ag-ui-rs 0.3.1` (all single-maintainer). **No maintained official Rust crate.**
- The wire model is one serde enum: `#[serde(tag="type", rename_all="SCREAMING_SNAKE_CASE", rename_all_fields="camelCase")]` (15 variants: RUN_*, STEP_*, TEXT_MESSAGE_*, TOOL_CALL_*, STATE_*, CUSTOM). SSE endpoint = `POST /agui` taking `RunAgentInput`, returning `axum::response::Sse` of `data: {json}\n\n`; malformed input is a 422 before the stream starts (per the transport spec). Test prints the raw frames (RUN_STARTED, TEXT_MESSAGE_START/CONTENT×2/END, RUN_FINISHED) and checks shapes against docs.ag-ui.com events. Add variants as the client needs them.

### 6. HTTP stack — **axum 0.8.9 + tower-http 0.6.11 + hand-rolled JSON-RPC peer** — `crates/vos-http`
- Proven (3 tests): `/rpc` WebSocket upgrade; **Origin + Host guard** (`AllowedOrigins`, middleware): evil Origin → 403 on the handshake, DNS-rebind Host rejected, no-Origin (native client) allowed by config; static serving with `ServeDir` + `index.html` SPA fallback + precompressed gzip.
- **JSON-RPC: hand-rolled peer** (~120 lines, `rpc.rs`; envelope types live in the domain). Recommended over `jsonrpsee 0.26.1`: we need the *server* to issue requests to the client and await the answer on the same socket. The test does exactly that (client request → handler calls `peer.request("client/echo")` → client answers → original response), plus notifications both ways, per-request tasks (no deadlock), pending-request cleanup on disconnect. jsonrpsee's server model is request/response + subscriptions (not tested here; from its docs).
- Gotcha found by the e2e: a notification emitted *during* a request can arrive before its response; clients must not assume order.

### 7. Shared types — **`ts-rs 12.0.1` (TS) + `schemars 1.2.3` (JSON Schema) + Luau emitters (the runtime `ui` module and the app declarations)** — `crates/vos-codegen`
- Domain types derive `TS`/`JsonSchema` behind a `codegen` feature (derive-only, no I/O). `cargo run -p vos-codegen --example generate` writes `generated/{types.ts, ui.lua, schemas.json}`. ts-rs honours serde attributes (`tag`, `rename_all`): `UiNode` comes out as the discriminated union `{ "type": "stack", … } | { "type": "button", … }`. The Lua emitter walks the JSON Schema (`oneOf`, `$ref`, recursive `#`, optional fields) into Luau `export type` declarations (it first emitted LuaCATS `---@class` definitions; NEOC-140 moved apps to Luau); **test: a wrongly-shaped node is flagged by luau-lsp (the negative fixtures in `scripts/fixtures/negative/`)**.
- `specta 2.0.0-rc.28` / `specta-typescript 0.0.15` are still pre-release: not chosen. Rough edges: ts-rs warns on `#[serde(try_from)]` (we override with `ts(type="string")`); `#[serde(default)]` fields need `#[ts(optional)]` to become optional in TS.
- **Superseded by wave A2:** the component schema is Rust data in `vos-domain` (`ui` module) and `vos-codegen` emits `types/generated/{vos.ts, ui.lua}`, the Luau declarations `types/generated/vos.d.luau` (which also carries neoc's declarations of the modules an app is offered, read from `types/neoc.d.luau`, and the one `declare require`) and `docs/reference/components.md`; see `docs/reference/components.md` and the `vos-codegen` crate docs.

### 8. Distribution
- **Binary:** host, the `release-host` profile (inherits `release`), `opt-level="z"`, LTO, `codegen-units=1`, `strip`, `panic=abort`: **15.1 MiB** (15 866 640 bytes, NEOC-113, on Luau and with neoc-vm's whole module set; the 5.5 MiB of the Luau spike no longer applies), bundled SQLite, rustls, axum, rmcp, ACP, notify. Opt-level 3 is 25-40% faster on VM creation/dispatch (0.14 vs 0.19 ms, 1.8 vs 3.1 µs); ship level 3 unless size matters (the Luau spike's C++ VM was compiled by `cc` with the profile's opt-level, so `z` also affects VM speed).
- **Cross-compile:** pure crates `cargo check --target x86_64-unknown-linux-gnu` fine. Anything with C/C++ (Luau, SQLite, `aws-lc-sys` for rustls) fails without a cross C toolchain (`cc-rs: failed to find x86_64-linux-gnu-gcc`). Recommend **`cargo-zigbuild`** (or build natively per OS in release CI); the Docker daemon wasn't running, so Linux builds were **not verified**. macOS: native arm64 built; x86_64-apple-darwin is just `rustup target add` (same SDK). `bundled` SQLite + vendored Lua mean no system libs to link.
- **Dev hot reload:** see section 1 (proven).

## Recommended crate list (versions resolved in `Cargo.lock`)
`tokio 1.54.1` · `axum 0.8.9` (ws, macros) · `tower-http 0.6.11` (fs) · `mlua 0.12` (luau, async, serialize, send, macros; in `neoc-vm` and the `neoc` CLI) · `rusqlite 0.43.0` (bundled, hooks) · `rmcp 3.11.0` · `agent-client-protocol 5.3.0` (process, stdio) · `reqwest 0.13.5` (rustls) · `serde 1`/`serde_json 1.0.154` · `ts-rs 12.0.1` · `schemars 1.2.3` · `notify 8.2.2` · `thiserror 2` (libs) · `anyhow 1` (host binary only) · `async-trait 0.1` · `parking_lot 0.12` (no lock poisoning ⇒ no `unwrap`) · `sha2 0.10` · `tokio-util 0.7` · `futures-util`. Dev: `tokio-tungstenite 0.30`, `cargo-nextest`, `cargo-deny`. Hand-rolled: JSON-RPC peer, AG-UI events. Not used: sqlx (probe only), jsonrpsee, specta.

## Workspace layout (hexagonal; each adapter is its own crate depending inward only)

```
                       vos-host  (bin; wiring only; the ONLY place anyhow + every adapter appear)
                                  |
   driving adapters               |                         driven adapters
   vos-http  (/rpc WS, guard, static, RpcHub)   vos-lua   (AppRuntime) --> neoc-vm (Luau VM lib)
   vos-mcp   (/mcp, ToolRegistry)               vos-sqlite (DataStore, AppData)
   vos-agui  (/agui SSE)                        vos-fetch  (Fetcher: file/http/https)
   vos-acp   (AgentConnector, ACP client)       vos-codegen (dev-time: TS + Lua + schemas)
                      \                                  /
                       vos-application   ports (traits) + use cases (AppService), in-memory fakes (`testing`)
                                  |
                       vos-domain        pure model: AppId, AppRef/Manifest (+sha256 verify), ToolGrant,
                                             AgentIdentity, ToolSpec/QualifiedToolName, Notification,
                                             UiNode/UiPatch/UiEvent, JSON-RPC + protocol messages. No I/O.
```

| crate | responsibility | may depend on |
|---|---|---|
| `domain` | the model above; unit tests without any framework | serde, serde_json, sha2, thiserror (+ ts-rs/schemars behind `codegen`) |
| `application` | **ports**: `AppRuntime, DataStore/AppData, ToolRegistry, Notifier, EventSink, AgentConnector, AgentDirectory, Clock, Fetcher`. **Use cases** (`AppService`): install-from-url (fetch → sha256 verify → migrations → load → publish tools → announce), uninstall(+purge), dispatch UI event, tool call with permission check, start agent run. Runtime-free (tokio is a dev-dependency only) | domain, async-trait, parking_lot, thiserror |
| `lua`, `sqlite`, `fetch`, `mcp`, `acp`, `agui`, `http`, `codegen` | one adapter each | application, domain (never another adapter; dev-deps may) |
| `neoc-vm` | support library, not an adapter: the per-app Lua VM (mlua on Luau, built from source), its environment, the module registry and the lifecycle of what apps spawn. Depends on no other crate in the workspace | external crates only; **only `lua` (and the `neoc` CLI) may depend on it** (`check-deps`, `deny.toml`) |
| `host` | build adapters, hand each to its port, mount routers, bind 127.0.0.1, graceful shutdown | everything |

- A `ports` crate split (adapters depend on traits only, not use cases) is cheap if `application` grows; driving adapters (`http`, `mcp`) legitimately need the use cases.
- Add later: `sdk` (the Luau declarations in `types/generated/vos.d.luau` + stubs shipped to app authors), `registry` (a list of app-ref URLs, pure).
- **Error types per layer:** `DomainError` → `PortError` (what adapters return; each adapter maps its errors with `From`) → `AppError` (use cases; wraps both) → JSON-RPC code / MCP `isError` at the edge. `anyhow` only in `main`.
- **Workspace config (in `Cargo.toml`):** `[workspace.package]`, `[workspace.dependencies]` with pinned versions, `[workspace.lints]`: clippy `pedantic` warn, `unwrap_used`/`expect_used`/`panic` deny, `unsafe_code = "forbid"`; `clippy.toml` with `allow-unwrap-in-tests`; `rustfmt.toml`. (Integration-test helper fns need a one-line `#![allow(clippy::unwrap_used)]` in each `tests/*.rs`; the in-memory fakes carry one `allow`.)
- **Dependency direction enforcement — recommend a `cargo metadata` gate (built as `scripts/check-deps.ts`)** (60 lines over `cargo metadata`): an explicit per-crate allow-list of internal edges, an allow-list of external crates for `domain`/`application`, and a framework deny-list (tokio, axum, mlua, rusqlite, reqwest, rmcp…) for the core. Proven: passes now; adding `application → lua` fails with `internal edge not allowed`. Keep `cargo-deny` too (`deny.toml`: `bans` with `wrappers` pins each framework to its adapter, plus licenses/advisories/sources). Crate edges alone aren't enough (a stray `use tokio` in `application` would compile if the dep were present), hence the gate.
- **Test strategy per layer** (all demonstrated): domain = plain unit tests (7); application = use cases against in-memory port fakes, no mocking library (4: install/verify/tamper-no-state-change/uninstall); adapters = integration tests through the real library (lua: sandbox/modules/lifecycle/async/hot reload; sqlite; mcp with an rmcp client; acp against a fake agent + the real adapter; http with a WS client; fetch against a local server); end-to-end = `vos-host/tests/e2e.rs` driving `/rpc` + `/mcp` + `/agui` of the real wiring. Run with `cargo nextest run --workspace` (0.5 s).

## Open risks
1. **App typing is lint-only.** Apps are Luau; luau-lsp checks them against the generated Luau declarations (`bun run lint:luau`) but nothing is checked at run time beyond what the host validates at its edge. Luau does not flag an unknown key in a table literal, and an app's own bundled module is `any` to the file that requires it in the editor (luau-lsp resolves a relative `require` only while `require` is its built-in, which the declarations replace); `lint:luau` type-checks those modules in a second pass without the `declare require`.
2. **Nothing bounds an app's CPU, memory or time** (a decision for a homelab host: see "No permissions, no budgets"). A runaway app costs the host a worker thread until it is unloaded. SQLite has `busy_timeout` (2 s) and the progress handler for its own statements; there is no per-call timeout.
3. **Memory per VM ~0.5 MiB** baseline and one gate per VM: fine for dozens of apps; measure real apps. CPU fairness relies on tokio's multi-thread pool.
4. **rmcp / ACP / MCP spec churn** (rmcp 3.x already carries two protocol generations; ACP crate is at 5.x with a draft v2). Pin, re-run the adapter tests on each bump, keep each behind its port. Claude-side MCP clients today speak 2025-x (sessions); both are served.
5. **Notification ordering** on `/rpc` (notifications can overtake responses) must be part of the client contract.
6. **Auth on `/rpc`: closed by device pairing** (see "Pairing" below). Every client route needs a paired device's session cookie, from loopback too, so another local user or process cannot connect without a code; the Origin/Host guard still stops cross-site pages and DNS rebinding. What is left: a pairing code is shown to whoever can read the host's terminal, and sessions never expire (revoke them in Settings > Devices).
7. **ACP→AG-UI mapping** (prompt streaming, permission requests, tool-call events) is the largest unproven piece; the transport pieces on both sides are proven.
8. **Linux/Windows builds unverified** (needs zigbuild/CI); `x86_64-pc-windows-gnu` target is installed but untried; `notify` and process spawning differ per OS.
9. **Lua ergonomics:** no debugger, error spans only as `app/app.lua:line` (the bundle's line, which `retain_lines` keeps equal to the source line for an app without modules).
10. **Sandbox findings from the second Luau code review, as the Lua 5.1 port left them.** Fixed in the port: effects an
    unloaded or replaced app queued are dropped, not flushed; a `readOnly` call can no longer use `app.on`,
    `app.window`, `app.tools.register`, `app.thread` or the shell and window methods (`lua::vm::Shared::writable`).
    Still open, and now deliberate because the port has no budgets (see "No permissions, no budgets"):
    - a view tree's type, key, prop, slot and event names are copied per node with no byte charge (the tree is
      bounded by node count and depth, and prop values by the shared `Walk`), so a tree with very long names can use
      host memory before the node limit trips (`vos-lua/src/view.rs`);
    - `lua_to_json` formats child paths eagerly, so a huge key can make one conversion slow while it holds the VM
      gate (`convert.rs`);
    - string bytes are counted before `to_string_lossy`, so invalid UTF-8 (each bad byte becomes a 3-byte U+FFFD)
      is undercounted;
    - the `app.db` source constructors (`query`, `row`, `scalar`) still work in a `readOnly` call: they declare a
      source, they do not write.

## What I did not do / failed
- Did not run Docker/Linux builds (daemon down). Did not test `jsonrpsee` (reasoned from docs). Did not test `https://` fetches (no network test; reqwest+rustls is compiled and works for `http://`).
- No file in the source checkout was modified; no processes left running.

## Running the host

The host is the product: one binary that serves the built client, `/rpc`, `/agui` and `/mcp`.

```sh
cargo run -p vos-host                  # serve (bun run host does the same)
cargo run -p vos-host -- pair          # print a fresh pairing code (alias: --new-pairing-code)
cargo run -p vos-host -- --config f    # use a specific config file
RUST_LOG=vos_host=debug,info cargo run -p vos-host
```

It listens on `0.0.0.0:7400` by default. At start it prints the URLs it is reachable on (loopback, each LAN
address, the hostname) and a **pairing code**. `SIGINT` and `SIGTERM` shut it down gracefully: it stops
accepting, drains open connections for 3 seconds, unloads every app (their data is kept), stops agent
processes, and exits 0. Logging is `tracing`, filtered by `RUST_LOG` (default `info`), written to stderr; the
banner and pairing code go to stdout.

### Pairing

Every client route needs a paired device, **including from loopback**: there is no trusted-network shortcut.

1. The host prints a code like `K7QM-3XHR`: 8 characters from a 31-symbol alphabet without `0 O 1 I L`
   (about 40 bits), valid 10 minutes, single use. `vos pair` issues another at any time, even while the
   host is running (they share `<data>/system.db`).
2. The client's static files load unauthenticated. The shell asks `GET /session` (`{"paired":false}`), shows its
   pairing screen, and posts `POST /pair` with `{"code": "...", "deviceName": "..."}`. The device name defaults
   to a label from the `User-Agent`.
3. The host answers with `Set-Cookie: vos_session=<256-bit token>; Path=/; HttpOnly; SameSite=Strict`
   (plus `Secure` over TLS) and `{"device": {...}}`. The database stores only the SHA-256 of the token and of the
   code, with the device name, creation time and last-seen time.
4. `/rpc` and `/agui` (and any route added later: protected is the default) require that cookie, else `401`.
5. Wrong codes are rate limited per client address (an IPv6 address counts as its /64): after 5 attempts in a minute,
   `429` with `Retry-After`, even for the right code, and 20 a minute across all clients; one noisy client cannot
   stall the others. 10 wrong guesses in 10 minutes destroy every outstanding code. Over TLS the cookie is
   `__Host-vos_session` (Secure, `Path=/`, no Domain) and only that name is accepted.
6. Sessions have no expiry; they last until revoked. The JSON-RPC methods `host.devices.list` (`{devices:[{id, name,
   createdAt, lastSeenAt, current}]}`) and `host.devices.revoke` (`{id}`) back Settings > Devices. Revoking a
   device hangs up its sockets, ends its open `/agui` event streams and ends its session.
7. `/rpc` limits: params are a typed struct per method (`deny_unknown_fields`; a mistake is `-32602`, never a silent
   default), at most 64 requests in flight per connection (then `-32000` at once), frames up to `MAX_MESSAGE_BYTES`
   (1 MiB). The Origin/Host guard reads `Host`, or the URI authority over HTTP/2, and refuses when both are present
   and differ.

`/mcp` is separate: per-agent bearer tokens only. A paired session is not an MCP credential and an MCP token is
not a session. For local MCP clients the host writes `<data>/mcp.json` (mode 0600,
`{"url": ".../mcp", "headers": {"Authorization": "Bearer ..."}}`); the token changes on every start.

The **Origin/Host guard** covers every route. A request's `Host` must be a loopback address, one of this
machine's interface addresses (re-read every 10 s, so joining a network works without a restart), its hostname,
any `*.local` name, or a name in `allowed_hosts`; own names and addresses must carry the listening port. A browser
`Origin` must satisfy the same rule. Everything else gets `403`, which is what stops DNS rebinding and
cross-site WebSockets.

### LAN and TLS

Reaching the host from another device needs only the default bind; the client then opens
`http://<lan-address>:7400`. Plain HTTP on the LAN is fine for the desktop itself, but **a PWA's service worker
(install, offline) needs a secure context**, and only `localhost` is exempt. For a PWA on another device, serve TLS:

```toml
[tls]
cert = "certs/vos.pem"   # certificate chain, PEM
key = "certs/vos.key"    # private key, PEM
```

With TLS the host speaks HTTPS directly (rustls) and sets `Secure` on the cookie; `mcp.json` then holds an
`https://` URL. Use a certificate the devices trust (a local CA such as mkcert, or a real name via a tunnel or
reverse proxy). Behind a TLS-terminating proxy, leave TLS off, set `secure_cookies = true` and list the proxy's
name in `allowed_hosts`.

### Configuration

Defaults, then `vos.toml`, then environment variables. The file is `--config <path>`, else
`VOS_CONFIG`, else `./vos.toml`, else `<data_dir>/vos.toml`. Unknown keys are an error.

| `vos.toml` | environment | default |
|---|---|---|
| `data_dir` | `VOS_DATA` | `~/.vos` (mode 0700): `system.db`, `apps/<id>.db`, `mcp.json` |
| `bind` | `VOS_BIND` | `0.0.0.0` |
| `port` | `VOS_PORT` | `7400` (`0` picks a free port) |
| `client_dir` | `VOS_CLIENT_DIR` | `client/dist`; not built = API only |
| `registries` | `VOS_REGISTRIES` (comma list) | the catalog Discover browses: registry files (`{"apps": ["<id>/<version>/app-ref.json"]}`, relative URLs resolve against the registry) or app refs, by URL; default `dist/apps/registry.json` (what `bun run build:apps` writes) over `file://` when it exists |
| `install` | `VOS_INSTALL` (comma list) | none: app-ref URLs the host makes sure are installed at every start |
| `allowed_hosts` | `VOS_ALLOWED_HOSTS` (comma list) | none |
| `dev_client_port` | `VOS_DEV_CLIENT_PORT` | none: also accept the Vite dev server on this port |
| `dev_reload` | `VOS_DEV_RELOAD` | off: watch the `file://` app bundles and rebuild an app's VM (database kept) when one changes |
| `secure_cookies` | `VOS_SECURE_COOKIES` | on when TLS is configured |
| `[tls] cert`, `key` | `VOS_TLS_CERT`, `VOS_TLS_KEY` | TLS off |

An explicit `client_dir` that is not built is an error; when the default `client/dist` is not built the host serves the
API only and logs a warning.

**The rename's one-time move.** The program used to have another name and kept its data (and its config file) in a
hidden directory of the home folder named after it. On start, when the data directory in use is the default `~/.vos`,
`~/.vos` does not exist and that older directory does, the host renames it to `~/.vos` (and its config file to
`vos.toml`, when there is no `vos.toml` yet), so paired devices, app data, the agent roster and the chat history carry
over. `crates/vos-host/src/migrate.rs` holds the exact old names (with the stored-thread alias in `crates/vos-domain/src/thread.rs`, the only code that spells them) and the rules:
if `~/.vos` exists nothing happens and the two are never merged; if neither exists nothing happens; an explicit
`VOS_DATA` or `data_dir` is never a reason to move the old default. A rename that fails stops the start with an error
naming both paths and loses no data (nothing is copied or deleted; at worst the config file keeps its new name inside the old directory). A browser's session cookie and
the client's local settings are keyed by the old name, so each paired device pairs again once (`vos pair`).
A config file with the old name, found where a `vos.toml` would be read (the working directory, the data directory) with no `vos.toml` beside it, stops the start: the error names the file and says to rename it (with a `vos.toml` beside it, that one is used and the old file is warned about). The old environment variables (the same names with the old prefix) are no longer read, and the host refuses to start while any is set (`serve` and `pair` both): the error names each variable and its `VOS_*` replacement, so a loopback `bind` kept in the environment cannot silently become the default `0.0.0.0`.

### The gate

`bun run lint` (adds `cargo fmt --check`, clippy with `-D warnings`, `scripts/check-deps.ts`, `cargo deny check`,
`lint:luau`, `lint:stylua`, `codegen:check`), `bun run typecheck` (adds `cargo check --workspace --all-targets` and
`tsc -p types`), `bun run test` (adds `cargo nextest run --workspace --exclude neoc --exclude neoc-vm` and `bun test types scripts`), `bun run format` (adds
`cargo fmt` and StyLua). `codegen:check` fails when the committed `types/generated/*` and `docs/reference/components.md`
are stale against the Rust schema (`bun run codegen` regenerates them). `lint:luau` runs `luau-lsp analyze` (pinned 1.70.1, strict) over every Luau app and fixture against the generated
declarations in `types/generated/vos.d.luau` (one definitions file: the `app` API and `vos:ui`, neoc's declarations of the `std:`/`lib:`/`vnd:` modules an app is offered, `lib:test`, `lib:compile` and `std:workers` typed unusable, and the ONE `declare require`: two `declare require` do not merge, so the overloads are written together); negative fixtures (`scripts/fixtures/negative/*.luau`, lines marked
`-- expect-error: <text>`) must be flagged, exactly, with their marker counts pinned, so a broken definitions file cannot pass silently. `lint:luau` and `lint:stylua`
skip with a message when `luau-lsp` or `stylua` is not installed (and fail under `VOS_GATE=1` or `CI`).

## Apps: lifecycle, data and the app API (wave A4)

- **Installed apps persist.** `system.db` records every install (`installed_apps`: id, app-ref URL, version,
  enabled). At start `AppService::restore_installed` re-fetches each app from its URL (re-verifying checksums) and
  loads it; the configured `registries` are then applied on top (missing ones installed, failed ones retried). An
  app that cannot load is marked **failed** (and announced as `app.failed`), the others start. Uninstall forgets
  the record; shutdown keeps it.
- **Browsing and previewing:** `app.available` lists what the configured `registries` offer (`{apps: [AppPreview], errors}`; a registry that cannot be read is an error line, the rest still list); `app.preview {url}` reads one app ref **without installing or running anything** (manifest, source host, whether the id is installed). `app.list` returns each installed app with its state, `icon`, `tint`, `description`, `source` and, while it runs, its `window` (`w`, `h`, `minW`, `minH`) from `app.manifest`. Types: `crates/vos-domain/src/api.rs`. Package manifests carry optional `icon`, `tint`, `description` and `requires: {host: ">=1.0"}` (checked against `HOST_API`); an app ref's file URLs may be relative to it.
- **Install is what was reviewed.** `app.preview` returns the app ref's `sha256`; `app.install {url, sha256}` installs only that ref (a changed ref is refused: preview it again). `app.install` takes both `url` and `sha256` (the digest is required on the wire). The install record keeps the ref digest. The digest is a record of the ref last loaded, not a pin: it pins the install to what was reviewed, and every later successful load of a changed ref (a dev `app.reload`, a restart) records the new digest, because a reload legitimately changes files. A later load (restart, `app.reload`, enable, restore) is checked on what matters instead it refuses a ref that now serves another app id (the app is marked failed with a message, the record keeps the last digest). Package files must stay on the ref's scheme and origin (no `..`, no absolute path, no foreign host or `file://` from a remote ref). Fetches read in chunks up to a cap and refuse non-regular files; redirects stay on the same origin; a remote ref cannot reach private, link-local or metadata addresses. Caps: 2 MiB per file, 8 MiB per package, 256 files.
- **Query caps:** at most 10 000 rows and 16 MiB per result, SQLite values up to 8 MiB (`SQLITE_LIMIT_LENGTH`); BLOBs leave the database as base64 text.
- **Lifecycle over `/rpc`:** `app.install`, `app.uninstall {app, purge?}`, `app.list`, `app.enable`,
  `app.disable` (VM unloaded, tools retracted, data kept), `app.deleteData` (purge, then reload on an empty
  database), `app.reload` (dev: rebuild the VM from source, keep the database; the old version keeps running if the
  new one does not load).
- **The app API** is described in `types/generated/vos.d.luau` (Luau declarations, generated from `crates/vos-codegen/src/luau/app_api.luau.tpl`): `app.manifest`, `app.db` (`migrate`, `exec`,
  `all`, `one`, `tx`, and the sources `query` / `row` / `scalar`), `app.kv`, `app.state`, `app.tools.register`
  with `app.t` input builders, `app.notify`, `app.shell`, `app.window`.
  `require` at run time knows `vos:app`, `vos:ui` and the neoc `std:`, `lib:` and `vnd:` modules: an app's own modules (`./x`, `../x`) are bundled into its one `app.lua` when it is built, and darklua leaves the others as runtime `require`s (`.darklua.json` excludes `vos:*`, `std[:]*`, `lib[:]*` and `vnd[:]*`).
- **Live sources** (`vos-application::live`): the SQLite authorizer reports every table a prepared query reads,
  `commit_hook` batches become `ChangeBatch`es (coalesced per frame), and `LiveSources::refresh` re-runs the
  affected subscriptions in one read transaction and diffs them (ordered window keys + changed rows by content hash).
- **Two-way writes** (`AppService::write_field`): `baseRev` check, three-way merge of text
  (`vos_domain::merge3`: non-overlapping edits compose, overlapping edits go to the human), human wins for
  scalars, the replaced value kept as a revision for 30 days (`_ui_revisions`) with an undo (`restore_revision`).
- **SQLite gotchas found:** the update hook is blind to `WITHOUT ROWID` tables and `REPLACE` conflict deletes (tables
  an authorized statement writes are published too); a transaction holds an async write gate so nothing interleaves
  with it; a statement past its wall-clock budget is interrupted by the progress handler.
- **Lua gotchas found:** the count hook also fires when mlua resumes the coroutine to poll an await, so the CPU
  clock is paused while the app waits on the host (and a kill lands when the call returns); a Rust future that is
  aborted mid-await is only dropped at the next GC, so no lock or transaction is held across an await (a call that
  was cut off cannot wedge the next one; a failed host call at a C boundary is collected by `pcall` and at call end); `string` coercion makes `raw_get::<String>(5)` succeed for a number, so
  check the variant.

### Windows and the view protocol (wave A4, phase 2)

- **A window is a view instance.** `app.window(name, { client, state, render, onOpen, onClose })` declares a kind;
  `view.open` creates an instance with its own `w` (per-window `w.state`, `w.client` Refs and `:get`/`:set`, `w.env`
  Refs, `w:toast/focus/scrollTo/announce/copy/close`). `render(w)` runs as a pure, tracked scope: state it reads is
  the instance's dependency set, and a write inside it raises. The Lua node tree (from the generated
  `vos:ui`) becomes the wire tree, **handlers stay closures in the VM keyed `path#event`**, and the tree is
  validated (`validate_with`, with the view's client and source declarations) before it leaves the VM.
- **Sources per render.** A source factory called in `render` returns a handle `{["$source"] = id}` (any other field
  is a `src.<id>.<field>` ref; `$`-prefixed keys are nil). One id per (declaration, params) pair, stable across
  renders (`s2`, then `s2_2` for a second param set), so ops do not churn. `view.open` subscribes each source with its
  params resolved from the client's values; `source.set` re-parameterises or moves the window; a re-render that starts
  to use a new source declares it in `data.patch.declare` with its first window.
- **Ordering.** After a handler (or an agent tool, through the `AppObserver`) the view service *settles*: re-render
  every view whose last render read state that changed (keyed `diff` -> `view.ops`, `base`/`rev`; a tree over 1 000
  ops goes as `view.tree`), deliver the windows' commands and `client.set`, refresh the live sources. All of it is
  queued on the socket before the response, which therefore means "the UI already reflects it". Work the app does on
  its own (a `task.spawn` loop, a request to its own server) settles the same way, with no event: the runtime reports
  it through `AppActivity::background_settled` (see "One call at a time per VM"), and settling is one at a time per
  app, so an event's answer still comes after every op that was rendered before it. Notifications from
  the client are handled in order, inline, so `client.state` is FIFO ahead of a later event.
- **Two-way writes.** `row.set {source, key, field, value, baseRev}`: the binding kind (text / scalar) comes from the
  component schema (`ui::bindings`); the merge base is the value the host last sent for that row field (a ring of
  8); the ack carries `resolution` (`accepted` / `merged` / `replaced` / `rejected`) and, when a value was replaced,
  `undo {revisionId, lost}` that `row.restore` puts back. `persist = "app"` client values are stored in `_ui_state`
  and come back in the next `view.open`.
- **Opening windows and toast actions.** `app:openWindow(name, params?)` (an agent's tool bringing an app forward) goes
  through the `Shell` port to every connected client as the host notification `window.open {app, window, params?}`: the
  shell opens the window or raises the app's open one, then does `view.open`. A toast's action is
  `w:toast { text, action = { title, onTap } }`: the wire toast carries `action: {title, id}`, the VM keeps the closure
  (the last 32 per window, each run once), and the client reports the tap as `view.event` on path `$toast/<id>`,
  event `tap`.
- **Gaps.** `state.set` (two-way bound window state) answers an error: app/window state is read as values in render,
  so there is no `state.*` ref to bind; `view.resize` / `view.visibility` are accepted and ignored; renders are not
  coalesced per frame; host restart has no grace period (a reconnect opens fresh instances that adopt the client's
  persisted values); `window.open` goes to every connected client, not only the most recently active one, and a
  repeat for an app whose window is already open raises it without passing the new `params`.

## Agents over AG-UI, identity and permissions (wave B3)

Agents are **contacts** in `system.db`. The host launches each over ACP (stdio), keeps the process alive, and exposes
its chats to the client over AG-UI (`/agui`, HTTP + SSE, behind the device-session guard). ACP never reaches the
browser: `vos-acp` maps ACP to the domain's `AgentEvent`, the application's `ChatService` folds those into a thread
(history, state, interrupts) and into AG-UI events, `vos-agui` is the HTTP/SSE edge.

### The `/agui` contract

All routes need a paired device (`401` otherwise). Errors before a stream starts are JSON `{"error": "..."}`: `422` for a
body that is not a `RunAgentInput`, `404` unknown agent or thread, `409` a run is already in progress on the thread,
`502` the agent process could not start.

| route | purpose |
|---|---|
| `POST /agui/threads` `{threadId, agentId}` | Ensure a thread: start the agent process if needed, `session/new` (with the agent's own MCP token), idempotent for the same ids. Returns a `ThreadSnapshot`. |
| `GET /agui/threads/{id}` | The `ThreadSnapshot`: `{threadId, agentId, running, state, messages, interrupts}`. |
| `POST /agui` | A run: `RunAgentInput` (`threadId`, `runId`, `messages`, `state`, `tools`, `context`, `forwardedProps`, optional `resume`). The **last `user` message** is the prompt (the ACP session holds the context); `forwardedProps.agentId` creates the thread if `POST /agui/threads` was not called. Answers `text/event-stream`, one `data: <event json>` frame per AG-UI event. |
| `POST /agui/threads/{id}/cancel` | ACP `session/cancel` (also resolves a pending permission as cancelled). `204`. A client that drops the SSE connection mid-run cancels the same way. |
| `POST /agui/threads/{id}/settings` | `{"mode": "<id>"}` or `{"config": {"id": "<option id>", "value": <string or bool>}}`: ACP `session/set_mode` / `session/set_config_option`. Answers the new `ThreadState`; a running stream also gets a `STATE_DELTA`. |

**Events** (names and fields as docs.ag-ui.com; `camelCase`). One run is `RUN_STARTED`, a `STATE_SNAPSHOT`, the turn's
events, then `RUN_FINISHED` or `RUN_ERROR`.

| ACP | AG-UI |
|---|---|
| `session/prompt` accepted, finished with a stop reason | `RUN_STARTED` ... `RUN_FINISHED {outcome:{type:"success"}}` (`result: {stopReason}`) |
| prompt failed, agent exited, a `refusal` stop | `RUN_ERROR {message, code}` (`agent_exited`, `prompt_failed`, `refusal`) |
| `agent_message_chunk` | `TEXT_MESSAGE_START {role:"assistant"}`, `TEXT_MESSAGE_CONTENT`, `TEXT_MESSAGE_END` (a message spans consecutive chunks) |
| `agent_thought_chunk` | `REASONING_START`, `REASONING_MESSAGE_START {role:"reasoning"}`, `REASONING_MESSAGE_CONTENT`, `REASONING_MESSAGE_END`, `REASONING_END` |
| `tool_call` | `TOOL_CALL_START {toolCallName: title}`, `TOOL_CALL_ARGS` (raw input as JSON text, if any), `TOOL_CALL_END`, and a `CUSTOM vos.tool` |
| `tool_call_update` | `CUSTOM vos.tool {toolCallId, title, kind, status}` (`pending`, `in_progress`, `completed`, `failed`); on `completed`/`failed` with text output, `TOOL_CALL_RESULT` |
| `plan` | `STATE_DELTA` replacing `/plan` |
| `current_mode_update`, `config_option_update`, `available_commands_update`, `usage_update`, `session_info_update` | `STATE_DELTA` (RFC 6902 `replace`/`add`) under `/session/...` |
| `session/request_permission` | the run ends: `RUN_FINISHED {outcome:{type:"interrupt", interrupts:[...]}}` (AG-UI interrupts) |
| user-visible host errors (not ACP) | `RUN_ERROR` |

**Permission requests are AG-UI interrupts.** ACP's `session/request_permission` blocks the agent mid-turn. The host
ends the current run with `RUN_FINISHED {outcome:{type:"interrupt", interrupts:[Interrupt]}}` and keeps the ACP turn
suspended (up to 10 minutes, `expiresAt`, then it resolves as cancelled). The interrupt is
`{id, reason:"tool_call", message: <tool title>, toolCallId?, expiresAt, responseSchema:{type:"object", properties:
{optionId:{type:"string", enum:[...]}}, required:["optionId"]}, metadata:{"vos:options":[{optionId, name, kind}],
"vos:tool":{title, kind}}}` where `kind` is ACP's `allow_once`, `allow_always`, `reject_once` or `reject_always`.
The client answers with a **new run on the same thread** whose input has `resume:[{interruptId, status:"resolved",
payload:{optionId}}]` (or `status:"cancelled"` to deny) and no new user message; that run streams the rest of the
turn (`RUN_STARTED` again, ...). A reload re-offers pending interrupts through `GET /agui/threads/{id}`. A new user
message while interrupts are pending, a resume that misses an interrupt, an unknown id: `RUN_ERROR`.

**State** (what AG-UI does not define, typed in `vos_domain::thread`, zod-validated in the client): `ThreadState =
{ session: SessionState, plan: PlanEntry[] | null }`.

```jsonc
"session": {
  "agent":   { "name": "claude-agent-acp", "version": "0.108.0" } | null,
  "title":   "string" | null,
  "toolsConnected": true,                       // the agent got /mcp (HTTP MCP capability)
  "modes":   { "current": "default", "available": [{ "id", "name", "description"? }] } | null,
  "config":  [ { "id": "model", "name": "Model", "description"?, "category"?: "model" | "thought_level" | "mode" | string,
                 "kind": "select", "current": "opus", "options": [{ "value", "name", "description"? }] },
               { "id", "name", "kind": "boolean", "current": true } ],
  "commands": [{ "name": "compact", "description": "...", "hint"?: "..." }],   // slash commands: send "/name args" as the user message
  "usage":   { "used": 12000, "size": 200000, "cost"?: { "amount": 0.12, "currency": "USD" } } | null
}
```

**Thread history** (`ThreadSnapshot.messages`, AG-UI messages in order): `{id, role:"user", content}`; `{id,
role:"assistant", content}` (a text segment); `{id, role:"reasoning", content}`; a tool call is an assistant message
`{id, role:"assistant", toolCalls:[{id, type:"function", function:{name, arguments}}], vos:{title, kind,
status}}` followed by `{id, role:"tool", toolCallId, content}` once it has output; a failed run is `{id, role:"system",
content, vos:{error:true}}`. History lives in `system.db` (`thread_messages`); the ACP session does not survive a
host restart (a new one starts with the next prompt; the transcript stays).

**Agents and tokens.** `Agent {id, name, avatar?, role, runtime:{command, args, env}, grants, approval}`. Grants are
**deny by default**: `{"apps": []}` means no tools; `{"apps": ["notes"]}` is an allow-list; `{"all": true}` is the
explicit "all apps" option. `approval` (`ask` | `auto_read_only` | `allow_all`, default `auto_read_only`) says what the
host does with an agent's ACP permission requests: `ask` prompts always. `auto_read_only` skips the prompt only when
the permission title is exactly `mcp__vos__<app>__<tool>`, that tool exists and is declared `readOnly`, the agent is
granted its app, and the request's kind is one of `read`, `search` or `fetch` (an allowlist: `execute`, `edit`, `delete`,
move, `think`, `other`, an unknown kind or no kind at all asks), and it answers with allow-once, never a standing allow; anything else
(shell, edits, other apps' tools) prompts. `allow_all` (`AllowEverything` in code) never prompts and approves *every*
request, Bash and edits included; grants still bound which app tools the agent sees. An option kind the host does not
know is never chosen automatically. Open risk: the title and kind are the agent's own text, so an agent could title a
shell call exactly like a granted read-only tool; the exact-match rule narrows this but ACP carries nothing stronger. A persistent
**MCP bearer token** is stored hashed (SHA-256; shown once, at `agents.create` and `agents.token.rotate`), for agents the
host does not launch (an agent without a `runtime.command` is such an external client: it has an identity, grants and a
token, and the host never starts it). A host-launched agent gets a **fresh random token for every session** (one per
chat thread), kept only as a hash in memory, handed to the agent in `session/new` and revoked when the session ends or
the agent is deleted: every `tools/call` is attributed (`ctx.actor = {kind="agent", id,
name}`) and checked against that agent's grants. `tools/list` is filtered to the agent's granted apps; `tools/list_changed`
goes to that agent's sessions when its grants change or an app it may use is published or retracted.

`/rpc` methods (for a future Contacts/Settings UI), all behind the device session; `agents.create` and
`agents.token.rotate` are the only places a plaintext token is ever returned:

| method | params | result |
|---|---|---|
| `agents.list` | none | `{agents: AgentRow[]}`: no tokens, and the runtime as `{command, args, envKeys}` (env values never leave the host) |
| `agents.create` | `{id?, name, avatar?, role?, runtime:{command, args?, env?}, grants?, approval?}` | `{agent, token}` (`id` defaults to a slug of the name) |
| `agents.update` | `{id, name?, avatar? (null clears), role?, runtime?, approval?}` | `{agent}` |
| `agents.delete` | `{id}` | `null`; its tokens and sessions' access die with it |
| `agents.grants.set` | `{id, grants:{all?, apps?}}` | `{agent}`; the agent's MCP sessions get `tools/list_changed` |
| `agents.token.rotate` | `{id}` | `{token}`; the old persistent token stops working |

Errors: `-32602` bad params, `-32001` invalid agent, `-32002` no such agent, `-32004` id taken. Every change also
broadcasts `agents.changed {id}` to the paired clients. Deleting an agent, or changing its command, args or env, stops
its ACP process; each agent's process starts under its own lock and `initialize` times out after 30 s, so one hung
agent holds up only itself.

**Configuration.** `vos.toml` may list `[[agents]]` (an `Agent`: `id`, `name`, `role`, `avatar`, `[agents.runtime]`
`command`/`args`/`env`, `[agents.grants]` `all`/`apps`, `approval`). They seed the roster: an entry is added when no
agent has its id and left alone afterwards (edit through `agents.*`). With none configured, Claude is added if its ACP
adapter is installed (`node_modules/@agentclientprotocol/claude-agent-acp`), with no grants. The host also keeps a
`local` agent (external, all apps) whose token is rewritten into `<data>/mcp.json` at every start: the one seeded
exception to deny-by-default, because it is the person's own MCP client.

**Why deny by default, and `auto_read_only`.** A new agent can do nothing until a person names the apps it may use, and
"all apps" is a switch that has to be flipped on purpose. `auto_read_only` rests on the `readOnly` declaration
of the app: the host stops such a tool writing through the `app.*` API (below) but not through the app's own `std:fs`,
`vnd:rusqlite` or `std:net` code, so it trusts an app the person installed to declare honestly (apps are the person's
own code: see "No permissions, no budgets"). Every tool that is not declared `readOnly`, and everything that is not
a vos tool, still asks.

**Apps and chats.** An app shows a chat with `ui.ChatTranscript { thread = chat { thread = <id>, agent = <id> } }`
where `local chat = app.thread()`. A `thread` source declares only the thread and the agent (literals or Refs); it
runs no query, is never subscribed, and sends no `data.patch`: the client opens the thread on `/agui`. (When a param
Ref changes the client re-asks with `source.set`; the host answers `{kind:"value", value:null}`.) The `send` and
`stop` events also reach the app (`e.payload.text`), so it can title the chat. An app can read the roster with
`app.agents:list()` (`{id, name, avatar?, role}`, never grants, tokens or commands). A literal `agent` of
`app.thread()` must be on the roster. Thread ids are scoped to the app: the renderer opens an
app's thread on `/agui` as `<app>:<id>` (the app chooses `id`, the renderer adds the prefix), so no app can name another
app's chat.

**`readOnly` is enforced.** A tool declared `readOnly = true` runs with app writes blocked. The app's database handle
is `AppData::read_only()`: SQLite `PRAGMA query_only` for each statement (checked when the statement runs, so a statement
cached while writable cannot sneak a write through; the authorizer refuses app SQL that tries to lift it), so
`db:exec`, writes in `db:tx`, DDL and a write smuggled through `db:all ... returning` all fail. `app.kv:set`/`delete` and
`db:migrate` raise with a message that says why. A `readOnly` call also cannot assign `app.state`, `app.notify`, open a window (whose `onOpen` could write) or
drive the shell (`app.shell.*`), and the flag travels through the `AppRuntime` port, not only the Lua adapter.
`openWindow` accepts only windows the app declared. A tool that must write is not `readOnly`: the Calculator's `calculate` records the sum in its history, so it is not
`readOnly` (its `history` tool is).

**Tool names** are local identifiers `[a-z][a-z0-9_]*` (no dots); the MCP name is `<appId>__<tool>`.

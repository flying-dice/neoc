#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! A generator of hostile scripts, and the invariants every one of them must leave intact.
//!
//! Lua 5.1 cannot yield across a `string.gsub` callback, a sort comparator, a metamethod or an iterator, so a host
//! call made there fails with a Lua error, and the script (which may catch it) goes on. Each way that can leave a
//! call half-done (a future, a lend mark, a lock, a transaction) was found one case at a time; this test builds
//! programs out of the ingredients and checks the invariants over all of them instead.
//!
//! A program is a few *steps*. A step is one **leaf** (an idle wait, a database or key-value read or write, a
//! mutex, a write through `app.state`, `app.notify` or `app:openWindow`), put inside a **context** where the host
//! call cannot suspend (or none), and wrapped in **controls** (`pcall`, `xpcall`, coroutines, a plain yield, tasks,
//! a nested `db:tx`, an error thrown mid-body, a wait left in flight and resumed inside a transaction). It runs as a
//! tool of one of four kinds: `readOnly`, writable, writable with concurrent second tools, or `readOnly` calling the
//! app's own server. Beside it run side calls that get in whenever the VM is free: a counter bump, a second writer, a
//! `readOnly` peek that tries every write, and a *meddler* that resumes the threads the program left behind (its own
//! top-level thread, the threads its tasks run in, the threads inside its `pcall`s, `xpcall`s and nested protected
//! calls, its coroutines), with and without arguments, from another call.
//! Per case, within a timeout:
//!
//! - (a) nothing `app.*` could write (db, kv, state, notify, windows) was written in a read-only context;
//! - (b) nothing ran inside a call between its waits: while a step that cannot suspend runs, a concurrent tool
//!   that bumps a counter never gets in;
//! - (c) no acknowledged write was lost, and (d) a `db:tx` that returned success committed everything its body
//!   acknowledged (and one that failed left none of it behind);
//! - (e) the app still answers a ping tool and the database still answers a query (nothing is wedged);
//! - (f) the host did not panic (a panic in a call is a failed join);
//! - (g) a thread the script did not make (a call's top-level thread, a task's thread, the coroutine a `pcall` or
//!   `xpcall` runs its body in, found with `coroutine.running()`) is never resumed by a script: a `coroutine.resume`
//!   of one from anywhere returns false. Only the coroutines the script made (`create`, `wrap`) can be resumed.
//!
//! A second test runs the same programs and reloads or unloads the app in the middle: nothing may be wedged, no VM may
//! keep the database, and a `readOnly` program still left nothing behind.
//!
//! The scripts are generated from a seed, so a failure names the case and prints the program. Environment:
//! `CHAOS_CASES` (default 1500), `CHAOS_DISTURBED_CASES` (300), `CHAOS_SEED` (default fixed), `CHAOS_ONLY=<case>` to run one case
//! verbosely, `CHAOS_WATCHDOG` (seconds, default 120) before a hung run is ended.

mod common;

use agentos_application::{
    ports::{AppData, AppRuntime, Notifier, PortResult},
    testing::RecordingShell,
};
use agentos_domain::Notification;
use agentos_lua::LuaRuntime;
use common::{agent_ctx, db, id, package, timed_tool as tool, try_load_with};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::Semaphore};

// ---- a seeded generator ----------------------------------------------------------------------------------------

/// `SplitMix64`: small, seedable, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    ReadOnly,
    Writable,
    Concurrent,
    OwnServer,
}

impl Kind {
    fn read_only(self) -> bool {
        matches!(self, Self::ReadOnly | Self::OwnServer)
    }
}

/// What a leaf is, for the checks: can it hold the call at a wait that may lend the VM?
struct Leaf {
    code: String,
    idle: bool,
}

/// The leaf `n` of a program (`n` is unique, so the rows a leaf writes can be told apart).
fn leaf(rng: &mut Rng, n: usize, kind: Kind) -> Leaf {
    let mut leaves: Vec<Leaf> = vec![
        Leaf { code: "task.wait(0)".into(), idle: true },
        Leaf { code: "time.sleep(time.Duration.from_millis(1))".into(), idle: true },
        Leaf { code: "hyper.get(URL)".into(), idle: true },
        Leaf { code: "local _ = app.db:one('select count(*) as n from t')".into(), idle: false },
        Leaf { code: "local _ = app.db:all('select v from t order by a')".into(), idle: false },
        Leaf {
            code: format!("app.db:exec('insert into t(v) values (:v)', {{ v = {n} }}) rec({n})"),
            idle: false,
        },
        Leaf { code: "local _ = app.kv:get('k1')".into(), idle: false },
        Leaf { code: format!("app.kv:set('k1', {n}) wrote('kv')"), idle: false },
        Leaf { code: "mu:with_lock(function() end)".into(), idle: true },
        Leaf { code: "mu:with_lock(function() task.wait(0) end)".into(), idle: true },
        Leaf { code: format!("attempt('state', function() state.v = {n} end)"), idle: false },
        Leaf { code: "attempt('notify', function() app.notify { title = 'chaos' } end)".into(), idle: false },
        Leaf { code: "attempt('window', function() app:openWindow('w') end)".into(), idle: false },
    ];
    if kind == Kind::OwnServer {
        leaves.push(Leaf { code: "hyper.get(OWN)".into(), idle: true });
        leaves.push(Leaf { code: "hyper.get(OWN)".into(), idle: true });
    }
    let at = rng.below(leaves.len());
    leaves.swap_remove(at)
}

/// Where the leaf runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Pcall,
    Xpcall,
    /// A `pcall` inside a `pcall`: both protected coroutines are stashed.
    NestedPcall,
    CoCreate,
    CoWrap,
    Yield,
    Spawn,
    Defer,
    Delay,
    Tx,
    Error,
    /// A wait started where Lua cannot suspend at it, left in flight, and resumed inside a transaction.
    Stale,
    /// A task started inside a transaction (it waits for the transaction to end), with a statement after it.
    SpawnInTx,
}

impl Control {
    /// The leaf runs in work that the call does not wait for (a task) or where waiting is refused (a transaction).
    fn detaches(self) -> bool {
        matches!(self, Self::Spawn | Self::Defer | Self::Delay | Self::Tx | Self::Stale | Self::SpawnInTx)
    }
}

const CONTROLS: &[Control] = &[
    Control::Pcall,
    Control::Xpcall,
    Control::NestedPcall,
    Control::CoCreate,
    Control::CoWrap,
    Control::Yield,
    Control::Spawn,
    Control::Defer,
    Control::Delay,
    Control::Tx,
    Control::Error,
    Control::Stale,
    Control::SpawnInTx,
];

/// Where Lua 5.1 cannot yield. Each gets the body in place of `<S>`.
const CONTEXTS: &[(&str, &str)] = &[
    ("gsub callback", "string.gsub('a', 'a', function() <S> end)"),
    ("sort comparator", "table.sort({ 3, 2, 1 }, function(a, b) <S> return a < b end)"),
    ("__index", "local _ = setmetatable({}, { __index = function() <S> end }).x"),
    ("__add", "local _ = setmetatable({}, { __add = function() <S> return 1 end }) + 1"),
    ("for iterator", "for _ in function() <S> end do end"),
];

fn wrap(control: Control, body: &str) -> String {
    match control {
        // each protected call stashes the thread its body runs in, for the meddlers to find (it is not the script's)
        Control::Pcall => format!("pcall(function() SH[#SH + 1] = coroutine.running() {body} end)"),
        Control::Xpcall => format!(
            "xpcall(function() SH[#SH + 1] = coroutine.running() {body} end, function(e) return e end)"
        ),
        Control::NestedPcall => format!(
            "pcall(function() SH[#SH + 1] = coroutine.running() pcall(function() SH[#SH + 1] = coroutine.running() {body} end) end)"
        ),
        Control::CoCreate => format!(
            "do local co = coroutine.create(function() {body} end) keep[#keep + 1] = co coroutine.resume(co) end"
        ),
        // the thread inside a `wrap` is the script's own: it is kept with the coroutines the meddlers may resume
        Control::CoWrap => format!(
            "do local w = coroutine.wrap(function() keep[#keep + 1] = coroutine.running() {body} end) WR[#WR + 1] = w pcall(w) end"
        ),
        Control::Yield => format!("{body} coroutine.yield() coroutine.yield()"),
        Control::Spawn => format!("task_start(task.spawn, function() {body} end)"),
        Control::Defer => format!("task_start(task.defer, function() {body} end)"),
        Control::Delay => format!("task_start(function(f) return task.delay(0.003, f) end, function() {body} end)"),
        Control::Tx => format!("TX(function() {body} end)"),
        Control::Error => format!("pcall(function() {body} error('boom') end)"),
        Control::SpawnInTx => format!(
            "TX(function() task_start(task.spawn, function() {body} end) local _ = app.db:one('select 1 as one') end)"
        ),
        Control::Stale => format!(
            "do local co = coroutine.create(function() {body} end) keep[#keep + 1] = co \
             pcall(string.gsub, 'a', 'a', function() coroutine.resume(co) end) \
             TX(function() coroutine.resume(co) end) coroutine.resume(co) end"
        ),
    }
}

/// One step of a program.
struct Step {
    code: String,
    describe: String,
}

fn step(rng: &mut Rng, n: usize, kind: Kind) -> Step {
    if rng.chance(8) {
        // coroutines left from earlier steps: whatever they were doing, they are over or resumed here
        return Step {
            code: "for _, co in ipairs(keep) do coroutine.resume(co) end".into(),
            describe: "resume every coroutine kept so far".into(),
        };
    }
    if rng.chance(5) {
        // the threads the script did not make (the call's own, its tasks', its protected calls'): it cannot resume them
        return Step {
            code: "for _, t in ipairs(SH) do if coroutine.resume(t) then viol('(g) the program resumed a thread it did not make') end end".into(),
            describe: "resume every thread the program did not make".into(),
        };
    }
    let Leaf { code, idle } = leaf(rng, n, kind);
    let mut describe = code.clone();
    let mut body = code;
    let mut may_lend = idle;
    for _ in 0..rng.below(2) {
        let control = *rng.pick(CONTROLS);
        may_lend &= !control.detaches();
        body = wrap(control, &body);
        describe.push_str(" (inner control)");
    }
    if rng.chance(65) {
        let (name, template) = *rng.pick(CONTEXTS);
        let _ = write!(describe, " in a {name}");
        // the context fails the host call with a Lua error, so it runs under `pcall`, with a counter check around it
        body = format!(
            "do local c0 = counter pcall(function() {} end) if counter ~= c0 then viol('another call ran inside a {name}') end end",
            template.replace("<S>", &body)
        );
        may_lend = false;
    }
    for _ in 0..rng.below(3) {
        let control = *rng.pick(CONTROLS);
        may_lend &= !control.detaches();
        body = wrap(control, &body);
    }
    // a step that cannot suspend at an idle wait must not let anything in (b)
    let code = if may_lend {
        body
    } else {
        format!("do local c0 = counter {body} if counter ~= c0 then viol('another call ran inside a step that cannot suspend: ' .. {describe:?}) end end")
    };
    Step { code, describe }
}

const HEADER: &str = r#"
local task = require("std:task")
local time = require("std:time")
local hyper = require("vnd:hyper")
local net = require("std:net")
local collections = require("std:collections")
local mu = collections.Mutex.new()
local state = app.state { v = 0 }
local counter = 0
local A, X, V, keep, pending, txp = {}, {}, {}, {}, 0, nil
-- what the program leaves behind for another call to find: the threads it did not make (its top-level thread, the
-- threads its tasks and its protected calls run in: no script may resume them) and the functions of its `coroutine.wrap`s
local SH, WR = {}, {}
-- set while a side call resumes the program's own script coroutines: they run in the resumer's turn, as Lua says
local MEDDLING = false

local function viol(msg) V[#V + 1] = msg end
-- an acknowledged database write: it must be there at the end, unless the transaction it was in failed
local function rec(n)
  if MEDDLING then return end
  if RO then viol("a db write was acknowledged in a readOnly call") end
  if txp then txp[#txp + 1] = n else A[#A + 1] = n end
end
local function wrote(what) if RO and not MEDDLING then viol("a " .. what .. " write was acknowledged in a readOnly call") end end
local function attempt(what, f) if pcall(f) and RO and not MEDDLING then viol("app." .. what .. " changed the world in a readOnly call") end end
-- a task is counted until its body ends, so the test knows when everything the program started is over
local function task_start(start, f)
  pending = pending + 1
  start(function() SH[#SH + 1] = coroutine.running() pcall(f) pending = pending - 1 end)
end
-- a `db:tx` whose body records into `txp` only while the body runs (the transaction holds the VM, so nothing else
-- does meanwhile); what it acknowledged counts only if the transaction succeeded
local function TX(body)
  local mine = {}
  local ok = pcall(app.db.tx, app.db, function()
    local saved = txp
    txp = mine
    local done, e = pcall(body)
    txp = saved
    if not done then error(e, 0) end
  end)
  for _, v in ipairs(mine) do if ok then A[#A + 1] = v else X[#X + 1] = v end end
end

app.db:migrate { [1] = "create table t(a integer primary key autoincrement, v integer)" }
app.window("w", { render = function(w) end })
"#;

const TOOLS: &str = r#"
local others = {}
app.tool { name = "bump", run = function() counter = counter + 1 return counter end }
-- the program's threads that are not the script's own: resuming one, from any call, must be refused (g)
local function steal(label)
  for i, t in ipairs(SH) do
    for _, extra in ipairs { 0, 1, 2 } do
      local ok
      if extra == 0 then ok = coroutine.resume(t) elseif extra == 1 then ok = coroutine.resume(t, "x") else ok = coroutine.resume(t, nil, 1) end
      if ok then viol("(g) " .. label .. " resumed a thread the script did not make") end
    end
    -- nor can it hand one to the scheduler, which would drive it under the scheduler
    if pcall(task.spawn, t) then viol("(g) " .. label .. " scheduled a thread the script did not make") end
  end
end
-- the program's script coroutines may be resumed from another call (they run in its turn); nothing may wedge or panic
local function resume_scripts()
  MEDDLING = true
  for _, co in ipairs(keep) do coroutine.resume(co) coroutine.resume(co, "x") end
  for _, w in ipairs(WR) do pcall(w) pcall(w, "x") end
  MEDDLING = false
end
app.tool { name = "meddle", run = function()
  counter = counter + 1
  steal("a meddling call")
  -- a program that is read-only is not to be written for by another call's turn
  if not RO then resume_scripts() end
  return counter
end }
-- a read-only call beside the program: every write is refused, and what it reads is consistent
app.tool { name = "peek", readOnly = true, run = function()
  counter = counter + 1
  local function refused(what, f) if pcall(f) then viol("a readOnly side call: " .. what .. " changed the world") end end
  refused("db", function() app.db:exec("insert into t(v) values (97)") end)
  refused("kv", function() app.kv:set("k1", 97) end)
  refused("state", function() state.v = 97 end)
  refused("notify", function() app.notify { title = "peek" } end)
  refused("window", function() app:openWindow("w") end)
  refused("tx", function() app.db:tx(function() app.db:exec("insert into t(v) values (96)") end) end)
  steal("a readOnly call")
  resume_scripts()
  return app.db:one("select count(*) as n from t").n
end }
app.tool { name = "other", run = function()
  local v = 1000 + #others
  local r = app.db:exec("insert into t(v) values (:v)", { v = v })
  others[#others + 1] = v
  return { v = v, changes = r.changes }
end }
app.tool { name = "ping", readOnly = true, run = function() return "pong" end }
app.tool { name = "report", readOnly = true, run = function()
  return { A = A, X = X, V = V, pending = pending, state = state.v }
end }
"#;

const SERVER: &str = r#"
local server = hyper.serve(net.TcpListener.bind("127.0.0.1:0"), function(req)
  -- a request made while a readOnly call is alive is read-only too
  attempt("state", function() state.v = 99 end)
  attempt("db", function() app.db:exec("insert into t(v) values (98)") end)
  return { status = 200, body = "ok" }
end)
OWN = "http://" .. server:addr() .. "/"
"#;

struct Case {
    index: usize,
    kind: Kind,
    source: String,
    steps: Vec<Step>,
}

fn generate(seed: u64, index: usize, url: &str) -> Case {
    let mut rng = Rng(seed ^ (u64::try_from(index).unwrap().wrapping_mul(0xD1B5_4A32_D192_ED03)));
    let kind = *rng.pick(&[Kind::ReadOnly, Kind::Writable, Kind::Concurrent, Kind::OwnServer]);
    let steps: Vec<Step> = (0..2 + rng.below(4)).map(|i| step(&mut rng, 1 + i, kind)).collect();
    let mut source = format!("local RO = {}\nlocal URL = {url:?}\nlocal OWN\n", kind.read_only());
    source.push_str(HEADER);
    if kind == Kind::OwnServer {
        source.push_str(SERVER);
    }
    source.push_str(TOOLS);
    let _ = writeln!(
        source,
        "app.tool {{ name = \"prog\", readOnly = RO, run = function()\n  SH[#SH + 1] = coroutine.running()"
    );
    for step in &steps {
        let _ = writeln!(source, "  -- {}\n  {}", step.describe, step.code);
    }
    source.push_str("  task.wait(0.001)\n  return 'done'\nend }\n");
    Case { index, kind, source, steps }
}

// ---- running a case and checking it ------------------------------------------------------------------------------

async fn canned_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                    .await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/")
}

/// Run one case; the failures it found, each naming the invariant.
async fn run(case: &Case) -> Vec<String> {
    let data = db();
    let Ok(vm) = try_load_with("chaos", &case.source, data.clone()).await else {
        return vec!["the generated program did not load".into()];
    };
    let vm = Arc::new(vm);
    let (mut failures, acknowledged) = drive(case, &vm).await;
    match settle(&vm).await {
        Ok(report) => failures.extend(verify(case, &vm, &data, &report, &acknowledged).await),
        Err(why) => failures.push(why),
    }
    failures
}

/// The calls that try to get in beside a program of this kind.
fn side_tools(kind: Kind) -> &'static [&'static str] {
    if kind == Kind::Concurrent {
        &["bump", "other", "meddle", "peek"]
    } else {
        &["bump", "meddle", "peek"]
    }
}

/// Run the program with two loops of second tools that try to get in whenever the VM is free (there is always one
/// waiting for the gate, so a call that lends it when it should not is caught). Returns what went wrong, and the
/// writes of the second tool that were acknowledged.
async fn drive(case: &Case, vm: &Arc<agentos_lua::AppVm>) -> (Vec<String>, BTreeSet<i64>) {
    let mut failures = vec![];
    let prog = {
        let vm = vm.clone();
        tokio::spawn(async move { tool(&vm, "prog", 20).await })
    };
    let over = Arc::new(AtomicBool::new(false));
    let side: Vec<_> = (0..2)
        .map(|lane| {
            let (vm, over, kind) = (vm.clone(), over.clone(), case.kind);
            tokio::spawn(async move {
                let mut done = vec![];
                for i in 0.. {
                    if over.load(Ordering::SeqCst) {
                        break;
                    }
                    let tools = side_tools(kind);
                    let name = tools[(i + lane) % tools.len()];
                    done.push((name, tool(&vm, name, 20).await));
                }
                done
            })
        })
        .collect();
    let outcome = prog.await.unwrap();
    over.store(true, Ordering::SeqCst);
    if std::env::var("CHAOS_ONLY").is_ok() {
        println!("-- the program returned: {outcome:?}");
    }
    match outcome {
        Ok(_) => {}
        // the script itself raised: that is its own business
        Err(e) if e.starts_with("the call failed") => {}
        Err(e) => failures.push(e),
    }
    let mut acknowledged = BTreeSet::new();
    for lane in side {
        for call in lane.await.unwrap() {
            match call {
                ("other", Ok(v)) if v["changes"] == 1 => {
                    acknowledged.insert(v["v"].as_i64().unwrap());
                }
                (_, Ok(_)) => {}
                (name, Err(e)) => failures.push(format!("{name}: {e}")),
            }
        }
    }
    (failures, acknowledged)
}

/// What the program started (tasks, delays) runs when the VM is free: wait for it to end, and take the report.
async fn settle(vm: &Arc<agentos_lua::AppVm>) -> Result<Value, String> {
    let settled = Instant::now();
    while settled.elapsed() < Duration::from_secs(10) {
        match tool(vm, "report", 10).await {
            Ok(report) if report["pending"] == 0 => return Ok(report),
            Ok(_) => tokio::time::sleep(Duration::from_millis(10)).await,
            Err(e) => return Err(format!("report: {e}")),
        }
    }
    Err("(e) the tasks the program started never ended".into())
}

/// The invariants, over what the case left behind.
async fn verify(
    case: &Case,
    vm: &Arc<agentos_lua::AppVm>,
    data: &Arc<agentos_sqlite::AppDb>,
    report: &Value,
    acknowledged: &BTreeSet<i64>,
) -> Vec<String> {
    let mut failures = vec![];
    // (e) nothing is wedged: the app answers, and so does the database (a window query does not go through the VM)
    match tool(vm, "ping", 5).await {
        Ok(v) if v == "pong" => {}
        other => failures.push(format!("(e) ping: {other:?}")),
    }
    let rows = match tokio::time::timeout(
        Duration::from_secs(3),
        data.query("select v from t".into(), vec![]),
    )
    .await
    {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => return vec![format!("the database failed: {e}")],
        Err(_) => return vec!["(e) the database's write gate is held by something that is gone".into()],
    };
    let have: BTreeSet<i64> = rows.iter().filter_map(|r| r["v"].as_i64()).collect();
    let numbers = |key: &str| -> BTreeSet<i64> {
        report[key].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
    };

    // the program's own checks: (a) writes in a read-only call, (b) another call inside a step that cannot suspend
    for v in report["V"].as_array().cloned().unwrap_or_default() {
        failures.push(v.as_str().unwrap_or("?").to_owned());
    }
    // (c) and (d): what was acknowledged is there, and what a failed transaction acknowledged is not
    for lost in numbers("A").iter().chain(acknowledged).filter(|v| !have.contains(v)) {
        failures.push(format!("(c/d) the write {lost} was acknowledged and is gone"));
    }
    for kept in numbers("X").iter().filter(|v| have.contains(v)) {
        failures
            .push(format!("(d) the write {kept} was made in a transaction that failed and is still there"));
    }
    // (a) in a read-only call nothing was written, by any means
    if case.kind.read_only() {
        if let Some(v) = have.iter().find(|v| **v < 1000) {
            failures.push(format!("(a) a readOnly call left the row {v} in the database"));
        }
        if report["state"] != 0 {
            failures.push(format!("(a) a readOnly call changed app.state: {}", report["state"]));
        }
        let kv = data.query("select count(*) as n from _kv".into(), vec![]).await.unwrap();
        if kv[0]["n"] != 0 {
            failures.push("(a) a readOnly call wrote to app.kv".into());
        }
        let effects = vm.take_effects();
        if !effects.is_empty() {
            failures.push(format!("(a) a readOnly call queued effects: {effects:?}"));
        }
    }
    failures
}

/// A wedge that blocks a worker thread also stops the timers that would report it: this ends the process, which the
/// test runner reports as a failure, instead of letting the run hang. Disarmed when dropped.
struct Watchdog(Arc<AtomicBool>);

impl Watchdog {
    fn arm(test: &'static str) -> Self {
        let limit = Duration::from_secs(env("CHAOS_WATCHDOG", 120));
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < limit {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            eprintln!("{test}: still running after {limit:?}: a call is wedged with a worker thread blocked");
            std::process::exit(101);
        });
        Self(done)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn generated_scripts_leave_every_invariant_intact() {
    let _watchdog = Watchdog::arm("generated_scripts_leave_every_invariant_intact");
    let cases: usize = env("CHAOS_CASES", 1500);
    let seed: u64 = env("CHAOS_SEED", 0x00A6_E575);
    let only: Option<usize> = std::env::var("CHAOS_ONLY").ok().and_then(|v| v.parse().ok());
    let url = canned_server().await;
    let started = Instant::now();
    let gate = Arc::new(Semaphore::new(env("CHAOS_PARALLEL", 8)));
    let mut runs = vec![];
    for index in (0..cases).filter(|i| only.is_none_or(|only| only == *i)) {
        let case = generate(seed, index, &url);
        if only.is_some() {
            println!("{}", case.source);
        }
        let gate = gate.clone();
        runs.push(tokio::spawn(async move {
            let _slot = gate.acquire().await.unwrap();
            let failures = run(&case).await;
            (case, failures)
        }));
    }
    let (mut total, mut steps, mut broken) = (0, 0, vec![]);
    for run in runs {
        let (case, failures) = run.await.expect("(f) the host did not panic");
        total += 1;
        steps += case.steps.len();
        if !failures.is_empty() {
            broken.push((case, failures));
        }
    }
    println!("chaos: {total} cases, {steps} steps, seed {seed:#x}, {:?}", started.elapsed());
    if let Some((case, _)) = broken.first() {
        let list: Vec<String> = broken
            .iter()
            .map(|(c, f)| format!("case {} ({:?}): {}", c.index, c.kind, f.join("; ")))
            .collect();
        panic!(
            "{} of {total} cases broke an invariant (rerun one with CHAOS_ONLY=<case> CHAOS_SEED={seed}):\n{}\n\nthe first program (case {}):\n{}",
            broken.len(),
            list.join("\n"),
            case.index,
            case.source.split("app.tool { name = \"prog\"").nth(1).unwrap_or(&case.source),
        );
    }
}

// ---- a reload or an unload in the middle of a program ------------------------------------------------------------

struct Quiet;

#[async_trait::async_trait]
impl Notifier for Quiet {
    async fn notify(&self, _: Notification) -> PortResult<()> {
        Ok(())
    }
}

/// What the app is told to be, for the runtime (which needs a manifest; the plain VM does not).
const MANIFEST: &str = "app.manifest { id = \"chaos\", name = \"Chaos\", icon = \"gear\" }\n";

/// A call through the runtime, as the host makes it; the error names what went wrong (a call that ended with an
/// error because the app was unloaded is not a failure: one that never ends is).
async fn runtime_tool(runtime: &Arc<LuaRuntime>, name: &'static str) -> Result<Value, String> {
    let runtime = runtime.clone();
    let call = tokio::spawn(async move {
        tokio::time::timeout(
            Duration::from_secs(20),
            runtime.call_tool(&id("chaos"), name, json!({}), agent_ctx(0), false),
        )
        .await
    });
    match call.await {
        Err(e) => Err(format!("(f) the call panicked or was aborted: {e}")),
        Ok(Err(_)) => Err(format!("(e) {name} never finished: the app is wedged")),
        Ok(Ok(Ok(v))) => Ok(v),
        Ok(Ok(Err(e))) => Err(format!("the call failed: {e}")),
    }
}

#[derive(Clone, Copy, Debug)]
enum Disturbance {
    Reload,
    Unload,
}

/// Run a case and reload or unload the app `after_ms` into it. The failures found.
async fn run_disturbed(case: &Case, disturbance: Disturbance, after_ms: u64) -> Vec<String> {
    let data = db();
    let runtime = Arc::new(LuaRuntime::new(Arc::new(Quiet), Arc::new(RecordingShell::default())));
    let source = format!("{MANIFEST}{}", case.source);
    let load = |runtime: Arc<LuaRuntime>, data: Arc<dyn AppData>, source: String| async move {
        runtime.load(&id("chaos"), package(&source, &[]), data).await
    };
    if let Err(e) = load(runtime.clone(), data.clone(), source.clone()).await {
        return vec![format!("the generated program did not load: {e}")];
    }
    let over = Arc::new(AtomicBool::new(false));
    let prog = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime_tool(&runtime, "prog").await })
    };
    let side: Vec<_> = (0..2)
        .map(|lane| {
            let (runtime, over) = (runtime.clone(), over.clone());
            tokio::spawn(async move {
                let mut failures = vec![];
                for i in 0.. {
                    if over.load(Ordering::SeqCst) {
                        break;
                    }
                    let name = ["bump", "meddle", "peek"][(i + lane) % 3];
                    // an error is the app going away; only a call that never ends or panics is a failure
                    if let Err(e) = runtime_tool(&runtime, name).await {
                        if e.starts_with("(e)") || e.starts_with("(f)") {
                            failures.push(format!("{name}: {e}"));
                        }
                    }
                }
                failures
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(after_ms)).await;
    let mut failures = vec![];
    match disturbance {
        Disturbance::Reload => {
            if let Err(e) = load(runtime.clone(), data.clone(), source).await {
                failures.push(format!("the reload failed: {e}"));
            }
        }
        Disturbance::Unload => {
            if let Err(e) = runtime.unload(&id("chaos")).await {
                failures.push(format!("the unload failed: {e}"));
            }
        }
    }
    // the program ends, one way or the other (an error because the app went away is fine; never ending is not)
    if let Err(e) = prog.await.unwrap() {
        if e.starts_with("(e)") || e.starts_with("(f)") {
            failures.push(e);
        }
    }
    over.store(true, Ordering::SeqCst);
    for lane in side {
        failures.extend(lane.await.unwrap());
    }
    // nothing is wedged: the database's write gate is free, and nothing the program left behind has it
    if tokio::time::timeout(Duration::from_secs(3), data.query("select v from t".into(), vec![]))
        .await
        .is_err()
    {
        failures.push("(e) the database's write gate is held by something that is gone".into());
    }
    match disturbance {
        Disturbance::Reload => match runtime_tool(&runtime, "ping").await {
            Ok(v) if v == "pong" => {}
            other => failures.push(format!("(e) the new version does not answer: {other:?}")),
        },
        Disturbance::Unload => {
            // an unloaded app is gone: asking it is an error, quickly
            if runtime_tool(&runtime, "ping").await.is_ok() {
                failures.push("an unloaded app still answers".into());
            }
        }
    }
    // a read-only program left nothing behind, in whichever version it ran
    if case.kind.read_only() {
        if let Ok(rows) = data.query("select v from t where v < 1000".into(), vec![]).await {
            if !rows.is_empty() {
                failures.push(format!("(a) a readOnly call left rows in the database: {rows:?}"));
            }
        }
    }
    drop(runtime);
    // no VM, task or transaction of the app is left holding the database, in either version (aborted tasks let go as
    // they are polled)
    let settled = Instant::now();
    while Arc::strong_count(&data) > 1 && settled.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if Arc::strong_count(&data) > 1 {
        failures.push("(e) a VM, task or transaction of the app still holds the database".into());
    }
    failures
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reload_or_unload_in_the_middle_of_a_script_leaves_nothing_wedged() {
    let _watchdog = Watchdog::arm("a_reload_or_unload_in_the_middle_of_a_script_leaves_nothing_wedged");
    let cases: usize = env("CHAOS_DISTURBED_CASES", 300);
    let seed: u64 = env("CHAOS_SEED", 0x00A6_E575) ^ 0x00D1_570B;
    let url = canned_server().await;
    let gate = Arc::new(Semaphore::new(env("CHAOS_PARALLEL", 8)));
    let mut runs = vec![];
    let only: Option<usize> = std::env::var("CHAOS_ONLY").ok().and_then(|v| v.parse().ok());
    for index in (0..cases).filter(|i| only.is_none_or(|only| only == *i)) {
        let case = generate(seed, index, &url);
        let mut rng = Rng(seed ^ u64::try_from(index).unwrap());
        let disturbance = *rng.pick(&[Disturbance::Reload, Disturbance::Unload]);
        let after_ms = *rng.pick(&[0, 0, 1, 2, 3, 5, 8, 13, 21]);
        let gate = gate.clone();
        runs.push(tokio::spawn(async move {
            let _slot = gate.acquire().await.unwrap();
            let failures = run_disturbed(&case, disturbance, after_ms).await;
            (case, disturbance, after_ms, failures)
        }));
    }
    let mut broken = vec![];
    for run in runs {
        let (case, disturbance, after_ms, failures) = run.await.expect("(f) the host did not panic");
        if !failures.is_empty() {
            broken.push(format!(
                "case {} ({:?}, {disturbance:?} after {after_ms} ms): {}",
                case.index,
                case.kind,
                failures.join("; ")
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "{} of {cases} cases broke an invariant:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// The generator is not vacuous: over the default cases every ingredient appears.
#[test]
fn the_generator_uses_every_ingredient() {
    let url = "http://127.0.0.1:1/";
    let programs: String = (0..300).map(|i| generate(0x00A6_E575, i, url).source).collect();
    for ingredient in [
        "task.wait(0)",
        "time.sleep(",
        "hyper.get(URL)",
        "hyper.get(OWN)",
        "app.db:one(",
        "app.db:exec(",
        "app.kv:get(",
        "app.kv:set(",
        "with_lock(function() end)",
        "with_lock(function() task.wait(0) end)",
        "attempt('state'",
        "attempt('notify'",
        "attempt('window'",
        "string.gsub('a'",
        "table.sort({ 3, 2, 1 }",
        "__index = function",
        "__add = function",
        "for _ in function()",
        "pcall(function()",
        "xpcall(function()",
        "coroutine.create(",
        "coroutine.wrap(",
        "coroutine.yield()",
        "task_start(task.spawn",
        "task_start(task.defer",
        "task.delay(0.003",
        "TX(function()",
        "error('boom')",
        "coroutine.resume(co) end end",
        "for _, t in ipairs(SH) do",
        "WR[#WR + 1] = w",
        "TX(function() coroutine.resume(co) end)",
    ] {
        assert!(programs.contains(ingredient), "no program uses `{ingredient}`");
    }
    for kind in [Kind::ReadOnly, Kind::Writable, Kind::Concurrent, Kind::OwnServer] {
        assert!((0..300).any(|i| generate(0x00A6_E575, i, url).kind == kind), "no program of kind {kind:?}");
    }
}

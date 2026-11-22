#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test harness
//! Every function of every module the VM offers, called with hostile numbers, strings and tables
//! (infinities, NaN, negatives, `2^53`, `2^63`, `1e300`, invalid UTF-8, mid-character text, cyclic and
//! absurdly keyed tables), and the methods of the handles they return. Each call must give a value or a Lua
//! error: a panic is a bug, because a panic reachable from script input takes the host down (the release
//! profile unwinds, but a panic inside a native callback still ends the call it was made in and a stack
//! overflow or an allocation failure aborts the process regardless).
//!
//! The harness records panics with a hook (mlua turns a callback's panic into a Lua error that `pcall`
//! returns as a string, and outside `pcall` resumes the unwind at the outermost call; the hook sees the
//! panic either way, so neither hides it) and a watchdog turns a hang
//! into a failure that names the call. Allocation failures and stack overflows abort the process: that
//! is a failure too, the test binary dies on the call it printed last (`HOSTILE_TRACE=1`).
//!
//! The sweep comes in two sizes. `no_module_function_panics_on_a_quick_hostile_sweep` is in the gate: every function with
//! every hostile value alone and in pairs from the reduced pool, no three-argument combinations and none of the texts
//! nested a hundred thousand deep ([`DEEP`]). The targeted tests below feed those depths to the parsers that recurse on
//! them (the YAML, TOML and XML decoders, glob patterns, Lua patterns and template macros); `lib:json` and `vnd:serde_json`
//! have their own depth tests. `no_module_function_panics_on_hostile_input` is the full sweep
//! (about two million calls, minutes), `#[ignore]`d as slow: `bun run slow` runs it.
//!
//! The sweep and the other tests here run on a 2 MiB thread (the stack of a default runtime worker): a
//! recursion that only fits a bigger stack is a stack overflow, which aborts the host, so it fails here.
//!
//! Not swept: `std:thread`, `std:net` and the network modules (`vnd:bollard`, `vnd:hyper`, `vnd:sqlx_*`, `vnd:testcontainers_modules`), which open
//! sockets or spawn workers, `vnd:portable_pty` (it starts processes), `vnd:keyring` (its `store()` is the credential store of whoever runs the tests), `std:workers` (a process-wide Ctrl-C signal) and `std:task` (its sleeps are covered by `std/task.rs` tests). The sweep
//! runs in a scratch working directory.

use mlua::{AnyUserData, Function, Lua, MultiValue, Table, Value};
use neoc_vm::lua::sandbox::make_engine;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// Every module name the registry holds (`modules::names`), so a module added later is swept too.
fn module_names() -> Vec<String> {
    let names: Vec<String> = neoc_vm::lua::modules::names().map(String::from).collect();
    assert!(names.len() >= 50, "found only {} modules", names.len());
    names
}

const SKIPPED_MODULES: &[&str] = &[
    "std:thread",
    "std:net",
    "std:task",
    "std:workers",
    "vnd:bollard",
    "vnd:hyper",
    "vnd:keyring",
    "vnd:portable_pty",
    "vnd:sqlx_mysql",
    "vnd:sqlx_postgres",
    "vnd:testcontainers_modules",
];

/// Functions that block on purpose (stdin, sleeps, servers) and are not an input-validation surface.
const SKIPPED_FUNCTIONS: &[&str] = &[
    "stdin",
    "read_line",
    "sleep",
    "park",
    "wait",
    "serve",
    "serve_static",
];

/// The hostile values, as Lua source (built in the VM so tables and functions are real).
const POOL: &str = r#"
    -- an options table with one hostile number under every option name the modules read
    local function opts(n)
      local t = {}
      for _, key in ipairs { "context", "width", "max_size", "max_output_size", "level", "limit", "size", "length",
        "count", "depth", "indent", "max_len", "ttl", "capacity", "timeout", "step_by", "max_entries", "mtime",
        "mode", "radius", "offset", "max_depth" } do t[key] = n end
      return t
    end
    local cyc = {} cyc[1] = cyc cyc.self = cyc
    local deep = {} for _ = 1, 50000 do deep = { deep } end
    return {
        true, false,
        0, -0, 1, -1, 255, 256, 65536, 2^31, 2^32, -2^31, 2^53, 2^53 + 2, -2^53, 2^63, -2^63, 2^64, 1e300, -1e300,
        1/0, -1/0, 0/0, 0.5, -0.5, 1e-300,
        "", "a\0b", "\255\254\253", "\195", "Ü", "ÜÜÜÜÜÜÜÜÜÜ", "&ÜÜÜÜÜÜÜÜ;", "%ÜÜ", "a=b", "🦀",
        ("é"):rep(50), ("&"):rep(100), ("\240\159"):rep(20),
        {}, { 1, 2, 3 }, { a = 1 }, { [2^53] = 1 }, { [1e300] = "x" }, { [-1] = 1, [0] = 2 },
        { 1, nil, 3 }, { "a", "b" }, { {}, {} }, cyc, deep,
        function() end,
        print,
        opts(1e300), opts(2^63), opts(2^53), opts(-1), opts(0/0), opts(1/0), opts(-1/0),
        -- text that nests: parsers, pattern compilers and template engines recurse on these
        -- (past the recursion limit of every parser, small enough that quadratic ones like `diff` stay quick)
        ("["):rep(3000), ("{"):rep(3000), ("("):rep(3000), ("<a>"):rep(2000), ("(?:"):rep(2000),
        ("{a,"):rep(130) .. "a" .. ("}"):rep(130), ("a?"):rep(2000),
        ("{% for i in [1] %}"):rep(500), ("{% if true %}"):rep(500), "{{ 1" .. (" + 1"):rep(3000) .. " }}",
        -- Tera 1.20 panics on these: `i64::MIN % -1`, and a macro call as an operand of arithmetic or of `<`
        -- (`lib:template` refuses the second and catches the first)
        "{{ (0 - 9223372036854775807 - 1) % -1 }}",
        "{% macro m() %}1{% endmacro m %}{{ 1 + self::m() }}{% if self::m() > 0 %}a{% endif %}",
        -- Luau buffers: alone, empty, and inside a table (a serializer must refuse one, never panic on it)
        buffer.create(8), buffer.create(0), { b = buffer.fromstring("ab") },
    }
"#;

/// Text and tables nested a hundred thousand deep, for every bracket kind a parser, a pattern compiler or a
/// template engine might recurse on (and the keys of nested tables). [`POOL`]'s nesting is only thousands deep, which
/// the recursion limits of the parsers in use survive: a glob parser that recursed once per `{` still fit
/// there and overflowed here. Swept with one and two arguments only (every function would be quadratic in these).
const DEEP: &str = r#"
    local N = 100000
    local deep, deep_keys = {}, {}
    for _ = 1, N do deep = { deep } deep_keys = { a = deep_keys } end
    return {
        ("["):rep(N), ("{"):rep(N), ("("):rep(N), ("<"):rep(N), ("<a>"):rep(N), ("[["):rep(N), ("{{"):rep(N),
        ("(?:"):rep(N), ("(?"):rep(N), ("[:"):rep(N), ("{a,"):rep(N), ("a{b,"):rep(N), ("a{1,"):rep(N),
        ("${"):rep(N), ("<!--"):rep(N), ("- "):rep(N), ("a = ["):rep(N), ('{"a":'):rep(N), ("[1,"):rep(N),
        ("{%"):rep(N), ("{{ ("):rep(N), ("%("):rep(N), ("\\"):rep(N), ("*"):rep(N), ("&"):rep(N),
        ("["):rep(N) .. ("]"):rep(N), ("{"):rep(N) .. ("}"):rep(N), ("("):rep(N) .. (")"):rep(N),
        ("<a>"):rep(N) .. ("</a>"):rep(N), ("{a,"):rep(N) .. ("}"):rep(N), ("a{b,"):rep(N) .. ("}"):rep(N),
        deep, deep_keys,
    }
"#;

/// A reduced pool (indices into [`POOL`]) for three-argument combinations and for the methods of handles:
/// `1`, `-1`, `2^63`, `-2^63`, `1e300`, NaN; `""`, `"a\0b"`, `"&ÜÜÜÜÜÜÜÜ;"`, invalid UTF-8 cut mid-character;
/// `{}`, `{ 1, 2, 3 }`, `{ a = 1 }`; and the options tables with `1e300`, `2^63` and NaN under every name. (The
/// buffers at the end of [`POOL`] are not in it: every function and method gets each of them alone, and adding one
/// here would grow the pairs, and the handles they fill, by an eighth.)
const SMALL_POOL: &[usize] = &[4, 5, 15, 16, 18, 22, 26, 27, 32, 38, 39, 40, 41, 52, 53, 56];

static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How much of the pool a sweep uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Size {
    /// Every combination: single values, all pairs of the pool, all triples of the reduced pool, the deep texts.
    Full,
    /// The gate's size: single values and the pairs of the reduced pool.
    Quick,
}

struct Harness {
    size: Size,
    lua: Lua,
    pool: Vec<Value>,
    deep: Vec<Value>,
    panics: Arc<Mutex<Vec<String>>>,
    current: Arc<Mutex<String>>,
    calls: usize,
    rt: tokio::runtime::Runtime,
    trace: bool,
}

impl Harness {
    fn call(&mut self, label: &str, f: &Function, args: &[Value]) -> Option<Vec<Value>> {
        self.calls += 1;
        CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.trace {
            eprintln!("{label} #{}", args.len());
        }
        *self.current.lock().unwrap() = format!(
            "{label}({})",
            args.iter().map(describe).collect::<Vec<_>>().join(", ")
        );
        let before = self.panics.lock().unwrap().len();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.rt.block_on(async {
                f.call_async::<MultiValue>(MultiValue::from_iter(args.iter().cloned()))
                    .await
            })
        }));
        if outcome.is_ok() {
            // a panic inside Tera that `lib:template` caught and returned as an error is not a failure: the
            // call completed. (One that unwound through the call still is.)
            let mut recorded = self.panics.lock().unwrap();
            let mut at = before;
            while at < recorded.len() {
                if recorded[at].contains("/tera-") {
                    recorded.remove(at);
                } else {
                    at += 1;
                }
            }
        }
        if outcome.is_err() && self.panics.lock().unwrap().len() == before {
            self.panics.lock().unwrap().push(format!(
                "{}: unwound without a recorded message",
                self.current.lock().unwrap()
            ));
        }
        match outcome {
            Ok(Ok(values)) => Some(values.into_iter().collect()),
            _ => None,
        }
    }

    fn sweep_function(&mut self, label: &str, f: &Function, prefix: &[Value]) -> Vec<AnyUserData> {
        let mut handles = Vec::new();
        let keep = |h: &mut Vec<AnyUserData>, out: Option<Vec<Value>>| {
            for v in out.into_iter().flatten() {
                if let Value::UserData(ud) = v
                    && h.len() < 4
                {
                    h.push(ud);
                }
            }
        };
        let pool = self.pool.clone();
        let with = |rest: &[Value]| -> Vec<Value> {
            prefix.iter().cloned().chain(rest.iter().cloned()).collect()
        };
        let out = self.call(label, f, &with(&[]));
        keep(&mut handles, out);
        for a in &pool {
            let out = self.call(label, f, &with(std::slice::from_ref(a)));
            keep(&mut handles, out);
        }
        // A handle accumulates what earlier calls did to it, so its methods get the reduced pool throughout
        // (a million-element array would make every later call a slow one).
        let wide: Vec<&Value> = if prefix.is_empty() && self.size == Size::Full {
            pool.iter().collect()
        } else {
            SMALL_POOL.iter().map(|&i| &pool[i]).collect()
        };
        for a in &wide {
            for b in &wide {
                let out = self.call(label, f, &with(&[(*a).clone(), (*b).clone()]));
                keep(&mut handles, out);
            }
        }
        // the deep nesting goes in as an argument, alone and next to a plain one (a pattern to
        // compile or a text to parse, whichever way round), for module functions: the methods of a handle take the same text
        // through the constructors swept here, and sweeping them too would only repeat the cost
        if prefix.is_empty() && self.size == Size::Full {
            let plain = self.lua.create_string("x").map(Value::String).unwrap();
            for d in self.deep.clone() {
                for args in [
                    vec![d.clone()],
                    vec![d.clone(), plain.clone()],
                    vec![plain.clone(), d.clone()],
                ] {
                    let out = self.call(label, f, &args);
                    keep(&mut handles, out);
                }
            }
        }
        if prefix.is_empty() && self.size == Size::Full {
            for &i in SMALL_POOL {
                for &j in SMALL_POOL {
                    for &k in SMALL_POOL {
                        let args = with(&[pool[i].clone(), pool[j].clone(), pool[k].clone()]);
                        let out = self.call(label, f, &args);
                        keep(&mut handles, out);
                    }
                }
            }
        }
        handles
    }

    fn sweep_table(&mut self, label: &str, t: &Table, depth: usize) {
        let mut entries: Vec<(String, Value)> =
            t.pairs::<String, Value>().filter_map(Result::ok).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in entries {
            let path = format!("{label}.{name}");
            match value {
                Value::Function(f) if !SKIPPED_FUNCTIONS.contains(&name.as_str()) => {
                    let handles = self.sweep_function(&path, &f, &[]);
                    for (n, ud) in handles.into_iter().enumerate() {
                        self.sweep_handle(&format!("{path}()#{n}"), &ud);
                    }
                }
                Value::Table(inner) if depth < 2 => self.sweep_table(&path, &inner, depth + 1),
                _ => {}
            }
        }
    }

    /// The methods of a handle: found through the `__index` table mlua builds for a userdata type.
    fn sweep_handle(&mut self, label: &str, ud: &AnyUserData) {
        let Ok(mt) = ud.metatable() else { return };
        let Ok(Value::Table(methods)) = mt.get::<Value>("__index") else {
            return;
        };
        let names: Vec<String> = methods
            .pairs::<String, Value>()
            .filter_map(Result::ok)
            .map(|(k, _)| k)
            .collect();
        for name in names {
            if SKIPPED_FUNCTIONS.contains(&name.as_str()) {
                continue;
            }
            let Ok(Value::Function(f)) = methods.get::<Value>(name.as_str()) else {
                continue;
            };
            // a method takes the handle itself first
            self.sweep_function(
                &format!("{label}:{name}"),
                &f,
                &[Value::UserData(ud.clone())],
            );
        }
    }
}

fn describe(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let b = s.as_bytes();
            format!("{:?}", String::from_utf8_lossy(&b[..b.len().min(24)]))
        }
        Value::Table(t) => format!("table(len {})", t.raw_len()),
        Value::UserData(_) => "userdata".into(),
        other => format!("{other:?}"),
    }
}

fn sweep(size: Size) -> (usize, Vec<String>) {
    let scratch = tempfile::tempdir().unwrap();
    std::env::set_current_dir(scratch.path()).unwrap();

    let panics = Arc::new(Mutex::new(Vec::<String>::new()));
    let current = Arc::new(Mutex::new(String::new()));
    let hook_panics = panics.clone();
    let hook_current = current.clone();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(ToString::to_string).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                info.payload()
                    .downcast_ref::<&str>()
                    .map(ToString::to_string)
            })
            .unwrap_or_default();
        let line = format!("{at}: {msg}  <= {}", hook_current.lock().unwrap());
        hook_panics.lock().unwrap().push(line);
    }));

    // a call that never returns is named after ten quiet seconds
    {
        let current = current.clone();
        std::thread::spawn(move || {
            let mut last = usize::MAX;
            loop {
                std::thread::sleep(Duration::from_secs(10));
                let now = CALLS.load(std::sync::atomic::Ordering::Relaxed);
                if now == last {
                    eprintln!("STUCK for 10s in {}", current.lock().unwrap());
                }
                last = now;
            }
        });
    }
    let only = std::env::var("HOSTILE_ONLY").ok();
    let mut total = 0;
    for name in module_names()
        .iter()
        .map(String::as_str)
        .filter(|n| !SKIPPED_MODULES.contains(n))
    {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let lua = make_engine().unwrap();
        let pool: Table = lua.load(POOL).eval().unwrap();
        let pool: Vec<Value> = pool
            .sequence_values::<Value>()
            .map(Result::unwrap)
            .collect();
        let deep: Table = if size == Size::Quick || std::env::var_os("HOSTILE_NO_DEEP").is_some() {
            lua.create_table().unwrap()
        } else {
            lua.load(DEEP).eval().unwrap()
        };
        let deep: Vec<Value> = deep
            .sequence_values::<Value>()
            .map(Result::unwrap)
            .collect();
        let module: Table = lua
            .load(format!("return require({name:?})"))
            .eval()
            .unwrap();
        let mut h = Harness {
            size,
            lua,
            pool,
            deep,
            panics: panics.clone(),
            current: current.clone(),
            calls: 0,
            trace: std::env::var_os("HOSTILE_TRACE").is_some(),
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
        };
        let before = panics.lock().unwrap().len();
        h.sweep_table(name, &module, 0);
        total += h.calls;
        eprintln!(
            "{name}: {} calls, {} panics",
            h.calls,
            panics.lock().unwrap().len() - before
        );
        let _ = &h.lua;
    }
    let _ = std::panic::take_hook();
    let found = panics.lock().unwrap().clone();
    (total, found)
}

/// Runs a sweep on a 2 MiB thread and fails on any panic it recorded (or a hang).
fn assert_sweep_is_quiet(size: Size, limit: Duration) {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let _ = tx.send(sweep(size));
        })
        .unwrap();
    match rx.recv_timeout(limit) {
        Ok((calls, panics)) => {
            eprintln!("{calls} hostile calls");
            let mut by_site: std::collections::BTreeMap<String, String> =
                std::collections::BTreeMap::new();
            for p in panics {
                let site = p.split("  <= ").next().unwrap_or_default().to_owned();
                by_site.entry(site).or_insert(p);
            }
            let unique: Vec<String> = by_site.into_values().collect();
            assert!(
                unique.is_empty(),
                "{} distinct panics:\n{}",
                unique.len(),
                unique.join("\n")
            );
        }
        Err(e) => panic!("the sweep hung or died: {e}"),
    }
}

/// The gate's sweep: see the module comment. The sweep installs a process-wide panic hook, so run it by itself
/// (`cargo nextest`, one process per test, does; `cargo test` runs the other tests of this file beside it, and a
/// failure of one of those shows up here too).
#[test]
fn no_module_function_panics_on_a_quick_hostile_sweep() {
    assert_sweep_is_quiet(Size::Quick, Duration::from_mins(5));
}

/// The full sweep, about two million calls: minutes. `bun run slow` runs it.
#[test]
#[ignore = "slow: about two million hostile calls; run with `bun run slow` (scripts/check-slow.sh)"]
fn no_module_function_panics_on_hostile_input() {
    assert_sweep_is_quiet(Size::Full, Duration::from_mins(15));
}

/// Run `f` on a 2 MiB thread, the stack of a default runtime worker; a panic or stack overflow fails the test.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .expect("the call panicked")
}

/// Luau's pattern matcher recurses in C like PUC Lua 5.1's, but counts its depth and gives up past
/// `LUAI_MAXCCALLS` (200) with "pattern too complex". These patterns would take tens of thousands of C frames:
/// through `find`, `match`, `gmatch` and `gsub`, called as functions and as string methods, each must be a
/// Lua error on a 2 MiB stack, not a stack overflow (an abort of the host). So no depth guard of our own is
/// needed on top of the engine's (the `patterns.rs` guard of the Lua 5.1 build has no counterpart here).
///
/// Each item is followed by a literal (`a?b`) and the subject alternates `ab`, so the matcher goes deep at
/// once instead of backtracking (`a+a+a+...` over `aaaa...` is a polynomial search: slow, not a crash, and
/// what the engine does with any pattern; there is no time budget).
#[test]
fn a_pattern_with_tens_of_thousands_of_items_is_an_error_not_a_stack_overflow() {
    let results = on_small_stack(|| {
        let lua = neoc_vm::lua::sandbox::make_engine().unwrap();
        let out: Table = lua
            .load(
                r#"
                local out = {}
                local subject = ("ab"):rep(20000)
                -- the repetition items hit the depth limit; nested captures hit the capture limit first
                for _, item in ipairs { "a?b", "a*b", "a+b", "a-b", "(a)b", "(", "(a?b)" } do
                  local pattern = item:rep(20000)
                  local calls = {
                    function() return string.find(subject, pattern) end,
                    function() return string.match(subject, pattern) end,
                    function() return string.gmatch(subject, pattern)() end,
                    function() return string.gsub(subject, pattern, "") end,
                    function() return subject:find(pattern) end,
                    function() return subject:gsub(pattern, "x") end,
                  }
                  for i, call in ipairs(calls) do
                    local ok, err = pcall(call)
                    out[#out + 1] = item .. "|" .. i .. "|" .. tostring(ok) .. "|" .. tostring(err)
                  end
                end
                return out
                "#,
            )
            .eval()
            .unwrap();
        out.sequence_values::<String>()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    });
    assert_eq!(results.len(), 42);
    for r in &results {
        let mut parts = r.splitn(4, '|');
        let (item, _, ok, err) = (
            parts.next().unwrap(),
            parts.next(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        assert_eq!(ok, "false", "{r}");
        let want = if item.starts_with('(') {
            "too many captures"
        } else {
            "pattern too complex"
        };
        assert!(err.contains(want), "{r}");
    }
}

/// Patterns inside the engine's depth limit still match (on a 2 MiB thread, nested inside protected calls), and
/// what is not a pattern is untouched.
#[test]
fn patterns_within_the_depth_limit_still_match_and_plain_searches_are_not_interpreted() {
    on_small_stack(|| {
        let lua = neoc_vm::lua::sandbox::make_engine().unwrap();
        let ok: bool = lua
            .load(
                r##"
                local items = 100
                local subject = ("a"):rep(items)
                local function nested(n, f) if n == 0 then return f() end return select(2, pcall(nested, n - 1, f)) end
                local within = nested(100, function()
                  return subject:find(("a?"):rep(items)) == 1 and select(2, subject:find(("a*"):rep(items))) == items
                end)
                local grouped = select("#", subject:match(("(a)"):rep(32))) == 32
                -- a plain find reads no pattern, whatever it holds
                local plain = (("x*"):rep(600)):find(("x*"):rep(600), 1, true) == 1
                local over = pcall(string.find, ("ab"):rep(1000), ("a?b"):rep(1000))
                local gsub = (("ab"):rep(100)):gsub("(a)(b)", "%2%1") == ("ba"):rep(100)
                return within and grouped and plain and not over and gsub
                "##,
            )
            .eval()
            .unwrap();
        assert!(ok);
    });
}

/// globset parses `{...}` recursively: a pattern of nested alternations overflowed even a 16 MiB stack, an
/// abort of the host. Every place the VM compiles a glob goes through one bounded helper: `lib:glob` (every
/// constructor and `ignore`), `lib:archive`'s `include` and `lib:http`'s `serve_static`.
#[test]
fn deeply_nested_glob_patterns_are_an_error_in_every_entry_point() {
    let lines = on_small_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let lua = neoc_vm::lua::sandbox::make_engine().unwrap();
        let out: Table = rt
            .block_on(
                lua.load(
                    r#"
                    local glob, archive, http = require("lib:glob"), require("lib:archive"), require("lib:http")
                    local out = {}
                    for _, pattern in ipairs {
                      ("{"):rep(100000) .. ("}"):rep(100000), ("a{b,"):rep(100000), ("{"):rep(100000),
                      ("{a,"):rep(100) .. "a" .. ("}"):rep(100),
                    } do
                      local calls = {
                        function() return glob.is_match(pattern, "a") end,
                        function() return glob.Glob.new(pattern) end,
                        function() return glob.GlobSet.new({ pattern }) end,
                        function() return glob.scan(pattern, { cwd = "." }) end,
                        function() return glob.scan("*", { cwd = ".", ignore = { pattern } }) end,
                        function() return archive.tar_extract("not a tar", "out", { include = { pattern } }) end,
                        function() return archive.zip_extract("not a zip", "out", { include = { pattern } }) end,
                        function() return http.serve_static(pattern, { port = 0, host = "127.0.0.1" }) end,
                      }
                      for i, call in ipairs(calls) do
                        local ok, value, err = pcall(call)
                        -- a Lua error, or the (nil, message) pair: never a crash, never a handle
                        local message = (not ok and tostring(value)) or (value == nil and tostring(err)) or "ACCEPTED"
                        out[#out + 1] = i .. ":" .. message
                      end
                    end
                    return out
                    "#,
                )
                .eval_async::<Table>(),
            )
            .unwrap();
        out.sequence_values::<String>()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    });
    assert_eq!(lines.len(), 32);
    assert!(
        lines.iter().all(|l| l.contains("glob pattern")),
        "{lines:#?}"
    );
}

/// Render `source` with `lib:template` on a 2 MiB thread and return its error; a stack overflow aborts the
/// test binary, so reaching the assertion is the proof.
fn template_error_on_small_stack(source: String) -> String {
    on_small_stack(move || {
        let lua = make_engine().unwrap();
        let (out, err): (Option<String>, Option<String>) = lua
            .load(r#"return require("lib:template").render(..., {})"#)
            .call(source)
            .unwrap();
        assert!(out.is_none(), "rendered {out:?}");
        err.unwrap()
    })
}

/// `lib:template` bounds what it renders. A chain of twenty thousand macros, none recursive, once overflowed the
/// guard's own walk (it recursed once per macro), and a cycle check that restarted at every macro took minutes.
#[test]
fn a_chain_of_twenty_thousand_macros_is_refused_without_overflowing() {
    let mut src = String::new();
    for i in 0..20_000 {
        src.push_str(&format!(
            "{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}",
            i + 1
        ));
    }
    src.push_str("{% macro m20000() %}end{% endmacro m20000 %}{{ self::m0() }}");
    let started = std::time::Instant::now();
    let err = template_error_on_small_stack(src);
    assert!(err.contains("too complex"), "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "took {:?}",
        started.elapsed()
    );
}

/// Tera evaluates the arguments of a macro call before it enters the macro, but a call inside an argument is
/// still nested in the call around it: eight levels of arguments in each of 23 macros went through the guard as
/// siblings and aborted a debug build on 2 MiB.
#[test]
fn macro_calls_nested_in_arguments_are_counted_as_nested() {
    let nested = |callee: usize| {
        format!(
            "{}1{}",
            format!("self::m{callee}(x=").repeat(8),
            ")".repeat(8)
        )
    };
    let mut src = String::new();
    for i in 0..23 {
        src.push_str(&format!(
            "{{% macro m{i}(x) %}}{{{{ {} }}}}{{% endmacro m{i} %}}",
            nested(i + 1)
        ));
    }
    src.push_str("{% macro m23(x) %}{{ nope }}{% endmacro m23 %}{{ self::m0(x=1) }}");
    assert!(template_error_on_small_stack(src).contains("too complex"));
}

/// A macro that never stops calling itself, directly, through blocks, through another macro and through
/// argument nesting, ends in an ordinary render error on 2 MiB.
#[test]
fn unbounded_macro_recursion_is_a_render_error_on_a_small_stack() {
    for src in [
        "{% macro f(n) %}{{ self::f(n=n) }}{% endmacro f %}{{ self::f(n=1) }}",
        "{% macro f(n) %}{% for i in [1] %}{% if true %}{% for j in [1] %}{{ self::f(n=n) }}{% endfor %}{% endif %}{% endfor %}{% endmacro f %}{{ self::f(n=1) }}",
        "{% macro a() %}{{ self::b() }}{% endmacro a %}{% macro b() %}{{ self::a() }}{% endmacro b %}{{ self::a() }}",
        "{% macro f(x) %}{{ self::f(x=self::f(x=self::f(x=1))) }}{% endmacro f %}{{ self::f(x=1) }}",
    ] {
        let err = template_error_on_small_stack(src.to_owned());
        assert!(
            err.contains("levels deep") || err.contains("too complex"),
            "{src:.50}: {err}"
        );
    }
}

/// One large call cycle: a ring of sixty thousand macros. The cost of an edge must not grow with the size of the
/// cycle (a membership scan per edge took 12.6 s in debug); the recursion is bounded at run time, so the render
/// is an ordinary error.
#[test]
#[ignore = "slow: a wall-clock bound over a 60 000-macro ring, seconds in a debug build and flaky under load; run with `bun run slow` (scripts/check-slow.sh)"]
fn a_ring_of_sixty_thousand_macros_is_planned_in_linear_time() {
    let n = 60_000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!(
            "{{% macro m{i}() %}}{{{{ self::m{}() }}}}{{% endmacro m{i} %}}",
            (i + 1) % n
        ));
    }
    src.push_str("{{ self::m0() }}");
    let started = std::time::Instant::now();
    let err = template_error_on_small_stack(src);
    assert!(err.contains("levels deep"), "{err:.200}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
}

/// Parse each text (a Lua chunk returning a list of strings) with `require(module).decode_string` on a 2 MiB thread
/// and return, per text, the error it gave or `ACCEPTED`. A stack overflow aborts the test binary, so reaching the
/// assertions is the proof that the parser stays on the stack.
fn decode_deep_texts(module: &'static str, texts: &'static str) -> Vec<String> {
    on_small_stack(move || {
        let lua = make_engine().unwrap();
        let out: Table = lua
            .load(
                r#"
                local module, texts = ...
                local decode_string = require(module).decode_string
                local out = {}
                for _, text in ipairs(texts) do
                  local ok, value, err = pcall(decode_string, text)
                  out[#out + 1] = (not ok and tostring(value)) or (value == nil and tostring(err)) or "ACCEPTED"
                end
                return out
                "#,
            )
            .call((module, lua.load(texts).eval::<Table>().unwrap()))
            .unwrap();
        out.sequence_values::<String>()
            .map(Result::unwrap)
            .collect()
    })
}

/// The quick sweep leaves out [`DEEP`], the texts nested a hundred thousand deep; these are the ones a data parser
/// would recurse on, fed straight to each parser. serde_yaml scans the whole text without recursing and cuts the
/// descent at its recursion limit (128), so ten thousand levels prove it as well as a hundred thousand would, in a
/// fraction of the time. Its scan of nested flow mappings (`{a: {a: ...`) is quadratic in the depth (half a second at
/// ten thousand levels in a debug build, a minute and a half at a hundred thousand), so that text is fed shallower.
#[test]
fn deeply_nested_yaml_is_an_error_not_a_stack_overflow() {
    let lines = decode_deep_texts(
        "vnd:serde_yaml",
        r#"local N = 10000 return {
            ("["):rep(N) .. ("]"):rep(N), ("["):rep(N), ("- "):rep(N) .. "1", ("{a: "):rep(2000) .. ("}"):rep(2000),
        }"#,
    );
    assert_eq!(lines.len(), 4);
    assert!(
        lines.iter().all(|l| l.contains("recursion limit exceeded")),
        "{lines:#?}"
    );
}

/// toml's parser stops nesting arrays and inline tables at its recursion limit (80).
#[test]
fn deeply_nested_toml_is_an_error_not_a_stack_overflow() {
    let lines = decode_deep_texts(
        "vnd:toml",
        r#"local N = 100000 return {
            "a = " .. ("["):rep(N) .. ("]"):rep(N), "a = " .. ("{b = "):rep(N) .. "1" .. ("}"):rep(N),
            "a = " .. ("[1,"):rep(N),
        }"#,
    );
    assert_eq!(lines.len(), 3);
    assert!(
        lines.iter().all(|l| l.contains("max recursion depth met")),
        "{lines:#?}"
    );
}

/// quick-xml's deserializer stops at its recursion limit (128) before the value reaches `serde_json` or Lua.
#[test]
fn deeply_nested_xml_is_an_error_not_a_stack_overflow() {
    let lines = decode_deep_texts(
        "vnd:quick_xml",
        r#"local N = 100000 return {
            ("<a>"):rep(N) .. ("</a>"):rep(N), ("<a>"):rep(N), ("<a><b>"):rep(N) .. ("</b></a>"):rep(N),
        }"#,
    );
    assert_eq!(lines.len(), 3);
    assert!(
        lines
            .iter()
            .all(|l| l.contains("recursion limit of 128 exceeded")),
        "{lines:#?}"
    );
}

/// `std:fs` against trees built to hang or overflow a naive implementation, on a 2 MiB thread: a directory nest
/// as deep as the operating system's path limit allows, symlink loops (to a parent, to itself, in a cycle of two),
/// and counts no tree can satisfy. Each call must finish with a value or an `(nil, err)` pair, and nothing may be
/// followed out of the tree (a victim directory next to it must survive the recursive remove).
#[test]
fn std_fs_tree_operations_survive_deep_nests_and_symlink_loops() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_owned();
    let lines = on_small_stack(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let lua = make_engine().unwrap();
        lua.globals().set("root", root).unwrap();
        let out: Table = rt
            .block_on(
                lua.load(
                    r#"
                    local fs = require("std:fs")
                    local out = {}
                    local function note(label, value, err)
                      out[#out + 1] = label .. ":" .. (value ~= nil and "ok" or ("err " .. tostring(fs.error_kind(err))))
                    end
                    assert(fs.create_dir(root .. "/victim") and fs.write(root .. "/victim/precious", "p"))

                    -- a nest as deep as the path limit allows (the create fails at the limit: stop there)
                    local deep = root .. "/deep"
                    assert(fs.create_dir(deep))
                    local levels = 0
                    for _ = 1, 4000 do
                      local nested = deep .. "/d"
                      if not fs.create_dir(nested) then break end
                      deep, levels = nested, levels + 1
                    end
                    assert(levels > 20, "the nest is too shallow to mean anything: " .. levels)
                    -- loops, near the top of the nest (a link at its bottom would exceed the path limit)
                    assert(fs.symlink(root, root .. "/deep/up"))
                    assert(fs.symlink(root .. "/victim", root .. "/deep/to_victim"))
                    assert(fs.symlink(".", root .. "/deep/self"))
                    fs.symlink("two", root .. "/one")
                    fs.symlink("one", root .. "/two")

                    note("walk", fs.walk(root .. "/deep"))
                    note("walk depth", fs.walk(root .. "/deep", { max_depth = 2 }))
                    note("walk huge depth", fs.walk(root .. "/deep", { max_depth = 2^63 }))
                    note("walk hostile depth", fs.walk(root .. "/deep", { max_depth = 0/0 }))
                    note("copy_dir", fs.copy_dir(root .. "/deep", root .. "/c"))
                    note("copy_dir again", fs.copy_dir(root .. "/deep", root .. "/c"))
                    note("copy_dir into itself", fs.copy_dir(root .. "/deep", root .. "/deep/d/inside"))
                    note("copy over loop", fs.copy(root .. "/one", root .. "/x"))
                    note("walk loop link", fs.walk(root .. "/one"))
                    note("metadata loop link", fs.metadata(root .. "/one"))
                    note("remove copy", fs.remove_dir_all(root .. "/c"))
                    note("remove deep", fs.remove_dir_all(root .. "/deep"))
                    note("remove loop link", fs.remove_dir_all(root .. "/one"))
                    note("remove loop link as file", fs.remove_file(root .. "/one"))
                    note("victim kept", fs.read(root .. "/victim/precious"))
                    note("deep gone", fs.exists(root .. "/deep") == false and true or nil, "x")
                    for _, count in ipairs { 2^53, 2^63, 1e300, -1, 0/0, 1/0 } do
                      note("hostile mode", fs.set_permissions(root .. "/victim", { mode = count }))
                    end
                    return out
                    "#,
                )
                .eval_async::<Table>(),
            )
            .unwrap();
        out.sequence_values::<String>()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    });
    let seen = lines.join("\n");
    for expected in [
        "walk:ok",
        "walk depth:ok",
        "walk huge depth:ok",
        "walk hostile depth:err invalid_input",
        "copy_dir:ok",
        "copy_dir again:err already_exists",
        "copy_dir into itself:err invalid_input",
        // the cycle of two (`one` -> `two` -> `one`): resolving it fails at once, with the kind of a symlink loop
        "copy over loop:err other",
        "walk loop link:err other",
        "metadata loop link:err other",
        "remove copy:ok",
        "remove deep:ok",
        "remove loop link:err not_a_directory",
        "remove loop link as file:ok",
        "victim kept:ok",
    ] {
        assert!(seen.contains(expected), "missing `{expected}` in:\n{seen}");
    }
    assert_eq!(
        seen.matches("hostile mode:err invalid_input").count(),
        6,
        "{seen}"
    );
}

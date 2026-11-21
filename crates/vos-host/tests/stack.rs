#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // integration-test helpers
//! The host's threads have the stack that recursion through an app's host callbacks needs.
//!
//! Luau stops a recursion that goes through a C function or a metamethod at 200 C levels with an ordinary error, but
//! the 200 levels, with a Rust host function in each, must fit the thread's stack: in a debug build a level through
//! `Mutex:with_lock` needs up to 4.5 MiB in all (neoc-vm's `lifecycle.rs` has the table), more than tokio's default
//! 2 MiB (the main thread's stack is whatever the operating system decides, which is why the top-level future does not
//! run there). The host builds its runtime with
//! `vos_host::runtime::build` and runs its top-level future on a worker, so every thread that polls a VM has
//! `VM_THREAD_STACK`.
//!
//! The child process (this binary again, `stack_child`) builds that runtime exactly as `main` does, loads an app that
//! recurses until Luau refuses and checks that the host answers an ordinary error. Tests build in debug, the build that
//! needs the most stack. Which cases would notice a runtime on tokio's default 2 MiB: only `with_lock` (measured by
//! putting 2 MiB in `runtime.rs`: that case aborts, the gsub, tostring, index and notify cases still pass; neoc-vm's
//! table puts their levels well under 1 MiB). The others hold the 200-level error on the host's runtime, through the
//! engine's own machinery and, for `notify`, through a host function the adapter registers (`app.notify` reads a field
//! of its argument through a metatable, whose `__index` calls `app.notify` again). The `run` tests below hold the
//! wrapper's error and panic arms. What no test holds is `main.rs` calling the two functions (two lines).

use serde_json::json;
use std::{
    collections::BTreeMap,
    process::Command,
    sync::{Arc, Mutex},
};
use vos_application::ports::{AppPackage, CallContext};
use vos_domain::{Actor, AppId};
use vos_lua::{AppVm, VmConfig};
use vos_sqlite::AppDb;

const CASE: &str = "STACK_CASE";

/// An app with one tool per recursion: through `Mutex:with_lock`, a `string.gsub` callback, a `__tostring` and a
/// `__index`. Each reports whether the recursion ended in an error and how deep it got.
const APP: &str = r#"
    app.manifest { id = "stack", name = "Stack", icon = "gear" }
    local Mutex = require("std:collections").Mutex
    local function probe(name, nest)
      app.tool { name = name, run = function()
        local depth = 0
        local ok, e = pcall(nest, function() depth = depth + 1 end)
        return { ok = ok, depth = depth, err = tostring(e) }
      end }
    end
    probe("with_lock", function(count)
      local function nest() count() return Mutex.new():with_lock(nest) end
      return nest()
    end)
    probe("gsub", function(count)
      local function nest() count() return (string.gsub("a", "a", nest)) end
      return nest()
    end)
    probe("tostring", function(count)
      local mt = {}
      mt.__tostring = function() count() return tostring(setmetatable({}, mt)) end
      return tostring(setmetatable({}, mt))
    end)
    probe("index", function(count)
      local t = setmetatable({}, { __index = function(t, k) count() return t[k] end })
      return t.x
    end)
    probe("notify", function(count)
      local t = setmetatable({}, { __index = function(t) count() app.notify(t) end })
      app.notify(t)
    end)
"#;

async fn run_case(case: &str) -> String {
    let package = AppPackage {
        entry: "app.luau".into(),
        files: BTreeMap::from([("app.luau".to_owned(), APP.as_bytes().to_vec())]),
    };
    let data = Arc::new(AppDb::open(None, &[]).unwrap());
    let vm = AppVm::load(&AppId::parse("stack").unwrap(), package, data, VmConfig::default()).await.unwrap();
    let ctx = CallContext { actor: Actor::agent("claude", "Claude"), now_ms: 0 };
    let out = vm.call_tool(case, json!({}), &ctx, false).await.unwrap();
    assert_eq!(out["ok"], false, "{case}: the recursion never ended: {out}");
    assert!(out["depth"].as_u64().unwrap() >= 50, "{case}: stopped too early: {out}");
    format!("{case}: depth {} err {}", out["depth"], out["err"])
}

/// The child: does nothing unless `STACK_CASE` names a case.
#[test]
fn stack_child() {
    let Ok(case) = std::env::var(CASE) else { return };
    let runtime = vos_host::runtime::build().unwrap();
    let saw = Arc::new(Mutex::new(String::new()));
    let seen = saw.clone();
    vos_host::runtime::run(&runtime, async move {
        *seen.lock().unwrap() = run_case(&case).await;
        Ok(())
    })
    .unwrap();
    println!("SURVIVED {}", saw.lock().unwrap());
}

fn survives(case: &str) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "stack_child", "--nocapture", "--test-threads=1"])
        .env(CASE, case)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let tail =
        stderr.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    assert!(
        out.status.success() && stdout.contains(&format!("SURVIVED {case}:")),
        "the host's runtime did not survive recursion through `{case}`: {:?}\n{stdout}\n{tail}",
        out.status
    );
}

#[test]
fn recursion_through_mutex_with_lock_is_an_error_on_the_hosts_threads() {
    survives("with_lock");
}

#[test]
fn recursion_through_a_gsub_callback_is_an_error_on_the_hosts_threads() {
    survives("gsub");
}

#[test]
fn recursion_through_a_tostring_metamethod_is_an_error_on_the_hosts_threads() {
    survives("tostring");
}

#[test]
fn recursion_through_an_index_metamethod_is_an_error_on_the_hosts_threads() {
    survives("index");
}

#[test]
fn recursion_through_a_host_function_the_adapter_registers_is_an_error_on_the_hosts_threads() {
    survives("notify");
}

#[test]
fn run_gives_back_what_the_top_level_future_returned_and_raises_its_panic() {
    let runtime = vos_host::runtime::build().unwrap();
    vos_host::runtime::run(&runtime, async { Ok(()) }).unwrap();
    let e = vos_host::runtime::run(&runtime, async { anyhow::bail!("no") }).unwrap_err();
    assert_eq!(e.to_string(), "no");
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = vos_host::runtime::run(&runtime, async { panic!("top-level boom") });
    }));
    let payload = panicked.unwrap_err();
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"top-level boom"));
}

//! The test runner of Luau test scripts: `lib:test`, after Bun's and Jest's.
//!
//! ```luau
//! local t = require("lib:test")
//!
//! t.describe("base64", function()
//!     t.test("encodes", function()
//!         t.expect(base64.encode("hi")):toBe("aGk=")
//!     end)
//! end)
//! ```
//!
//! `describe` names a suite and runs its body; `test` runs one case, counts it as passed or failed and prints the
//! line; `expect(value)` gives a `Matcher` whose methods raise when the value is not what they say. The bodies may
//! call functions that wait (a sleep, a request). A program reads the counts with [`summary`] when the script ends.
//!
//! A test that does not end is the watchdog's: an OS thread that, [`DEFAULT_TEST_TIMEOUT_SECS`] seconds
//! (`NEOC_TEST_TIMEOUT_SECS`) after a test started, reports it and ends the process with status 124, as
//! `timeout(1)` does. A thread, because a test that blocks the runtime's thread never reaches an `.await` a timer
//! could interrupt.
//!
//! The API's rules:
//! - A matcher is a typed object and its assertions are methods: `expect(x):toBe(y)`. The names are Jest's.
//! - A failed expectation raises a typed `ExpectError` (`not_equal`, `not_nil`, `not_truthy`, `not_falsy`,
//!   `not_contained`, `not_a_container`, `did_not_throw`, `not_a_function`) that carries both values as text; a failed
//!   test's line shows it whole (`lib:test.Matcher:toBe: not_equal: expected 1 to be 2`).
//! - Suites nest: a test under `describe("a")` under `describe("b")` is `b > a > name`, and a test after an inner suite
//!   is still in the outer one.
//! - `toContain` with an empty string holds for every string.
//! - A body that is not a function, and an argument too many, are refused with their place.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mlua::{Lua, Value};
use parking_lot::Mutex;

/// Default per-test wall-clock limit, overridable with `NEOC_TEST_TIMEOUT_SECS`.
const DEFAULT_TEST_TIMEOUT_SECS: u64 = 60;

/// Exit status when the watchdog ends the process (that of `timeout(1)`).
const TIMEOUT_EXIT_CODE: i32 = 124;

/// The test now running, in any VM of the process, and when it is given up on. `None` between tests.
static ACTIVE_TEST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static WATCHDOG: std::sync::Once = std::sync::Once::new();

fn test_timeout() -> Duration {
    let secs = std::env::var("NEOC_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TEST_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// The test that has run past its deadline at `now`, if one has.
fn overdue(now: Instant) -> Option<String> {
    ACTIVE_TEST.lock().as_ref().filter(|(deadline, _)| now >= *deadline).map(|(_, label)| label.clone())
}

/// Arm the watchdog for `label`, for as long as the guard lives: dropped, it disarms, so a test whose future is
/// cancelled leaves no deadline behind.
fn arm_watchdog(label: &str) -> WatchdogGuard {
    WATCHDOG.call_once(|| {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                if let Some(label) = overdue(Instant::now()) {
                    println!("  \x1b[31m✗\x1b[0m {label}");
                    println!(
                        "    \x1b[2mtimed out after {}s with no result\x1b[0m",
                        test_timeout().as_secs()
                    );
                    crate::exit(TIMEOUT_EXIT_CODE);
                }
            }
        });
    });
    *ACTIVE_TEST.lock() = Some((Instant::now() + test_timeout(), label.to_owned()));
    WatchdogGuard
}

struct WatchdogGuard;

impl Drop for WatchdogGuard {
    fn drop(&mut self) {
        *ACTIVE_TEST.lock() = None;
    }
}

/// What the tests of one VM came to, and the suites now open.
#[derive(Default)]
struct Run {
    passed: u32,
    failed: u32,
    suites: Vec<String>,
}

impl Run {
    /// `suite > inner suite > name`.
    fn label(&self, name: &str) -> String {
        self.suites.iter().map(String::as_str).chain([name]).collect::<Vec<_>>().join(" > ")
    }
}

#[derive(Clone, Default)]
struct RunState(Arc<Mutex<Run>>);

/// This VM's run, made the first time the module is used.
fn run_of(lua: &Lua) -> RunState {
    if let Some(run) = lua.app_data_ref::<RunState>() {
        return run.clone();
    }
    let run = RunState::default();
    lua.set_app_data(run.clone());
    run
}

/// How many tests of this VM passed and failed: for the program that ran the script, once it ends.
#[must_use]
pub fn summary(lua: &Lua) -> (u32, u32) {
    let run = run_of(lua);
    let run = run.0.lock();
    (run.passed, run.failed)
}

/// A value as a failed expectation shows it.
fn display(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(n) => n.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => match s.to_str() {
            Ok(st) => format!("{st:?}"),
            Err(_) => format!("<binary {} bytes>", s.as_bytes().len()),
        },
        Value::Table(_) => "<table>".into(),
        Value::Function(_) => "<function>".into(),
        _ => "<value>".into(),
    }
}

/// Lua's truth: only `nil` and `false` are falsy.
fn is_falsy(v: &Value) -> bool {
    matches!(v, Value::Nil | Value::Boolean(false))
}

/// Why a test failed, without the traceback mlua wraps a host function's error in.
fn extract_message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(s) => s.clone(),
        mlua::Error::CallbackError { cause, .. } => extract_message(cause),
        other => other.to_string(),
    }
}

// A matcher's methods are Jest's (`toBe`, `toBeNil`), not snake case: the names a test author knows, and the ones
// the same test reads with in TypeScript (`expect(x).toBe(y)`).
#[allow(non_snake_case)]
#[mlua_ir::module("lib:test")]
pub mod test {
    use mlua::{Lua, Value};
    use mlua_ir::{Callback, Live, LuaError, Object, Raise, unknown_reason};
    use serde::Serialize;

    use super::{arm_watchdog, display, extract_message, is_falsy, run_of};

    unknown_reason!(
        /// A value under test, and what it is compared with.
        Tested = "whatever value a test checks or compares with: a test may expect anything of anything"
    );

    /// Why an expectation did not hold. The values are shown as a failed test's line shows them: a string in
    /// quotes, a number, `nil`, `<table>`, `<function>`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ExpectError {
        /// `toBe`: the two values are not equal.
        #[error("expected {actual} to be {expected}")]
        NotEqual {
            /// The value under test.
            actual: String,
            /// What it was expected to be.
            expected: String,
        },
        /// `toBeNil`: the value is not `nil`.
        #[error("expected nil, got {actual}")]
        NotNil {
            /// The value under test.
            actual: String,
        },
        /// `toBeTruthy`: the value is `nil` or `false`.
        #[error("expected truthy, got {actual}")]
        NotTruthy {
            /// The value under test.
            actual: String,
        },
        /// `toBeFalsy`: the value is neither `nil` nor `false`.
        #[error("expected falsy, got {actual}")]
        NotFalsy {
            /// The value under test.
            actual: String,
        },
        /// `toContain`: the string does not have the substring, or the table does not hold the value.
        #[error("{actual} does not contain {item}")]
        NotContained {
            /// The value under test: the string, or `table`.
            actual: String,
            /// What it was expected to contain.
            item: String,
        },
        /// `toContain`: the value under test is not a string or a table, or a string was searched for what is not
        /// one.
        #[error("toContain requires a string or table value")]
        NotAContainer,
        /// `toThrow`: the function returned.
        #[error("expected function to throw, but it returned normally")]
        DidNotThrow,
        /// `toThrow`: the value under test is not a function.
        #[error("toThrow requires a function value")]
        NotAFunction,
    }

    /// A value under test: each method checks one thing of it and raises an `ExpectError` when it does not hold,
    /// which fails the test.
    #[derive(Clone, Object)]
    pub struct Matcher {
        actual: Value,
    }

    #[mlua_ir::object]
    impl Matcher {
        /// The value equals `expected`: the same string, number or boolean, or the same table or function (not
        /// one that looks the same).
        ///
        /// ```luau
        /// test.expect(1 + 1):toBe(2)
        /// ```
        pub fn toBe(&self, expected: Live<Tested>) -> Result<(), Raise<ExpectError>> {
            if self.actual == expected.value {
                Ok(())
            } else {
                Err(ExpectError::NotEqual {
                    actual: display(&self.actual),
                    expected: display(&expected.value),
                }
                .into())
            }
        }

        /// The value is `nil`.
        pub fn toBeNil(&self) -> Result<(), Raise<ExpectError>> {
            if self.actual.is_nil() {
                Ok(())
            } else {
                Err(ExpectError::NotNil { actual: display(&self.actual) }.into())
            }
        }

        /// The value is neither `nil` nor `false`.
        pub fn toBeTruthy(&self) -> Result<(), Raise<ExpectError>> {
            if is_falsy(&self.actual) {
                Err(ExpectError::NotTruthy { actual: display(&self.actual) }.into())
            } else {
                Ok(())
            }
        }

        /// The value is `nil` or `false`.
        pub fn toBeFalsy(&self) -> Result<(), Raise<ExpectError>> {
            if is_falsy(&self.actual) {
                Ok(())
            } else {
                Err(ExpectError::NotFalsy { actual: display(&self.actual) }.into())
            }
        }

        /// A string has the substring `item`, or a table holds the value `item` under some key.
        ///
        /// ```luau
        /// test.expect("hello world"):toContain("lo w")
        /// test.expect({ "a", "b" }):toContain("b")
        /// ```
        pub fn toContain(&self, item: Live<Tested>) -> mlua::Result<Result<(), Raise<ExpectError>>> {
            let item = item.value;
            let found = match (&self.actual, &item) {
                (Value::String(text), Value::String(part)) => {
                    let (haystack, needle) = (text.as_bytes(), part.as_bytes());
                    needle.is_empty() || haystack.windows(needle.len()).any(|window| window == &*needle)
                }
                (Value::Table(table), _) => {
                    let mut found = false;
                    for pair in table.pairs::<Value, Value>() {
                        if pair?.1 == item {
                            found = true;
                            break;
                        }
                    }
                    found
                }
                _ => return Ok(Err(ExpectError::NotAContainer.into())),
            };
            Ok(if found {
                Ok(())
            } else {
                let actual = if self.actual.is_table() { "table".to_owned() } else { display(&self.actual) };
                Err(ExpectError::NotContained { actual, item: display(&item) }.into())
            })
        }

        /// The value is a function that raises when called (with no arguments). The function is called here and
        /// now, and may not wait.
        ///
        /// ```luau
        /// test.expect(function() error("no") end):toThrow()
        /// ```
        pub fn toThrow(&self) -> Result<(), Raise<ExpectError>> {
            // The function is called directly: what it raises is caught here, never turned into text, so an error
            // value whose `__tostring` fails is a throw like any other.
            match &self.actual {
                Value::Function(f) => match f.call::<Value>(()) {
                    Err(_) => Ok(()),
                    Ok(_) => Err(ExpectError::DidNotThrow.into()),
                },
                _ => Err(ExpectError::NotAFunction.into()),
            }
        }
    }

    /// Take `value` under test.
    ///
    /// ```luau
    /// test.expect(result):toBe("aGk=")
    /// test.expect(err):toBeNil()
    /// ```
    pub fn expect(value: Live<Tested>) -> Matcher {
        Matcher { actual: value.value }
    }

    /// Run `body` as the suite `name`: the tests in it are named `name > test`, and suites nest. What `body`
    /// raises outside a test is raised here.
    #[host("Vm")]
    pub async fn describe(lua: Lua, name: String, body: Callback<fn()>) -> mlua::Result<()> {
        println!("\n{name}");
        let run = run_of(&lua);
        run.0.lock().suites.push(name);
        let result = body.function().call_async::<()>(()).await;
        run.0.lock().suites.pop();
        result
    }

    /// Run `body` as the test `name`: passed when it returns, failed when it raises (an expectation that did not
    /// hold, or any other error), and counted and printed either way. A failed test raises nothing: the tests
    /// after it run.
    #[host("Vm")]
    pub async fn test(lua: Lua, name: String, body: Callback<fn()>) {
        let run = run_of(&lua);
        let label = run.0.lock().label(&name);
        let watchdog = arm_watchdog(&label);
        let outcome = body.function().call_async::<()>(()).await;
        drop(watchdog);
        match outcome {
            Ok(()) => {
                run.0.lock().passed += 1;
                println!("  \x1b[32m✓\x1b[0m {label}");
            }
            Err(e) => {
                run.0.lock().failed += 1;
                println!("  \x1b[31m✗\x1b[0m {label}");
                println!("    \x1b[2m{}\x1b[0m", extract_message(&e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use mlua::{Lua, Value};

    use super::*;

    fn engine() -> Lua {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:test")"#).eval().unwrap();
        lua.globals().set("test", m).unwrap();
        lua
    }

    fn run<T: mlua::FromLuaMulti>(lua: &Lua, code: &str) -> T {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(lua.load(code).eval_async()).unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    /// The text of what `code` (an expression that calls a matcher) raises.
    fn raised(lua: &Lua, code: &str) -> String {
        lua.load(format!(
            "local ok, err = pcall(function() {code} end) assert(not ok, 'held') return tostring(err)"
        ))
        .eval()
        .unwrap_or_else(|e| panic!("{code}: {e}"))
    }

    #[test]
    fn tests_are_counted_and_a_failed_one_does_not_stop_the_rest() {
        let lua = engine();
        assert_eq!(summary(&lua), (0, 0));
        let ran: String = run(
            &lua,
            r#"
            local ran = {}
            test.test("passes", function() table.insert(ran, "a") end)
            test.test("fails an expectation", function() table.insert(ran, "b") test.expect(1):toBe(2) table.insert(ran, "never") end)
            test.test("raises", function() error("boom") end)
            test.describe("suite", function()
                test.test("passes too", function() table.insert(ran, "c") return "a value nobody reads" end)
            end)
            return table.concat(ran, ",")
            "#,
        );
        assert_eq!(ran, "a,b,c");
        assert_eq!(summary(&lua), (2, 2));
        // each VM counts its own
        assert_eq!(summary(&engine()), (0, 0));
    }

    #[test]
    fn test_and_describe_return_nothing_and_refuse_what_is_not_a_body() {
        let lua = engine();
        let returned: (i64, i64) = run(
            &lua,
            r##"return select("#", test.test("t", function() end)), select("#", test.describe("d", function() end))"##,
        );
        assert_eq!(returned, (0, 0));
        for (code, place) in [
            (r#"test.test("t")"#, "lib:test.test: argument #2 'body'"),
            (r#"test.test("t", "body")"#, "lib:test.test: argument #2 'body'"),
            (r#"test.test(function() end)"#, "lib:test.test: argument #1 'name'"),
            (r#"test.describe("d", {})"#, "lib:test.describe: argument #2 'body'"),
            (r#"test.describe("d", function() end, 1)"#, "lib:test.describe: takes 2 arguments, got more"),
            (r#"test.expect(1, 2)"#, "lib:test.expect: takes 1 argument, got more"),
            (r#"test.expect(1):toBeNil(1)"#, "lib:test.Matcher:toBeNil: takes 0 arguments, got more"),
        ] {
            let err = runtime_error(&lua, code);
            assert!(err.contains(place), "{code}: {err}");
        }
        assert_eq!(summary(&lua), (1, 0), "a refused call is not a test");
    }

    fn runtime_error(lua: &Lua, code: &str) -> String {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(lua.load(code).exec_async()).expect_err(code).to_string()
    }

    /// Suites nest, and the end of one is not the end of those around it.
    #[test]
    fn a_test_is_named_after_every_suite_around_it() {
        let run = Run { suites: vec!["outer".into(), "inner".into()], ..Run::default() };
        assert_eq!(run.label("name"), "outer > inner > name");
        assert_eq!(Run::default().label("name"), "name");

        let lua = engine();
        let state = run_of(&lua);
        let probe = {
            let state = state.clone();
            lua.create_function(move |_, ()| Ok(state.0.lock().label("?"))).unwrap()
        };
        lua.globals().set("label", probe).unwrap();
        let labels: String = super::tests::run(
            &lua,
            r#"
            local seen = { label() }
            test.describe("outer", function()
                table.insert(seen, label())
                test.describe("inner", function() table.insert(seen, label()) end)
                table.insert(seen, label())
            end)
            table.insert(seen, label())
            return table.concat(seen, "|")
            "#,
        );
        assert_eq!(labels, "?|outer > ?|outer > inner > ?|outer > ?|?");
    }

    /// What a suite's body raises outside a test is raised by `describe`, and the suite is closed all the same.
    #[test]
    fn describe_raises_what_its_body_raises() {
        let lua = engine();
        let (ok, err): (bool, String) = run(
            &lua,
            r#"local ok, err = pcall(test.describe, "broken", function() error("outside a test") end)
               return ok, tostring(err)"#,
        );
        assert!(!ok && err.contains("outside a test"), "{err}");
        assert!(run_of(&lua).0.lock().suites.is_empty());
        assert_eq!(summary(&lua), (0, 0));
    }

    /// A body may wait: `test` and `describe` are async, and a test that waits is one test.
    #[test]
    fn bodies_may_wait() {
        let lua = engine();
        let order: String = run(
            &lua,
            r#"
            local time = require("std:time")
            local order = {}
            test.describe("waits", function()
                time.sleep(time.Duration.from_millis(1))
                test.test("sleeps", function()
                    table.insert(order, "before")
                    time.sleep(time.Duration.from_millis(5))
                    table.insert(order, "after")
                end)
                table.insert(order, "next")
            end)
            return table.concat(order, ",")
            "#,
        );
        assert_eq!(order, "before,after,next");
        assert_eq!(summary(&lua), (1, 0));
    }

    #[test]
    fn matchers_hold_or_raise_a_typed_error() {
        let lua = engine();
        // these hold: nothing is raised, nothing is returned
        lua.load(
            r##"
            local expect = test.expect
            local t = {}
            assert(select("#", expect(1):toBe(1)) == 0)
            expect("a"):toBe("a") expect(nil):toBe(nil) expect(true):toBe(true) expect(t):toBe(t) expect(1.5):toBe(1.5)
            expect(nil):toBeNil()
            expect(0):toBeTruthy() expect(""):toBeTruthy() expect(t):toBeTruthy() expect(true):toBeTruthy()
            expect(nil):toBeFalsy() expect(false):toBeFalsy()
            expect("hello world"):toContain("lo w") expect("abc"):toContain("") expect(""):toContain("")
            expect("a\0b\255"):toContain("\0b\255")
            expect({ "a", "b" }):toContain("b") expect({ k = 7 }):toContain(7) expect({ t }):toContain(t)
            expect(function() error("no") end):toThrow()
            expect(function() error({ code = 1 }) end):toThrow()
            expect(error):toThrow()
            "##,
        )
        .exec()
        .unwrap();

        for (code, text) in [
            ("test.expect(1):toBe(2)", "lib:test.Matcher:toBe: not_equal: expected 1 to be 2"),
            (r#"test.expect("a"):toBe("b")"#, r#"lib:test.Matcher:toBe: not_equal: expected "a" to be "b""#),
            (r#"test.expect("1"):toBe(1)"#, r#"lib:test.Matcher:toBe: not_equal: expected "1" to be 1"#),
            ("test.expect({}):toBe({})", "lib:test.Matcher:toBe: not_equal: expected <table> to be <table>"),
            ("test.expect(nil):toBe(false)", "lib:test.Matcher:toBe: not_equal: expected nil to be false"),
            (
                "test.expect(print):toBe(1.5)",
                "lib:test.Matcher:toBe: not_equal: expected <function> to be 1.5",
            ),
            (
                r#"test.expect("\255"):toBe(1)"#,
                "lib:test.Matcher:toBe: not_equal: expected <binary 1 bytes> to be 1",
            ),
            ("test.expect(false):toBeNil()", "lib:test.Matcher:toBeNil: not_nil: expected nil, got false"),
            (
                "test.expect(nil):toBeTruthy()",
                "lib:test.Matcher:toBeTruthy: not_truthy: expected truthy, got nil",
            ),
            (
                "test.expect(false):toBeTruthy()",
                "lib:test.Matcher:toBeTruthy: not_truthy: expected truthy, got false",
            ),
            ("test.expect(0):toBeFalsy()", "lib:test.Matcher:toBeFalsy: not_falsy: expected falsy, got 0"),
            (
                r#"test.expect("abc"):toContain("d")"#,
                r#"lib:test.Matcher:toContain: not_contained: "abc" does not contain "d""#,
            ),
            (r#"test.expect({ "a" }):toContain("b")"#, r#"table does not contain "b""#),
            (r#"test.expect({ 1 }):toContain({})"#, "table does not contain <table>"),
            (
                r#"test.expect("abc"):toContain(1)"#,
                "lib:test.Matcher:toContain: not_a_container: toContain requires a string or table value",
            ),
            (r#"test.expect(5):toContain(5)"#, "toContain requires a string or table value"),
            (
                "test.expect(function() end):toThrow()",
                "lib:test.Matcher:toThrow: did_not_throw: expected function to throw, but it returned normally",
            ),
            (
                "test.expect(5):toThrow()",
                "lib:test.Matcher:toThrow: not_a_function: toThrow requires a function value",
            ),
        ] {
            let err = raised(&lua, code);
            assert!(err.contains(text), "{code}: {err}");
        }
    }

    /// mlua's message handler runs a value's
    /// `__tostring` to make the text of an error, and `toThrow` calls the function directly, so an error value
    /// whose `__tostring` fails is caught, not rendered, on a small stack too.
    #[test]
    fn to_throw_catches_an_error_value_whose_tostring_fails() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let lua = engine();
                let out: Vec<String> = lua
                    .load(
                        r#"local r = {}
                           local mt = {}
                           mt.__tostring = function() pcall(error, "x") error(setmetatable({}, mt)) end
                           r[1] = tostring((pcall(function() test.expect(function() error(setmetatable({}, mt)) end):toThrow() end)))
                           r[2] = tostring((pcall(function() test.expect(function() error("plain") end):toThrow() end)))
                           r[3] = tostring(select(2, pcall(function() test.expect(function() end):toThrow() end)))
                           return r"#,
                    )
                    .eval()
                    .unwrap();
                assert_eq!(out[0], "true", "toThrow caught the failing error value");
                assert_eq!(out[1], "true");
                assert!(out[2].contains("expected function to throw"), "{}", out[2]);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// `describe` is an async host function that runs its body, and a
    /// body that calls `describe` again recurses through the host. Luau stops it at its C-call limit: an error the
    /// script catches, on the stack a VM thread has, not an overflow.
    #[test]
    fn describe_nested_in_itself_ends_in_an_error() {
        let out: String = std::thread::Builder::new()
            .stack_size(crate::VM_THREAD_STACK)
            .spawn(|| {
                let lua = engine();
                run(
                    &lua,
                    r#"local depth = 0
                       local function nest() depth = depth + 1 test.describe("d", nest) end
                       local ok, e = pcall(nest)
                       return tostring(ok) .. ",depth>100:" .. tostring(depth > 100)"#,
                )
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(out, "false,depth>100:true");
    }

    /// The watchdog's clock: a test is overdue once its deadline has passed, and not after it ended. (That it then
    /// ends the process with 124 is the binary's test: it cannot be seen from inside the process.)
    #[test]
    fn a_test_is_overdue_past_its_deadline_and_never_after_it_ended() {
        // the one test that arms the watchdog with a deadline it looks at; `NEOC_TEST_TIMEOUT_SECS` is not set here
        assert_eq!(test_timeout(), Duration::from_secs(DEFAULT_TEST_TIMEOUT_SECS));
        assert_eq!(TIMEOUT_EXIT_CODE, 124);
        let guard = arm_watchdog("suite > slow");
        let now = Instant::now();
        assert_eq!(overdue(now), None);
        if ACTIVE_TEST.lock().as_ref().is_some_and(|(_, label)| label == "suite > slow") {
            assert_eq!(
                overdue(now + test_timeout() + Duration::from_secs(1)).as_deref(),
                Some("suite > slow")
            );
        }
        drop(guard);
        assert_eq!(overdue(now + test_timeout() + Duration::from_secs(1)), None);
    }

    /// A test's failure line is the error without the traceback of the host function it came through.
    #[test]
    fn a_failure_is_shown_without_its_traceback() {
        let lua = engine();
        let err = lua.load("test.expect(1):toBe(2)").exec().expect_err("raised");
        assert_eq!(extract_message(&err), "lib:test.Matcher:toBe: not_equal: expected 1 to be 2");
        let err = lua.load("error('plain', 0)").exec().expect_err("raised");
        assert!(
            extract_message(&err).starts_with(
                "plain
stack traceback:"
            ),
            "a script's own error is mlua's text of it"
        );
        let value: Value = lua.load("return 1").eval().unwrap();
        assert_eq!(display(&value), "1");
    }

    /// An embedder that runs scripts it does not trust leaves the runner out of what it offers.
    #[test]
    fn an_embedder_can_leave_it_out() {
        let lua = crate::make_bare_engine().expect("engine");
        let allowed: Vec<&str> = crate::modules::names().filter(|name| *name != "lib:test").collect();
        crate::modules::install_allowed(&lua, &allowed).expect("install");
        let err = lua.load(r#"return require("lib:test")"#).eval::<Value>().expect_err("refused").to_string();
        assert!(err.contains(r#"unknown module "lib:test""#), "{err}");
    }
}

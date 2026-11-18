//! Bun-style test runner exposed to Lua as `lib.test`.
//!
//! Dot-syntax is used for matcher chains (no `self`), matching Bun/Jest:
//!   expect(result).toBe("aGk=")
//!   `expect(err).toBeNil()`
//!
//! Each `expect(val)` call stores the value in the Lua registry under a unique
//! key so standalone (non-self) closures can retrieve it.

use mlua::{Lua, Table, Value};

use crate::lua::facade::Module;

const STATE_KEY: &str = "_AGENTOS_TEST";

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:test` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// `describe` / `test` / `it` are async (created with `create_async_function`)
/// and `expect` returns a matcher table whose chained assertions raise
/// `mlua::Error::RuntimeError` directly — none use the `(value, error)` idiom or
/// an `#[instrument]` span, so the facade's `func` / `try_func` path does not
/// apply. The matcher methods live on a plain returned table, not a userdata
/// type. The free functions' signatures are recorded for the header via
/// [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:test")?;
    init_state(lua)?;
    register_suite_fns(lua, &mut m)?;
    register_expect(lua, &mut m)?;
    Ok(m)
}

/// Seed the suite state (`passed`/`failed`/`current_suite`) in the registry.
fn init_state(lua: &Lua) -> mlua::Result<()> {
    let state = lua.create_table()?;
    state.set("passed", 0_u32)?;
    state.set("failed", 0_u32)?;
    state.set("current_suite", Value::Nil)?;
    lua.set_named_registry_value(STATE_KEY, state)?;
    Ok(())
}

/// Register `describe` plus the `test` / `it` aliases — the suite-structure and
/// test-case entry points, all async so bodies may call yielding functions.
fn register_suite_fns(lua: &Lua, m: &mut Module<'_>) -> mlua::Result<()> {
    // describe(name, fn) — async so test bodies can call yielding functions
    m.set(
        "describe",
        lua.create_async_function(|lua, (name, f): (String, mlua::Function)| async move {
            println!("\n{name}");
            let state: Table = lua.named_registry_value(STATE_KEY)?;
            state.set("current_suite", lua.create_string(name.as_bytes())?)?;
            let result = f.call_async::<()>(()).await;
            state.set("current_suite", Value::Nil)?;
            result
        })?,
    )?;
    m.record_fn(
        "describe",
        &[("name", "string"), ("fn", "LuaValue")],
        "()",
        "Declare a named test suite. Suites nest — `describe` inside `describe` prefixes the test name.",
    );

    // test(name, fn) / it(name, fn) — async for the same reason
    let run_test = lua.create_async_function(|lua, (name, f): (String, mlua::Function)| async move {
        let state: Table = lua.named_registry_value(STATE_KEY)?;
        let suite: Option<mlua::LuaString> = state.get("current_suite")?;
        let label = match suite {
            Some(s) => format!("{} > {name}", s.to_str()?),
            None => name,
        };
        match f.call_async::<()>(()).await {
            Ok(()) => {
                let n: u32 = state.get("passed")?;
                state.set("passed", n + 1)?;
                println!("  \x1b[32m✓\x1b[0m {label}");
            }
            Err(e) => {
                let n: u32 = state.get("failed")?;
                state.set("failed", n + 1)?;
                println!("  \x1b[31m✗\x1b[0m {label}");
                println!("    \x1b[2m{}\x1b[0m", extract_message(&e));
            }
        }
        Ok(())
    })?;
    m.set("test", run_test.clone())?;
    m.record_fn(
        "test",
        &[("name", "string"), ("fn", "LuaValue")],
        "()",
        "Declare a named test case. The callback may be async. Alias: `it`.",
    );
    m.set("it", run_test)?;
    m.record_fn(
        "it",
        &[("name", "string"), ("fn", "LuaValue")],
        "()",
        "Alias for `test`. Prefer this inside a `describe` block for BDD-style readability.",
    );
    Ok(())
}

/// Register `expect(value)`, which wraps a value in the dot-syntax matcher table.
fn register_expect(lua: &Lua, m: &mut Module<'_>) -> mlua::Result<()> {
    m.set("expect", lua.create_function(|lua, value: Value| make_matcher(lua, value))?)?;
    m.record_fn(
        "expect",
        &[("value", "LuaValue")],
        "LuaValue",
        "Wrap `value` in a matcher table exposing dot-syntax assertions (`toBe`, `toBeNil`, `toBeTruthy`, `toBeFalsy`, `toContain`, `toThrow`).",
    );
    Ok(())
}

/// Called by `main` after the script finishes to get (passed, failed) counts.
pub fn summary(lua: &Lua) -> mlua::Result<(u32, u32)> {
    let state: Table = lua.named_registry_value(STATE_KEY)?;
    Ok((state.get("passed")?, state.get("failed")?))
}

// ---------------------------------------------------------------------------
// Matcher construction
// ---------------------------------------------------------------------------

/// Register a dot-syntax matcher that captures the expect-ed value directly.
/// `mlua::Value` is a cheap, `Send`-able registry handle, so each matcher closure
/// just clones it — no shared slot to stash and re-read.
fn register_matcher<A, F>(lua: &Lua, m: &Table, name: &str, actual: Value, body: F) -> mlua::Result<()>
where
    A: mlua::FromLuaMulti + 'static,
    F: Fn(Value, A) -> mlua::Result<()> + mlua::MaybeSend + 'static,
{
    m.set(name, lua.create_function(move |_, args: A| body(actual.clone(), args))?)
}

fn make_matcher(lua: &Lua, value: Value) -> mlua::Result<Table> {
    let m = lua.create_table()?;

    // toBe(expected) — strict equality
    register_matcher(lua, &m, "toBe", value.clone(), |actual, expected: Value| {
        if actual == expected {
            Ok(())
        } else {
            Err(mlua::Error::RuntimeError(format!(
                "expected {} to be {}",
                display(&actual),
                display(&expected),
            )))
        }
    })?;

    // toBeNil()
    register_matcher(lua, &m, "toBeNil", value.clone(), |actual, (): ()| {
        if matches!(actual, Value::Nil) {
            Ok(())
        } else {
            Err(mlua::Error::RuntimeError(format!("expected nil, got {}", display(&actual))))
        }
    })?;

    // toBeTruthy()
    register_matcher(lua, &m, "toBeTruthy", value.clone(), |actual, (): ()| {
        if is_falsy(&actual) {
            Err(mlua::Error::RuntimeError(format!("expected truthy, got {}", display(&actual))))
        } else {
            Ok(())
        }
    })?;

    // toBeFalsy()
    register_matcher(lua, &m, "toBeFalsy", value.clone(), |actual, (): ()| {
        if is_falsy(&actual) {
            Ok(())
        } else {
            Err(mlua::Error::RuntimeError(format!("expected falsy, got {}", display(&actual))))
        }
    })?;

    // toContain(item) — substring for strings, value search for tables
    register_matcher(lua, &m, "toContain", value.clone(), |actual, item: Value| match (&actual, &item) {
        (Value::String(s), Value::String(sub)) => {
            let haystack = s.as_bytes();
            let needle = sub.as_bytes();
            if haystack.windows(needle.len()).any(|w| needle == w) {
                Ok(())
            } else {
                Err(mlua::Error::RuntimeError(format!(
                    "{} does not contain {}",
                    display(&actual),
                    display(&item),
                )))
            }
        }
        (Value::Table(tbl), _) => {
            let mut found = false;
            for pair in tbl.clone().pairs::<Value, Value>() {
                if pair?.1 == item {
                    found = true;
                    break;
                }
            }
            if found {
                Ok(())
            } else {
                Err(mlua::Error::RuntimeError(format!("table does not contain {}", display(&item))))
            }
        }
        _ => Err(mlua::Error::RuntimeError("toContain requires a string or table value".into())),
    })?;

    // toThrow() — value must be a callable that raises an error
    register_matcher(lua, &m, "toThrow", value, |actual, (): ()| match actual {
        Value::Function(f) => match f.call::<Value>(()) {
            Err(_) => Ok(()),
            Ok(_) => {
                Err(mlua::Error::RuntimeError("expected function to throw, but it returned normally".into()))
            }
        },
        _ => Err(mlua::Error::RuntimeError("toThrow requires a function value".into())),
    })?;

    Ok(m)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Lua's truth rules: only `nil` and `false` are falsy. Shared by
/// `toBeTruthy` (asserts not-falsy) and `toBeFalsy` (asserts falsy).
fn is_falsy(v: &Value) -> bool {
    matches!(v, Value::Nil | Value::Boolean(false))
}

fn extract_message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(s) => s.clone(),
        mlua::Error::CallbackError { cause, .. } => extract_message(cause),
        other => other.to_string(),
    }
}

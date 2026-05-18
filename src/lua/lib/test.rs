//! Bun-style test runner exposed to Luau as `lib.test`.
//!
//! Dot-syntax is used for matcher chains (no `self`), matching Bun/Jest:
//!   expect(result).toBe("aGk=")
//!   expect(err).toBeNil()
//!
//! Each `expect(val)` call stores the value in the Lua registry under a unique
//! key so standalone (non-self) closures can retrieve it.

use mlua::{Lua, Table, Value};

const STATE_KEY: &str = "_NEOC_TEST";
const CTR_KEY: &str = "_NEOC_EXPECT_CTR";

// TODO: clean-code - 0.6 - SRP: module() does state init, describe/test/it/expect registration, and println formatting — split into init_state, register_suite_fns, register_expect.
pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let state = lua.create_table()?;
    state.set("passed", 0_u32)?;
    state.set("failed", 0_u32)?;
    state.set("current_suite", Value::Nil)?;
    lua.set_named_registry_value(STATE_KEY, state)?;

    let ctr = lua.create_table()?;
    ctr.set("n", 0_u64)?;
    lua.set_named_registry_value(CTR_KEY, ctr)?;

    let t = lua.create_table()?;

    // describe(name, fn) — async so test bodies can call yielding functions
    t.set(
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

    // test(name, fn) / it(name, fn) — async for the same reason
    let run_test =
        lua.create_async_function(|lua, (name, f): (String, mlua::Function)| async move {
            let state: Table = lua.named_registry_value(STATE_KEY)?;
            let suite: Option<mlua::String> = state.get("current_suite")?;
            let label = match suite {
                Some(s) => format!("{} > {name}", s.to_str()?),
                None => name,
            };
            match f.call_async::<()>(()).await {
                Ok(_) => {
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
    t.set("test", run_test.clone())?;
    t.set("it", run_test)?;

    // expect(value) -> matcher table with standalone (dot-syntax) methods
    t.set(
        "expect",
        lua.create_function(|lua, value: Value| make_matcher(lua, value))?,
    )?;

    Ok(t)
}

/// Called by `main` after the script finishes to get (passed, failed) counts.
pub fn summary(lua: &Lua) -> mlua::Result<(u32, u32)> {
    let state: Table = lua.named_registry_value(STATE_KEY)?;
    Ok((state.get("passed")?, state.get("failed")?))
}

// ---------------------------------------------------------------------------
// Matcher construction
// ---------------------------------------------------------------------------

// TODO: clean-code - 0.6 - KISS: stashing each expect value under a counter-keyed registry slot is over-engineered — mlua values can be cloned and captured directly in the matcher closure.
fn next_key(lua: &Lua) -> mlua::Result<String> {
    let ctr: Table = lua.named_registry_value(CTR_KEY)?;
    let n: u64 = ctr.get("n")?;
    ctr.set("n", n + 1)?;
    Ok(format!("_ev{n}"))
}

// TODO: clean-code - 0.85 - DRY: every matcher repeats {let k = key.clone(); m.set(name, create_function(move |lua, _| { registry_value(&k)?; ... Err(RuntimeError(...))}))} — extract a register_matcher helper.
fn make_matcher(lua: &Lua, value: Value) -> mlua::Result<Table> {
    let key = next_key(lua)?;
    lua.set_named_registry_value(&key, value)?;

    let m = lua.create_table()?;

    // toBe(expected) — strict equality
    {
        let k = key.clone();
        m.set(
            "toBe",
            lua.create_function(move |lua, expected: Value| {
                let actual: Value = lua.named_registry_value(&k)?;
                if actual == expected {
                    Ok(())
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "expected {} to be {}",
                        display(&actual),
                        display(&expected),
                    )))
                }
            })?,
        )?;
    }

    // toBeNil()
    {
        let k = key.clone();
        m.set(
            "toBeNil",
            lua.create_function(move |lua, _: ()| {
                let actual: Value = lua.named_registry_value(&k)?;
                if matches!(actual, Value::Nil) {
                    Ok(())
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "expected nil, got {}",
                        display(&actual)
                    )))
                }
            })?,
        )?;
    }

    // toBeTruthy()
    {
        let k = key.clone();
        m.set(
            "toBeTruthy",
            lua.create_function(move |lua, _: ()| {
                let actual: Value = lua.named_registry_value(&k)?;
                if !matches!(actual, Value::Nil | Value::Boolean(false)) {
                    Ok(())
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "expected truthy, got {}",
                        display(&actual)
                    )))
                }
            })?,
        )?;
    }

    // TODO: clean-code - 0.7 - DRY: toBeFalsy is toBeTruthy with the condition negated and the message swapped — share a single is_falsy predicate.
    // toBeFalsy()
    {
        let k = key.clone();
        m.set(
            "toBeFalsy",
            lua.create_function(move |lua, _: ()| {
                let actual: Value = lua.named_registry_value(&k)?;
                if matches!(actual, Value::Nil | Value::Boolean(false)) {
                    Ok(())
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "expected falsy, got {}",
                        display(&actual)
                    )))
                }
            })?,
        )?;
    }

    // toContain(item) — substring for strings, value search for tables
    {
        let k = key.clone();
        m.set(
            "toContain",
            lua.create_function(move |lua, item: Value| {
                let actual: Value = lua.named_registry_value(&k)?;
                match (&actual, &item) {
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
                            Err(mlua::Error::RuntimeError(format!(
                                "table does not contain {}",
                                display(&item)
                            )))
                        }
                    }
                    _ => Err(mlua::Error::RuntimeError(
                        "toContain requires a string or table value".into(),
                    )),
                }
            })?,
        )?;
    }

    // toThrow() — value must be a callable that raises an error
    {
        let k = key.clone();
        m.set(
            "toThrow",
            lua.create_function(move |lua, _: ()| {
                let actual: Value = lua.named_registry_value(&k)?;
                match actual {
                    Value::Function(f) => match f.call::<Value>(()) {
                        Err(_) => Ok(()),
                        Ok(_) => Err(mlua::Error::RuntimeError(
                            "expected function to throw, but it returned normally".into(),
                        )),
                    },
                    _ => Err(mlua::Error::RuntimeError(
                        "toThrow requires a function value".into(),
                    )),
                }
            })?,
        )?;
    }

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

fn extract_message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(s) => s.clone(),
        mlua::Error::CallbackError { cause, .. } => extract_message(cause),
        other => other.to_string(),
    }
}

//! `std:env` — environment, platform constants, and paths (mirrors `std::env`).

use mlua::{Lua, Table, Value};

use crate::lua::sandbox::{err, from_err, ok};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    t.set("os", std::env::consts::OS)?;
    t.set("arch", std::env::consts::ARCH)?;
    t.set("family", std::env::consts::FAMILY)?;
    t.set("exe_suffix", std::env::consts::EXE_SUFFIX)?;
    t.set("dll_prefix", std::env::consts::DLL_PREFIX)?;
    t.set("dll_suffix", std::env::consts::DLL_SUFFIX)?;

    // TODO: clean-code - 0.55 - DRY: home_dir/current_dir repeat PathBuf -> ok(lua, lossy) pattern; extract helper
    t.set(
        "home_dir",
        lua.create_function(|lua, ()| {
            #[allow(deprecated)]
            match std::env::home_dir() {
                Some(p) => ok(lua, p.to_string_lossy().as_ref()),
                None => err(lua, "env.home_dir: home directory not found"),
            }
        })?,
    )?;

    t.set(
        "temp_dir",
        lua.create_function(|_, ()| Ok(std::env::temp_dir().to_string_lossy().into_owned()))?,
    )?;

    t.set(
        "current_dir",
        lua.create_function(|lua, ()| match std::env::current_dir() {
            Ok(p) => ok(lua, p.to_string_lossy().as_ref()),
            Err(e) => from_err(lua, "env.current_dir", e),
        })?,
    )?;

    t.set(
        "var",
        lua.create_function(|lua, key: String| match std::env::var(&key) {
            Ok(v) => ok(lua, v),
            Err(e) => err(lua, format!("env.var: {key}: {e}")),
        })?,
    )?;

    t.set(
        "vars",
        lua.create_function(|lua, ()| {
            let tbl = lua.create_table()?;
            for (k, v) in std::env::vars_os() {
                if let (Some(k), Some(v)) = (k.to_str(), v.to_str()) {
                    tbl.set(k, v)?;
                }
            }
            Ok(tbl)
        })?,
    )?;

    // TODO: clean-code - 0.65 - KISS: nested match on _NEOC_ARGS Value variants is dense; flatten via early return
    // TODO: clean-code - 0.6 - DRY: identical "_NEOC_ARGS: expected array of strings" literal duplicated on two arms
    t.set(
        "args",
        lua.create_function(|lua, ()| {
            let globals = lua.globals();
            match globals.get::<Value>("_NEOC_ARGS") {
                Ok(Value::Nil) => Ok(Vec::<String>::new()),
                Ok(Value::Table(tbl)) => tbl
                    .sequence_values::<String>()
                    .collect::<mlua::Result<Vec<String>>>()
                    .map_err(|_| {
                        mlua::Error::runtime("env.args: _NEOC_ARGS: expected array of strings")
                    }),
                Ok(_) => Err(mlua::Error::runtime(
                    "env.args: _NEOC_ARGS: expected array of strings",
                )),
                Err(_) => Ok(Vec::new()),
            }
        })?,
    )?;

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::sandbox::make_engine;

    fn get_env(lua: &Lua) -> Table {
        let require: mlua::Function = lua.globals().get("require").unwrap();
        require.call("std:env").unwrap()
    }

    #[test]
    fn platform_constants_are_set() {
        let lua = make_engine().unwrap();
        let env = get_env(&lua);
        let os: String = env.get("os").unwrap();
        assert!(!os.is_empty());
        let arch: String = env.get("arch").unwrap();
        assert!(!arch.is_empty());
    }

    #[test]
    fn var_returns_existing() {
        unsafe { std::env::set_var("_NEOC_TEST_VAR", "hello") };
        let lua = make_engine().unwrap();
        let env = get_env(&lua);
        let var_fn: mlua::Function = env.get("var").unwrap();
        let (val, err): (Value, Value) = var_fn.call("_NEOC_TEST_VAR").unwrap();
        assert_eq!(val.as_str().unwrap(), "hello");
        assert!(err.is_nil());
        unsafe { std::env::remove_var("_NEOC_TEST_VAR") };
    }

    #[test]
    fn var_returns_error_for_missing() {
        let lua = make_engine().unwrap();
        let env = get_env(&lua);
        let var_fn: mlua::Function = env.get("var").unwrap();
        let (val, err): (Value, Value) = var_fn.call("_NEOC_DEFINITELY_MISSING_VAR_XYZ").unwrap();
        assert!(val.is_nil());
        let msg = err.as_str().unwrap();
        assert!(msg.starts_with("env.var: _NEOC_DEFINITELY_MISSING_VAR_XYZ:"));
    }
}

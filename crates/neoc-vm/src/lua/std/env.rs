//! `std:env` — environment, platform constants, and paths (mirrors `std::env`).

use mlua::{Lua, Table, Value};

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};

// Return a filesystem path to Lua as a lossy UTF-8 string under the
// `(value, nil)` convention. Shared by `home_dir` / `current_dir`.
fn ok_path(lua: &Lua, p: &std::path::Path) -> mlua::Result<(Value, Value)> {
    ok(lua, p.to_string_lossy().as_ref())
}

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:env` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]: the
/// hand-written registration carried **no** `#[instrument]` span, so routing
/// through the facade's `func` / `try_func` would add one and break
/// byte-identical tracing. `home_dir` and `var` bake their own literal error
/// messages, `args` raises `mlua::Error` directly, and the platform constants
/// are plain string values — none fit the facade's regular paths. Each is
/// recorded via
/// what `neoc_macros` reads from the implementation /
/// [`crate::lua::facade::Module::const_`].
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:env")?;

    /// Target operating system (e.g. `"linux"`, `"macos"`, `"windows"`).
    #[lua("string", constant)]
    m.set("os", std::env::consts::OS)?;
    /// Target CPU architecture (e.g. `"x86_64"`, `"aarch64"`).
    #[lua("string", constant)]
    m.set("arch", std::env::consts::ARCH)?;
    /// Target OS family (`"unix"` or `"windows"`).
    #[lua("string", constant)]
    m.set("family", std::env::consts::FAMILY)?;
    /// Executable file suffix for the target (e.g. `".exe"` on Windows).
    #[lua("string", constant)]
    m.set("exe_suffix", std::env::consts::EXE_SUFFIX)?;
    /// Dynamic-library filename prefix for the target (e.g. `"lib"`).
    #[lua("string", constant)]
    m.set("dll_prefix", std::env::consts::DLL_PREFIX)?;
    /// Dynamic-library filename suffix for the target (e.g. `".so"`, `".dylib"`).
    #[lua("string", constant)]
    m.set("dll_suffix", std::env::consts::DLL_SUFFIX)?;

    /// Return the current user's home directory, or an error if it cannot be determined.
    #[lua("Result<string, string>")]
    m.set(
        "home_dir",
        lua.create_function(|lua, ()| {
            #[allow(deprecated)]
            match std::env::home_dir() {
                Some(p) => ok_path(lua, &p),
                None => err(lua, "env.home_dir: home directory not found"),
            }
        })?,
    )?;

    /// Return the platform temporary-file directory.
    #[lua("string")]
    m.set(
        "temp_dir",
        lua.create_function(|_, ()| Ok(std::env::temp_dir().to_string_lossy().into_owned()))?,
    )?;

    /// Return the process's current working directory, or an error.
    #[lua("Result<string, string>")]
    m.set(
        "current_dir",
        lua.create_function(|lua, ()| match std::env::current_dir() {
            Ok(p) => ok_path(lua, &p),
            Err(e) => from_err(lua, "env.current_dir", e),
        })?,
    )?;

    /// Read the environment variable `key`, or an error if it is unset or not valid Unicode.
    #[lua("Result<string, string>")]
    m.set(
        "var",
        lua.create_function(|lua, key: String| match std::env::var(&key) {
            Ok(v) => ok(lua, v),
            Err(e) => err(lua, format!("env.var: {key}: {e}")),
        })?,
    )?;

    /// Return every environment variable as a `{ [string]: string }` table, skipping non-Unicode entries.
    #[lua("LuaValue")]
    m.set(
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

    /// Return the CLI arguments passed to the script as a 1-indexed `{ string }` table.
    #[lua("LuaValue")]
    m.set(
        "args",
        lua.create_function(|lua, ()| {
            const ERR: &str = "env.args: _NEOC_ARGS: expected array of strings";
            let tbl = match lua.globals().get::<Value>("_NEOC_ARGS") {
                // Unset, nil, or unreadable → no args.
                Err(_) | Ok(Value::Nil) => return Ok(Vec::<String>::new()),
                Ok(Value::Table(tbl)) => tbl,
                Ok(_) => return Err(mlua::Error::runtime(ERR)),
            };
            tbl.sequence_values::<String>()
                .collect::<mlua::Result<Vec<String>>>()
                .map_err(|_| mlua::Error::runtime(ERR))
        })?,
    )?;

    Ok(m)
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
        assert_eq!(val.as_string().unwrap().to_str().unwrap(), "hello");
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
        let msg = err.as_string().unwrap().to_str().unwrap();
        assert!(msg.starts_with("env.var: _NEOC_DEFINITELY_MISSING_VAR_XYZ:"));
    }
}

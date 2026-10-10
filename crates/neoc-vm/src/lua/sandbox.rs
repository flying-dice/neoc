//! Engine factory + (value, err) helpers shared by every lib module.

use mlua::{IntoLua, Lua, StdLib, Value};

use crate::lua::modules;

/// Globals removed after the standard libraries load.
///
/// Each is a sandbox hazard: runtime code loading (`load`, `loadstring`,
/// `dofile`, `loadfile`, `string.dump`), environment/identity tampering
/// (`getfenv`, `setfenv`, `newproxy`, `module`), and GC control
/// (`collectgarbage`). Paths are dotted; the last segment is the key removed
/// from its parent table.
const STRIPPED_GLOBALS: &[&str] = &[
    "load",
    "loadstring",
    "dofile",
    "loadfile",
    "getfenv",
    "setfenv",
    "newproxy",
    "module",
    "collectgarbage",
    "string.dump",
    // Not loaded by StdLib selection, but `luaopen_base` and the LuaJIT
    // runtime may register them regardless — remove any that leak.
    "io",
    "os",
    "package",
    "jit",
    "ffi",
    "debug",
];

/// Build a fresh LuaJIT engine with our curated stdlib in place.
///
/// Only the language-semantics libraries load: TABLE/STRING/MATH/
/// BIT, plus BASE which `mlua` always opens along with `coroutine` (`pairs`, `ipairs`, `tostring`,
/// `pcall`, ...). IO/OS/PACKAGE/DEBUG/JIT/FFI never load — they are sandbox
/// hazards and our `fs`/`os`/`process` modules cover their legitimate uses.
/// The unsafe parts of BASE and STRING are then stripped
/// (see [`STRIPPED_GLOBALS`]) and the custom `require` is installed.
pub fn make_engine() -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT,
        mlua::LuaOptions::new(),
    )?;
    strip_unsafe_globals(&lua)?;
    lua.set_app_data(crate::lua::std::task::Scheduler::new());
    modules::install(&lua)?;
    Ok(lua)
}

fn strip_unsafe_globals(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    for path in STRIPPED_GLOBALS {
        match path.split_once('.') {
            None => globals.raw_set(*path, Value::Nil)?,
            Some((table, key)) => {
                if let Value::Table(t) = globals.raw_get::<Value>(table)? {
                    t.raw_set(key, Value::Nil)?;
                }
            }
        }
    }
    Ok(())
}

/// Convert a Rust `i64` to a Lua number.
///
/// LuaJIT numbers are IEEE doubles, so integers beyond 2^53 lose precision on
/// the way in; callers that need exact 64-bit values must pass strings.
pub fn i64_to_lua(v: i64) -> Value {
    Value::Number(v as f64)
}

/// Helper: build a `(value, nil)` success tuple.
pub fn ok<V: IntoLua>(lua: &Lua, v: V) -> mlua::Result<(Value, Value)> {
    Ok((v.into_lua(lua)?, Value::Nil))
}

/// Helper: build a `(nil, "msg")` error tuple.
pub fn err(lua: &Lua, msg: impl Into<String>) -> mlua::Result<(Value, Value)> {
    let s = lua.create_string(msg.into().as_bytes())?;
    Ok((Value::Nil, Value::String(s)))
}

/// Helper: turn an `anyhow`-ish stringifiable error into the `(nil, err)` tuple.
pub fn from_err<E: ::std::fmt::Display>(
    lua: &Lua,
    prefix: &str,
    e: E,
) -> mlua::Result<(Value, Value)> {
    err(lua, format!("{prefix}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Lua {
        make_engine().expect("engine")
    }

    /// `type(<path>)` evaluated inside the sandbox.
    fn type_of(lua: &Lua, path: &str) -> String {
        lua.load(format!("return type({path})"))
            .eval()
            .unwrap_or_else(|e| panic!("type({path}): {e}"))
    }

    #[test]
    fn given_fresh_engine_when_probing_unsafe_globals_then_each_is_nil() {
        let lua = engine();
        for path in STRIPPED_GLOBALS {
            assert_eq!(type_of(&lua, path), "nil", "{path} must be stripped");
        }
    }

    #[test]
    fn given_fresh_engine_when_probing_hazard_libraries_then_each_is_absent() {
        let lua = engine();
        for lib in ["io", "os", "package", "jit", "ffi", "debug"] {
            assert_eq!(type_of(&lua, lib), "nil", "{lib} must not be reachable");
        }
    }

    #[test]
    fn given_fresh_engine_when_probing_kept_libraries_then_each_is_a_table() {
        let lua = engine();
        for lib in ["coroutine", "table", "string", "math", "bit"] {
            assert_eq!(type_of(&lua, lib), "table", "{lib} must stay available");
        }
        for f in [
            "pairs",
            "ipairs",
            "pcall",
            "tostring",
            "setmetatable",
            "select",
        ] {
            assert_eq!(type_of(&lua, f), "function", "{f} must stay available");
        }
    }

    #[test]
    fn given_fresh_engine_when_requiring_a_module_then_custom_require_resolves_it() {
        let lua = engine();
        let ty: String = lua
            .load(r#"return type(require("lib:base64"))"#)
            .eval()
            .unwrap();
        assert_eq!(ty, "table");
    }

    #[test]
    fn given_fresh_engine_when_requiring_unknown_module_then_custom_require_errors() {
        let lua = engine();
        let err = lua
            .load(r#"return require("lib:no_such_module")"#)
            .eval::<Value>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("require: unknown module"), "got: {err}");
    }

    #[test]
    fn given_stripped_load_when_script_uses_it_then_call_fails() {
        let lua = engine();
        let res = lua.load(r#"return load("return 1")"#).eval::<Value>();
        assert!(res.is_err(), "load must be unreachable");
    }

    #[test]
    fn given_bit_library_when_xor_then_computes() {
        let lua = engine();
        let n: i64 = lua.load("return bit.bxor(5, 3)").eval().unwrap();
        assert_eq!(n, 6);
    }
}

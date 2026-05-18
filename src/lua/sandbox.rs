//! Engine factory + (value, err) helpers shared by every lib module.

use mlua::{IntoLua, Lua, StdLib, Value};

use crate::lua::modules;

/// Build a fresh Luau engine with our curated stdlib in place.
///
/// We deliberately drop most of Lua's own stdlib (io/os/package/debug) and
/// replace them with rust-flavoured modules. We keep BASE/COROUTINE/TABLE/
/// STRING/MATH/UTF8/BUFFER/VECTOR so language semantics stay intact —
/// `pairs`, `ipairs`, `tostring`, `string.format`, etc. are essential to write
/// idiomatic Luau even when the rust modules are the canonical surface.
pub fn make_engine() -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        // BASE is non-negotiable: pairs/ipairs/type/tostring/tonumber/select/
        // setmetatable/error/pcall/assert/next live there. Without it the
        // language is barely usable. We keep COROUTINE/TABLE/STRING/MATH/UTF8/
        // BUFFER/VECTOR as well — they're the parts of Lua's stdlib that don't
        // overlap with our rust-flavoured modules. We deliberately drop IO/OS/
        // PACKAGE/DEBUG: those are sandbox hazards and our `fs`/`os`/`process`
        // modules cover their legitimate uses.
        StdLib::ALL_SAFE,
        mlua::LuaOptions::new(),
    )?;
    lua.set_app_data(crate::lua::std::task::Scheduler::new());
    modules::install(&lua)?;
    Ok(lua)
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

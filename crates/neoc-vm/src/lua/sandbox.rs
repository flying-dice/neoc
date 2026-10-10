//! Engine factory + (value, err) helpers shared by every lib module.

use mlua::{AsChunk, Chunk, ChunkMode, IntoLua, Lua, StdLib, Value};

use crate::lua::modules;

/// Globals removed after the standard libraries load.
///
/// Each is a sandbox hazard: runtime code loading (`load`, `loadstring`,
/// `dofile`, `loadfile`, `string.dump`), environment/identity tampering
/// (`getfenv`, `setfenv`, `newproxy`, `module`), and GC control and
/// introspection (`collectgarbage`, `gcinfo`). Paths are dotted; the last
/// segment is the key removed from its parent table.
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
    "gcinfo",
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

/// Load script source as a text-only chunk.
///
/// LuaJIT bytecode is unverified — a crafted `\x1bLJ` chunk corrupts memory
/// and escapes the sandbox — so every entry point that runs script-supplied
/// source goes through here and refuses binary chunks.
#[track_caller]
pub fn load_source<'a>(lua: &Lua, source: impl AsChunk<'a>) -> Chunk<'a> {
    lua.load(source).set_mode(ChunkMode::Text)
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

/// Stripped LuaJIT bytecode for `return 7` — no debug info or chunk name, so
/// every byte is ASCII and it survives the `String` conversion script-facing
/// loaders apply. The escape-vector fixture for the text-only loader tests.
///
/// `string.dump(f, true)` in an unsandboxed state: `Function::dump(true)` does
/// not strip under `LuaJIT`.
#[cfg(test)]
pub(crate) fn ascii_bytecode() -> String {
    let lua = Lua::new();
    let dump: mlua::String = lua
        .load("return string.dump(function() return 7 end, true)")
        .eval()
        .expect("dump");
    let dump = dump.as_bytes().to_vec();
    // Header: `\x1bLJ`, version, flags; `BCDUMP_F_STRIP` = 0x02.
    assert_eq!(dump[4] & 0x02, 0x02, "dump must be stripped");
    String::from_utf8(dump).expect("stripped dump of `return 7` is ASCII")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const TWO_53: i64 = 1 << 53;

    fn engine() -> Lua {
        make_engine().expect("engine")
    }

    /// `name -> type(value)` for every key of the table at `path`.
    fn surface(lua: &Lua, path: &str) -> BTreeMap<String, String> {
        lua.load(format!(
            "local t = {{}} for k, v in pairs({path}) do t[k] = type(v) end return t"
        ))
        .eval()
        .unwrap_or_else(|e| panic!("surface({path}): {e}"))
    }

    fn expected(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(k, t)| ((*k).to_string(), (*t).to_string()))
            .collect()
    }

    // Allowlists, not denylists: a global or string function a LuaJIT/mlua
    // bump adds fails here until someone decides whether it is safe.
    #[test]
    fn given_fresh_engine_when_enumerating_globals_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "_G"),
            expected(&[
                // Libraries.
                ("_G", "table"),
                ("bit", "table"),
                ("coroutine", "table"),
                ("math", "table"),
                ("string", "table"),
                ("table", "table"),
                // Base functions kept after stripping.
                ("assert", "function"),
                ("error", "function"),
                ("getmetatable", "function"),
                ("ipairs", "function"),
                ("next", "function"),
                ("pairs", "function"),
                ("pcall", "function"),
                ("print", "function"),
                ("rawequal", "function"),
                ("rawget", "function"),
                ("rawset", "function"),
                ("select", "function"),
                ("setmetatable", "function"),
                ("tonumber", "function"),
                ("tostring", "function"),
                ("type", "function"),
                ("unpack", "function"),
                ("xpcall", "function"),
                ("_VERSION", "string"),
                // The custom module resolver.
                ("require", "function"),
            ])
        );
    }

    #[test]
    fn given_fresh_engine_when_enumerating_string_library_then_exactly_the_allowlist_remains() {
        let lua = engine();
        assert_eq!(
            surface(&lua, "string"),
            expected(&[
                ("byte", "function"),
                ("char", "function"),
                ("find", "function"),
                ("format", "function"),
                ("gmatch", "function"),
                ("gsub", "function"),
                ("len", "function"),
                ("lower", "function"),
                ("match", "function"),
                ("rep", "function"),
                ("reverse", "function"),
                ("sub", "function"),
                ("upper", "function"),
            ])
        );
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
    fn given_bytecode_when_loaded_as_source_then_rejected() {
        let lua = engine();
        let source = ascii_bytecode();
        assert!(
            source.starts_with("\x1bLJ"),
            "fixture must be LuaJIT bytecode"
        );
        let err = load_source(&lua, &source)
            .eval::<Value>()
            .expect_err("bytecode must not load")
            .to_string();
        assert!(err.contains("binary"), "got: {err}");
    }

    #[test]
    fn given_text_when_loaded_as_source_then_runs() {
        let lua = engine();
        let n: i64 = load_source(&lua, "return 7").eval().unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn given_integers_up_to_2_pow_53_when_converted_then_exact() {
        for v in [0, 1, -1, -42, TWO_53, -TWO_53, i64::MIN] {
            let Value::Number(n) = i64_to_lua(v) else {
                panic!("{v}: expected a number");
            };
            assert_eq!(n as i64, v, "{v} must round-trip");
        }
    }

    #[test]
    fn given_integers_beyond_2_pow_53_when_converted_then_rounded_to_nearest_double() {
        for (v, want) in [
            (TWO_53 + 1, TWO_53 as f64),
            (-(TWO_53 + 1), -(TWO_53 as f64)),
            (i64::MAX, 9_223_372_036_854_775_808.0),
        ] {
            assert_eq!(i64_to_lua(v), Value::Number(want), "{v}");
        }
    }

    #[test]
    fn given_bit_library_when_xor_then_computes() {
        let lua = engine();
        let n: i64 = lua.load("return bit.bxor(5, 3)").eval().unwrap();
        assert_eq!(n, 6);
    }
}

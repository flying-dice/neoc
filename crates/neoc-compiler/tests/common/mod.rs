//! Shared test support: an in-process `LuaJIT` runner for emitted Lua.
//!
//! Emitted modules run on the same engine family the `neoc` runtime embeds, so a
//! program that passes here is valid `LuaJIT` (Lua 5.1) — no external interpreter
//! on `PATH`, and no trial is ever skipped for lack of one.

use std::sync::{Arc, Mutex};

use mlua::{Lua, MultiValue, Value};

/// Run `program` (a Lua 5.1 chunk) under `LuaJIT` and return everything it
/// `print`ed, tab-separated per call and newline-terminated, as `print` renders it.
///
/// # Errors
///
/// Returns the engine's message when the chunk fails to load or raises.
pub fn run_lua(program: &str) -> Result<String, String> {
    let lua = Lua::new();
    let out = Arc::new(Mutex::new(String::new()));
    let sink = Arc::clone(&out);
    let print = lua
        .create_function(move |lua, args: MultiValue| {
            let tostring: mlua::Function = lua.globals().get("tostring")?;
            let mut line = Vec::with_capacity(args.len());
            for arg in args {
                let text: Value = tostring.call(arg)?;
                line.push(text.to_string()?);
            }
            let mut buf = sink.lock().expect("print sink lock");
            buf.push_str(&line.join("\t"));
            buf.push('\n');
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    lua.globals()
        .set("print", print)
        .map_err(|e| e.to_string())?;
    lua.load(program)
        .set_name("=conformance")
        .exec()
        .map_err(|e| e.to_string())?;
    let text = out.lock().expect("print sink lock").clone();
    Ok(text)
}

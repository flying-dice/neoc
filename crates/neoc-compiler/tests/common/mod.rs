//! Shared test support: an in-process `LuaJIT` runner for emitted Lua.
//!
//! Emitted modules run in the `neoc` runtime's own sandboxed engine
//! ([`sandbox::make_engine`]) — same curated stdlib, same stripped globals — so a
//! program that passes here runs under `neoc run`. No external interpreter on
//! `PATH`, and no trial is ever skipped for lack of one.

use std::sync::{Arc, Mutex};

use mlua::{MultiValue, Value};
use neoc_vm::lua::sandbox;

/// What a program printed, and how its run ended.
pub struct LuaRun {
    /// Everything the program `print`ed before it finished or raised.
    pub stdout: String,
    /// The engine's message when the chunk raised; `None` when it ran to
    /// completion.
    pub raised: Option<String>,
}

/// Run `program` (a Lua 5.1 chunk) in the runtime sandbox and return everything
/// it `print`ed, tab-separated per call and newline-terminated, as `print`
/// renders it.
///
/// # Errors
///
/// Returns the engine's message when the chunk fails to load or raises.
#[allow(
    dead_code,
    reason = "shared across test binaries; conformance calls only run_lua_capturing"
)]
pub fn run_lua(program: &str) -> Result<String, String> {
    let run = run_lua_capturing(program)?;
    match run.raised {
        Some(message) => Err(message),
        None => Ok(run.stdout),
    }
}

/// Run `program` like [`run_lua`], but keep what it printed when it raises, so
/// a caller can pin the output produced before a rejection.
///
/// # Errors
///
/// Returns the engine's message when the sandbox cannot be built or the chunk
/// fails to load — never for a raise, which lands in [`LuaRun::raised`].
pub fn run_lua_capturing(program: &str) -> Result<LuaRun, String> {
    let lua = sandbox::make_engine().map_err(|e| e.to_string())?;
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
    let chunk = sandbox::load_source(&lua, program)
        .set_name("=conformance")
        .into_function()
        .map_err(|e| e.to_string())?;
    let raised = chunk.call::<()>(()).err().map(|e| e.to_string());
    let stdout = out.lock().expect("print sink lock").clone();
    Ok(LuaRun { stdout, raised })
}

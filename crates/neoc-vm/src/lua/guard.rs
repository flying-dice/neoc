//! Running script code from Rust so that no error value can abort the process.
//!
//! When a thread the host resumes (`Function::call_async`, `Chunk::exec_async`, `Thread::resume`, a `std:task`
//! task) dies of an error, mlua turns the error value into text with `luaL_tolstring`, which runs the value's
//! `__tostring`, under a message handler of its own. On LuaJIT a handler that has made a protected call of its
//! own (a `__tostring` that calls `pcall`) and then fails is run again by its own failure, until the C stack is
//! gone and the process aborts (`guarded_xpcall.lua` and `guarded_call.lua` close the same recursion for `xpcall` and a
//! synchronous host call).
//!
//! The fix is that the error never leaves the thread as a value with a metatable. A script function is run
//! through [`call_guarded_async`] (or, for a task, as a thread whose body is `xpcall`), under the
//! [`error_report`] below, and the thread *returns* `false` and the error instead of raising. An error that is
//! already a Rust error (a host function failed, mlua's `WrappedFailure`) is handed on as it is, so what the
//! caller can look into is what it always was. Anything else is turned into text here, with the traceback of the
//! point of the error, made while the failing stack was still there.

use mlua::{Error, FromLuaMulti, Function, IntoLuaMulti, Lua, MultiValue, Table, Thread, Value};

use crate::lua::sandbox::load_source;

/// What the guarded threads of an engine are made of, kept in its app data.
struct Parts {
    /// The builtin `xpcall`: the body of a thread that runs a function.
    xpcall: Function,
    /// The message handler `xpcall` is given (`handler` of `guarded_thread.lua`, which calls [`error_report`]).
    handler: Function,
    /// `convert` of `guarded_thread.lua`: the text of an error value.
    convert: Function,
    /// `drive` of `guarded_thread.lua`: the body of a thread that runs a coroutine of the script's own.
    drive: Function,
}

/// Build the parts of this engine. Called while the engine is built, before a script runs, so that the builtins
/// they use are the real ones whatever a script does to `_G` later, and before the engine's `xpcall` is replaced
/// (a guarded thread wants the builtin: its handler is a host function that cannot fail the way a script's can).
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let coroutine: Table = globals.get("coroutine")?;
    let (handler, convert, drive): (Function, Function, Function) =
        load_source(lua, include_str!("guarded_thread.lua"))
            .set_name("=guarded_thread")
            .call((
                lua.create_function(error_report)?,
                globals.get::<Value>("pcall")?,
                globals.get::<Value>("tostring")?,
                globals.get::<Value>("type")?,
                globals.get::<Value>("select")?,
                globals.get::<Value>("unpack")?,
                coroutine.get::<Value>("resume")?,
                coroutine.get::<Value>("yield")?,
                coroutine.get::<Value>("status")?,
            ))?;
    lua.set_app_data(Parts {
        xpcall: globals.get("xpcall")?,
        handler,
        convert,
        drive,
    });
    Ok(())
}

fn parts(lua: &Lua) -> mlua::Result<mlua::AppDataRef<'_, Parts>> {
    lua.app_data_ref::<Parts>().ok_or_else(|| {
        Error::runtime("this engine was not built by neoc-vm: it has no guarded thread parts")
    })
}

/// Call a script function from an async context, with `args`, so that an error it raises can never abort the
/// process, whatever the error value is. This is the entry point for a program that embeds the VM and runs script
/// code with `await` (a tool, a handler, a request): use it instead of `Function::call_async`, `Chunk::exec_async`
/// or `Chunk::eval_async`. [`call_guarded`](crate::lua::sandbox::call_guarded) is the synchronous counterpart.
///
/// What the caller gets back is what a plain `call_async` would give it. A failure that is a Rust error (a host
/// function failed) is the same `mlua::Error` as ever, to downcast. Any other failure is an
/// `mlua::Error::RuntimeError` whose text is the error value converted the way mlua does it (running its
/// `__tostring`), followed by the traceback of the point of the error. An error value whose `__tostring` fails is
/// reported as `error object's __tostring failed: <reason>`, with the traceback, not as a crash.
///
/// # Errors
/// The function raised.
pub async fn call_guarded_async<R: FromLuaMulti>(
    lua: &Lua,
    function: &Function,
    args: impl IntoLuaMulti,
) -> mlua::Result<R> {
    let (xpcall, handler) = {
        let parts = parts(lua)?;
        (parts.xpcall.clone(), parts.handler.clone())
    };
    let mut all = args.into_lua_multi(lua)?;
    all.push_front(Value::Function(handler));
    all.push_front(Value::Function(function.clone()));
    let returned: MultiValue = xpcall.call_async(all).await?;
    R::from_lua_multi(into_call_result(lua, returned)?, lua)
}

/// A thread for a `std:task` task: a host thread whose body runs the function (under `xpcall`, with the handler)
/// or the coroutine of the script's own that the task was given.
pub(crate) struct Guarded {
    thread: Thread,
    /// What the body is first given: the function or coroutine to run.
    target: Value,
    /// The message handler, for the body that is `xpcall`.
    handler: Option<Function>,
}

impl Guarded {
    /// A host thread that runs `function`.
    pub(crate) fn function(lua: &Lua, function: Function) -> mlua::Result<Self> {
        let parts = parts(lua)?;
        Ok(Self {
            thread: lua.create_thread(parts.xpcall.clone())?,
            target: Value::Function(function),
            handler: Some(parts.handler.clone()),
        })
    }

    /// A host thread that drives a coroutine the script made.
    pub(crate) fn coroutine(lua: &Lua, coroutine: Thread) -> mlua::Result<Self> {
        let parts = parts(lua)?;
        Ok(Self {
            thread: lua.create_thread(parts.drive.clone())?,
            target: Value::Thread(coroutine),
            handler: None,
        })
    }

    /// The thread, and the values to resume it with the first time: what the body is given, then the task's own
    /// `args`.
    pub(crate) fn into_parts(self, mut args: MultiValue) -> (Thread, MultiValue) {
        if let Some(handler) = self.handler {
            args.push_front(Value::Function(handler));
        }
        args.push_front(self.target);
        (self.thread, args)
    }
}

/// What the body of a guarded thread answered, as the result a plain call would have given: `true` and the
/// values, or `false` and the error value (or the answer of [`error_report`] for it).
pub(crate) fn into_call_result(lua: &Lua, mut returned: MultiValue) -> mlua::Result<MultiValue> {
    match returned.pop_front() {
        Some(Value::Boolean(true)) => Ok(returned),
        _ => Err(failure(lua, returned.pop_front())),
    }
}

/// The error a guarded thread's failure stands for.
fn failure(lua: &Lua, raised: Option<Value>) -> Error {
    let (value, traceback) = match raised {
        // a Rust error, handed on as it is: the one to look into
        Some(Value::Error(error)) => return without_base_frame(*error),
        Some(Value::Table(answer)) => (
            answer.raw_get::<Value>(1).unwrap_or(Value::Nil),
            answer.raw_get::<Option<String>>(2).unwrap_or(None),
        ),
        // the handler never ran (out of memory, out of stack), or a coroutine of the script's own failed
        Some(other) => (other, None),
        None => (Value::Nil, None),
    };
    let mut text = match value {
        Value::String(text) => text.to_string_lossy(),
        other => convert(lua, other),
    };
    // mlua reads the text as a C string: it ends at the first NUL
    if let Some(end) = text.find('\0') {
        text.truncate(end);
    }
    if let Some(traceback) = traceback {
        text.push('\n');
        text.push_str(&traceback);
    }
    Error::runtime(text)
}

/// The text of an error value that is not a string, as `luaL_tolstring` makes it, except that a `__tostring` that
/// fails is reported as that: it is run by `convert` in Lua, which it cannot fail.
fn convert(lua: &Lua, value: Value) -> String {
    if let Value::Table(table) = &value
        && let Some(metatable) = table.metatable()
        && matches!(metatable.raw_get::<Value>("__tostring"), Ok(Value::Nil))
    {
        // `luaL_tolstring` names the value after its metatable's `__name`; `tostring` does not
        let name = match metatable.raw_get::<Value>("__name") {
            Ok(Value::String(name)) => name.to_string_lossy(),
            _ => value.type_name().to_owned(),
        };
        return format!("{name}: {:p}", value.to_pointer());
    }
    let converted = parts(lua)
        .map(|parts| parts.convert.clone())
        .and_then(|convert| convert.call::<mlua::LuaString>(value));
    match converted {
        Ok(text) => text.to_string_lossy(),
        Err(_) => "error object's __tostring failed".to_owned(),
    }
}

// ---- tracebacks -------------------------------------------------------------------------------------------------

/// How many frames mlua's traceback prints before and after the gap in a deep stack (`COMPAT53_LEVELS1` and
/// `COMPAT53_LEVELS2` of mlua-sys, which supplies `luaL_traceback` on LuaJIT); a traceback of more than
/// `HEAD + TAIL + 1` frames has the gap. (`tests/guarded_errors.rs` compares every depth around it: a bump of
/// mlua-sys that changes the numbers fails there.)
const HEAD: usize = 10;
const TAIL: usize = 11;

/// What a traceback prints for the frames it leaves out: `...\t(skipping N levels)`.
const GAP: &str = "...\t(skipping";

/// The frames under the function a guarded thread runs: `xpcall`, the body of the thread.
const BASE_FRAMES: usize = 1;

/// Runs at the point of the error, called by the message handler of `xpcall` with the stack of the failing code
/// still there, for an error value that is not a userdata. It calls nothing of the script's, so it cannot fail the
/// way a `__tostring` can. It answers `{ value, traceback }` for [`failure`] to make the text from.
fn error_report(lua: &Lua, raised: Value) -> mlua::Result<Value> {
    let answer = lua.create_table_with_capacity(2, 0)?;
    answer.raw_set(1, raised)?;
    if let Some(traceback) = traceback(lua) {
        answer.raw_set(2, traceback)?;
    }
    Ok(Value::Table(answer))
}

/// The text mlua gives an error that left a thread (`luaL_traceback` of the dead thread), made now, at the error,
/// without the frame of the thread body, and printed as `luaL_traceback` prints it. A deep stack makes the traceback
/// leave its middle out, and the frame of the thread body is the last of it: the gap is closed again, with at most
/// one more traceback, for the frames that come into view or leave it.
fn traceback(lua: &Lua) -> Option<String> {
    // level 0 is this function (the message handler tail-calls it), level 1 the function that raised
    let first = page(lua, 1)?;
    let mut frames = match gap_in(&first) {
        None => first,
        Some(skipped) => {
            let with_base = HEAD + skipped + TAIL;
            let without_base = with_base - BASE_FRAMES;
            if without_base <= HEAD + TAIL + 1 {
                // the thread body's frame makes the gap, but without it the traceback has none: all of it
                let mut all = first[..HEAD].to_vec();
                all.extend(page(lua, 1 + HEAD)?);
                all
            } else {
                // the gap ends at frames that are the thread body's now: the frames before them, from the gap, are
                // the first of the end of the traceback
                let from_gap = page(lua, 1 + with_base - TAIL - BASE_FRAMES)?;
                let mut frames = first[..HEAD].to_vec();
                frames.push(gap(without_base - HEAD - TAIL));
                frames.extend(from_gap.into_iter().take(BASE_FRAMES));
                frames.extend(first[HEAD + 1..].iter().cloned());
                frames
            }
        }
    };
    frames.truncate(frames.len().saturating_sub(BASE_FRAMES));
    if let Some(first) = frames.first_mut()
        && let Some(without_line) = without_line(lua, first)
    {
        *first = without_line;
    }
    let mut out = String::from("stack traceback:");
    for frame in frames {
        out.push_str("\n\t");
        out.push_str(&frame);
    }
    Some(out)
}

/// The frames, from `level` down, that mlua's own traceback prints: all of them, or a gap (as a line of its own)
/// between the first `HEAD` and the last `TAIL`.
fn page(lua: &Lua, level: usize) -> Option<Vec<String>> {
    let page = lua.traceback(None, level).ok()?;
    let page = page.to_str().ok()?;
    let mut lines = page.split("\n\t");
    lines.next(); // "stack traceback:"
    Some(lines.map(str::to_owned).collect())
}

/// The line that stands for `levels` frames a traceback leaves out.
fn gap(levels: usize) -> String {
    format!("{GAP} {levels} levels)")
}

/// How many frames the page leaves out, if it has a gap.
fn gap_in(frames: &[String]) -> Option<usize> {
    frames
        .get(HEAD)?
        .strip_prefix(GAP)?
        .trim_start()
        .strip_suffix(" levels)")?
        .parse()
        .ok()
}

/// The first frame of a traceback taken at the error, without its line when the function that failed is a Lua
/// one: the thread mlua takes the traceback of is dead by then, and a dead thread cannot say what line a Lua
/// function was at (the error was not a call it made).
fn without_line(lua: &Lua, first: &str) -> Option<String> {
    lua.inspect_stack(1, |frame| {
        let line = frame.current_line()?;
        let source = frame.source();
        let short = source.short_src?;
        if source.what == "C" {
            return None;
        }
        first
            .strip_prefix(&format!("{short}:{line}:"))
            .map(|rest| format!("{short}:{rest}"))
    })
    .flatten()
}

/// What a frame of the thread body looks like in a traceback: a C function nothing called (`xpcall`, the body of the
/// thread).
const BASE_FRAME: &str = "[C]: in ?";

/// A Rust error that was raised inside a guarded thread carries the traceback mlua took at the time (a
/// `CallbackError`), which ends with the frame of the thread body: leave it out, so that the traceback is the one
/// the same call would have had in a thread of its own. (In a deep stack the traceback has a gap, and the frame
/// that would now be the first of its last part is in the gap: it stays there, counted in the levels skipped.)
fn without_base_frame(error: Error) -> Error {
    match error {
        Error::CallbackError { traceback, cause } => {
            let kept = match traceback.rsplit_once("\n\t") {
                Some((rest, BASE_FRAME)) => rest.to_owned(),
                _ => traceback,
            };
            Error::CallbackError {
                traceback: kept,
                cause,
            }
        }
        other => other,
    }
}

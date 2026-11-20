//! Running script code from Rust so that an error value cannot abort the process.
//!
//! When a thread the host resumes (`Function::call_async`, `Chunk::exec_async`, `Thread::resume`, a `std:task`
//! task) dies of an error, mlua turns the error value into text with `luaL_tolstring`, which runs the value's
//! `__tostring`, under a message handler of its own. On LuaJIT a handler that has made a protected call of its
//! own (a `__tostring` that calls `pcall`) and then fails is run again by its own failure, until the C stack is
//! gone and the process aborts (`guarded_xpcall.lua` closes the same recursion for the `xpcall` a script calls).
//!
//! The fix is that the error never leaves the thread as a value with a metatable. A script function is run
//! through [`call_guarded_async`], or [`call_guarded`] when the call is synchronous (or, for a task, as a thread
//! whose body is `xpcall`), under a message handler that takes the traceback at the point of the error, and the
//! thread *returns* `false` and the error instead of raising. An error that is already a Rust error (a host function
//! failed, mlua's `WrappedFailure`) is handed on as it is, so what the caller can look into is what it always was.
//! Anything else is turned into text here, with the traceback of the point of the error, made while the failing
//! stack was still there.
//!
//! **Known limit (NEOC-123).** This guards where the *script's own* code raises. It does not guard a host function that
//! reads a script value from Rust through that value's metamethods (`Table::get` on a table with an `__index` that
//! raises, a conversion of a table whose `__len` raises, a handler's return value read by `vnd:hyper`): mlua wraps
//! each of those Lua API calls in a protected call whose message handler runs the error value's `__tostring`, and the
//! same recursion aborts the process. Closing that takes a change inside mlua.

use mlua::{Error, FromLuaMulti, Function, IntoLuaMulti, Lua, MultiValue, Table, Thread, Value};
use parking_lot::Mutex;

use crate::lua::sandbox::load_source;

/// What the guarded threads of an engine are made of, kept in its app data.
struct Parts {
    /// The builtin `xpcall`: the body of a thread that runs a function.
    xpcall: Function,
    /// The message handler `xpcall` is given (`handler` of `guarded_thread.lua`, which calls [`error_report`]).
    handler: Function,
    /// `convert` of `guarded_thread.lua`: the text of a `nil`, a boolean or a number.
    convert: Function,
    /// `callmeta` of `guarded_thread.lua`: the text of a value from its `__tostring`, which cannot fail the host.
    callmeta: Function,
    /// `drive` of `guarded_thread.lua`: the body of a thread that runs a coroutine of the script's own.
    drive: Function,
}

/// The synchronous guarded calls that are running: for each, the thread it runs in and how many frames of that
/// thread are under its `xpcall`, so that the traceback of an error in it can leave that one frame out.
#[derive(Default)]
struct SyncCalls(Mutex<Vec<(usize, usize)>>);

/// Build the parts of this engine. Called while the engine is built, before a script runs, so that the builtins
/// they use are the real ones whatever a script does to `_G` later, and before the engine's `xpcall` is replaced
/// (a guarded thread wants the builtin: its handler is a host function that cannot fail the way a script's can).
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let coroutine: Table = globals.get("coroutine")?;
    let (handler, convert, callmeta, drive): (Function, Function, Function, Function) =
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
        callmeta,
        drive,
    });
    lua.set_app_data(SyncCalls::default());
    Ok(())
}

fn parts(lua: &Lua) -> mlua::Result<mlua::AppDataRef<'_, Parts>> {
    lua.app_data_ref::<Parts>().ok_or_else(|| {
        Error::runtime("this engine was not built by neoc-vm: it has no guarded thread parts")
    })
}

/// Call a script function from an async context, with `args`, so that an error it raises cannot abort the process,
/// whatever the error value is. This is the entry point for a program that embeds the VM and runs script code with
/// `await` (a tool, a handler, a request): use it instead of `Function::call_async`, `Chunk::exec_async` or
/// `Chunk::eval_async` (turn a chunk into a function with `Chunk::into_function` first: that is the statement
/// form; an expression such as `"1 + 1"` is not a chunk to `exec_async` either, only to `eval_async`).
/// [`call_guarded`] is the synchronous counterpart.
///
/// What the caller gets back is what a plain `call_async` would give it, with the exceptions listed under "Calling
/// script code from Rust" in CLAUDE.md. A failure that is a Rust error (a host function failed) is the same
/// `mlua::Error` as ever, to downcast; an out-of-memory error is `mlua::Error::MemoryError`; any other failure is an
/// `mlua::Error::RuntimeError` whose text is the error value converted the way mlua does it (running its
/// `__tostring`), followed by the traceback of the point of the error. An error value whose `__tostring` fails is
/// reported as `error object's __tostring failed`, followed by `: <reason>` when the reason is text, and then the
/// traceback, not as a crash. Nothing here guards a host function that reads a script value through its
/// metamethods (see the module doc).
///
/// # Errors
/// The function raised, or the engine was not built by [`make_engine`](crate::lua::sandbox::make_engine),
/// `make_engine_in` or `make_bare_engine` (it has nothing to guard with).
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
    R::from_lua_multi(into_call_result(lua, returned, false)?, lua)
}

/// Call a script function from Rust, synchronously, with `args`, so that an error it raises cannot abort the process,
/// whatever the error value is. Use this, not `Function::call`, wherever a host function calls a callback the script
/// gave it. It is not for a call that may suspend: that is [`call_guarded_async`].
///
/// What the caller gets back is what a plain `Function::call` would give it, with the same exceptions as
/// [`call_guarded_async`] (and the traceback of a Rust error raised inside has one more frame, the `xpcall` the
/// function runs under).
///
/// # Errors
/// The function raised, or the engine was not built by neoc-vm.
pub fn call_guarded<R: FromLuaMulti>(
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
    // the frames under the `xpcall` this call adds: the traceback of an error leaves that one out
    let entry = SyncEntry::push(lua, levels(lua));
    let returned: MultiValue = xpcall.call(all)?;
    drop(entry);
    R::from_lua_multi(into_call_result(lua, returned, true)?, lua)
}

/// A running synchronous guarded call, in the list the message handler reads; removed when it is dropped.
struct SyncEntry<'a>(&'a Lua);

impl<'a> SyncEntry<'a> {
    fn push(lua: &'a Lua, below: usize) -> Self {
        if let Some(calls) = lua.app_data_ref::<SyncCalls>() {
            calls.0.lock().push((thread_id(lua), below));
        }
        Self(lua)
    }
}

impl Drop for SyncEntry<'_> {
    fn drop(&mut self) {
        if let Some(calls) = self.0.app_data_ref::<SyncCalls>() {
            calls.0.lock().pop();
        }
    }
}

/// Which thread is running.
fn thread_id(lua: &Lua) -> usize {
    lua.current_thread().to_pointer() as usize
}

/// How many frames the running thread has.
fn levels(lua: &Lua) -> usize {
    let exists = |level: usize| lua.inspect_stack(level, |_| ()).is_some();
    let mut high = 1;
    while exists(high) {
        high *= 2;
    }
    let mut low = high / 2;
    // `low` exists (or is 0), `high` does not: find the first level that does not
    while low + 1 < high {
        let middle = usize::midpoint(low, high);
        if exists(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
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
pub(crate) fn into_call_result(
    lua: &Lua,
    mut returned: MultiValue,
    sync: bool,
) -> mlua::Result<MultiValue> {
    match returned.pop_front() {
        Some(Value::Boolean(true)) => Ok(returned),
        _ => Err(failure(lua, returned.pop_front(), sync)),
    }
}

/// The error a guarded thread's failure stands for. `sync` is whether the call ran under the `xpcall` of a
/// synchronous call (in the middle of a thread) and not as the body of a thread.
fn failure(lua: &Lua, raised: Option<Value>, sync: bool) -> Error {
    let (value, traceback) = match raised {
        // a Rust error, handed on as it is: the one to look into
        Some(Value::Error(error)) => return without_base_frame(*error, sync),
        Some(Value::Table(answer)) => (
            answer.raw_get::<Value>(1).unwrap_or(Value::Nil),
            answer.raw_get::<Option<String>>(2).unwrap_or(None),
        ),
        // LuaJIT runs no handler for an out-of-memory error: the message is all there is, and the error is
        // the one a plain call gives
        Some(Value::String(message)) if message.as_bytes() == b"not enough memory" => {
            return Error::MemoryError("not enough memory".to_owned());
        }
        // the handler never ran (out of stack) or failed, or a host userdata that was raised as a value
        Some(other) => (other, None),
        None => (Value::Nil, None),
    };
    let mut text = describe(lua, value);
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

/// The text of an error value, as `luaL_tolstring` makes it: a string as it is; a value with a `__tostring` from
/// that, run by `callmeta` in Lua, which a failing `__tostring` cannot take the host down with; any other value by its
/// type name (or the `__name` of its metatable) and address.
fn describe(lua: &Lua, value: Value) -> String {
    let metatable_field = |name: &str| -> Option<Value> {
        let field = match &value {
            Value::Table(table) => table.metatable()?.raw_get::<Value>(name).ok()?,
            Value::UserData(data) => data.metatable().ok()?.get::<Value>(name).ok()?,
            _ => return None,
        };
        (!matches!(field, Value::Nil)).then_some(field)
    };
    let described = match &value {
        Value::String(text) => return text.to_string_lossy(),
        Value::Nil | Value::Boolean(_) | Value::Integer(_) | Value::Number(_) => {
            parts(lua).and_then(|parts| parts.convert.call::<mlua::LuaString>(value.clone()))
        }
        _ => match metatable_field("__tostring") {
            Some(tostring) => parts(lua).and_then(|parts| {
                parts
                    .callmeta
                    .call::<mlua::LuaString>((tostring, value.clone()))
            }),
            None => {
                let name = match metatable_field("__name") {
                    Some(Value::String(name)) => name.to_string_lossy(),
                    _ => value.type_name().to_owned(),
                };
                return format!("{name}: {:p}", value.to_pointer());
            }
        },
    };
    match described {
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

/// Runs at the point of the error, called by the message handler of `xpcall` with the stack of the failing code
/// still there, for an error value that is not a userdata. It calls nothing of the script's, so it cannot fail the
/// way a `__tostring` can. It answers `{ value, traceback }` for [`failure`] to make the text from.
fn error_report(lua: &Lua, raised: Value) -> mlua::Result<Value> {
    // inside a synchronous guarded call of this thread, the `xpcall` is in the middle of the stack: the frames under it
    // are the caller's. As the body of a thread it is the last frame.
    let sync_below = lua.app_data_ref::<SyncCalls>().and_then(|calls| {
        let me = thread_id(lua);
        calls
            .0
            .lock()
            .iter()
            .rev()
            .find(|(thread, _)| *thread == me)
            .map(|(_, below)| *below)
    });
    // an overflow of the stack is raised by LuaJIT itself, in the function that called: a dead thread still has the
    // line of it
    let overflow = matches!(&raised, Value::String(text) if text.as_bytes() == b"stack overflow");
    let answer = lua.create_table_with_capacity(2, 0)?;
    answer.raw_set(1, raised)?;
    if let Some(traceback) = traceback(lua, sync_below, overflow) {
        answer.raw_set(2, traceback)?;
    }
    Ok(Value::Table(answer))
}

/// The text mlua gives an error (`luaL_traceback`), made now, at the error, without the frame of the `xpcall` the
/// function runs under, and printed as `luaL_traceback` prints it. A deep stack makes the traceback leave its middle
/// out; the gap is closed again where the left-out frame comes into view, with at most two more tracebacks.
///
/// `below` is how many frames are under that `xpcall` (a synchronous call: the caller's frames), or `None` when it is
/// the body of the thread (nothing is under it, and mlua takes its traceback from a dead thread: no line for the
/// Lua function that failed, unless `keep_line`).
fn traceback(lua: &Lua, below: Option<usize>, keep_line: bool) -> Option<String> {
    // level 0 is this function (the message handler tail-calls it), level 1 the function that raised. A synchronous call
    // is taken by mlua from inside its message handler, a C function that is the first frame of the traceback; the
    // traceback of a dead thread starts at the function that raised.
    let start = usize::from(below.is_none());
    let first = page(lua, start)?;
    let total = match gap_in(&first) {
        None => first.len(),
        Some(skipped) => HEAD + skipped + TAIL,
    };
    // the frame of the `xpcall`, counted from the top
    let removed = total.saturating_sub(1 + below.unwrap_or(0));
    let kept = total - 1;
    // the frames to show of the stack without it: all of them, or the first `HEAD` and the last `TAIL`
    let shown: Vec<usize> = if kept <= HEAD + TAIL + 1 {
        (0..kept).collect()
    } else {
        (0..HEAD).chain(kept - TAIL..kept).collect()
    };
    let mut frames = Vec::with_capacity(shown.len() + 1);
    for (position, index) in shown.iter().enumerate() {
        if position == HEAD && kept > HEAD + TAIL + 1 {
            frames.push(gap(kept - HEAD - TAIL));
        }
        let original = if *index < removed { *index } else { index + 1 };
        // the first page has the first `HEAD` frames and the last `TAIL`; any other is asked for
        let line = if original < HEAD || (original < first.len() && gap_in(&first).is_none()) {
            first.get(original).cloned()
        } else if gap_in(&first).is_some() && original >= total - TAIL {
            first.get(HEAD + 1 + original - (total - TAIL)).cloned()
        } else {
            page(lua, start + original)?.into_iter().next()
        };
        frames.push(line?);
    }
    if !keep_line
        && below.is_none()
        && let Some(first) = frames.first_mut()
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
/// between the first `HEAD` and the last `TAIL`. A frame that is not UTF-8 is read lossily, as the text of an error
/// is.
fn page(lua: &Lua, level: usize) -> Option<Vec<String>> {
    let page = lua.traceback(None, level).ok()?;
    let page = page.to_string_lossy();
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
/// that would now be the first of its last part is in the gap: it stays there, counted in the levels skipped.) Under
/// the `xpcall` of a synchronous call (`sync`) the frame is in the middle, among the caller's, and stays.
fn without_base_frame(error: Error, sync: bool) -> Error {
    match error {
        Error::CallbackError { traceback, cause } if !sync => {
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

#[cfg(test)]
mod tests {
    use mlua::{Error, Lua, MultiValue, Value};

    use super::{BASE_FRAME, describe, failure, into_call_result, without_base_frame};
    use crate::lua::sandbox::make_engine;

    #[test]
    fn a_thread_that_returned_nothing_failed_with_nothing_to_say() {
        let lua = make_engine().unwrap();
        let error = failure(&lua, None, false);
        assert!(
            matches!(&error, Error::RuntimeError(text) if text == "nil"),
            "{error:?}"
        );
        let error = into_call_result(&lua, MultiValue::new(), false).unwrap_err();
        assert!(
            matches!(&error, Error::RuntimeError(text) if text == "nil"),
            "{error:?}"
        );
    }

    #[test]
    fn a_value_is_described_as_lua_l_tolstring_does() {
        let lua = make_engine().unwrap();
        let text = |source: &str| {
            let value: Value = lua.load(source).eval().unwrap();
            describe(&lua, value)
        };
        assert_eq!(text("nil"), "nil");
        assert_eq!(text("false"), "false");
        assert_eq!(text("42"), "42");
        assert_eq!(text("4.5"), "4.5");
        assert_eq!(text("'s'"), "s");
        assert_eq!(
            text("setmetatable({}, { __tostring = function() return 7 end })"),
            "7"
        );
        assert_eq!(
            text("setmetatable({}, { __tostring = function() error('x', 0) end })"),
            "error object's __tostring failed: x"
        );
        assert_eq!(
            text("setmetatable({}, { __tostring = function() error({}) end })"),
            "error object's __tostring failed"
        );
        assert!(text("{}").starts_with("table: 0x"));
        assert!(text("setmetatable({}, { __name = 'Named' })").starts_with("Named: 0x"));
        assert!(text("print").starts_with("function: 0x"));
        assert!(text("coroutine.create(print)").starts_with("thread: 0x"));
    }

    /// Without what `make_engine` installs there is nothing to run a `__tostring` under: said, not raised.
    #[test]
    fn a_value_is_described_even_when_the_engine_has_no_guard() {
        let lua = Lua::new();
        let value: Value = lua
            .load("setmetatable({}, { __tostring = function() return 's' end })")
            .eval()
            .unwrap();
        assert_eq!(describe(&lua, value), "error object's __tostring failed");
    }

    #[test]
    fn the_base_frame_goes_from_the_traceback_of_a_rust_error_that_left_a_thread() {
        let with_gap = format!(
            "stack traceback:\n\t[C]: in function 'error'\n\t...\t(skipping 9 levels)\n\t{BASE_FRAME}"
        );
        let error = || Error::CallbackError {
            traceback: with_gap.clone(),
            cause: std::sync::Arc::new(Error::runtime("x")),
        };
        // the frame is cut, the gap is as it was
        let Error::CallbackError { traceback, .. } = without_base_frame(error(), false) else {
            panic!("a callback error stays one")
        };
        assert_eq!(
            traceback,
            "stack traceback:\n\t[C]: in function 'error'\n\t...\t(skipping 9 levels)"
        );
        // under the `xpcall` of a synchronous call it is among the caller's frames, and stays
        let Error::CallbackError { traceback, .. } = without_base_frame(error(), true) else {
            panic!("a callback error stays one")
        };
        assert_eq!(traceback, with_gap);
        // another frame at the foot is not the base
        let other = Error::CallbackError {
            traceback: "stack traceback:\n\tx: in main chunk".to_owned(),
            cause: std::sync::Arc::new(Error::runtime("x")),
        };
        let Error::CallbackError { traceback, .. } = without_base_frame(other, false) else {
            panic!("a callback error stays one")
        };
        assert_eq!(traceback, "stack traceback:\n\tx: in main chunk");
    }
}

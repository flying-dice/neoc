//! Errors from app code: reported with the app's file and line, never a panic in the host.

use neoc_vm::mlua;
use std::fmt;

/// A failure inside app code (or a host call it made).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// `notes/app.luau:12: message`: what is shown to people and agents.
    pub summary: String,
    /// The Lua stack, for logs and the dev banner.
    pub traceback: Option<String>,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary)
    }
}
impl std::error::Error for ScriptError {}

/// `ctx:fail(message)`: the handler reports a problem to the caller. Not a bug in the app, so it carries
/// no file and line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolFailure(pub String);

impl fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ToolFailure {}

/// `error("RPC-32602 Title cannot be empty", 0)`: the app's way to fail a call with a JSON-RPC error code. A message that
/// starts with `RPC`, a whole number and a space is that error: the code, and the message after the space (no file or
/// line: the app wrote it for the caller). Any other error an app raises is an application error with its location.
#[must_use]
pub fn rpc_failure(e: &mlua::Error) -> Option<(i32, String)> {
    let (message, _) = flatten(e);
    let (code, text) = message.trim().strip_prefix("RPC")?.split_once(' ')?;
    let code: i32 = code.parse().ok()?;
    let text = text.trim();
    (code != 0 && !text.is_empty()).then(|| (code, text.to_owned()))
}

/// Does a message already begin with `chunk.luau:LINE:` (or `chunk.lua:LINE:`)?
fn has_location(message: &str) -> bool {
    let Some((head, rest)) = message.split_once(':') else { return false };
    if !head.contains(".lua") {
        return false;
    }
    rest.split_once(':').is_some_and(|(line, _)| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
}

/// The first `file.luau:LINE` (or `file.lua:LINE`) frame of a Lua traceback.
fn first_frame(traceback: &str) -> Option<String> {
    traceback.lines().find_map(|l| {
        let l = l.trim();
        let (name, rest) = l.split_once(':')?;
        if !name.contains(".lua") {
            return None;
        }
        let line: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (!line.is_empty()).then(|| format!("{name}:{line}"))
    })
}

fn flatten(e: &mlua::Error) -> (String, Option<String>) {
    match e {
        mlua::Error::RuntimeError(s) => match s.split_once("\nstack traceback:") {
            Some((msg, tb)) => (msg.to_owned(), Some(format!("stack traceback:{tb}"))),
            None => (s.clone(), None),
        },
        mlua::Error::CallbackError { traceback, cause } => {
            let (msg, tb) = flatten(cause);
            (msg, tb.or_else(|| Some(traceback.clone())))
        }
        mlua::Error::WithContext { context, cause } => {
            let (msg, tb) = flatten(cause);
            (format!("{context}: {msg}"), tb)
        }
        mlua::Error::SyntaxError { message, .. } => (message.clone(), None),
        mlua::Error::MemoryError(_) => ("memory limit exceeded".to_owned(), None),
        other => (other.to_string(), None),
    }
}

/// What a script raised that is not a message: a table, a function, a userdata or a thread (`error({ code = 7 })`).
const NOT_A_MESSAGE: &str = "the script raised a value that is not a message";

/// Whether `message` is Lua's own text for such a value (`table: 0x600000a44200`): the address of a host object, which no
/// client or agent is ever told. The address is C's `%p`: `0x` and hex digits on Linux and macOS, and on Windows the
/// whole pointer in hex with no prefix (`table: 0000025328FAFCF0`).
fn is_value_address(message: &str) -> bool {
    /// The hex digits of a pointer printed whole: 8 on a 32-bit host, 16 on a 64-bit one.
    const WHOLE_POINTER: [usize; 2] = [8, 16];
    ["table", "function", "userdata", "thread", "buffer", "vector"].iter().any(|kind| {
        message.strip_prefix(kind).and_then(|rest| rest.strip_prefix(": ")).is_some_and(|address| {
            let (hex, bare) = match address.strip_prefix("0x") {
                Some(hex) => (hex, false),
                None => (address, true),
            };
            !hex.is_empty()
                && hex.bytes().all(|b| b.is_ascii_hexdigit())
                && (!bare || WHOLE_POINTER.contains(&hex.len()))
        })
    })
}

impl ScriptError {
    /// Describe a Lua error, adding the file and line of the app frame when the message lacks one.
    #[must_use]
    pub fn from_lua(e: &mlua::Error) -> Self {
        let (message, traceback) = flatten(e);
        let message = message.trim();
        let message = if is_value_address(message) { NOT_A_MESSAGE.to_owned() } else { message.to_owned() };
        let summary = if has_location(&message) {
            message
        } else {
            match traceback.as_deref().and_then(first_frame) {
                Some(frame) => format!("{frame}: {message}"),
                None => message,
            }
        };
        Self { summary, traceback }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn a_value_that_is_not_a_message_never_shows_its_address() {
        for text in [
            "table: 0x600000a44200",
            "function: 0x1",
            "userdata: 0xdeadBEEF",
            "thread: 0x10",
            "table: 0000025328FAFCF0",
            "function: 00A1B2C3",
        ] {
            assert!(is_value_address(text), "{text}");
        }
        for text in [
            "table: 0x",
            "table: 0xzz",
            "a table: 0x1",
            "boom",
            "table: nil",
            "table: 12",
            "table: 12345678 rows",
        ] {
            assert!(!is_value_address(text), "{text}");
        }
    }

    fn run(src: &str) -> mlua::Error {
        let lua = Lua::new();
        lua.load(src).set_name("=notes/app.lua").exec().unwrap_err()
    }

    #[test]
    fn a_lua_error_carries_the_app_file_and_line_without_the_stack() {
        let e = ScriptError::from_lua(&run("local x = 1\nerror('boom')"));
        assert_eq!(e.summary, "notes/app.lua:2: boom");
        assert!(e.traceback.as_deref().is_some_and(|t| t.starts_with("stack traceback:")), "{e:?}");
    }

    #[test]
    fn a_host_error_gets_the_location_of_the_app_frame_that_called_it() {
        let lua = Lua::new();
        let f = lua
            .create_function(|_, ()| -> mlua::Result<()> {
                Err(mlua::Error::external(ToolFailure("db is down".into())))
            })
            .unwrap();
        lua.globals().set("host", f).unwrap();
        let err = lua.load("local a = 1\n\nhost()").set_name("=notes/app.lua").exec().unwrap_err();
        let e = ScriptError::from_lua(&err);
        assert!(e.summary.starts_with("notes/app.lua:3: "), "{e}");
        assert!(e.summary.ends_with("db is down"), "{e}");
    }

    #[test]
    fn a_syntax_error_names_its_line() {
        let e = ScriptError::from_lua(&run("local x = = 1"));
        assert!(e.summary.starts_with("notes/app.lua:1:"), "{e}");
    }

    #[test]
    fn a_luau_chunk_is_located_like_a_lua_one() {
        // an app is built into `app.luau` now: its errors must keep their location
        let lua = Lua::new();
        let err = lua.load("local x = 1\nerror('boom')").set_name("=calculator/app.luau").exec().unwrap_err();
        assert_eq!(ScriptError::from_lua(&err).summary, "calculator/app.luau:2: boom");
        assert!(has_location("calculator/app.luau:12: boom"));
        assert_eq!(
            first_frame("stack traceback:\n\t[C] in ?\n\tcalculator/app.luau:7 function f").as_deref(),
            Some("calculator/app.luau:7")
        );
    }

    #[test]
    fn location_detection() {
        assert!(has_location("notes/app.lua:12: boom"));
        assert!(!has_location("boom: not a location"));
        assert!(!has_location("cpu budget exceeded"));
        assert_eq!(
            first_frame("stack traceback:\n\t[C] in ?\n\tnotes/lib.lua:7 function f").as_deref(),
            Some("notes/lib.lua:7")
        );
    }
}

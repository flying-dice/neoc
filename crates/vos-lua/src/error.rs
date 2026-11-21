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

/// `ctx:fail(message)`: the tool reports a problem to the caller. Not a bug in the app, so it carries
/// no file and line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolFailure(pub String);

impl fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ToolFailure {}

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

impl ScriptError {
    /// Describe a Lua error, adding the file and line of the app frame when the message lacks one.
    #[must_use]
    pub fn from_lua(e: &mlua::Error) -> Self {
        let (message, traceback) = flatten(e);
        let message = message.trim().to_owned();
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

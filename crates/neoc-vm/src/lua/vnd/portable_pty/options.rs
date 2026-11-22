//! What a script passes to `vnd:portable_pty`, read before the call's future exists.
//!
//! A script table can run script code when it is read (a `__index`), so the options of `spawn_command` are turned
//! into plain data in the synchronous part of the call, as `vnd:bollard` and `vnd:hyper` read theirs. A key that is
//! not one of the call's is refused, so a misspelt option fails instead of being ignored.

use mlua::Value;
use serde_json::{Map, Value as Json};

use super::error::PtyError;
use crate::lua::serde_guard::refuse_buffer;

/// The size of a terminal the crate opens when none is given: `portable_pty::PtySize::default()`.
pub(super) const DEFAULT_ROWS: u16 = 24;
pub(super) const DEFAULT_COLS: u16 = 80;

/// The most rows or columns a terminal has. ConPTY holds a size in signed 16-bit numbers, so this is the largest
/// size every platform can be asked for.
pub(super) const MAX_SIZE: u16 = 32_767;

/// The most arguments a command takes here, and the most environment variables.
pub(super) const MAX_ITEMS: usize = 1024;

/// The longest program, argument, directory or environment value, in bytes. The operating system has its own limit
/// on all of them together, and a command past that is a `spawn` error.
pub(super) const MAX_TEXT: usize = 128 * 1024;

const ALLOWED: [&str; 6] = ["program", "args", "cwd", "env", "rows", "cols"];

/// The command of one `spawn_command`, as plain data.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Command {
    /// `None` is the user's default shell (`CommandBuilder::new_default_prog`).
    pub(super) program: Option<String>,
    pub(super) args: Vec<String>,
    pub(super) cwd: Option<String>,
    /// Added to the environment the process has.
    pub(super) env: Vec<(String, String)>,
    pub(super) rows: u16,
    pub(super) cols: u16,
}

/// The options of `spawn_command`. A buffer is refused (raised, as every serde conversion of a script value does:
/// `serde_guard`); anything else that is wrong is `invalid_input`.
pub(super) fn command(value: Option<&Value>, op: &str) -> mlua::Result<Result<Command, PtyError>> {
    let json = match value {
        None | Some(Value::Nil) => Json::Null,
        Some(value) => {
            refuse_buffer(value, op)?;
            match serde_json::to_value(value) {
                Ok(json) => json,
                Err(e) => return Ok(Err(PtyError::invalid_input(e.to_string()))),
            }
        }
    };
    Ok(read(json))
}

fn read(json: Json) -> Result<Command, PtyError> {
    let fields = match json {
        Json::Null => Map::new(),
        Json::Object(fields) => fields,
        // an empty Luau table may arrive as an empty list
        Json::Array(items) if items.is_empty() => Map::new(),
        _ => {
            return Err(PtyError::invalid_input(
                "the options must be a table of named options",
            ));
        }
    };
    if let Some(unknown) = fields.keys().find(|key| !ALLOWED.contains(&key.as_str())) {
        return Err(PtyError::invalid_input(format!(
            "unknown option `{unknown}` (this call takes: {})",
            ALLOWED.join(", ")
        )));
    }
    let program = string(&fields, "program")?;
    if program.as_deref() == Some("") {
        return Err(wrong("program", "the name or the path of a program"));
    }
    let args = strings(&fields, "args")?;
    if program.is_none() && !args.is_empty() {
        return Err(PtyError::invalid_input(
            "option `args` needs `program`: the default shell takes no arguments",
        ));
    }
    let cwd = string(&fields, "cwd")?;
    if cwd.as_deref() == Some("") {
        return Err(wrong("cwd", "the path of a directory"));
    }
    Ok(Command {
        program,
        args,
        cwd,
        env: env(&fields)?,
        rows: dimension(fields.get("rows"), "rows")?.unwrap_or(DEFAULT_ROWS),
        cols: dimension(fields.get("cols"), "cols")?.unwrap_or(DEFAULT_COLS),
    })
}

/// The `(rows, cols)` of a `resize`.
pub(super) fn size(rows: f64, cols: f64) -> Result<(u16, u16), PtyError> {
    Ok((whole(rows, "rows")?, whole(cols, "cols")?))
}

fn dimension(value: Option<&Json>, key: &str) -> Result<Option<u16>, PtyError> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => match value.as_f64() {
            Some(n) => whole(n, key).map(Some),
            None => Err(wrong(key, &size_words())),
        },
    }
}

fn whole(n: f64, key: &str) -> Result<u16, PtyError> {
    if n.fract() == 0.0 && n >= 1.0 && n <= f64::from(MAX_SIZE) {
        // checked just above: a whole number that fits
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Ok(n as u16);
    }
    Err(wrong(key, &size_words()))
}

fn size_words() -> String {
    format!("a whole number from 1 to {MAX_SIZE}")
}

/// A string the operating system takes as it is: no NUL byte, and not longer than [`MAX_TEXT`].
fn text(value: &Json, key: &str, what: &str) -> Result<String, PtyError> {
    match value {
        Json::String(s) if s.contains('\0') => Err(PtyError::invalid_input(format!(
            "option `{key}` cannot contain a NUL byte"
        ))),
        Json::String(s) if s.len() > MAX_TEXT => Err(PtyError::invalid_input(format!(
            "option `{key}` holds a string longer than {MAX_TEXT} bytes"
        ))),
        Json::String(s) => Ok(s.clone()),
        _ => Err(wrong(key, what)),
    }
}

fn string(fields: &Map<String, Json>, key: &str) -> Result<Option<String>, PtyError> {
    match fields.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value) => text(value, key, "a string").map(Some),
    }
}

fn strings(fields: &Map<String, Json>, key: &str) -> Result<Vec<String>, PtyError> {
    let expected = "a list of strings";
    match fields.get(key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        // an empty Luau table may arrive as an empty map
        Some(Json::Object(map)) if map.is_empty() => Ok(Vec::new()),
        Some(Json::Array(items)) if items.len() > MAX_ITEMS => Err(too_many(key)),
        Some(Json::Array(items)) => items.iter().map(|item| text(item, key, expected)).collect(),
        Some(_) => Err(wrong(key, expected)),
    }
}

fn env(fields: &Map<String, Json>) -> Result<Vec<(String, String)>, PtyError> {
    let expected = "a table of variable names, each with a string";
    match fields.get("env") {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Array(items)) if items.is_empty() => Ok(Vec::new()),
        Some(Json::Object(map)) if map.len() > MAX_ITEMS => Err(too_many("env")),
        Some(Json::Object(map)) => map
            .iter()
            .map(|(name, value)| {
                if name.is_empty() || name.contains('=') || name.contains('\0') {
                    return Err(PtyError::invalid_input(
                        "option `env` holds a name that is empty or contains `=` or a NUL byte",
                    ));
                }
                Ok((name.clone(), text(value, "env", expected)?))
            })
            .collect(),
        Some(_) => Err(wrong("env", expected)),
    }
}

fn wrong(key: &str, expected: &str) -> PtyError {
    PtyError::invalid_input(format!("option `{key}` must be {expected}"))
}

fn too_many(key: &str) -> PtyError {
    PtyError::invalid_input(format!(
        "option `{key}` holds more than {MAX_ITEMS} entries"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_options_is_the_default_shell_at_the_crates_default_size() {
        for none in [Json::Null, json!({}), json!([])] {
            assert_eq!(
                read(none).unwrap(),
                Command {
                    program: None,
                    args: vec![],
                    cwd: None,
                    env: vec![],
                    rows: 24,
                    cols: 80,
                }
            );
        }
    }

    #[test]
    fn a_command_keeps_its_arguments_directory_environment_and_size() {
        let command = read(json!({
            "program": "sh",
            "args": ["-c", "echo hi"],
            "cwd": "/tmp",
            "env": { "TERM": "xterm-256color" },
            "rows": 30,
            "cols": 100,
        }))
        .unwrap();
        assert_eq!(command.program.as_deref(), Some("sh"));
        assert_eq!(command.args, ["-c", "echo hi"]);
        assert_eq!(command.cwd.as_deref(), Some("/tmp"));
        assert_eq!(
            command.env,
            [("TERM".to_owned(), "xterm-256color".to_owned())]
        );
        assert_eq!((command.rows, command.cols), (30, 100));
    }

    #[test]
    fn what_is_not_a_command_is_refused_with_the_option_named() {
        let refused = |options: Json| read(options).unwrap_err().to_string();
        assert!(refused(json!({ "progam": "sh" })).contains("unknown option `progam`"));
        assert!(refused(json!("sh")).contains("table of named options"));
        assert!(refused(json!({ "program": "" })).contains("`program`"));
        assert!(refused(json!({ "program": 7 })).contains("`program`"));
        assert!(refused(json!({ "program": "a\u{0}b" })).contains("NUL"));
        assert!(refused(json!({ "args": ["-l"] })).contains("needs `program`"));
        assert!(refused(json!({ "program": "sh", "args": "-c" })).contains("`args`"));
        assert!(refused(json!({ "program": "sh", "args": [1] })).contains("`args`"));
        assert!(refused(json!({ "cwd": "" })).contains("`cwd`"));
        assert!(refused(json!({ "env": ["A=1"] })).contains("`env`"));
        assert!(refused(json!({ "env": { "A": 1 } })).contains("`env`"));
        assert!(refused(json!({ "env": { "A=B": "1" } })).contains("`env`"));
        assert!(refused(json!({ "env": { "": "1" } })).contains("`env`"));
        let long = "x".repeat(MAX_TEXT + 1);
        assert!(refused(json!({ "program": long })).contains("longer than"));
        let many: Vec<String> = vec![String::new(); MAX_ITEMS + 1];
        assert!(refused(json!({ "program": "sh", "args": many })).contains("more than"));
    }

    #[test]
    fn a_size_is_whole_numbers_every_platform_holds() {
        assert_eq!(size(1.0, 32_767.0).unwrap(), (1, 32_767));
        for bad in [0.0, -1.0, 1.5, 32_768.0, f64::NAN, f64::INFINITY] {
            assert!(size(bad, 80.0).is_err(), "{bad} rows was accepted");
            assert!(size(24.0, bad).is_err(), "{bad} cols was accepted");
        }
        let refused = |options: Json| read(options).unwrap_err().to_string();
        assert!(refused(json!({ "rows": 0 })).contains("`rows`"));
        assert!(refused(json!({ "cols": "80" })).contains("`cols`"));
        assert!(refused(json!({ "cols": 1e9 })).contains("`cols`"));
    }
}

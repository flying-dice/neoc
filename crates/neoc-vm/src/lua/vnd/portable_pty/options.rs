//! What a script passes to `vnd:portable_pty`, read before the call's future exists.
//!
//! A script table can run script code when it is read (a `__index`), so the options of `spawn_command` are turned
//! into plain data in the synchronous part of the call, as `vnd:bollard` and `vnd:hyper` read theirs. A key that is
//! not one of the call's is refused, so a misspelt option fails instead of being ignored.

use mlua::Value;
use serde_json::Value as Json;

use super::error::PtyError;
use crate::lua::command_options::{self, Refused, env, name, named, strings, wrong};
use crate::lua::facade::{List, Map, Shape};

/// The size of a terminal the crate opens when none is given: `portable_pty::PtySize::default()`.
pub(super) const DEFAULT_ROWS: u16 = 24;
pub(super) const DEFAULT_COLS: u16 = 80;

/// The most rows or columns a terminal has. ConPTY holds a size in signed 16-bit numbers, so this is the largest
/// size every platform can be asked for.
pub(super) const MAX_SIZE: u16 = 32_767;

/// The table `spawn_command` takes: [`command`] reads it, and refuses what is not a key of it.
#[neoc_macros::lua_record(shape)]
pub(super) struct Spawn {
    program: Option<String>,
    args: Option<List<String>>,
    cwd: Option<String>,
    env: Option<Map<String, String>>,
    rows: Option<u16>,
    cols: Option<u16>,
}

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

impl From<Refused> for PtyError {
    fn from(refused: Refused) -> Self {
        PtyError::invalid_input(refused.0)
    }
}

/// The options of `spawn_command`. A buffer is refused (raised, as every serde conversion of a script value does:
/// `serde_guard`); anything else that is wrong is `invalid_input`.
pub(super) fn command(value: Option<&Value>, op: &str) -> mlua::Result<Result<Command, PtyError>> {
    Ok(command_options::to_json(value, op)?
        .map_err(PtyError::from)
        .and_then(read))
}

fn read(json: Json) -> Result<Command, PtyError> {
    let fields = named(json, Spawn::KEYS)?;
    let program = name(&fields, "program", "the name or the path of a program")?;
    let args = strings(&fields, "args")?;
    if program.is_none() && !args.is_empty() {
        return Err(PtyError::invalid_input(
            "option `args` needs `program`: the default shell takes no arguments",
        ));
    }
    Ok(Command {
        program,
        args,
        cwd: name(&fields, "cwd", "the path of a directory")?,
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
            None => Err(wrong(key, &size_words()).into()),
        },
    }
}

fn whole(n: f64, key: &str) -> Result<u16, PtyError> {
    if n.fract() == 0.0 && n >= 1.0 && n <= f64::from(MAX_SIZE) {
        // checked just above: a whole number that fits
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Ok(n as u16);
    }
    Err(wrong(key, &size_words()).into())
}

fn size_words() -> String {
    format!("a whole number from 1 to {MAX_SIZE}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::command_options::{MAX_ITEMS, MAX_TEXT};
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

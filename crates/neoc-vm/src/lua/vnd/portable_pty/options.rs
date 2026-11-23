//! What a script passes to `vnd:portable_pty`, read before the call's future exists.
//!
//! The options of `spawn_command` are read into [`Spawn`] in the synchronous part of the call, and raw, so
//! reading them runs no script code. A key that is not one of the call's is refused, so a misspelt option fails
//! instead of being ignored.

use mlua::Value;

use super::error::PtyError;
use crate::lua::command_options::{Args, Dir, Env, Program};
use crate::lua::facade::DeclType;
use crate::lua::facade::args::{Field, Refused};
use crate::lua::serde_guard::refuse_buffer;

/// The size of a terminal the crate opens when none is given: `portable_pty::PtySize::default()`.
pub(super) const DEFAULT_ROWS: u16 = 24;
pub(super) const DEFAULT_COLS: u16 = 80;

/// The most rows or columns a terminal has. ConPTY holds a size in signed 16-bit numbers, so this is the largest
/// size every platform can be asked for.
pub(super) const MAX_SIZE: u16 = 32_767;

/// The table `spawn_command` takes. A key that is not a field is refused, and so is a value its field does not
/// take.
#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Spawn {
    program: Option<Program>,
    args: Option<Args>,
    cwd: Option<Dir>,
    env: Option<Env>,
    rows: Option<Dimension>,
    cols: Option<Dimension>,
}

/// The rows or the columns of a terminal: a whole number from 1 to [`MAX_SIZE`].
#[derive(Debug)]
pub(super) struct Dimension(u16);

impl DeclType for Dimension {
    fn decl_type() -> String {
        u16::decl_type()
    }
}

impl Field for Dimension {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        let n = match value {
            Value::Integer(n) => n as f64,
            Value::Number(n) => n,
            _ => return Err(Refused::option(key, &size_words())),
        };
        whole(n, key).map(Self)
    }
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

/// The command the options say (none is the default shell at the default size).
impl TryFrom<Spawn> for Command {
    type Error = PtyError;

    fn try_from(spawn: Spawn) -> Result<Self, PtyError> {
        let program = spawn.program.map(|program| program.0);
        let args = spawn.args.unwrap_or_default().0;
        if program.is_none() && !args.is_empty() {
            return Err(PtyError::invalid_input(
                "option \"args\" needs \"program\": the default shell takes no arguments",
            ));
        }
        Ok(Command {
            program,
            args,
            cwd: spawn.cwd.map(|cwd| cwd.0),
            env: spawn.env.unwrap_or_default().0,
            rows: spawn.rows.map_or(DEFAULT_ROWS, |rows| rows.0),
            cols: spawn.cols.map_or(DEFAULT_COLS, |cols| cols.0),
        })
    }
}

/// The `(rows, cols)` of a `resize`.
pub(super) fn size(rows: f64, cols: f64) -> Result<(u16, u16), Refused> {
    Ok((whole(rows, "rows")?, whole(cols, "cols")?))
}

fn whole(n: f64, key: &str) -> Result<u16, Refused> {
    if n.fract() == 0.0 && n >= 1.0 && n <= f64::from(MAX_SIZE) {
        // checked just above: a whole number that fits
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Ok(n as u16);
    }
    Err(Refused::option(key, &size_words()))
}

fn size_words() -> String {
    format!("a whole number from 1 to {MAX_SIZE}")
}

/// The command a script's options say (none is the default shell at the default size). A buffer is raised, as
/// every conversion of a script value raises it (`serde_guard`); anything else that is wrong is `invalid_input`.
pub(super) fn command(value: Option<Value>, op: &str) -> mlua::Result<Result<Command, PtyError>> {
    let spawn = match value {
        None | Some(Value::Nil) => Ok(Spawn::default()),
        Some(value) => {
            refuse_buffer(&value, op)?;
            Spawn::from_options(value)
        }
    };
    Ok(spawn.map_err(PtyError::from).and_then(Command::try_from))
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;
    use crate::lua::command_options::{MAX_ITEMS, MAX_TEXT};

    fn read(lua: &Lua, source: &str) -> Result<Command, PtyError> {
        let value: Value = lua.load(source).eval().unwrap();
        Spawn::from_options(value)?.try_into()
    }

    #[test]
    fn no_options_is_the_default_shell_at_the_crates_default_size() {
        let lua = Lua::new();
        let none = Command {
            program: None,
            args: vec![],
            cwd: None,
            env: vec![],
            rows: 24,
            cols: 80,
        };
        assert_eq!(read(&lua, "{}").unwrap(), none);
        assert_eq!(Command::try_from(Spawn::default()).unwrap(), none);
    }

    #[test]
    fn a_command_keeps_its_arguments_directory_environment_and_size() {
        let command = read(
            &Lua::new(),
            r#"{
                program = "sh",
                args = { "-c", "echo hi" },
                cwd = "/tmp",
                env = { TERM = "xterm-256color" },
                rows = 30,
                cols = 100,
            }"#,
        )
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
        let lua = Lua::new();
        let refused = |options: &str| read(&lua, options).unwrap_err().to_string();
        assert!(refused("{ progam = 'sh' }").contains("unknown option \"progam\""));
        assert!(refused("'sh'").contains("must be a table"));
        assert!(refused("{ program = '' }").contains("\"program\""));
        assert!(refused("{ program = 7 }").contains("\"program\""));
        assert!(refused("{ program = 'a\\0b' }").contains("NUL"));
        assert!(refused("{ args = { '-l' } }").contains("needs \"program\""));
        assert!(refused("{ program = 'sh', args = '-c' }").contains("\"args\""));
        assert!(refused("{ program = 'sh', args = { 1 } }").contains("\"args\""));
        assert!(refused("{ cwd = '' }").contains("\"cwd\""));
        assert!(refused("{ env = { 'A=1' } }").contains("\"env\""));
        assert!(refused("{ env = { A = 1 } }").contains("\"env\""));
        assert!(refused("{ env = { ['A=B'] = '1' } }").contains("\"env\""));
        assert!(refused("{ env = { [''] = '1' } }").contains("\"env\""));
        let long = format!("{{ program = string.rep('x', {}) }}", MAX_TEXT + 1);
        assert!(refused(&long).contains("longer than"));
        let many = format!(
            "{{ program = 'sh', args = table.create({}, '') }}",
            MAX_ITEMS + 1
        );
        assert!(refused(&many).contains("more than"));
    }

    #[test]
    fn a_size_is_whole_numbers_every_platform_holds() {
        assert_eq!(size(1.0, 32_767.0).unwrap(), (1, 32_767));
        for bad in [0.0, -1.0, 1.5, 32_768.0, f64::NAN, f64::INFINITY] {
            assert!(size(bad, 80.0).is_err(), "{bad} rows was accepted");
            assert!(size(24.0, bad).is_err(), "{bad} cols was accepted");
        }
        let lua = Lua::new();
        let refused = |options: &str| read(&lua, options).unwrap_err().to_string();
        assert!(refused("{ rows = 0 }").contains("\"rows\""));
        assert!(refused("{ cols = '80' }").contains("\"cols\""));
        assert!(refused("{ cols = 1e9 }").contains("\"cols\""));
    }
}

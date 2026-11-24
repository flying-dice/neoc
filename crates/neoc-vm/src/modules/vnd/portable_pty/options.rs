//! What the options of `vnd:portable_pty` say, checked before anything is started.
//!
//! The shape of the options is the module's typed table ([`Spawn`]), read by mlua-ir: a field of the wrong type or
//! an unknown key never gets here. What is checked here is what the type does not say: what the operating system
//! would not take (`command_options`), a size no platform holds, and arguments without a program.

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::portable_pty::{PtyError, Spawn};
use neoc_vm_core::command_options::{self, Refused};

/// The size of a terminal the crate opens when none is given: `portable_pty::PtySize::default()`.
pub(super) const DEFAULT_ROWS: u16 = 24;
pub(super) const DEFAULT_COLS: u16 = 80;

/// The most rows or columns a terminal has. ConPTY holds a size in signed 16-bit numbers, so this is the largest
/// size every platform can be asked for.
pub(super) const MAX_SIZE: u16 = 32_767;

/// The command of one `spawn_command`, as plain data.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Command {
    /// `None` is the user's default shell (`CommandBuilder::new_default_prog`).
    pub(super) program: Option<PathBuf>,
    pub(super) args: Vec<String>,
    pub(super) cwd: Option<PathBuf>,
    /// Added to the environment the process has.
    pub(super) env: BTreeMap<String, String>,
    pub(super) rows: u16,
    pub(super) cols: u16,
}

/// The command the options say (none is the default shell at the default size).
pub(super) fn command(spawn: Spawn) -> Result<Command, PtyError> {
    if let Some(program) = &spawn.program {
        command_options::program("program", program.as_ref())?;
    }
    let args = spawn.args.unwrap_or_default();
    command_options::args("args", &args)?;
    if spawn.program.is_none() && !args.is_empty() {
        return Err(PtyError::InvalidInput {
            reason: "option \"args\" needs \"program\": the default shell takes no arguments".to_owned(),
        });
    }
    if let Some(cwd) = &spawn.cwd {
        command_options::dir("cwd", cwd.as_ref())?;
    }
    let env = spawn.env.unwrap_or_default();
    command_options::env("env", &env)?;
    Ok(Command {
        program: spawn.program.map(|program| program.0),
        args,
        cwd: spawn.cwd.map(|cwd| cwd.0),
        env,
        rows: dimension("rows", spawn.rows.unwrap_or(DEFAULT_ROWS))?,
        cols: dimension("cols", spawn.cols.unwrap_or(DEFAULT_COLS))?,
    })
}

/// The `(rows, cols)` of a `resize`.
pub(super) fn size(rows: u16, cols: u16) -> Result<(u16, u16), PtyError> {
    Ok((dimension("rows", rows)?, dimension("cols", cols)?))
}

/// The rows or the columns of a terminal: from 1 to [`MAX_SIZE`].
fn dimension(option: &'static str, n: u16) -> Result<u16, Refused> {
    if (1..=MAX_SIZE).contains(&n) {
        Ok(n)
    } else {
        Err(Refused { option, reason: format!("must be a whole number from 1 to {MAX_SIZE}") })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoc_vm_core::command_options::{MAX_ITEMS, MAX_TEXT};
    use neoc_vm_core::osstr::OsPath;

    fn refused(spawn: Spawn) -> String {
        match command(spawn) {
            Err(PtyError::InvalidInput { reason }) => reason,
            other => panic!("it is refused as invalid input: {other:?}"),
        }
    }

    fn program(name: &str) -> Spawn {
        Spawn { program: Some(OsPath::of(name)), ..Spawn::default() }
    }

    #[test]
    fn no_options_is_the_default_shell_at_the_crates_default_size() {
        let none =
            Command { program: None, args: vec![], cwd: None, env: BTreeMap::new(), rows: 24, cols: 80 };
        assert_eq!(command(Spawn::default()), Ok(none));
    }

    #[test]
    fn a_command_keeps_its_arguments_directory_environment_and_size() {
        let command = command(Spawn {
            args: Some(vec!["-c".to_owned(), "echo hi".to_owned()]),
            cwd: Some(OsPath::of("/tmp")),
            env: Some(BTreeMap::from([("TERM".to_owned(), "xterm-256color".to_owned())])),
            rows: Some(30),
            cols: Some(100),
            ..program("sh")
        })
        .unwrap();
        assert_eq!(command.program, Some(PathBuf::from("sh")));
        assert_eq!(command.args, ["-c", "echo hi"]);
        assert_eq!(command.cwd, Some(PathBuf::from("/tmp")));
        assert_eq!(command.env, BTreeMap::from([("TERM".to_owned(), "xterm-256color".to_owned())]));
        assert_eq!((command.rows, command.cols), (30, 100));
    }

    #[test]
    fn what_is_not_a_command_is_refused_with_the_option_named() {
        let env = |name: &str, value: &str| Spawn {
            env: Some(BTreeMap::from([(name.to_owned(), value.to_owned())])),
            ..Spawn::default()
        };
        assert!(refused(program("")).contains("\"program\""));
        assert!(refused(program("a\0b")).contains("NUL"));
        assert!(
            refused(Spawn { args: Some(vec!["-l".to_owned()]), ..Spawn::default() })
                .contains("needs \"program\"")
        );
        assert!(refused(Spawn { args: Some(vec!["a\0b".to_owned()]), ..program("sh") }).contains("\"args\""));
        assert!(refused(Spawn { cwd: Some(OsPath::of("")), ..Spawn::default() }).contains("\"cwd\""));
        assert!(refused(env("A=B", "1")).contains("\"env\""));
        assert!(refused(env("", "1")).contains("\"env\""));
        assert!(refused(env("A", "a\0b")).contains("\"env\""));
        assert!(refused(program(&"x".repeat(MAX_TEXT + 1))).contains("longer than"));
        let many = Spawn { args: Some(vec![String::new(); MAX_ITEMS + 1]), ..program("sh") };
        assert!(refused(many).contains("more than"));
    }

    #[test]
    fn a_size_is_whole_numbers_every_platform_holds() {
        assert_eq!(size(1, 32_767), Ok((1, 32_767)));
        for bad in [0, 32_768, u16::MAX] {
            assert!(size(bad, 80).is_err(), "{bad} rows was accepted");
            assert!(size(24, bad).is_err(), "{bad} cols was accepted");
        }
        assert!(refused(Spawn { rows: Some(0), ..Spawn::default() }).contains("\"rows\""));
        assert!(refused(Spawn { cols: Some(40_000), ..Spawn::default() }).contains("\"cols\""));
        assert_eq!(
            refused(Spawn { cols: Some(0), ..Spawn::default() }),
            "option \"cols\" must be a whole number from 1 to 32767"
        );
    }
}

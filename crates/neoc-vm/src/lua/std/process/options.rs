//! What a script passes to `process.spawn`: a table of options, read into [`Spawn`] before the call first waits
//! (see [`crate::lua::command_options`]).

use mlua::Value;

use super::error::ProcessError;
use crate::lua::command_options::{Args, Dir, Env, Program};
use crate::lua::facade::DeclType;
use crate::lua::facade::args::{Field, Refused};
use crate::lua::serde_guard::refuse_buffer;

/// The table `spawn` takes. A key that is not a field is refused, and so is a value its field does not take.
#[neoc_macros::lua_record(from)]
#[derive(Debug)]
pub(super) struct Spawn {
    program: Program,
    args: Option<Args>,
    cwd: Option<Dir>,
    env: Option<Env>,
    clear_env: Option<bool>,
    stdin: Option<Stream>,
    stdout: Option<Stream>,
    stderr: Option<Stream>,
}

/// What one of the process's three standard streams is (`std::process::Stdio`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stream {
    /// A pipe between the process and the script.
    Piped,
    /// Nothing: reads end at once and writes are thrown away.
    Null,
    /// The engine's own stream.
    Inherit,
}

impl DeclType for Stream {
    fn decl_type() -> String {
        r#""piped" | "null" | "inherit""#.to_owned()
    }
}

impl Field for Stream {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        let names = "\"piped\", \"null\" or \"inherit\"";
        let stream = match &value {
            Value::String(name) => match &*name.as_bytes() {
                b"piped" => Some(Stream::Piped),
                b"null" => Some(Stream::Null),
                b"inherit" => Some(Stream::Inherit),
                _ => None,
            },
            _ => None,
        };
        stream.ok_or_else(|| Refused::option(key, names))
    }
}

/// The command of one `spawn`, as plain data.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Command {
    pub(super) program: String,
    pub(super) args: Vec<String>,
    /// `None` is the engine's working directory.
    pub(super) cwd: Option<String>,
    /// Added to the environment the process has.
    pub(super) env: Vec<(String, String)>,
    /// Start from an empty environment instead of the engine's.
    pub(super) clear_env: bool,
    pub(super) stdin: Stream,
    pub(super) stdout: Stream,
    pub(super) stderr: Stream,
}

/// The command the options say: a stream is a pipe unless the script says otherwise.
impl From<Spawn> for Command {
    fn from(spawn: Spawn) -> Self {
        Command {
            program: spawn.program.0,
            args: spawn.args.unwrap_or_default().0,
            cwd: spawn.cwd.map(|cwd| cwd.0),
            env: spawn.env.unwrap_or_default().0,
            clear_env: spawn.clear_env.unwrap_or(false),
            stdin: spawn.stdin.unwrap_or(Stream::Piped),
            stdout: spawn.stdout.unwrap_or(Stream::Piped),
            stderr: spawn.stderr.unwrap_or(Stream::Piped),
        }
    }
}

impl From<Refused> for ProcessError {
    fn from(refused: Refused) -> Self {
        ProcessError::invalid_input(refused.0)
    }
}

/// The command a script's options say. A buffer is raised, as every conversion of a script value raises it
/// (`serde_guard`); anything else that is wrong is `invalid_input`.
pub(super) fn command(
    value: Option<Value>,
    op: &str,
) -> mlua::Result<Result<Command, ProcessError>> {
    let value = value.unwrap_or(Value::Nil);
    refuse_buffer(&value, op)?;
    Ok(Spawn::from_options(value)
        .map(Command::from)
        .map_err(ProcessError::from))
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;

    fn read(lua: &Lua, source: &str) -> Result<Command, ProcessError> {
        let value: Value = lua.load(source).eval().unwrap();
        Ok(Spawn::from_options(value)?.into())
    }

    #[test]
    fn a_program_alone_has_three_pipes_and_the_engines_environment_and_directory() {
        assert_eq!(
            read(&Lua::new(), "{ program = 'cat' }").unwrap(),
            Command {
                program: "cat".to_owned(),
                args: vec![],
                cwd: None,
                env: vec![],
                clear_env: false,
                stdin: Stream::Piped,
                stdout: Stream::Piped,
                stderr: Stream::Piped,
            }
        );
    }

    #[test]
    fn a_command_keeps_everything_it_was_given() {
        let command = read(
            &Lua::new(),
            r#"{
                program = "sh",
                args = { "-c", "echo hi" },
                cwd = "/tmp",
                env = { A = "1" },
                clear_env = true,
                stdin = "null",
                stdout = "piped",
                stderr = "inherit",
            }"#,
        )
        .unwrap();
        assert_eq!(command.program, "sh");
        assert_eq!(command.args, ["-c", "echo hi"]);
        assert_eq!(command.cwd.as_deref(), Some("/tmp"));
        assert_eq!(command.env, [("A".to_owned(), "1".to_owned())]);
        assert!(command.clear_env);
        assert_eq!(
            (command.stdin, command.stdout, command.stderr),
            (Stream::Null, Stream::Piped, Stream::Inherit)
        );
    }

    #[test]
    fn what_is_not_a_command_is_refused_with_the_option_named() {
        let lua = Lua::new();
        let refused = |options: &str| read(&lua, options).unwrap_err().to_string();
        for none in ["{}", "{ args = { '-l' } }", "{ program = '' }"] {
            assert!(refused(none).contains("\"program\""));
        }
        assert!(refused("{ progam = 'sh' }").contains("unknown option \"progam\""));
        assert!(refused("{ program = 'sh', cwd = '' }").contains("\"cwd\""));
        assert!(refused("{ program = 'sh', clear_env = 'yes' }").contains("\"clear_env\""));
        for key in ["stdin", "stdout", "stderr"] {
            for bad in ["'pipe'", "1", "true", "{}"] {
                let words = refused(&format!("{{ program = 'sh', {key} = {bad} }}"));
                assert!(words.starts_with("invalid_input: "), "{words}");
                assert!(words.contains(&format!("\"{key}\"")), "{words}");
            }
        }
    }
}

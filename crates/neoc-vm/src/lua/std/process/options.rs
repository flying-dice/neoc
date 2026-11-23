//! What a script passes to `process.spawn`, read into plain data before the call first waits (see
//! [`crate::lua::command_options`]).

use mlua::Value;
use serde_json::{Map as Fields, Value as Json};

use super::error::ProcessError;
use crate::lua::command_options::{self, Refused, env, name, named, strings, wrong};
use crate::lua::facade::{DeclType, List, Map, Shape};

/// The table `spawn` takes: [`command`] reads it, and refuses what is not a key of it.
#[neoc_macros::lua_record(shape)]
pub(super) struct Spawn {
    program: String,
    args: Option<List<String>>,
    cwd: Option<String>,
    env: Option<Map<String, String>>,
    clear_env: Option<bool>,
    stdin: Option<Stream>,
    stdout: Option<Stream>,
    stderr: Option<Stream>,
}

impl DeclType for Stream {
    fn decl_type() -> String {
        r#""piped" | "null" | "inherit""#.to_owned()
    }
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

impl Stream {
    const NAMES: &str = "\"piped\", \"null\" or \"inherit\"";

    fn parse(name: &str) -> Option<Self> {
        match name {
            "piped" => Some(Stream::Piped),
            "null" => Some(Stream::Null),
            "inherit" => Some(Stream::Inherit),
            _ => None,
        }
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

impl From<Refused> for ProcessError {
    fn from(refused: Refused) -> Self {
        ProcessError::invalid_input(refused.0)
    }
}

/// The options of `spawn`. A buffer is refused (raised, as every serde conversion of a script value does:
/// `serde_guard`); anything else that is wrong is `invalid_input`.
pub(super) fn command(
    value: Option<&Value>,
    op: &str,
) -> mlua::Result<Result<Command, ProcessError>> {
    Ok(command_options::to_json(value, op)?
        .map_err(ProcessError::from)
        .and_then(read))
}

fn read(json: Json) -> Result<Command, ProcessError> {
    let fields = named(json, Spawn::KEYS)?;
    let program_words = "the name or the path of a program";
    let Some(program) = name(&fields, "program", program_words)? else {
        return Err(wrong("program", program_words).into());
    };
    Ok(Command {
        program,
        args: strings(&fields, "args")?,
        cwd: name(&fields, "cwd", "the path of a directory")?,
        env: env(&fields)?,
        clear_env: flag(&fields, "clear_env")?,
        stdin: stream(&fields, "stdin")?,
        stdout: stream(&fields, "stdout")?,
        stderr: stream(&fields, "stderr")?,
    })
}

fn flag(fields: &Fields<String, Json>, key: &str) -> Result<bool, Refused> {
    match fields.get(key) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(flag)) => Ok(*flag),
        Some(_) => Err(wrong(key, "true or false")),
    }
}

/// The stream under `key`: a pipe unless the script says otherwise.
fn stream(fields: &Fields<String, Json>, key: &str) -> Result<Stream, Refused> {
    match fields.get(key) {
        None | Some(Json::Null) => Ok(Stream::Piped),
        Some(Json::String(name)) => Stream::parse(name).ok_or_else(|| wrong(key, Stream::NAMES)),
        Some(_) => Err(wrong(key, Stream::NAMES)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_program_alone_has_three_pipes_and_the_engines_environment_and_directory() {
        assert_eq!(
            read(json!({ "program": "cat" })).unwrap(),
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
        let command = read(json!({
            "program": "sh",
            "args": ["-c", "echo hi"],
            "cwd": "/tmp",
            "env": { "A": "1" },
            "clear_env": true,
            "stdin": "null",
            "stdout": "piped",
            "stderr": "inherit",
        }))
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
        let refused = |options: Json| read(options).unwrap_err().to_string();
        for none in [Json::Null, json!({}), json!({ "args": ["-l"] })] {
            assert!(refused(none).contains("`program`"));
        }
        assert!(refused(json!({ "program": "" })).contains("`program`"));
        assert!(refused(json!({ "progam": "sh" })).contains("unknown option `progam`"));
        assert!(refused(json!({ "program": "sh", "cwd": "" })).contains("`cwd`"));
        assert!(refused(json!({ "program": "sh", "clear_env": "yes" })).contains("`clear_env`"));
        for key in ["stdin", "stdout", "stderr"] {
            for bad in [json!("pipe"), json!(1), json!(true), json!({})] {
                let mut options = json!({ "program": "sh" });
                options[key] = bad;
                let words = refused(options);
                assert!(words.starts_with("invalid_input: "), "{words}");
                assert!(words.contains(&format!("`{key}`")), "{words}");
            }
        }
    }
}

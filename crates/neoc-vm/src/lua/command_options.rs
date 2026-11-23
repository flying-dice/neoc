//! The options of a call that starts a command, as the types of their fields: what `std:process` and
//! `vnd:portable_pty` share (a program, its arguments, a directory, environment variables).
//!
//! Each is a field of the module's own table of options (`neoc_macros::lua_record(from)`), read from the script's
//! value by [`Field`] in the synchronous part of the call and raw, so reading runs no script code. What is
//! refused is a [`Refused`]: the words for the script, which each module wraps in its own `invalid_input` error.

use mlua::Value;

use crate::lua::facade::args::{Field, Refused, list_len};
use crate::lua::facade::{DeclType, List, Map};

/// The most arguments a command takes, and the most environment variables.
pub(crate) const MAX_ITEMS: usize = 1024;

/// The longest program, argument, directory or environment value, in bytes. The operating system has its own limit
/// on all of them together, and a command past that fails when it is started.
pub(crate) const MAX_TEXT: usize = 128 * 1024;

/// A string the operating system takes as it is: text with no NUL byte, and not longer than [`MAX_TEXT`].
fn text(value: &Value, key: &str, what: &str) -> Result<String, Refused> {
    let Value::String(text) = value else {
        return Err(Refused::option(key, what));
    };
    let Ok(text) = text.to_str() else {
        return Err(Refused::option(key, what));
    };
    if text.contains('\0') {
        return Err(Refused(format!(
            "option \"{key}\" cannot contain a NUL byte"
        )));
    }
    if text.len() > MAX_TEXT {
        return Err(Refused(format!(
            "option \"{key}\" holds a string longer than {MAX_TEXT} bytes"
        )));
    }
    Ok(text.to_owned())
}

/// A string that names something (`what`): it is not empty.
fn name(value: &Value, key: &str, what: &str) -> Result<String, Refused> {
    match text(value, key, what)? {
        name if name.is_empty() => Err(Refused::option(key, what)),
        name => Ok(name),
    }
}

fn too_many(key: &str) -> Refused {
    Refused(format!(
        "option \"{key}\" holds more than {MAX_ITEMS} entries"
    ))
}

/// The program of a command: a path, or a name looked up in `PATH`. A key that takes one and has none is refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Program(pub(crate) String);

impl DeclType for Program {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Field for Program {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        name(&value, key, "the name or the path of a program").map(Self)
    }
}

/// The directory a command starts in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Dir(pub(crate) String);

impl DeclType for Dir {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Field for Dir {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        name(&value, key, "the path of a directory").map(Self)
    }
}

/// The arguments of a command: a list of at most [`MAX_ITEMS`] strings. A key beside the list's own
/// (`table.pack` adds `n`) is not read; a table with keys and no list is refused.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Args(pub(crate) Vec<String>);

impl DeclType for Args {
    fn decl_type() -> String {
        List::<String>::decl_type()
    }
}

impl Field for Args {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        let expected = "a list of strings";
        let Value::Table(list) = value else {
            return Err(Refused::option(key, expected));
        };
        let Some(len) = list_len(&list) else {
            return Err(Refused::option(key, expected));
        };
        if len > MAX_ITEMS {
            return Err(too_many(key));
        }
        (1..=len)
            .map(|index| text(&list.raw_get(index).unwrap_or(Value::Nil), key, expected))
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

/// The environment variables a command is given: at most [`MAX_ITEMS`] names, each with a string.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Env(pub(crate) Vec<(String, String)>);

impl DeclType for Env {
    fn decl_type() -> String {
        Map::<String, String>::decl_type()
    }
}

impl Field for Env {
    fn field(key: &str, value: Value) -> Result<Self, Refused> {
        let expected = "a table of variable names, each with a string";
        let Value::Table(table) = value else {
            return Err(Refused::option(key, expected));
        };
        let mut env = Vec::new();
        for pair in table.pairs::<Value, Value>() {
            let Ok((Value::String(name), value)) = pair else {
                return Err(Refused::option(key, expected));
            };
            if env.len() == MAX_ITEMS {
                return Err(too_many(key));
            }
            let name = name
                .to_str()
                .map(|name| name.to_owned())
                .unwrap_or_default();
            if name.is_empty() || name.contains('=') || name.contains('\0') {
                return Err(Refused(format!(
                    "option \"{key}\" holds a name that is empty or contains `=` or a NUL byte"
                )));
            }
            env.push((name, text(&value, key, expected)?));
        }
        // a table has no order of its own: the same environment is given in the same order
        env.sort();
        Ok(Self(env))
    }
}

/// Check that `cwd` is a directory. It is checked here and not left to the start of the command: the operating
/// system reports a missing directory as if the program were missing.
pub(crate) fn existing_dir(cwd: &str) -> Result<(), String> {
    if std::path::Path::new(cwd).is_dir() {
        Ok(())
    } else {
        Err(format!(
            "the directory `{cwd}` does not exist or is not a directory"
        ))
    }
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;

    fn read<T: Field>(lua: &Lua, key: &str, source: &str) -> Result<T, String> {
        let value: Value = lua.load(source).eval().unwrap();
        T::field(key, value).map_err(|e| e.0)
    }

    #[test]
    fn a_command_keeps_its_program_arguments_and_environment() {
        let lua = Lua::new();
        assert_eq!(read::<Program>(&lua, "program", "'sh'").unwrap().0, "sh");
        assert_eq!(
            read::<Args>(&lua, "args", "{ '-c', 'echo hi' }").unwrap().0,
            ["-c", "echo hi"]
        );
        assert!(read::<Args>(&lua, "args", "{}").unwrap().0.is_empty());
        // the list part is the arguments, as `table.pack` gives them with its `n`
        assert_eq!(
            read::<Args>(&lua, "args", "table.pack('-c', 'x')")
                .unwrap()
                .0,
            ["-c", "x"]
        );
        assert_eq!(
            read::<Env>(&lua, "env", "{ B = '2', A = '1' }").unwrap().0,
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
        assert!(read::<Env>(&lua, "env", "{}").unwrap().0.is_empty());
        assert_eq!(read::<Option<Dir>>(&lua, "cwd", "nil").unwrap(), None);
    }

    #[test]
    fn what_the_operating_system_would_not_take_is_refused_with_the_option_named() {
        let lua = Lua::new();
        let program = |source| read::<Program>(&lua, "program", source).unwrap_err();
        assert!(program("''").contains("\"program\""));
        assert!(program("7").contains("\"program\""));
        assert!(program("nil").contains("\"program\""));
        assert!(program("'a\\0b'").contains("NUL"));
        assert!(program("'\\255'").contains("\"program\""));
        let long = format!("string.rep('x', {})", MAX_TEXT + 1);
        assert!(program(&long).contains("longer than"));

        let args = |source| read::<Args>(&lua, "args", source).unwrap_err();
        assert!(args("'-c'").contains("\"args\""));
        assert!(args("{ 1 }").contains("\"args\""));
        assert!(args("{ path = '/tmp/x' }").contains("\"args\""));
        assert!(args("{ 'a', nil, 'c' }").contains("\"args\""));
        let many = format!("table.create({}, '')", MAX_ITEMS + 1);
        assert!(args(&many).contains("more than"));

        let env = |source| read::<Env>(&lua, "env", source).unwrap_err();
        assert!(env("{ 'A=1' }").contains("\"env\""));
        assert!(env("{ A = 1 }").contains("\"env\""));
        assert!(env("{ ['A=B'] = '1' }").contains("\"env\""));
        assert!(env("{ [''] = '1' }").contains("\"env\""));
        assert!(
            read::<Dir>(&lua, "cwd", "''")
                .unwrap_err()
                .contains("\"cwd\"")
        );
    }

    #[test]
    fn a_directory_that_is_not_there_is_said_so() {
        assert!(existing_dir(&std::env::temp_dir().to_string_lossy()).is_ok());
        assert!(
            existing_dir("/nowhere/neoc-no-such-directory")
                .unwrap_err()
                .contains("does not exist")
        );
    }
}

//! What a command may be made of: a program, its arguments, a directory, environment variables. What `std:process`
//! and `vnd:portable_pty` check.
//!
//! The shape of a command is its module's own typed table, read by mlua-ir: a field of the wrong type never gets
//! here. What is checked here is what the type does not say and the operating system would not take (an empty
//! program, a NUL byte) or should not be asked to (a thousand arguments too many). What is refused is a
//! [`Refused`]: the option and the words for the script, which each module puts in its own `invalid_input` error.

use std::collections::BTreeMap;
use std::path::Path;

/// The most arguments a command takes, and the most environment variables.
pub const MAX_ITEMS: usize = 1024;

/// The longest program, argument, directory or environment value, in bytes. The operating system has its own limit
/// on all of them together, and a command past that fails when it is started.
pub const MAX_TEXT: usize = 128 * 1024;

/// An option of a command that was refused, and why: `option "<option>" <reason>`.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused {
    pub option: &'static str,
    pub reason: String,
}

impl Refused {
    fn must_be(option: &'static str, what: &str) -> Self {
        Self { option, reason: format!("must be {what}") }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "option \"{}\" {}", self.option, self.reason)
    }
}

/// Bytes the operating system takes as they are: no NUL byte, and not longer than [`MAX_TEXT`].
fn text(option: &'static str, bytes: &[u8]) -> Result<(), Refused> {
    if bytes.contains(&0) {
        return Err(Refused { option, reason: "cannot contain a NUL byte".to_owned() });
    }
    if bytes.len() > MAX_TEXT {
        return Err(Refused { option, reason: format!("holds a string longer than {MAX_TEXT} bytes") });
    }
    Ok(())
}

fn too_many(option: &'static str) -> Refused {
    Refused { option, reason: format!("holds more than {MAX_ITEMS} entries") }
}

/// The program of a command: a path, or a name looked up in `PATH`. Not empty.
pub fn program(option: &'static str, program: &Path) -> Result<(), Refused> {
    let bytes = program.as_os_str().as_encoded_bytes();
    if bytes.is_empty() {
        return Err(Refused::must_be(option, "the name or the path of a program"));
    }
    text(option, bytes)
}

/// The directory a command starts in. Not empty.
pub fn dir(option: &'static str, dir: &Path) -> Result<(), Refused> {
    let bytes = dir.as_os_str().as_encoded_bytes();
    if bytes.is_empty() {
        return Err(Refused::must_be(option, "the path of a directory"));
    }
    text(option, bytes)
}

/// The arguments of a command: at most [`MAX_ITEMS`] of them.
pub fn args(option: &'static str, args: &[String]) -> Result<(), Refused> {
    if args.len() > MAX_ITEMS {
        return Err(too_many(option));
    }
    args.iter().try_for_each(|arg| text(option, arg.as_bytes()))
}

/// The environment variables a command is given: at most [`MAX_ITEMS`] names, each with a value.
pub fn env(option: &'static str, env: &BTreeMap<String, String>) -> Result<(), Refused> {
    if env.len() > MAX_ITEMS {
        return Err(too_many(option));
    }
    for (name, value) in env {
        if name.is_empty() || name.contains('=') || name.contains('\0') {
            return Err(Refused {
                option,
                reason: "holds a name that is empty or contains `=` or a NUL byte".to_owned(),
            });
        }
        text(option, value.as_bytes())?;
    }
    Ok(())
}

/// Check that `cwd` is a directory. It is checked here and not left to the start of the command: the operating
/// system reports a missing directory as if the program were missing.
pub fn existing_dir(cwd: &Path) -> Result<(), String> {
    if cwd.is_dir() {
        Ok(())
    } else {
        Err(format!("the directory `{}` does not exist or is not a directory", cwd.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(result: Result<(), Refused>) -> String {
        result.expect_err("it is refused").to_string()
    }

    #[test]
    fn a_command_keeps_its_program_arguments_and_environment() {
        assert_eq!(program("program", Path::new("sh")), Ok(()));
        assert_eq!(args("args", &["-c".to_owned(), "echo hi".to_owned(), String::new()]), Ok(()));
        assert_eq!(args("args", &[]), Ok(()));
        let variables = BTreeMap::from([("B".to_owned(), "2".to_owned()), ("A".to_owned(), String::new())]);
        assert_eq!(env("env", &variables), Ok(()));
        assert_eq!(env("env", &BTreeMap::new()), Ok(()));
        assert_eq!(dir("cwd", Path::new("/tmp")), Ok(()));
    }

    #[test]
    fn what_the_operating_system_would_not_take_is_refused_with_the_option_named() {
        let program = |name: &str| program("program", Path::new(name));
        assert_eq!(refused(program("")), "option \"program\" must be the name or the path of a program");
        assert!(refused(program("a\0b")).contains("NUL"));
        assert!(refused(program(&"x".repeat(MAX_TEXT + 1))).contains("longer than"));
        assert_eq!(program(&"x".repeat(MAX_TEXT)), Ok(()));

        assert!(refused(args("args", &["ok".to_owned(), "a\0b".to_owned()])).starts_with("option \"args\""));
        assert!(refused(args("args", &vec![String::new(); MAX_ITEMS + 1])).contains("more than"));
        assert_eq!(args("args", &vec![String::new(); MAX_ITEMS]), Ok(()));

        let one = |name: &str, value: &str| BTreeMap::from([(name.to_owned(), value.to_owned())]);
        for name in ["A=B", "", "A\0"] {
            assert!(
                refused(env("env", &one(name, "1"))).starts_with("option \"env\" holds a name"),
                "{name:?}"
            );
        }
        assert!(refused(env("env", &one("A", "a\0b"))).contains("NUL"));
        let many: BTreeMap<String, String> =
            (0..=MAX_ITEMS).map(|n| (format!("V{n}"), String::new())).collect();
        assert!(refused(env("env", &many)).contains("more than"));

        assert_eq!(refused(dir("cwd", Path::new(""))), "option \"cwd\" must be the path of a directory");
        assert!(refused(dir("cwd", Path::new("a\0b"))).contains("NUL"));
    }

    #[test]
    fn a_directory_that_is_not_there_is_said_so() {
        assert!(existing_dir(&std::env::temp_dir()).is_ok());
        assert!(
            existing_dir(Path::new("/nowhere/neoc-no-such-directory"))
                .unwrap_err()
                .contains("does not exist")
        );
    }
}

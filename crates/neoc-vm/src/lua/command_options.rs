//! The options of a call that starts a command, read into plain data: what `std:process` and `vnd:portable_pty`
//! share (a program, its arguments, a directory, environment variables).
//!
//! A script table can run script code when it is read (a `__index`), so a module turns its options into plain data
//! in the synchronous part of the call ([`to_json`]) and reads the fields from that ([`named`] and the readers
//! below). A key that is not one of the call's is refused, so a misspelt option fails instead of being ignored.
//!
//! Everything refused here is a [`Refused`]: the words for the script, which each module wraps in its own
//! `invalid_input` error.

use std::fmt;

use mlua::Value;
use serde_json::{Map, Value as Json};

use crate::lua::serde_guard::refuse_buffer;

/// The most arguments a command takes, and the most environment variables.
pub(crate) const MAX_ITEMS: usize = 1024;

/// The longest program, argument, directory or environment value, in bytes. The operating system has its own limit
/// on all of them together, and a command past that fails when it is started.
pub(crate) const MAX_TEXT: usize = 128 * 1024;

/// Why an option was refused, in words for the script.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refused(pub(crate) String);

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The options a script passed, as plain data. A buffer is refused (raised, as every serde conversion of a script
/// value does: `serde_guard`); a value serde cannot convert is a [`Refused`].
pub(crate) fn to_json(value: Option<&Value>, op: &str) -> mlua::Result<Result<Json, Refused>> {
    match value {
        None | Some(Value::Nil) => Ok(Ok(Json::Null)),
        Some(value) => {
            refuse_buffer(value, op)?;
            Ok(serde_json::to_value(value).map_err(|e| Refused(e.to_string())))
        }
    }
}

/// The named options in `json`, each of them one of `allowed`. No options at all is an empty map.
pub(crate) fn named(json: Json, allowed: &[&str]) -> Result<Map<String, Json>, Refused> {
    let fields = match json {
        Json::Null => Map::new(),
        Json::Object(fields) => fields,
        // an empty Luau table may arrive as an empty list
        Json::Array(items) if items.is_empty() => Map::new(),
        _ => {
            return Err(Refused(
                "the options must be a table of named options".to_owned(),
            ));
        }
    };
    if let Some(unknown) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(Refused(format!(
            "unknown option `{unknown}` (this call takes: {})",
            allowed.join(", ")
        )));
    }
    Ok(fields)
}

/// A string the operating system takes as it is: no NUL byte, and not longer than [`MAX_TEXT`].
fn text(value: &Json, key: &str, what: &str) -> Result<String, Refused> {
    match value {
        Json::String(s) if s.contains('\0') => {
            Err(Refused(format!("option `{key}` cannot contain a NUL byte")))
        }
        Json::String(s) if s.len() > MAX_TEXT => Err(Refused(format!(
            "option `{key}` holds a string longer than {MAX_TEXT} bytes"
        ))),
        Json::String(s) => Ok(s.clone()),
        _ => Err(wrong(key, what)),
    }
}

/// The string under `key`, when there is one.
pub(crate) fn string(fields: &Map<String, Json>, key: &str) -> Result<Option<String>, Refused> {
    match fields.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value) => text(value, key, "a string").map(Some),
    }
}

/// The string under `key`, which names something (`what`): one that is there is not empty.
pub(crate) fn name(
    fields: &Map<String, Json>,
    key: &str,
    what: &str,
) -> Result<Option<String>, Refused> {
    match string(fields, key)? {
        Some(name) if name.is_empty() => Err(wrong(key, what)),
        name => Ok(name),
    }
}

/// The list of strings under `key` (a command's arguments): at most [`MAX_ITEMS`] of them.
pub(crate) fn strings(fields: &Map<String, Json>, key: &str) -> Result<Vec<String>, Refused> {
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

/// The environment variables under `env`: at most [`MAX_ITEMS`] names, each with a string.
pub(crate) fn env(fields: &Map<String, Json>) -> Result<Vec<(String, String)>, Refused> {
    let expected = "a table of variable names, each with a string";
    match fields.get("env") {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::Array(items)) if items.is_empty() => Ok(Vec::new()),
        Some(Json::Object(map)) if map.len() > MAX_ITEMS => Err(too_many("env")),
        Some(Json::Object(map)) => map
            .iter()
            .map(|(name, value)| {
                if name.is_empty() || name.contains('=') || name.contains('\0') {
                    return Err(Refused(
                        "option `env` holds a name that is empty or contains `=` or a NUL byte"
                            .to_owned(),
                    ));
                }
                Ok((name.clone(), text(value, "env", expected)?))
            })
            .collect(),
        Some(_) => Err(wrong("env", expected)),
    }
}

/// The directory under `cwd`, when there is one. It is checked here and not left to the start of the command: the
/// operating system reports a missing directory as if the program were missing.
pub(crate) fn existing_dir(cwd: &str) -> Result<(), String> {
    if std::path::Path::new(cwd).is_dir() {
        Ok(())
    } else {
        Err(format!(
            "the directory `{cwd}` does not exist or is not a directory"
        ))
    }
}

/// "option `key` must be `expected`".
pub(crate) fn wrong(key: &str, expected: &str) -> Refused {
    Refused(format!("option `{key}` must be {expected}"))
}

fn too_many(key: &str) -> Refused {
    Refused(format!(
        "option `{key}` holds more than {MAX_ITEMS} entries"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(json: Json) -> Map<String, Json> {
        named(json, &["program", "args", "cwd", "env"]).unwrap()
    }

    #[test]
    fn no_options_is_an_empty_map_and_an_unknown_one_is_named() {
        for none in [Json::Null, json!({}), json!([])] {
            assert!(fields(none).is_empty());
        }
        let refused = named(json!({ "progam": "sh" }), &["program"]).unwrap_err();
        assert_eq!(
            refused.0,
            "unknown option `progam` (this call takes: program)"
        );
        assert!(
            named(json!("sh"), &["program"])
                .unwrap_err()
                .0
                .contains("table of named options")
        );
    }

    #[test]
    fn a_command_keeps_its_program_arguments_and_environment() {
        let fields =
            fields(json!({ "program": "sh", "args": ["-c", "echo hi"], "env": { "A": "1" } }));
        assert_eq!(
            name(&fields, "program", "a program").unwrap().as_deref(),
            Some("sh")
        );
        assert_eq!(strings(&fields, "args").unwrap(), ["-c", "echo hi"]);
        assert_eq!(env(&fields).unwrap(), [("A".to_owned(), "1".to_owned())]);
        assert_eq!(string(&fields, "cwd").unwrap(), None);
    }

    #[test]
    fn what_the_operating_system_would_not_take_is_refused_with_the_option_named() {
        let refused = |options: Json| {
            let fields = fields(options);
            name(&fields, "program", "the name or the path of a program")
                .and_then(|_| strings(&fields, "args"))
                .and_then(|_| env(&fields))
                .unwrap_err()
                .0
        };
        assert!(refused(json!({ "program": "" })).contains("`program`"));
        assert!(refused(json!({ "program": 7 })).contains("`program`"));
        assert!(refused(json!({ "program": "a\u{0}b" })).contains("NUL"));
        assert!(refused(json!({ "args": "-c" })).contains("`args`"));
        assert!(refused(json!({ "args": [1] })).contains("`args`"));
        assert!(refused(json!({ "env": ["A=1"] })).contains("`env`"));
        assert!(refused(json!({ "env": { "A": 1 } })).contains("`env`"));
        assert!(refused(json!({ "env": { "A=B": "1" } })).contains("`env`"));
        assert!(refused(json!({ "env": { "": "1" } })).contains("`env`"));
        let long = "x".repeat(MAX_TEXT + 1);
        assert!(refused(json!({ "program": long })).contains("longer than"));
        let many: Vec<String> = vec![String::new(); MAX_ITEMS + 1];
        assert!(refused(json!({ "args": many })).contains("more than"));
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

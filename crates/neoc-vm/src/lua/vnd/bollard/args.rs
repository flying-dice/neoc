//! What the methods of a `vnd:bollard` client take, as Rust types.
//!
//! Each argument is read from the script's value before the call's future exists (`facade::args`): a name is
//! checked against Docker's alphabet, a table Docker takes as a body becomes plain data, and a table of options
//! becomes its struct. What is refused is the call's own `invalid_input` error.

use std::collections::HashMap;

use mlua::{FromLua, Lua, Value};
use serde_json::Value as Json;

use super::options;
use crate::lua::facade::args::{Argument, Field, Refused};
use crate::lua::facade::{DeclType, List, Map};
use crate::lua::serde_guard::refuse_buffer;

/// An argument that is a string, read by `$read` (the text, then why it is refused).
macro_rules! text_argument {
    ($(#[$doc:meta])* $name:ident, $read:expr) => {
        $(#[$doc])*
        pub(super) struct $name(pub(super) String);

        impl DeclType for $name {
            fn decl_type() -> String {
                String::decl_type()
            }
        }

        impl Argument for $name {
            fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
                let text = String::from_lua(value, lua)?;
                Ok($read(&text).map(Self))
            }
        }
    };
}

text_argument!(
    /// The name or the id of a container.
    ContainerName,
    |name| options::name(name, "the container")
);
text_argument!(
    /// The name of a volume.
    VolumeName,
    |name| options::name(name, "the volume")
);
text_argument!(
    /// The name or the id of a network.
    NetworkName,
    |name| options::name(name, "the network")
);
text_argument!(
    /// An image reference (`nginx:1.27`).
    ImageRef,
    options::image
);

/// The image a pull names: its repository, and the tag unless the reference carries a digest.
pub(super) struct PullTarget {
    pub(super) repository: String,
    pub(super) tag: Option<String>,
}

impl DeclType for PullTarget {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Argument for PullTarget {
    fn read(value: Value, lua: &Lua) -> mlua::Result<Result<Self, Refused>> {
        let reference = String::from_lua(value, lua)?;
        Ok(options::pull_target(&reference).map(|(repository, tag)| Self { repository, tag }))
    }
}

/// One of Docker's own request bodies, as plain data: a table with Docker's field names.
pub(super) struct Body(pub(super) Json);

impl DeclType for Body {
    fn decl_type() -> String {
        Map::<String, Value>::decl_type()
    }
}

impl Argument for Body {
    fn read(value: Value, _: &Lua) -> mlua::Result<Result<Self, Refused>> {
        // a buffer is raised, as every serde conversion of a script value does
        refuse_buffer(&value, "bollard")?;
        Ok(options::json(&value).map(Self))
    }
}

/// The command of an `exec`: the program, then its arguments.
pub(super) struct Command(pub(super) Vec<String>);

impl DeclType for Command {
    fn decl_type() -> String {
        List::<String>::decl_type()
    }
}

impl Argument for Command {
    fn read(value: Value, _: &Lua) -> mlua::Result<Result<Self, Refused>> {
        refuse_buffer(&value, "bollard")?;
        let command = options::json(&value)
            .ok()
            .and_then(|command| serde_json::from_value::<Vec<String>>(command).ok())
            .filter(|command| !command.is_empty());
        Ok(command.map(Self).ok_or_else(|| {
            Refused(
                "the command must be a list of strings: the program, then its arguments".to_owned(),
            )
        }))
    }
}

/// A signal as Docker takes it: a name (`SIGHUP`, `HUP`) or a number, written as a string.
#[derive(Debug)]
pub(super) struct Signal(pub(super) String);

impl DeclType for Signal {
    fn decl_type() -> String {
        String::decl_type()
    }
}

impl Field for Signal {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let signal = String::field(name, value)?;
        if signal.is_empty() || !signal.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Refused::option(
                name,
                "a signal's name or number (`SIGTERM`, `HUP`, `9`)",
            ));
        }
        Ok(Self(signal))
    }
}

/// A positive number of seconds.
#[derive(Debug)]
pub(super) struct Seconds(pub(super) f64);

impl DeclType for Seconds {
    fn decl_type() -> String {
        f64::decl_type()
    }
}

impl Field for Seconds {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        f64::field(name, value)
            .ok()
            .filter(|secs| secs.is_finite() && *secs > 0.0)
            .map(Self)
            .ok_or_else(|| Refused::option(name, "a positive number of seconds"))
    }
}

/// Docker's `filters`: each name with the values it may have (`{ label = { "app=web" }, status = "running" }`;
/// one value may be written without its list).
#[derive(Debug)]
pub(super) struct Filters(pub(super) HashMap<String, Vec<String>>);

impl DeclType for Filters {
    fn decl_type() -> String {
        Map::<String, Value>::decl_type()
    }
}

impl Field for Filters {
    fn field(name: &str, value: Value) -> Result<Self, Refused> {
        let refused = || {
            Refused::option(
                name,
                "a table of filter names, each with a string or a list of strings",
            )
        };
        let Value::Table(filters) = value else {
            return Err(refused());
        };
        let mut read = HashMap::new();
        for pair in filters.pairs::<Value, Value>() {
            let Ok((filter @ Value::String(_), values)) = pair else {
                return Err(refused());
            };
            let filter = String::field(name, filter).map_err(|_| refused())?;
            let values = match values {
                one @ Value::String(_) => vec![String::field(name, one).map_err(|_| refused())?],
                many => List::<String>::field(name, many).map_err(|_| refused())?.0,
            };
            read.insert(filter, values);
        }
        Ok(Self(read))
    }
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Connect {
    pub(super) host: Option<String>,
    pub(super) timeout: Option<Seconds>,
    pub(super) tls_ca: Option<String>,
    pub(super) tls_cert: Option<String>,
    pub(super) tls_key: Option<String>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct ListContainers {
    pub(super) all: Option<bool>,
    pub(super) limit: Option<i32>,
    pub(super) size: Option<bool>,
    pub(super) filters: Option<Filters>,
}

/// The options of `stop_container` and `restart_container`.
#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Stop {
    pub(super) timeout: Option<i32>,
    pub(super) signal: Option<Signal>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Kill {
    pub(super) signal: Option<Signal>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct RemoveContainer {
    pub(super) force: Option<bool>,
    pub(super) volumes: Option<bool>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Logs {
    pub(super) follow: Option<bool>,
    pub(super) stdout: Option<bool>,
    pub(super) stderr: Option<bool>,
    pub(super) tail: Option<i32>,
    pub(super) since: Option<i32>,
    pub(super) until: Option<i32>,
    pub(super) timestamps: Option<bool>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct ExecOptions {
    pub(super) env: Option<List<String>>,
    pub(super) user: Option<String>,
    pub(super) working_dir: Option<String>,
    pub(super) privileged: Option<bool>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct ListImages {
    pub(super) all: Option<bool>,
    pub(super) digests: Option<bool>,
    pub(super) filters: Option<Filters>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Pull {
    pub(super) platform: Option<String>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct RemoveImage {
    pub(super) force: Option<bool>,
    pub(super) noprune: Option<bool>,
}

/// The options of a listing that takes only filters.
#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Filtered {
    pub(super) filters: Option<Filters>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Force {
    pub(super) force: Option<bool>,
}

#[neoc_macros::lua_record(from)]
#[derive(Debug, Default)]
pub(super) struct Events {
    pub(super) since: Option<i32>,
    pub(super) until: Option<i32>,
    pub(super) filters: Option<Filters>,
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    use super::*;

    fn read<T: Argument>(lua: &Lua, source: &str) -> Result<T, String> {
        let value: Value = lua.load(source).eval().unwrap();
        T::read(value, lua).unwrap().map_err(|e| e.0)
    }

    #[test]
    fn options_refuse_unknown_keys_and_wrong_types() {
        let lua = Lua::new();
        assert!(read::<Option<Logs>>(&lua, "nil").unwrap().is_none());
        let unknown = read::<Logs>(&lua, "{ tial = 5 }").unwrap_err();
        assert!(unknown.contains("unknown option \"tial\""), "{unknown}");
        assert!(read::<Logs>(&lua, "'all'").is_err());

        let logs = read::<Logs>(&lua, "{ follow = true, tail = 50 }").unwrap();
        assert_eq!((logs.follow, logs.tail), (Some(true), Some(50)));
        assert!(read::<Logs>(&lua, "{ follow = 1 }").is_err());
        assert!(read::<Logs>(&lua, "{ tail = 1.5 }").is_err());
        assert!(read::<Logs>(&lua, "{ tail = 1e12 }").is_err());

        let connect = read::<Connect>(&lua, "{ timeout = 2.5 }").unwrap();
        assert_eq!(connect.timeout.map(|secs| secs.0), Some(2.5));
        assert!(read::<Connect>(&lua, "{ timeout = 0 }").is_err());
        assert!(read::<Connect>(&lua, "{ host = true }").is_err());
    }

    #[test]
    fn filters_take_one_value_or_a_list() {
        let lua = Lua::new();
        let listed = read::<Filtered>(
            &lua,
            "{ filters = { status = 'running', label = { 'a=1', 'b' } } }",
        )
        .unwrap();
        let filters = listed.filters.unwrap().0;
        assert_eq!(filters["status"], vec!["running"]);
        assert_eq!(filters["label"], vec!["a=1", "b"]);
        assert!(read::<Filtered>(&lua, "{ filters = { status = 1 } }").is_err());
        assert!(read::<Filtered>(&lua, "{ filters = 'running' }").is_err());
        // a table of names is not a list of values: read as one it would be no filter at all
        assert!(
            read::<Filtered>(&lua, "{ filters = { label = { ['app=web'] = true } } }").is_err()
        );
        assert!(read::<ExecOptions>(&lua, "{ env = { A = '1' } }").is_err());
        assert!(read::<ExecOptions>(&lua, "{ env = {} }").is_ok());
    }

    #[test]
    fn a_signal_is_a_name_or_a_number() {
        let lua = Lua::new();
        let kill = read::<Kill>(&lua, "{ signal = 'SIGHUP' }").unwrap();
        assert_eq!(kill.signal.unwrap().0, "SIGHUP");
        let refused = read::<Kill>(&lua, "{ signal = 'HUP; rm' }").unwrap_err();
        assert!(refused.contains("option \"signal\" must be"), "{refused}");
        assert!(read::<Kill>(&lua, "{ signal = '' }").is_err());
    }

    #[test]
    fn a_name_is_checked_and_a_command_is_a_list_of_strings() {
        let lua = Lua::new();
        assert_eq!(read::<ContainerName>(&lua, "'web-1'").unwrap().0, "web-1");
        assert!(read::<ContainerName>(&lua, "'a/b'").is_err());
        assert_eq!(
            read::<Command>(&lua, "{ 'ls', '-l' }").unwrap().0,
            vec!["ls", "-l"]
        );
        assert!(read::<Command>(&lua, "{}").is_err());
        assert!(read::<Command>(&lua, "{ 1 }").is_err());
        let pull = read::<PullTarget>(&lua, "'nginx'").unwrap();
        assert_eq!(
            (pull.repository.as_str(), pull.tag.as_deref()),
            ("nginx", Some("latest"))
        );
    }
}

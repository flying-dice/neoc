//! Command-line arguments: `lib:args` (the `clap` crate).
//!
//! `parse(argv, config)` reads a script's arguments against the flags and subcommands `config` declares; `help`
//! writes the help text without reading any. The rules are clap's: a boolean flag is its presence, `--out file` and
//! `--out=file` are the same, everything after `--` is kept apart (`rest`), and the wording of errors and of the
//! help is clap's.
//!
//! A config clap would panic on is an error instead: two flags with the same name or the same short, a reserved
//! name (`help`, `__positional`, `__rest`), a short that is `-`, `=` or a space, a name that starts with `-`, a
//! subcommand named `help`. A flag may claim the short `-h` (for `--host`, say): it takes it from the built-in help,
//! which keeps `--help`.
//!
//! Ported from neoc-vm's `lib:args`. What changed on purpose:
//! - The result is typed. `parse` gives `{ kind = "parsed", flags, positional, rest, subcommand? }` or, for
//!   `--help`, `{ kind = "help", text }` (it gave one table with the flags beside `positional`, `rest`, `subcommand`
//!   and the subcommand's own table under its name, or `{ help = true, help_text }`). A flag's value is under
//!   `flags`, a subcommand is `subcommand = { name, flags, positional, rest }`.
//! - Flags are a list of `{ name, type, ... }` in the order the help shows them; the map form (`flags = { verbose =
//!   { ... } }`), which lost that order, is gone.
//! - A flag is a union on `type`: `default` has the flag's type, and a boolean flag has no `default`, `required` or
//!   `multiple` (a default that did not match, or `multiple` on a boolean, was an error returned at run time; a
//!   boolean's `default` and `required` were ignored).
//! - A config of the wrong shape (a missing `name` or `type`, a type that does not exist, a nested subcommand, an
//!   option that does not exist) is refused as an argument with its place; what only clap's rules forbid is the
//!   typed `invalid_config`.
//! - Errors are typed by what went wrong (`unknown_argument`, `invalid_value`, `missing_required`,
//!   `missing_subcommand`, `invalid_subcommand`, `conflict`, `usage`), with clap's text in `text`.

use std::collections::{BTreeMap, HashSet};

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

use args::{ArgsError, Config, Flag, FlagValue, Outcome, Subcommand, SubcommandConfig};

/// Ids every command already uses: clap's help, and this module's two positional arguments.
const POSITIONAL_ID: &str = "__positional";
const REST_ID: &str = "__rest";
const RESERVED_FLAG_NAMES: &[&str] = &["help", POSITIONAL_ID, REST_ID];

/// A list of flags, strictly: a table with named keys is refused. Read as a plain `Vec`, such a table (the map form
/// `flags = { verbose = { ... } }` that neoc-vm took) would be an empty list, and its flags silently not there.
fn flag_list<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<Flag>>, D::Error> {
    struct FlagList;

    impl<'de> serde::de::Visitor<'de> for FlagList {
        type Value = Option<Vec<Flag>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a list of flags")
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut flags = Vec::new();
            while let Some(flag) = seq.next_element()? {
                flags.push(flag);
            }
            Ok(Some(flags))
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            // an empty table is an empty list
            match map.next_key::<serde::de::IgnoredAny>()? {
                None => Ok(Some(Vec::new())),
                Some(_) => Err(serde::de::Error::custom(
                    "flags are a list of `{ name = ..., type = ... }`, not a table keyed by name",
                )),
            }
        }
    }

    deserializer.deserialize_any(FlagList)
}

impl Flag {
    fn name(&self) -> &str {
        match self {
            Flag::Boolean { name, .. } | Flag::String { name, .. } | Flag::Number { name, .. } => name,
        }
    }

    fn short_text(&self) -> Option<&str> {
        match self {
            Flag::Boolean { short, .. } | Flag::String { short, .. } | Flag::Number { short, .. } => short.as_deref(),
        }
    }

    fn desc(&self) -> Option<&str> {
        match self {
            Flag::Boolean { desc, .. } | Flag::String { desc, .. } | Flag::Number { desc, .. } => desc.as_deref(),
        }
    }

    /// The short alias; `None` when the flag has none. Checked by [`validate_flags`] to be one character.
    fn short(&self) -> Option<char> {
        self.short_text().and_then(|short| short.chars().next())
    }
}

fn invalid(reason: String) -> ArgsError {
    ArgsError::InvalidConfig { reason }
}

/// Where a config error is: the command's name.
fn place(command: &str) -> &str {
    if command.is_empty() { "config" } else { command }
}

/// Refuse what clap would panic on while it builds the command: a config is a script's input, and a panic here
/// takes the host down.
fn validate_flags(flags: &[Flag], command: &str) -> Result<(), ArgsError> {
    let place = place(command);
    let mut names: HashSet<&str> = HashSet::new();
    let mut shorts: HashSet<char> = HashSet::new();
    for flag in flags {
        let name = flag.name();
        if name.is_empty() {
            return Err(invalid(format!("{place}: flag name is empty")));
        }
        if RESERVED_FLAG_NAMES.contains(&name) {
            return Err(invalid(format!("{place}: flag name '{name}' is reserved")));
        }
        if name.starts_with('-') {
            return Err(invalid(format!("{place}: flag name '{name}' must not start with '-'")));
        }
        if !names.insert(name) {
            return Err(invalid(format!("{place}: flag '--{name}' is declared twice")));
        }
        if let Some(text) = flag.short_text() {
            let mut chars = text.chars();
            let (Some(short), None) = (chars.next(), chars.next()) else {
                return Err(invalid(format!(
                    "{place}: flag '--{name}' short alias '{text}' must be a single character"
                )));
            };
            if short == '-' || short == '=' || short.is_whitespace() {
                return Err(invalid(format!("{place}: flag '--{name}' short alias {short:?} is not allowed")));
            }
            if !shorts.insert(short) {
                return Err(invalid(format!("{place}: short alias '-{short}' is used by two flags")));
            }
        }
    }
    Ok(())
}

fn validate(config: &Config) -> Result<(), ArgsError> {
    if config.name.is_empty() {
        return Err(invalid("config.name is empty".into()));
    }
    validate_flags(config.flags.as_deref().unwrap_or_default(), &config.name)?;
    for (name, subcommand) in config.subcommands.iter().flatten() {
        if name.is_empty() || name.starts_with('-') || name == "help" {
            return Err(invalid(format!(
                "{}: subcommand name '{name}' invalid (must be non-empty, not 'help', no leading '-')",
                config.name
            )));
        }
        validate_flags(subcommand.flags.as_deref().unwrap_or_default(), name)?;
    }
    Ok(())
}

/// One command: the top one (with its subcommands) or a subcommand (with none).
struct CommandSpec<'a> {
    name: &'a str,
    description: Option<&'a str>,
    flags: &'a [Flag],
    subcommands: Option<&'a BTreeMap<String, SubcommandConfig>>,
}

impl<'a> CommandSpec<'a> {
    fn top(config: &'a Config) -> Self {
        Self {
            name: &config.name,
            description: config.description.as_deref(),
            flags: config.flags.as_deref().unwrap_or_default(),
            subcommands: config.subcommands.as_ref().filter(|subcommands| !subcommands.is_empty()),
        }
    }

    fn sub(name: &'a str, config: &'a SubcommandConfig) -> Self {
        Self {
            name,
            description: config.description.as_deref(),
            flags: config.flags.as_deref().unwrap_or_default(),
            subcommands: None,
        }
    }
}

/// `help_disabled`: the command above claimed `-h`, which in clap switches the built-in help off for every command
/// below it too.
fn build_command(spec: &CommandSpec, help_disabled: bool) -> Command {
    let mut command = Command::new(spec.name.to_owned()).no_binary_name(true);
    if let Some(description) = spec.description {
        command = command.about(description.to_owned());
    }
    for flag in spec.flags {
        command = command.arg(build_arg(flag));
    }
    // A flag that claims `-h` takes the short from clap's built-in help, and `--help` stays. Clap's
    // `disable_help_flag` also reaches the commands below, so each of those that does not claim `-h` itself gets
    // the whole `-h`/`--help` back.
    let claims_h = spec.flags.iter().any(|flag| flag.short() == Some('h'));
    if claims_h || help_disabled {
        let mut help = Arg::new("help").long("help").action(ArgAction::Help).help("Print help");
        if !claims_h {
            help = help.short('h');
        }
        command = command.disable_help_flag(true).arg(help);
    }

    match spec.subcommands {
        None => command
            .arg(Arg::new(POSITIONAL_ID).index(1).num_args(0..).action(ArgAction::Append))
            .arg(
                Arg::new(REST_ID)
                    .index(2)
                    .num_args(0..)
                    .action(ArgAction::Append)
                    .last(true)
                    .allow_hyphen_values(true),
            ),
        Some(subcommands) => {
            command = command.subcommand_required(true);
            for (name, subcommand) in subcommands {
                command = command.subcommand(build_command(&CommandSpec::sub(name, subcommand), help_disabled || claims_h));
            }
            command
        }
    }
}

fn build_arg(flag: &Flag) -> Arg {
    let mut arg = Arg::new(flag.name().to_owned()).long(flag.name().to_owned());
    if let Some(short) = flag.short() {
        arg = arg.short(short);
    }
    if let Some(desc) = flag.desc() {
        arg = arg.help(desc.to_owned());
    }
    let action = |multiple: &Option<bool>| if multiple.unwrap_or(false) { ArgAction::Append } else { ArgAction::Set };
    match flag {
        Flag::Boolean { .. } => arg.action(ArgAction::SetTrue),
        Flag::String { default, required, multiple, .. } => {
            arg = arg.num_args(1).action(action(multiple)).required(required.unwrap_or(false));
            match default {
                Some(default) => arg.default_value(default.clone()),
                None => arg,
            }
        }
        Flag::Number { default, required, multiple, .. } => {
            arg = arg
                .num_args(1)
                .action(action(multiple))
                .value_parser(value_parser!(f64))
                .allow_hyphen_values(true)
                .required(required.unwrap_or(false));
            match default {
                Some(default) => arg.default_value(default.to_string()),
                None => arg,
            }
        }
    }
}

/// One value of `id`. `ArgMatches::get_one` panics when the type is not the one the flag was built with.
fn try_one<T: Clone + Send + Sync + 'static>(matches: &ArgMatches, id: &str) -> Result<Option<T>, ArgsError> {
    matches
        .try_get_one::<T>(id)
        .map(|value| value.cloned())
        .map_err(|error| ArgsError::Usage { text: format!("flag '{id}': {error}") })
}

/// Every value of `id`; none when it was not given.
fn try_many<T: Clone + Send + Sync + 'static>(matches: &ArgMatches, id: &str) -> Result<Vec<T>, ArgsError> {
    matches
        .try_get_many::<T>(id)
        .map(|values| values.map(|values| values.cloned().collect()).unwrap_or_default())
        .map_err(|error| ArgsError::Usage { text: format!("flag '{id}': {error}") })
}

/// Each flag's value by name. A string or number flag that was not given and has no default is left out.
fn flag_values(flags: &[Flag], matches: &ArgMatches) -> Result<BTreeMap<String, FlagValue>, ArgsError> {
    let mut values = BTreeMap::new();
    for flag in flags {
        let name = flag.name();
        let value = match flag {
            Flag::Boolean { .. } => Some(FlagValue::Flag(try_one::<bool>(matches, name)?.unwrap_or(false))),
            Flag::String { multiple: Some(true), .. } => Some(FlagValue::Texts(try_many(matches, name)?)),
            Flag::String { .. } => try_one::<String>(matches, name)?.map(FlagValue::Text),
            Flag::Number { multiple: Some(true), .. } => Some(FlagValue::Numbers(try_many(matches, name)?)),
            Flag::Number { .. } => try_one::<f64>(matches, name)?.map(FlagValue::Number),
        };
        if let Some(value) = value {
            values.insert(name.to_owned(), value);
        }
    }
    Ok(values)
}

fn usage_error(error: &clap::Error) -> ArgsError {
    let text = error.to_string();
    match error.kind() {
        ErrorKind::UnknownArgument => ArgsError::UnknownArgument { text },
        ErrorKind::InvalidValue
        | ErrorKind::ValueValidation
        | ErrorKind::NoEquals
        | ErrorKind::TooManyValues
        | ErrorKind::TooFewValues
        | ErrorKind::WrongNumberOfValues => ArgsError::InvalidValue { text },
        ErrorKind::MissingRequiredArgument => ArgsError::MissingRequired { text },
        ErrorKind::MissingSubcommand => ArgsError::MissingSubcommand { text },
        ErrorKind::InvalidSubcommand => ArgsError::InvalidSubcommand { text },
        ErrorKind::ArgumentConflict => ArgsError::Conflict { text },
        _ => ArgsError::Usage { text },
    }
}

fn parse(argv: Vec<String>, config: &Config) -> Result<Outcome, ArgsError> {
    validate(config)?;
    let spec = CommandSpec::top(config);
    let matches = match build_command(&spec, false).try_get_matches_from(argv) {
        Ok(matches) => matches,
        Err(error)
            if matches!(error.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand) =>
        {
            return Ok(Outcome::Help { text: error.render().to_string() });
        }
        Err(error) => return Err(usage_error(&error)),
    };
    let flags = flag_values(spec.flags, &matches)?;
    let Some(subcommands) = spec.subcommands else {
        return Ok(Outcome::Parsed {
            flags,
            positional: try_many(&matches, POSITIONAL_ID)?,
            rest: try_many(&matches, REST_ID)?,
            subcommand: None,
        });
    };
    let subcommand = match matches.subcommand() {
        Some((name, sub_matches)) => {
            let config = subcommands
                .get(name)
                .ok_or_else(|| ArgsError::Usage { text: format!("subcommand '{name}' is not in the config") })?;
            Some(Subcommand {
                name: name.to_owned(),
                flags: flag_values(config.flags.as_deref().unwrap_or_default(), sub_matches)?,
                positional: try_many(sub_matches, POSITIONAL_ID)?,
                rest: try_many(sub_matches, REST_ID)?,
            })
        }
        None => None,
    };
    Ok(Outcome::Parsed { flags, positional: Vec::new(), rest: Vec::new(), subcommand })
}

fn help(config: &Config) -> Result<String, ArgsError> {
    validate(config)?;
    Ok(build_command(&CommandSpec::top(config), false).render_help().to_string())
}

#[mlua_ir::module("lib:args")]
pub mod args {
    use std::collections::BTreeMap;

    use mlua_ir::{LuaError, OneOf, Shape};
    use serde::{Deserialize, Serialize};

    /// A flag a command takes. Its `type` says what its value is.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
    pub enum Flag {
        /// A flag that is `true` when given and `false` when not.
        Boolean {
            /// The long name: `verbose` is `--verbose`.
            name: String,
            /// A one-character alias: `v` is `-v`.
            short: Option<String>,
            /// What the help says of it.
            desc: Option<String>,
        },
        /// A flag that takes text: `--out file` or `--out=file`.
        String {
            /// The long name: `out` is `--out`.
            name: String,
            /// A one-character alias: `o` is `-o`.
            short: Option<String>,
            /// What the help says of it.
            desc: Option<String>,
            /// Its value when it is not given.
            default: Option<String>,
            /// Whether leaving it out is an error.
            required: Option<bool>,
            /// Whether it may be given several times; its value is then the list of them.
            multiple: Option<bool>,
        },
        /// A flag that takes a number: `--count 3`, `--count=-7`.
        Number {
            /// The long name: `count` is `--count`.
            name: String,
            /// A one-character alias: `c` is `-c`.
            short: Option<String>,
            /// What the help says of it.
            desc: Option<String>,
            /// Its value when it is not given.
            default: Option<f64>,
            /// Whether leaving it out is an error.
            required: Option<bool>,
            /// Whether it may be given several times; its value is then the list of them.
            multiple: Option<bool>,
        },
    }

    /// A subcommand: its own flags, and its own positional arguments.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct SubcommandConfig {
        /// What the help says of it.
        pub description: Option<String>,
        /// Its flags, in the order the help shows them.
        #[serde(default, deserialize_with = "super::flag_list")]
        pub flags: Option<Vec<Flag>>,
    }

    /// A command line: the program's name, its flags, and its subcommands.
    #[derive(Debug, Clone, Serialize, Deserialize, Shape)]
    #[serde(deny_unknown_fields)]
    pub struct Config {
        /// The program's name, for the help and the usage line.
        pub name: String,
        /// What the help says the program does.
        pub description: Option<String>,
        /// The flags, in the order the help shows them.
        #[serde(default, deserialize_with = "super::flag_list")]
        pub flags: Option<Vec<Flag>>,
        /// The subcommands by name. With any, one of them must be given, and the positional arguments are the
        /// subcommand's.
        pub subcommands: Option<BTreeMap<String, SubcommandConfig>>,
    }

    /// What a flag was given: by its `type`, and a list when the flag is `multiple`.
    #[derive(Debug, Clone, PartialEq, Serialize, OneOf)]
    #[serde(untagged)]
    pub enum FlagValue {
        /// A boolean flag: whether it was given.
        Flag(bool),
        /// A number flag's value.
        Number(f64),
        /// A string flag's value.
        Text(String),
        /// A `multiple` number flag's values, in the order given.
        Numbers(Vec<f64>),
        /// A `multiple` string flag's values, in the order given.
        Texts(Vec<String>),
    }

    /// The subcommand that was given, and what it was given.
    #[derive(Debug, Clone, PartialEq, Serialize, Shape)]
    pub struct Subcommand {
        /// Its name.
        pub name: String,
        /// Its flags by name. A string or number flag that was not given and has no default is not there.
        pub flags: BTreeMap<String, FlagValue>,
        /// Its arguments that are not flags, in order.
        pub positional: Vec<String>,
        /// Everything after `--`, as it is.
        pub rest: Vec<String>,
    }

    /// What `parse` read.
    #[derive(Debug, Clone, PartialEq, Serialize, Shape)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum Outcome {
        /// The arguments were read.
        Parsed {
            /// The top command's flags by name. A string or number flag that was not given and has no default is
            /// not there.
            flags: BTreeMap<String, FlagValue>,
            /// The arguments that are not flags, in order. Empty when the config has subcommands: they are the
            /// subcommand's.
            positional: Vec<String>,
            /// Everything after `--`, as it is.
            rest: Vec<String>,
            /// The subcommand given, when the config has subcommands.
            subcommand: Option<Subcommand>,
        },
        /// `--help` or `-h` was given: nothing else was checked, and this is the text to show.
        Help {
            /// The help of the command it was asked of.
            text: String,
        },
    }

    /// Why arguments could not be read, or a help text written.
    #[derive(Debug, Clone, PartialEq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ArgsError {
        /// The config describes no command line: two flags with one name or one short, a reserved name, a short
        /// that is not one usable character, a subcommand that cannot be named so.
        #[error("{reason}")]
        InvalidConfig {
            /// What is wrong, and in which command.
            reason: String,
        },
        /// An argument is a flag the command does not have.
        #[error("{text}")]
        UnknownArgument {
            /// clap's words, with the usage line.
            text: String,
        },
        /// A flag's value is not what the flag takes (`--count=abc`), or is missing.
        #[error("{text}")]
        InvalidValue {
            /// clap's words, with the usage line.
            text: String,
        },
        /// A required flag was not given.
        #[error("{text}")]
        MissingRequired {
            /// clap's words, with the usage line.
            text: String,
        },
        /// The config has subcommands and none was given.
        #[error("{text}")]
        MissingSubcommand {
            /// clap's words, with the usage line.
            text: String,
        },
        /// The subcommand given is not one the config has.
        #[error("{text}")]
        InvalidSubcommand {
            /// clap's words, with the usage line.
            text: String,
        },
        /// A flag that takes one value was given twice.
        #[error("{text}")]
        Conflict {
            /// clap's words, with the usage line.
            text: String,
        },
        /// Anything else clap refuses.
        #[error("{text}")]
        Usage {
            /// clap's words.
            text: String,
        },
    }

    /// Read `argv` against `config`.
    ///
    /// ```luau
    /// local parsed = assert(args.parse({ "--verbose", "--out=a.txt", "in.txt" }, {
    ///     name = "tool",
    ///     flags = {
    ///         { name = "verbose", short = "v", type = "boolean" },
    ///         { name = "out", type = "string", default = "stdout" },
    ///     },
    /// }))
    /// if parsed.kind == "help" then
    ///     print(parsed.text)
    /// else
    ///     assert(parsed.flags.verbose == true and parsed.flags.out == "a.txt")
    ///     assert(parsed.positional[1] == "in.txt")
    /// end
    /// ```
    pub fn parse(argv: Vec<String>, config: Config) -> Result<Outcome, ArgsError> {
        super::parse(argv, &config)
    }

    /// The help text of `config`, as `--help` shows it.
    pub fn help(config: Config) -> Result<String, ArgsError> {
        super::help(&config)
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    use crate::make_engine;

    fn lua() -> Lua {
        let lua = make_engine().unwrap();
        let module: Table = lua.load(r#"return require("lib:args")"#).eval().unwrap();
        lua.globals().set("args", module).unwrap();
        lua
    }

    /// What `args.parse(argv, config)` gave: the outcome, with no error.
    fn parsed(lua: &Lua, argv: &str, config: &str) -> Table {
        let (outcome, err): (Value, Value) =
            lua.load(format!("return args.parse({argv}, {config})")).eval().unwrap();
        assert!(err.is_nil(), "{argv} {config}: {err:?}");
        outcome.as_table().unwrap_or_else(|| panic!("{argv} {config}: {outcome:?}")).clone()
    }

    /// The flags of a successful parse.
    fn flags(lua: &Lua, argv: &str, config: &str) -> Table {
        let outcome = parsed(lua, argv, config);
        assert_eq!(outcome.get::<String>("kind").unwrap(), "parsed");
        outcome.get("flags").unwrap()
    }

    /// The kind and the text of the error `call` returns.
    fn error(lua: &Lua, call: &str) -> (String, String) {
        let (value, kind, text): (Value, String, String) = lua
            .load(format!("local v, e = args.{call}\nreturn v, e.kind, tostring(e)"))
            .eval()
            .unwrap_or_else(|e| panic!("{call}: {e}"));
        assert!(value.is_nil(), "{call}");
        (kind, text)
    }

    /// What clap would panic on while it built the command (in a debug build; a release build took them with an
    /// unspecified winner) is an ordinary error: the config is a script's input.
    #[test]
    fn hostile_configs_are_errors_not_clap_panics() {
        let lua = lua();
        let configs = [
            r#"{name="t",flags={{name="a",type="boolean"},{name="a",type="string"}}}"#,
            r#"{name="t",flags={{name="help",type="boolean"}}}"#,
            r#"{name="t",flags={{name="__positional",type="number"}}}"#,
            r#"{name="t",flags={{name="__rest",type="string"}}}"#,
            r#"{name="t",flags={{name="a",short="b",type="boolean"},{name="b",short="b",type="boolean"}}}"#,
            r#"{name="t",flags={{name="--a",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="-",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="=",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short=" ",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="ab",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="",type="boolean"}}}"#,
            r#"{name="t",flags={{name="",type="boolean"}}}"#,
            r#"{name="",flags={}}"#,
            r#"{name="t",subcommands={help={}}}"#,
            r#"{name="t",subcommands={["-x"]={}}}"#,
            r#"{name="t",subcommands={a={flags={{name="x",type="string"},{name="x",type="string"}}}}}"#,
        ];
        for config in configs {
            for call in ["parse({}, CONFIG)", "parse({\"--a\", \"1\"}, CONFIG)", "parse({\"-h\"}, CONFIG)", "help(CONFIG)"] {
                let (kind, text) = error(&lua, &call.replace("CONFIG", config));
                assert_eq!(kind, "invalid_config", "{call} {config}: {text}");
            }
        }
        let (_, twice) = error(&lua, &format!("help({})", configs[0]));
        assert_eq!(twice, "lib:args.help: invalid_config: t: flag '--a' is declared twice");
        // the valid neighbours still work
        let flags = flags(
            &lua,
            r#"{"-b", "--a", "x"}"#,
            r#"{ name = "t", flags = {
                { name = "a", short = "a", type = "string" }, { name = "b", short = "b", type = "boolean" } } }"#,
        );
        assert!(flags.get::<bool>("b").unwrap());
        assert_eq!(flags.get::<String>("a").unwrap(), "x");
    }

    /// `-h` is the usual short for `--host`: a flag that claims it takes it from clap's built-in help, which keeps
    /// `--help`. Commands whose flags do not claim it still answer `-h`.
    #[test]
    fn a_flag_may_claim_the_short_h() {
        let lua = lua();
        let flags_of = flags(
            &lua,
            r#"{"-h", "example.org", "-p", "80"}"#,
            r#"{ name = "t", flags = {
                { name = "host", short = "h", type = "string" }, { name = "port", short = "p", type = "number" } } }"#,
        );
        assert_eq!(flags_of.get::<String>("host").unwrap(), "example.org");
        assert_eq!(flags_of.get::<f64>("port").unwrap(), 80.0);
        // `--help` still works for it, and so does `-h` where no flag claimed it
        for (argv, flag) in [
            (r#"{"--help"}"#, r#"{ name = "host", short = "h", type = "string" }"#),
            (r#"{"-h"}"#, r#"{ name = "all", type = "boolean" }"#),
        ] {
            let outcome = parsed(
                &lua,
                argv,
                &format!(r#"{{ name = "t", description = "tool", flags = {{ {flag} }} }}"#),
            );
            assert_eq!(outcome.get::<String>("kind").unwrap(), "help", "{argv}");
            assert!(outcome.get::<String>("text").unwrap().contains("tool"));
        }
        // subcommands keep their help when the top level claims `-h`, and one that claims it too keeps `--help`
        let top = r#"{ name = "t", flags = { { name = "host", short = "h", type = "string" } }, subcommands = {
            run = { description = "run it", flags = {} },
            serve = { description = "serve it", flags = { { name = "hash", short = "h", type = "string" } } },
        } }"#;
        for (argv, text) in
            [(r#"{"run","--help"}"#, "run it"), (r#"{"run","-h"}"#, "run it"), (r#"{"serve","--help"}"#, "serve it")]
        {
            let outcome = parsed(&lua, argv, top);
            assert_eq!(outcome.get::<String>("kind").unwrap(), "help", "{argv}");
            assert!(outcome.get::<String>("text").unwrap().contains(text), "{argv}");
        }
        let serve: Table = parsed(&lua, r#"{"serve","-h","abc"}"#, top).get("subcommand").unwrap();
        assert_eq!(serve.get::<String>("name").unwrap(), "serve");
        assert_eq!(serve.get::<Table>("flags").unwrap().get::<String>("hash").unwrap(), "abc");
        // and a flag claiming `-h` does not turn `-h` into a help request
        let outcome =
            parsed(&lua, r#"{"-h", "x"}"#, r#"{ name = "t", flags = { { name = "host", short = "h", type = "string" } } }"#);
        assert_eq!(outcome.get::<String>("kind").unwrap(), "parsed");
    }

    #[test]
    fn boolean_flag_is_its_presence() {
        let lua = lua();
        let config = r#"{ name = "t", flags = { { name = "verbose", short = "v", type = "boolean" } } }"#;
        assert!(flags(&lua, r#"{"--verbose"}"#, config).get::<bool>("verbose").unwrap());
        assert!(flags(&lua, r#"{"-v"}"#, config).get::<bool>("verbose").unwrap(), "the short alias");
        assert!(!flags(&lua, "{}", config).get::<bool>("verbose").unwrap(), "absent is false, not nil");
    }

    #[test]
    fn string_flag_takes_both_forms() {
        let lua = lua();
        let config = r#"{ name = "t", flags = { { name = "out", short = "o", type = "string" } } }"#;
        for argv in [r#"{"--out", "file.txt"}"#, r#"{"--out=file.txt"}"#, r#"{"-o", "file.txt"}"#] {
            assert_eq!(flags(&lua, argv, config).get::<String>("out").unwrap(), "file.txt", "{argv}");
        }
        assert!(flags(&lua, "{}", config).get::<Value>("out").unwrap().is_nil(), "not given, no default");
    }

    #[test]
    fn number_flag_is_a_number() {
        let lua = lua();
        let config = r#"{ name = "t", flags = { { name = "count", type = "number" } } }"#;
        assert_eq!(flags(&lua, r#"{"--count", "42"}"#, config).get::<f64>("count").unwrap(), 42.0);
        assert_eq!(flags(&lua, r#"{"--count=-7"}"#, config).get::<f64>("count").unwrap(), -7.0);
        assert_eq!(flags(&lua, r#"{"--count", "1.5"}"#, config).get::<f64>("count").unwrap(), 1.5);
        let (kind, text) = error(&lua, &format!(r#"parse({{"--count=abc"}}, {config})"#));
        assert_eq!(kind, "invalid_value");
        assert!(text.starts_with("lib:args.parse: invalid_value: ") && text.contains("abc"), "{text}");
    }

    #[test]
    fn required_flag_missing_is_named() {
        let lua = lua();
        let (kind, text) = error(
            &lua,
            r#"parse({}, { name = "t", flags = { { name = "out", type = "string", required = true } } })"#,
        );
        assert_eq!(kind, "missing_required");
        assert!(text.contains("--out"), "{text}");
    }

    #[test]
    fn unknown_flag_and_a_repeated_one_are_errors() {
        let lua = lua();
        let config = r#"{ name = "t", flags = { { name = "out", type = "string" } } }"#;
        let (kind, text) = error(&lua, &format!(r#"parse({{"--nope"}}, {config})"#));
        assert_eq!(kind, "unknown_argument");
        assert!(text.contains("--nope"), "{text}");
        let (kind, _) = error(&lua, &format!(r#"parse({{"--out", "a", "--out", "b"}}, {config})"#));
        assert_eq!(kind, "conflict");
    }

    #[test]
    fn defaults_applied_when_absent() {
        let lua = lua();
        let flags = flags(
            &lua,
            "{}",
            r#"{ name = "t", flags = {
                { name = "verbose", type = "boolean" },
                { name = "count", type = "number", default = 3 },
                { name = "out", type = "string", default = "stdout" },
            } }"#,
        );
        assert!(!flags.get::<bool>("verbose").unwrap());
        assert_eq!(flags.get::<f64>("count").unwrap(), 3.0);
        assert_eq!(flags.get::<String>("out").unwrap(), "stdout");
    }

    #[test]
    fn multiple_values_are_collected_in_order() {
        let lua = lua();
        let config = r#"{ name = "t", flags = {
            { name = "inc", type = "string", multiple = true }, { name = "n", type = "number", multiple = true } } }"#;
        let flags_of = flags(&lua, r#"{"--inc", "a", "--n", "2", "--inc", "b", "--n=-1"}"#, config);
        assert_eq!(flags_of.get::<Vec<String>>("inc").unwrap(), ["a", "b"]);
        assert_eq!(flags_of.get::<Vec<f64>>("n").unwrap(), [2.0, -1.0]);
        let none = flags(&lua, "{}", config);
        assert_eq!(none.get::<Table>("inc").unwrap().raw_len(), 0, "a list, though empty");
        assert_eq!(none.get::<Table>("n").unwrap().raw_len(), 0);
    }

    #[test]
    fn positional_and_rest_are_collected() {
        let lua = lua();
        let config = r#"{ name = "t", flags = {} }"#;
        let outcome = parsed(&lua, r#"{"x", "y", "--", "--y", "z"}"#, config);
        assert_eq!(outcome.get::<Vec<String>>("positional").unwrap(), ["x", "y"]);
        assert_eq!(outcome.get::<Vec<String>>("rest").unwrap(), ["--y", "z"]);
        // both are lists even when empty
        let outcome = parsed(&lua, "{}", r#"{ name = "t" }"#);
        assert_eq!(outcome.get::<Table>("positional").unwrap().raw_len(), 0);
        assert_eq!(outcome.get::<Table>("rest").unwrap().raw_len(), 0);
        assert!(outcome.get::<Value>("subcommand").unwrap().is_nil());
    }

    #[test]
    fn subcommand_routed() {
        let lua = lua();
        let config = r#"{ name = "t", flags = { { name = "verbose", type = "boolean" } }, subcommands = {
            build = { flags = { { name = "release", type = "boolean" } } },
            test  = { flags = {} },
        } }"#;
        let outcome = parsed(&lua, r#"{"--verbose", "build", "--release", "input.txt", "--", "-x"}"#, config);
        assert!(outcome.get::<Table>("flags").unwrap().get::<bool>("verbose").unwrap());
        assert_eq!(outcome.get::<Table>("positional").unwrap().raw_len(), 0, "they are the subcommand's");
        let sub: Table = outcome.get("subcommand").unwrap();
        assert_eq!(sub.get::<String>("name").unwrap(), "build");
        assert!(sub.get::<Table>("flags").unwrap().get::<bool>("release").unwrap());
        assert_eq!(sub.get::<Vec<String>>("positional").unwrap(), ["input.txt"]);
        assert_eq!(sub.get::<Vec<String>>("rest").unwrap(), ["-x"]);

        let (kind, _) = error(&lua, &format!("parse({{}}, {config})"));
        assert_eq!(kind, "missing_subcommand");
        let (kind, text) = error(&lua, &format!(r#"parse({{"deploy"}}, {config})"#));
        assert_eq!(kind, "invalid_subcommand", "{text}");
    }

    #[test]
    fn help_short_circuits() {
        let lua = lua();
        let outcome = parsed(&lua, r#"{"--help"}"#, r#"{ name = "t", description = "tool", flags = {} }"#);
        assert_eq!(outcome.get::<String>("kind").unwrap(), "help");
        assert!(outcome.get::<String>("text").unwrap().contains("tool"));
        // a required flag is not asked for
        let outcome = parsed(
            &lua,
            r#"{"-h"}"#,
            r#"{ name = "t", flags = { { name = "out", type = "string", required = true } } }"#,
        );
        assert_eq!(outcome.get::<String>("kind").unwrap(), "help");
        // before a subcommand it is the top command's help, after it the subcommand's
        let config = r#"{ name = "t", description = "the tool", subcommands = {
            build = { description = "build it", flags = { { name = "release", type = "boolean" } } } } }"#;
        let top: String = parsed(&lua, r#"{"--help", "build"}"#, config).get("text").unwrap();
        assert!(top.contains("the tool") && top.contains("build"), "{top}");
        let sub: String = parsed(&lua, r#"{"build", "--help"}"#, config).get("text").unwrap();
        assert!(sub.contains("build it") && sub.contains("--release"), "{sub}");
    }

    #[test]
    fn a_config_of_the_wrong_shape_is_refused_as_an_argument() {
        let lua = lua();
        for config in [
            r"{ flags = {} }",
            r#"{ name = "t", flags = { { name = "x", type = "object" } } }"#,
            r#"{ name = "t", flags = { { name = "x" } } }"#,
            r#"{ name = "t", flags = { { type = "string" } } }"#,
            r#"{ name = "t", flags = { { name = "v", type = "boolean", multiple = true } } }"#,
            r#"{ name = "t", flags = { { name = "v", type = "boolean", default = true } } }"#,
            r#"{ name = "t", flags = { { name = "n", type = "number", default = "three" } } }"#,
            r#"{ name = "t", flags = { { name = "s", type = "string", default = 3 } } }"#,
            r#"{ name = "t", flags = { x = { type = "string" } } }"#,
            r#"{ name = "t", subcommands = { build = { flags = {}, subcommands = { x = { flags = {} } } } } }"#,
            r#"{ name = "t", flagz = {} }"#,
        ] {
            let err = lua
                .load(format!("return args.parse({{}}, {config})"))
                .eval::<Value>()
                .expect_err(config)
                .to_string();
            assert!(err.contains("lib:args.parse: argument #2 'config'"), "{config}: {err}");
            let err = lua.load(format!("return args.help({config})")).eval::<Value>().expect_err(config).to_string();
            assert!(err.contains("lib:args.help: argument #1 'config'"), "{config}: {err}");
        }
        let err = lua
            .load(r#"return args.parse({ "a", 5 }, { name = "t" })"#)
            .eval::<Value>()
            .expect_err("argv is strings")
            .to_string();
        assert!(err.contains("lib:args.parse: argument #1 'argv'"), "{err}");
    }

    #[test]
    fn help_function_renders_flags_in_the_order_given() {
        let lua = lua();
        let text: String = lua
            .load(
                r#"return (assert(args.help({ name = "tool", description = "do work", flags = {
                    { name = "zeta", type = "boolean", desc = "the last letter" },
                    { name = "alpha", short = "a", type = "string" },
                }, })))"#,
            )
            .eval()
            .unwrap();
        assert!(text.contains("tool") && text.contains("do work") && text.contains("the last letter"), "{text}");
        assert!(text.contains("-a, --alpha") && text.contains("-h, --help"), "{text}");
        let (zeta, alpha) = (text.find("--zeta").unwrap(), text.find("--alpha").unwrap());
        assert!(zeta < alpha, "the order given, not the alphabet's:\n{text}");
        let listed: String = lua
            .load(r#"return (assert(args.help({ name = "tool", subcommands = { build = { description = "build it" } } })))"#)
            .eval()
            .unwrap();
        assert!(listed.contains("build") && listed.contains("build it"), "{listed}");
    }
}

//! CLI argument parsing — `lib:args`. Backed by `clap`.
//!
//! Public surface:
//! - `args.parse(argv: { string }, config) -> (parsed, err)`
//! - `args.help(config) -> (string, err)`
//!
//! Behaviour follows clap defaults — booleans are presence-only, error wording
//! is clap's, help formatting tracks clap's renderer version.
//!
//! A spec clap would panic on (a debug build; a release build accepted it with an unspecified winner) is an
//! error instead: duplicate or reserved flag names (`help`, `__positional`, `__rest`), two flags with the same
//! short, a short that is `-`, `=` or whitespace, a long name starting with `-`, a subcommand named `help`.
//! A flag may claim the short `-h` (for `--host`, say): it takes it from the built-in help, which keeps
//! `--help`.

use std::collections::HashSet;

use clap::{Arg, ArgAction, ArgMatches, Command, error::ErrorKind, value_parser};
use mlua::{Lua, Table, Value};

use crate::lua::facade::Module;

// ---------------------------------------------------------------------------
// Config types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlagType {
    Boolean,
    String,
    Number,
}

impl FlagType {
    fn name(self) -> &'static str {
        match self {
            FlagType::Boolean => "boolean",
            FlagType::String => "string",
            FlagType::Number => "number",
        }
    }
}

#[derive(Clone, Debug)]
struct FlagSpec {
    name: String,
    short: Option<char>,
    ty: FlagType,
    default: Option<DefaultValue>,
    required: bool,
    multiple: bool,
    desc: Option<String>,
}

#[derive(Clone, Debug)]
enum DefaultValue {
    String(String),
    Number(f64),
}

#[derive(Clone, Debug)]
struct CommandSpec {
    name: String,
    description: Option<String>,
    flags: Vec<FlagSpec>,
    subcommands: Vec<CommandSpec>,
}

// ---------------------------------------------------------------------------
// Lua entry points
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:args` surface through the facade. Both functions are
/// fallible: `parse` returns the parsed-config table, `help` the rendered help
/// text. The `(value, error)` shape, the `args.parse` / `args.help` error
/// prefixes, and the `lua::lib::args` spans are byte-identical to the
/// hand-written registration this replaces.
#[neoc_macros::lua_module]
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:args")?;
    /// Parse `argv` against `config`, returning the parsed-flags table (or, on `--help`/`-h`, a `{ help = true, help_text = ... }` table).
    #[lua(names(argv_tbl = "argv", config_tbl = "config"))]
    m.try_func("parse", |lua, (argv_tbl, config_tbl): (Table, Table)| {
        parse(lua, &argv_tbl, &config_tbl)
    })?;
    /// Render `config`'s help text without parsing any arguments.
    #[lua(names(config_tbl = "config"))]
    m.try_func("help", |_, config_tbl: Table| help(&config_tbl))?;
    Ok(m)
}

fn parse(lua: &Lua, argv_tbl: &Table, config_tbl: &Table) -> Result<Table, String> {
    let argv = read_argv(argv_tbl)?;
    let spec = read_command(config_tbl, true)?;
    let cmd = build_command(&spec, false);
    match cmd.try_get_matches_from(argv) {
        Ok(matches) => build_parsed_table(lua, &spec, &matches).map_err(|e| e.to_string()),
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            let text = err.render().to_string();
            build_help_table(lua, &text).map_err(|e| e.to_string())
        }
        Err(err) => Err(err.to_string()),
    }
}

fn help(config_tbl: &Table) -> Result<String, String> {
    let spec = read_command(config_tbl, true)?;
    let mut cmd = build_command(&spec, false);
    Ok(cmd.render_help().to_string())
}

// ---------------------------------------------------------------------------
// Config marshalling — Lua table → CommandSpec
// ---------------------------------------------------------------------------

fn read_argv(t: &Table) -> Result<Vec<String>, String> {
    // not presized: `#t` is whatever border Lua finds, so a forged table can report 2^30
    let mut out = Vec::new();
    for entry in t.clone().sequence_values::<mlua::LuaString>() {
        let s = entry.map_err(|e| format!("argv must be a string array: {e}"))?;
        let s = s
            .to_str()
            .map_err(|e| format!("argv contains non-utf8: {e}"))?;
        out.push(s.to_string());
    }
    Ok(out)
}

fn read_command(t: &Table, top_level: bool) -> Result<CommandSpec, String> {
    let name = match t.get::<Option<String>>("name").map_err(stringify)? {
        Some(n) if !n.is_empty() => n,
        _ if top_level => return Err("config.name is required".into()),
        _ => String::new(),
    };
    let description = t.get::<Option<String>>("description").map_err(stringify)?;
    let flags = match t.get::<Option<Value>>("flags").map_err(stringify)? {
        Some(Value::Nil) | None => Vec::new(),
        Some(Value::Table(ft)) => read_flags(&ft, &name)?,
        Some(_) => return Err(format!("{}: flags must be a table", err_ctx(&name))),
    };
    let subcommands = match t.get::<Option<Value>>("subcommands").map_err(stringify)? {
        Some(Value::Nil) | None => Vec::new(),
        Some(Value::Table(st)) => read_subcommands(&st, &name)?,
        Some(_) => return Err(format!("{}: subcommands must be a table", err_ctx(&name))),
    };
    Ok(CommandSpec {
        name,
        description,
        flags,
        subcommands,
    })
}

/// Ids clap (or this module's own positional arguments) already use in every command. A flag with one of
/// these names would collide with them.
const RESERVED_FLAG_NAMES: &[&str] = &["help", POSITIONAL_ID, REST_ID];
const POSITIONAL_ID: &str = "__positional";
const REST_ID: &str = "__rest";

/// Reject specs clap would panic on while building the command (duplicate or reserved ids, a clashing
/// short, a long that starts with a dash): a spec is script input, and a panic here takes the host down.
fn validate_flags(flags: &[FlagSpec], ctx: &str) -> Result<(), String> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut shorts: HashSet<char> = HashSet::new();
    for flag in flags {
        let name = flag.name.as_str();
        if RESERVED_FLAG_NAMES.contains(&name) {
            return Err(format!("{}: flag name '{name}' is reserved", err_ctx(ctx)));
        }
        if name.starts_with('-') {
            return Err(format!(
                "{}: flag name '{name}' must not start with '-'",
                err_ctx(ctx)
            ));
        }
        if !names.insert(name) {
            return Err(format!(
                "{}: flag '--{name}' is declared twice",
                err_ctx(ctx)
            ));
        }
        if let Some(c) = flag.short {
            if c == '-' || c.is_whitespace() || c == '=' {
                return Err(format!(
                    "{}: flag '--{name}' short alias {c:?} is not allowed",
                    err_ctx(ctx)
                ));
            }
            if !shorts.insert(c) {
                return Err(format!(
                    "{}: short alias '-{c}' is used by two flags",
                    err_ctx(ctx)
                ));
            }
        }
    }
    Ok(())
}

fn read_flags(t: &Table, ctx: &str) -> Result<Vec<FlagSpec>, String> {
    let flags = read_flag_list(t, ctx)?;
    validate_flags(&flags, ctx)?;
    Ok(flags)
}

fn read_flag_list(t: &Table, ctx: &str) -> Result<Vec<FlagSpec>, String> {
    if t.raw_len() > 0 {
        let mut out = Vec::new();
        for entry in t.clone().sequence_values::<Table>() {
            let entry = entry.map_err(|e| format!("{}: {e}", err_ctx(ctx)))?;
            out.push(read_flag_entry(&entry, None, ctx)?);
        }
        return Ok(out);
    }
    let mut out: Vec<FlagSpec> = Vec::new();
    for pair in t.clone().pairs::<String, Table>() {
        let (name, body) = pair.map_err(|e| format!("{}: {e}", err_ctx(ctx)))?;
        out.push(read_flag_entry(&body, Some(name), ctx)?);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn read_flag_entry(
    body: &Table,
    name_override: Option<String>,
    ctx: &str,
) -> Result<FlagSpec, String> {
    let name = match name_override {
        Some(n) => n,
        None => body
            .get::<Option<String>>("name")
            .map_err(stringify)?
            .ok_or_else(|| format!("{}: flag entry missing 'name'", err_ctx(ctx)))?,
    };
    if name.is_empty() {
        return Err(format!("{}: flag name is empty", err_ctx(ctx)));
    }

    let ty_str = body
        .get::<Option<String>>("type")
        .map_err(stringify)?
        .ok_or_else(|| format!("{}: flag '--{}' missing 'type'", err_ctx(ctx), name))?;
    let ty = match ty_str.as_str() {
        "boolean" => FlagType::Boolean,
        "string" => FlagType::String,
        "number" => FlagType::Number,
        other => {
            return Err(format!(
                "{}: flag '--{}' has invalid type '{}' (expected boolean, string, or number)",
                err_ctx(ctx),
                name,
                other
            ));
        }
    };

    let short = match body.get::<Option<String>>("short").map_err(stringify)? {
        None => None,
        Some(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => {
                    return Err(format!(
                        "{}: flag '--{}' short alias '{}' must be a single character",
                        err_ctx(ctx),
                        name,
                        s
                    ));
                }
            }
        }
    };

    let required = body
        .get::<Option<bool>>("required")
        .map_err(stringify)?
        .unwrap_or(false);
    let multiple = body
        .get::<Option<bool>>("multiple")
        .map_err(stringify)?
        .unwrap_or(false);
    if multiple && ty == FlagType::Boolean {
        return Err(format!(
            "{}: flag '--{}' cannot be both boolean and multiple",
            err_ctx(ctx),
            name
        ));
    }

    let default = read_default(body, ty, &name, ctx)?;
    let desc = body.get::<Option<String>>("desc").map_err(stringify)?;

    Ok(FlagSpec {
        name,
        short,
        ty,
        default,
        required,
        multiple,
        desc,
    })
}

fn read_default(
    body: &Table,
    ty: FlagType,
    name: &str,
    ctx: &str,
) -> Result<Option<DefaultValue>, String> {
    match body.get::<Value>("default").map_err(stringify)? {
        Value::Nil => Ok(None),
        // Boolean defaults are ignored — clap booleans are presence-only,
        // so the absent value is always `false` (the action's natural default).
        Value::Boolean(_) if ty == FlagType::Boolean => Ok(None),
        Value::String(s) if ty == FlagType::String => Ok(Some(DefaultValue::String(
            s.to_str().map_err(stringify)?.to_string(),
        ))),
        Value::Integer(i) if ty == FlagType::Number => Ok(Some(DefaultValue::Number(i as f64))),
        Value::Number(n) if ty == FlagType::Number => Ok(Some(DefaultValue::Number(n))),
        _ => Err(format!(
            "{}: flag '--{}' default does not match type '{}'",
            err_ctx(ctx),
            name,
            ty.name()
        )),
    }
}

fn read_subcommands(t: &Table, ctx: &str) -> Result<Vec<CommandSpec>, String> {
    let mut out = Vec::new();
    for pair in t.clone().pairs::<String, Table>() {
        let (name, body) = pair.map_err(|e| format!("{}: subcommands: {e}", err_ctx(ctx)))?;
        if name.is_empty() || name.starts_with('-') || name == "help" {
            return Err(format!(
                "{}: subcommand name '{}' invalid (must be non-empty, not 'help', no leading '-')",
                err_ctx(ctx),
                name
            ));
        }
        let mut sub = read_command(&body, false)?;
        sub.name = name;
        if !sub.subcommands.is_empty() {
            return Err(format!(
                "{}: subcommand '{}' cannot declare nested subcommands (single-depth only)",
                err_ctx(ctx),
                sub.name
            ));
        }
        out.push(sub);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn err_ctx(ctx: &str) -> &str {
    if ctx.is_empty() { "config" } else { ctx }
}

fn stringify<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Build clap::Command from CommandSpec
// ---------------------------------------------------------------------------

/// `help_disabled`: an ancestor claimed `-h`, which in clap 4.6 switches the built-in help off for every
/// command below it too.
fn build_command(spec: &CommandSpec, help_disabled: bool) -> Command {
    let mut cmd = Command::new(spec.name.clone()).no_binary_name(true);
    if let Some(desc) = &spec.description {
        cmd = cmd.about(desc.clone());
    }

    for flag in &spec.flags {
        cmd = cmd.arg(build_arg(flag));
    }
    // A flag that claims `-h` (`--host`, say) takes the short from clap's built-in help and `--help` stays.
    // Clap's `disable_help_flag` also reaches the commands below, so each of those that does not claim `-h`
    // itself gets the full `-h`/`--help` back.
    let claims_h = spec.flags.iter().any(|f| f.short == Some('h'));
    if claims_h || help_disabled {
        let mut help = Arg::new("help")
            .long("help")
            .action(ArgAction::Help)
            .help("Print help");
        if !claims_h {
            help = help.short('h');
        }
        cmd = cmd.disable_help_flag(true).arg(help);
    }

    if spec.subcommands.is_empty() {
        cmd = cmd
            .arg(
                Arg::new(POSITIONAL_ID)
                    .index(1)
                    .num_args(0..)
                    .action(ArgAction::Append),
            )
            .arg(
                Arg::new(REST_ID)
                    .index(2)
                    .num_args(0..)
                    .action(ArgAction::Append)
                    .last(true)
                    .allow_hyphen_values(true),
            );
    } else {
        cmd = cmd.subcommand_required(true);
        for sub in &spec.subcommands {
            cmd = cmd.subcommand(build_command(sub, help_disabled || claims_h));
        }
    }

    cmd
}

fn build_arg(flag: &FlagSpec) -> Arg {
    let mut arg = Arg::new(flag.name.clone()).long(flag.name.clone());
    if let Some(c) = flag.short {
        arg = arg.short(c);
    }
    if let Some(desc) = &flag.desc {
        arg = arg.help(desc.clone());
    }

    match flag.ty {
        FlagType::Boolean => {
            arg = arg.action(ArgAction::SetTrue);
        }
        FlagType::String => {
            arg = arg.num_args(1).action(if flag.multiple {
                ArgAction::Append
            } else {
                ArgAction::Set
            });
            if flag.required {
                arg = arg.required(true);
            }
            if let Some(DefaultValue::String(s)) = &flag.default {
                arg = arg.default_value(s.clone());
            }
        }
        FlagType::Number => {
            arg = arg
                .num_args(1)
                .action(if flag.multiple {
                    ArgAction::Append
                } else {
                    ArgAction::Set
                })
                .value_parser(value_parser!(f64))
                .allow_hyphen_values(true);
            if flag.required {
                arg = arg.required(true);
            }
            if let Some(DefaultValue::Number(n)) = &flag.default {
                arg = arg.default_value(n.to_string());
            }
        }
    }

    arg
}

// ---------------------------------------------------------------------------
// Map ArgMatches → Lua table
// ---------------------------------------------------------------------------

fn build_parsed_table(lua: &Lua, spec: &CommandSpec, matches: &ArgMatches) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    for flag in &spec.flags {
        let val = flag_value_to_lua(lua, flag, matches)?;
        t.set(flag.name.clone(), val)?;
    }

    let (positional, rest): (Vec<String>, Vec<String>) = if spec.subcommands.is_empty() {
        (
            try_many(matches, POSITIONAL_ID)?,
            try_many(matches, REST_ID)?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    t.set("positional", strings_to_table(lua, &positional)?)?;
    t.set("rest", strings_to_table(lua, &rest)?)?;

    if let Some((sub_name, sub_matches)) = matches.subcommand() {
        let sub_spec = spec
            .subcommands
            .iter()
            .find(|s| s.name == sub_name)
            .ok_or_else(|| {
                mlua::Error::external(format!(
                    "args.parse: subcommand '{sub_name}' not found in spec"
                ))
            })?;
        let sub_table = build_parsed_table(lua, sub_spec, sub_matches)?;
        t.set("subcommand", sub_name.to_string())?;
        t.set(sub_name.to_string(), sub_table)?;
    }

    Ok(t)
}

fn flag_value_to_lua(lua: &Lua, flag: &FlagSpec, matches: &ArgMatches) -> mlua::Result<Value> {
    match flag.ty {
        FlagType::Boolean => Ok(Value::Boolean(
            try_one::<bool>(matches, &flag.name)?.unwrap_or(false),
        )),
        FlagType::String => {
            if flag.multiple {
                let items: Vec<String> = try_many(matches, &flag.name)?;
                Ok(Value::Table(strings_to_table(lua, &items)?))
            } else {
                match try_one::<String>(matches, &flag.name)? {
                    Some(s) => Ok(Value::String(lua.create_string(&s)?)),
                    None => Ok(Value::Nil),
                }
            }
        }
        FlagType::Number => {
            if flag.multiple {
                let items: Vec<f64> = try_many(matches, &flag.name)?;
                let t = lua.create_table()?;
                for (i, n) in items.iter().enumerate() {
                    t.set(i + 1, *n)?;
                }
                Ok(Value::Table(t))
            } else {
                match try_one::<f64>(matches, &flag.name)? {
                    Some(n) => Ok(Value::Number(n)),
                    None => Ok(Value::Nil),
                }
            }
        }
    }
}

/// One value of `id`, with a type mismatch as an error. `ArgMatches::get_one` panics on one.
fn try_one<T: Clone + Send + Sync + 'static>(
    matches: &ArgMatches,
    id: &str,
) -> mlua::Result<Option<T>> {
    matches
        .try_get_one::<T>(id)
        .map(|v: Option<&T>| v.cloned())
        .map_err(|e| mlua::Error::external(format!("args.parse: flag '{id}': {e}")))
}

/// Every value of `id` (none when absent), with a type mismatch as an error.
fn try_many<T: Clone + Send + Sync + 'static>(
    matches: &ArgMatches,
    id: &str,
) -> mlua::Result<Vec<T>> {
    matches
        .try_get_many::<T>(id)
        .map(|v| v.map(|m| m.cloned().collect()).unwrap_or_default())
        .map_err(|e| mlua::Error::external(format!("args.parse: flag '{id}': {e}")))
}

fn strings_to_table(lua: &Lua, v: &[String]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (i, s) in v.iter().enumerate() {
        t.set(i + 1, s.clone())?;
    }
    Ok(t)
}

fn build_help_table(lua: &Lua, help_text: &str) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("positional", lua.create_table()?)?;
    t.set("rest", lua.create_table()?)?;
    t.set("help", true)?;
    t.set("help_text", help_text)?;
    Ok(t)
}

// ---------------------------------------------------------------------------
// Benches — focused on the public surface, gated behind the `bench` feature
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use mlua::Lua;

    let lua = Lua::new();
    let args_tbl = module(&lua).expect("module construct");
    lua.globals().set("args", args_tbl).expect("globals set");

    c.bench_function("lib::args::parse_simple", |b| {
        b.iter(|| {
            let _: (mlua::Value, mlua::Value) = lua
                .load(
                    r#"return args.parse(
                        {"--verbose", "--out", "file", "extra"},
                        { name = "t", flags = {
                            verbose = { type = "boolean" },
                            out = { type = "string" },
                        } }
                    )"#,
                )
                .eval()
                .expect("eval simple");
        });
    });

    c.bench_function("lib::args::parse_subcommand", |b| {
        b.iter(|| {
            let _: (mlua::Value, mlua::Value) = lua
                .load(
                    r#"return args.parse(
                        {"build", "--release", "input.txt"},
                        { name = "t", subcommands = {
                            build = { flags = { release = { type = "boolean" } } },
                            test  = { flags = {} },
                        } }
                    )"#,
                )
                .eval()
                .expect("eval subcommand");
        });
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    fn lua_with_args() -> Lua {
        let lua = Lua::new();
        let args_tbl = module(&lua).expect("module construct");
        lua.globals()
            .set("args", args_tbl)
            .expect("globals set args");
        lua
    }

    fn parse_call(lua: &Lua, src: &str) -> (mlua::Value, mlua::Value) {
        lua.load(src).eval().expect("eval parse_call")
    }

    /// Duplicate or reserved ids, a clashing short or a dashed long made clap panic in a debug build while
    /// it built the command (a release build accepted them silently, with an unspecified winner) or when a
    /// getter read the wrong type; the spec is script input, so each is an ordinary `(nil, err)` instead.
    #[test]
    fn hostile_specs_are_errors_not_clap_panics() {
        let lua = lua_with_args();
        let specs = [
            r#"{name="t",flags={{name="a",type="boolean"},{name="a",type="string"}}}"#,
            r#"{name="t",flags={help={type="boolean"}}}"#,
            r#"{name="t",flags={__positional={type="number"}}}"#,
            r#"{name="t",flags={__rest={type="string"}}}"#,
            r#"{name="t",flags={{name="a",short="b",type="boolean"},{name="b",short="b",type="boolean"}}}"#,
            r#"{name="t",flags={{name="--a",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="-",type="boolean"}}}"#,
            r#"{name="t",flags={{name="a",short="=",type="boolean"}}}"#,
            r#"{name="t",subcommands={help={}}}"#,
            r#"{name="t",subcommands={a={flags={{name="x",type="string"},{name="x",type="string"}}}}}"#,
        ];
        for spec in specs {
            for call in [
                "parse({}, SPEC)",
                "parse({\"--a\", \"1\"}, SPEC)",
                "parse({\"-h\"}, SPEC)",
                "help(SPEC)",
            ] {
                let src = format!("return args.{}", call.replace("SPEC", spec));
                let (v, e): (mlua::Value, mlua::Value) =
                    lua.load(&src).eval().expect("no panic, no throw");
                assert!(matches!(v, mlua::Value::Nil), "{src} -> {v:?}");
                assert!(
                    matches!(e, mlua::Value::String(_)),
                    "{src} should give an error string, got {e:?}"
                );
            }
        }
        // the valid neighbours still work
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({"-b", "--a", "x"}, { name = "t", flags = {
                { name = "a", short = "a", type = "string" }, { name = "b", short = "b", type = "boolean" } } })"#,
        );
        assert!(matches!(err, mlua::Value::Nil), "{err:?}");
        assert!(parsed.as_table().unwrap().get::<bool>("b").unwrap());
    }

    /// `-h` is the usual short for `--host`: a flag that claims it takes it from clap's built-in help, which
    /// keeps `--help`. Commands whose flags do not claim it still answer `-h`.
    #[test]
    fn a_flag_may_claim_the_short_h() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({"-h", "example.org", "-p", "80"}, { name = "t", flags = {
                { name = "host", short = "h", type = "string" }, { name = "port", short = "p", type = "number" } } })"#,
        );
        assert!(matches!(err, mlua::Value::Nil), "{err:?}");
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<String>("host").unwrap(), "example.org");
        assert_eq!(t.get::<f64>("port").unwrap(), 80.0);
        // `--help` still works for it, and so does `-h` where no flag claimed it
        for (argv, flag) in [
            (
                r#"{"--help"}"#,
                r#"{ name = "host", short = "h", type = "string" }"#,
            ),
            (r#"{"-h"}"#, r#"{ name = "all", type = "boolean" }"#),
        ] {
            let (parsed, _) = parse_call(
                &lua,
                &format!(
                    r#"return args.parse({argv}, {{ name = "t", description = "tool", flags = {{ {flag} }} }})"#
                ),
            );
            let t = parsed.as_table().unwrap();
            assert!(t.get::<bool>("help").unwrap(), "{argv}");
            assert!(t.get::<String>("help_text").unwrap().contains("tool"));
        }
        // subcommands keep their help when the top level claims `-h`, and one that claims it too keeps `--help`
        let top = r#"{ name = "t", flags = { { name = "host", short = "h", type = "string" } }, subcommands = {
            run = { description = "run it", flags = {} },
            serve = { description = "serve it", flags = { { name = "hash", short = "h", type = "string" } } },
        } }"#;
        for (argv, text) in [
            (r#"{"run","--help"}"#, "run it"),
            (r#"{"run","-h"}"#, "run it"),
            (r#"{"serve","--help"}"#, "serve it"),
        ] {
            let (parsed, err) = parse_call(&lua, &format!("return args.parse({argv}, {top})"));
            let t = parsed
                .as_table()
                .unwrap_or_else(|| panic!("{argv}: {err:?}"));
            assert!(t.get::<bool>("help").unwrap(), "{argv}");
            assert!(
                t.get::<String>("help_text").unwrap().contains(text),
                "{argv}"
            );
        }
        let (parsed, err) = parse_call(
            &lua,
            &format!(r#"return args.parse({{"serve","-h","abc"}}, {top})"#),
        );
        assert!(matches!(err, mlua::Value::Nil), "{err:?}");
        let serve: Table = parsed.as_table().unwrap().get("serve").unwrap();
        assert_eq!(serve.get::<String>("hash").unwrap(), "abc");
        // and a flag claiming `-h` does not turn `-h` into a help request
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"-h", "x"}, { name = "t", flags = { { name = "host", short = "h", type = "string" } } })"#,
        );
        assert!(
            parsed
                .as_table()
                .unwrap()
                .get::<Option<bool>>("help")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn boolean_flag_presence_sets_true() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({"--verbose"}, { name = "t", flags = { verbose = { type = "boolean" } } })"#,
        );
        assert!(matches!(err, mlua::Value::Nil));
        let t = parsed.as_table().unwrap();
        assert!(t.get::<bool>("verbose").unwrap());
    }

    #[test]
    fn boolean_flag_absent_is_false() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", flags = { verbose = { type = "boolean" } } })"#,
        );
        assert!(matches!(err, mlua::Value::Nil));
        let t = parsed.as_table().unwrap();
        assert!(!t.get::<bool>("verbose").unwrap());
    }

    #[test]
    fn string_flag_collected_with_space_form() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--out", "file.txt"}, { name = "t", flags = { out = { type = "string" } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<String>("out").unwrap(), "file.txt");
    }

    #[test]
    fn string_flag_collected_with_equals_form() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--out=file.txt"}, { name = "t", flags = { out = { type = "string" } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<String>("out").unwrap(), "file.txt");
    }

    #[test]
    fn number_flag_coerces() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--count", "42"}, { name = "t", flags = { count = { type = "number" } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<f64>("count").unwrap(), 42.0);
    }

    #[test]
    fn short_alias() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"-v"}, { name = "t", flags = { verbose = { type = "boolean", short = "v" } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert!(t.get::<bool>("verbose").unwrap());
    }

    #[test]
    fn required_flag_missing_errors() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", flags = { out = { type = "string", required = true } } })"#,
        );
        assert!(matches!(parsed, mlua::Value::Nil));
        assert!(matches!(err, mlua::Value::String(_)));
    }

    #[test]
    fn default_applied_when_absent() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", flags = { out = { type = "string", default = "stdout" } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<String>("out").unwrap(), "stdout");
    }

    #[test]
    fn multiple_strings_collected() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--inc", "a", "--inc", "b"}, { name = "t", flags = { inc = { type = "string", multiple = true } } })"#,
        );
        let t = parsed.as_table().unwrap();
        let inc: Table = t.get("inc").unwrap();
        assert_eq!(inc.get::<String>(1).unwrap(), "a");
        assert_eq!(inc.get::<String>(2).unwrap(), "b");
    }

    #[test]
    fn positional_collected() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"a", "b"}, { name = "t", flags = {} })"#,
        );
        let t = parsed.as_table().unwrap();
        let p: Table = t.get("positional").unwrap();
        assert_eq!(p.get::<String>(1).unwrap(), "a");
        assert_eq!(p.get::<String>(2).unwrap(), "b");
    }

    #[test]
    fn rest_collected_after_double_dash() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"x", "--", "--y", "z"}, { name = "t", flags = {} })"#,
        );
        let t = parsed.as_table().unwrap();
        let p: Table = t.get("positional").unwrap();
        let r: Table = t.get("rest").unwrap();
        assert_eq!(p.get::<String>(1).unwrap(), "x");
        assert_eq!(r.get::<String>(1).unwrap(), "--y");
        assert_eq!(r.get::<String>(2).unwrap(), "z");
    }

    #[test]
    fn rest_always_emitted_even_when_empty() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(&lua, r#"return args.parse({}, { name = "t", flags = {} })"#);
        let t = parsed.as_table().unwrap();
        let r: Table = t.get("rest").unwrap();
        assert_eq!(r.raw_len(), 0);
        let p: Table = t.get("positional").unwrap();
        assert_eq!(p.raw_len(), 0);
    }

    #[test]
    fn subcommand_routed() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse(
                {"build", "--release"},
                { name = "t", subcommands = {
                    build = { flags = { release = { type = "boolean" } } },
                    test  = { flags = {} },
                } }
            )"#,
        );
        let t = parsed.as_table().unwrap();
        assert_eq!(t.get::<String>("subcommand").unwrap(), "build");
        let sub: Table = t.get("build").unwrap();
        assert!(sub.get::<bool>("release").unwrap());
    }

    #[test]
    fn missing_subcommand_errors() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", subcommands = { build = { flags = {} } } })"#,
        );
        assert!(matches!(parsed, mlua::Value::Nil));
        assert!(matches!(err, mlua::Value::String(_)));
    }

    #[test]
    fn help_short_circuit() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--help"}, { name = "t", description = "tool", flags = {} })"#,
        );
        let t = parsed.as_table().unwrap();
        assert!(t.get::<bool>("help").unwrap());
        let text: String = t.get("help_text").unwrap();
        assert!(text.contains("tool"));
    }

    #[test]
    fn help_skips_required_validation() {
        let lua = lua_with_args();
        let (parsed, _) = parse_call(
            &lua,
            r#"return args.parse({"--help"}, { name = "t", flags = { out = { type = "string", required = true } } })"#,
        );
        let t = parsed.as_table().unwrap();
        assert!(t.get::<bool>("help").unwrap());
    }

    #[test]
    fn config_missing_name_rejected() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(&lua, r#"return args.parse({}, { flags = {} })"#);
        assert!(matches!(parsed, mlua::Value::Nil));
        let e = err.as_string().unwrap().to_str().unwrap().to_string();
        assert!(e.contains("name is required"));
    }

    #[test]
    fn config_unknown_flag_type_rejected() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", flags = { x = { type = "object" } } })"#,
        );
        assert!(matches!(parsed, mlua::Value::Nil));
        let e = err.as_string().unwrap().to_str().unwrap().to_string();
        assert!(e.contains("invalid type 'object'"));
    }

    #[test]
    fn config_multiple_on_boolean_rejected() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", flags = { v = { type = "boolean", multiple = true } } })"#,
        );
        assert!(matches!(parsed, mlua::Value::Nil));
        let e = err.as_string().unwrap().to_str().unwrap().to_string();
        assert!(e.contains("cannot be both boolean and multiple"));
    }

    #[test]
    fn config_nested_subcommand_rejected() {
        let lua = lua_with_args();
        let (parsed, err) = parse_call(
            &lua,
            r#"return args.parse({}, { name = "t", subcommands = {
                build = { flags = {}, subcommands = { x = { flags = {} } } }
            } })"#,
        );
        assert!(matches!(parsed, mlua::Value::Nil));
        let e = err.as_string().unwrap().to_str().unwrap().to_string();
        assert!(e.contains("nested subcommands"));
    }

    #[test]
    fn help_function_renders() {
        let lua = lua_with_args();
        let (text, err) = parse_call(
            &lua,
            r#"return args.help({ name = "tool", description = "do work", flags = { v = { type = "boolean" } } })"#,
        );
        assert!(matches!(err, mlua::Value::Nil));
        let s = text.as_string().unwrap().to_str().unwrap().to_string();
        assert!(s.contains("tool"));
        assert!(s.contains("--v"));
    }
}

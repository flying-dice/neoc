//! INI file parsing — `lib:ini`.
//!
//! Backed by the `rust-ini` crate with a dialect-normalisation layer
//! for bare keys, custom symbols, and colon escaping.

use ini::Ini;
use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, from_err, ok};

// Sentinels — safe because we reject null bytes before parsing.
const BARE_SENTINEL: &str = "\x01BARE\x01";
const COLON_ESCAPE: &str = "\x02";

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

struct ParseOpts {
    comment_symbols: Vec<char>,
    delimiters: Vec<char>,
    lowercase_keys: bool,
}

impl Default for ParseOpts {
    fn default() -> Self {
        Self {
            comment_symbols: vec![';', '#'],
            delimiters: vec!['='],
            lowercase_keys: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Dialect normalisation
// ---------------------------------------------------------------------------

/// Validate structure and normalise dialect differences before handing
/// to rust-ini. Handles: custom comment/delimiter symbols, bare keys,
/// and escapes literal `:` so rust-ini doesn't treat it as a delimiter.
fn preprocess(raw: &str, opts: &ParseOpts) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len() + 64);

    for (line_num, raw_line) in raw.lines().enumerate() {
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }

        if let Some(first) = trimmed.chars().next()
            && opts.comment_symbols.contains(&first)
        {
            out.push_str(";_\n");
            continue;
        }

        if trimmed.starts_with('[') {
            let end = trimmed
                .find(']')
                .ok_or_else(|| format!("line {}: unclosed section header", line_num + 1))?;
            let name = &trimmed[1..end];
            if name.is_empty() {
                return Err(format!("line {}: empty section name", line_num + 1));
            }
            out.push('[');
            out.push_str(&name.replace(':', COLON_ESCAPE));
            out.push(']');
            out.push('\n');
            continue;
        }

        if let Some(pos) = trimmed.find(|c: char| opts.delimiters.contains(&c)) {
            let key = trimmed[..pos].trim_end();
            let delim_len = trimmed[pos..].chars().next().unwrap().len_utf8();
            let value = trimmed[pos + delim_len..].trim();
            out.push_str(&key.replace(':', COLON_ESCAPE));
            out.push('=');
            out.push_str(&value.replace(':', COLON_ESCAPE));
            out.push('\n');
        } else {
            out.push_str(&trimmed.replace(':', COLON_ESCAPE));
            out.push('=');
            out.push_str(BARE_SENTINEL);
            out.push('\n');
        }
    }

    Ok(out)
}

fn unescape(s: &str) -> String {
    s.replace(COLON_ESCAPE, ":")
}

/// Borrow an `mlua::LuaString` as an owned UTF-8 `String`, mapping the (rare)
/// non-UTF-8 error into an mlua error. Shared by every INI key/value read.
fn str_of(s: &mlua::LuaString) -> mlua::Result<String> {
    s.to_str()
        .map(|s| s.to_owned())
        .map_err(|e| mlua::Error::external(e.to_string()))
}

/// Lowercase `s` when `lower` is set, else return it unchanged. Applied to every
/// section and key name under the `lowercase_keys` option.
fn case_fold(s: String, lower: bool) -> String {
    if lower { s.to_lowercase() } else { s }
}

// ---------------------------------------------------------------------------
// Lua entry points
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:ini` surface through the facade.
///
/// Both functions are set verbatim with [`crate::lua::facade::Module::set`]:
/// each carries its own `#[instrument]` span and mixes raised `mlua::Error`
/// (e.g. control-byte rejection) with `(value, error)` returns built directly
/// from `err` / `from_err`, so neither fits the facade's `func` / `try_func`
/// return model. Their signatures are recorded via
/// [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:ini")?;

    m.set("parse", lua.create_function(parse)?)?;
    m.record_fn(
        "parse",
        &[("text", "string"), ("opts", "LuaValue?")],
        "Result<LuaValue, string>",
        "Parse an INI string into a table of sections. Section-less keys land under the empty-string key; bare keys become `true`.",
    );

    m.set("stringify", lua.create_function(stringify)?)?;
    m.record_fn(
        "stringify",
        &[("config", "LuaValue"), ("opts", "LuaValue?")],
        "Result<string, string>",
        "Serialise a section table back to an INI string. The empty-string section is emitted first as section-less keys.",
    );

    Ok(m)
}

#[instrument(name = "ini.parse", target = "lua::lib::ini", skip_all)]
fn parse(
    lua: &Lua,
    (text, opts_table): (mlua::LuaString, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let raw = str_of(&text)?;

    if raw.bytes().any(|b| b <= 0x08) {
        return err(lua, "ini.parse: input contains control bytes");
    }

    let opts = read_opts(opts_table)?;

    let processed = match preprocess(&raw, &opts) {
        Ok(p) => p,
        Err(e) => return from_err(lua, "ini.parse", e),
    };

    let conf = match Ini::load_from_str_noescape(&processed) {
        Ok(c) => c,
        Err(e) => return from_err(lua, "ini.parse", e),
    };

    ok(lua, assemble_table(lua, &conf, &opts)?)
}

/// Build the Lua result table from a parsed INI document: one nested table per
/// section (the empty-string section holds section-less keys), with
/// `lowercase_keys` applied to section and property names.
fn assemble_table(lua: &Lua, conf: &Ini, opts: &ParseOpts) -> mlua::Result<Table> {
    let result = lua.create_table()?;
    for (section, properties) in conf.iter() {
        if section.is_none() && properties.is_empty() {
            continue;
        }

        let section_key = match section {
            Some(s) => case_fold(unescape(s), opts.lowercase_keys),
            None => String::new(),
        };

        // Reuse an existing section table when two source sections fold to the
        // same key. rust-ini merges sections sharing a raw name, but
        // `lowercase_keys` can still collide distinct names (`[Foo]` + `[FOO]`
        // → `foo`), so the entries below must accumulate, not overwrite.
        let section_table: Table = match result.get::<Option<Table>>(&*section_key)? {
            Some(t) => t,
            None => {
                let t = lua.create_table()?;
                result.set(section_key.as_str(), t.clone())?;
                t
            }
        };

        for (key, value) in properties.iter() {
            let lua_key = case_fold(unescape(key), opts.lowercase_keys);

            if value == BARE_SENTINEL {
                section_table.set(lua_key.as_str(), true)?;
            } else {
                let value = unescape(value);
                section_table.set(lua_key.as_str(), value.as_str())?;
            }
        }
    }
    Ok(result)
}

fn read_char_list(tbl: &Table, key: &str) -> mlua::Result<Option<Vec<char>>> {
    let Some(list) = tbl.get::<Option<Table>>(key)? else {
        return Ok(None);
    };
    let mut chars = Vec::new();
    for pair in list.pairs::<i64, mlua::LuaString>() {
        let (_, v) = pair?;
        let s = str_of(&v)?;
        if let Some(c) = s.chars().next() {
            chars.push(c);
        }
    }
    Ok(Some(chars))
}

fn read_opts(opts_table: Option<Table>) -> mlua::Result<ParseOpts> {
    let Some(tbl) = opts_table else {
        return Ok(ParseOpts::default());
    };

    let mut opts = ParseOpts::default();

    if let Some(syms) = read_char_list(&tbl, "comment_symbols")? {
        opts.comment_symbols = syms;
    }

    if let Some(delims) = read_char_list(&tbl, "delimiters")? {
        opts.delimiters = delims;
    }

    if let Some(lk) = tbl.get::<Option<bool>>("lowercase_keys")? {
        opts.lowercase_keys = lk;
    }

    Ok(opts)
}

// ---------------------------------------------------------------------------
// Stringify
// ---------------------------------------------------------------------------

#[instrument(name = "ini.stringify", target = "lua::lib::ini", skip_all)]
fn stringify(
    lua: &Lua,
    (config, opts_table): (Table, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let spacing = if let Some(tbl) = &opts_table {
        tbl.get::<Option<bool>>("spacing")?.unwrap_or(false)
    } else {
        false
    };

    let mut out = String::new();
    let mut first_section = true;

    if let Some(global) = config.get::<Option<Table>>("")? {
        write_props(&global, &mut out, spacing)?;
        first_section = false;
    }

    for pair in config.pairs::<mlua::LuaString, Table>() {
        let (k, section_table) = pair?;
        let section_name = str_of(&k)?;
        if section_name.is_empty() {
            continue;
        }
        if !first_section || !out.is_empty() {
            out.push('\n');
        }
        out.push('[');
        out.push_str(&section_name);
        out.push(']');
        out.push('\n');
        write_props(&section_table, &mut out, spacing)?;
        first_section = false;
    }

    ok(lua, out)
}

fn write_props(table: &Table, out: &mut String, spacing: bool) -> mlua::Result<()> {
    let delim = if spacing { " = " } else { "=" };
    for pair in table.pairs::<mlua::LuaString, Value>() {
        let (k, v) = pair?;
        let key = str_of(&k)?;
        // A bare key (no delimiter) denotes a `true` flag; every other scalar is
        // written `key<delim>value`. Non-scalar values (tables, functions,
        // userdata, nil) have no INI representation and are rejected — never
        // serialised through a host-language `Debug`, which would leak Rust
        // syntax into the output.
        match &v {
            Value::Boolean(true) => {
                out.push_str(&key);
                out.push('\n');
            }
            Value::String(s) => {
                out.push_str(&key);
                out.push_str(delim);
                let val = str_of(s)?;
                out.push_str(&val);
                out.push('\n');
            }
            Value::Boolean(false) => {
                out.push_str(&key);
                out.push_str(delim);
                out.push_str("false");
                out.push('\n');
            }
            Value::Integer(i) => {
                out.push_str(&key);
                out.push_str(delim);
                out.push_str(&i.to_string());
                out.push('\n');
            }
            Value::Number(n) => {
                out.push_str(&key);
                out.push_str(delim);
                out.push_str(&n.to_string());
                out.push('\n');
            }
            other => {
                return Err(mlua::Error::external(format!(
                    "ini.stringify: value for key \"{key}\" must be a string, number, or \
                     boolean, got {}",
                    other.type_name()
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "[section1]\nkey1=value1\nkey2=value2\n\n[section2]\nfoo=bar\nbaz\n".repeat(20);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("ini");

    g.bench_function("library", |b| {
        let opts = ParseOpts::default();
        b.iter(|| {
            let processed = preprocess(black_box(&payload), &opts).unwrap();
            let _ = Ini::load_from_str_noescape(&processed);
        });
    });

    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::LuaString| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::LuaString = ident.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    let chunk: mlua::Function = lua
        .load(r#"return function(p) return (require("lib:ini")).parse(p) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua", |b| {
        b.iter(|| {
            let _: mlua::Table = chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use mlua::{Lua, Table, Value};

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: Table = lua.load(r#"return require("lib:ini")"#).eval().unwrap();
        lua.globals().set("ini", m).unwrap();
        lua
    }

    #[test]
    fn parse_basic_sections() {
        let lua = engine();
        let (result, err): (Table, Value) = lua
            .load(r#"return ini.parse("[section]\nkey=value")"#)
            .eval()
            .unwrap();
        assert!(matches!(err, Value::Nil));
        let section: Table = result.get("section").unwrap();
        let val: String = section.get("key").unwrap();
        assert_eq!(val, "value");
    }

    #[test]
    fn parse_sectionless_keys() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("name=world")"#)
            .eval()
            .unwrap();
        let global: Table = result.get("").unwrap();
        let val: String = global.get("name").unwrap();
        assert_eq!(val, "world");
    }

    #[test]
    fn parse_empty_value() {
        let lua = engine();
        let (result, _): (Table, Value) =
            lua.load(r#"return ini.parse("[s]\nkey=")"#).eval().unwrap();
        let section: Table = result.get("s").unwrap();
        let val: String = section.get("key").unwrap();
        assert_eq!(val, "");
    }

    #[test]
    fn parse_bare_key() {
        let lua = engine();
        let (result, _): (Table, Value) =
            lua.load(r#"return ini.parse("[s]\nflag")"#).eval().unwrap();
        let section: Table = result.get("s").unwrap();
        let val: bool = section.get("flag").unwrap();
        assert!(val);
    }

    #[test]
    fn parse_duplicate_keys_last_wins() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("[s]\nk=a\nk=b")"#)
            .eval()
            .unwrap();
        let section: Table = result.get("s").unwrap();
        let val: String = section.get("k").unwrap();
        assert_eq!(val, "b");
    }

    #[test]
    fn parse_duplicate_sections_merge() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("[s]\na=1\n[s]\nb=2")"#)
            .eval()
            .unwrap();
        let section: Table = result.get("s").unwrap();
        let a: String = section.get("a").unwrap();
        let b: String = section.get("b").unwrap();
        assert_eq!(a, "1");
        assert_eq!(b, "2");
    }

    #[test]
    fn parse_empty_input() {
        let lua = engine();
        let (result, _): (Table, Value) = lua.load(r#"return ini.parse("")"#).eval().unwrap();
        let count: i64 = lua
            .load("local n=0; for _ in pairs(...) do n=n+1 end; return n")
            .call(result)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn parse_comments_only() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("; comment\n# another")"#)
            .eval()
            .unwrap();
        let count: i64 = lua
            .load("local n=0; for _ in pairs(...) do n=n+1 end; return n")
            .call(result)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn parse_no_inline_comments() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("[s]\nkey=value ; not a comment")"#)
            .eval()
            .unwrap();
        let section: Table = result.get("s").unwrap();
        let val: String = section.get("key").unwrap();
        assert_eq!(val, "value ; not a comment");
    }

    #[test]
    fn parse_lowercase_keys_option() {
        let lua = engine();
        let (result, _): (Table, Value) = lua
            .load(r#"return ini.parse("[Section]\nKey=val", { lowercase_keys = true })"#)
            .eval()
            .unwrap();
        let section: Table = result.get("section").unwrap();
        let val: String = section.get("key").unwrap();
        assert_eq!(val, "val");
    }

    #[test]
    fn parse_error_unclosed_section() {
        let lua = engine();
        let (val, err): (Value, Option<String>) =
            lua.load(r#"return ini.parse("[bad")"#).eval().unwrap();
        assert!(matches!(val, Value::Nil));
        let err = err.expect("error expected");
        assert!(err.contains("unclosed section"), "got: {err}");
    }

    #[test]
    fn parse_error_empty_section_name() {
        let lua = engine();
        let (val, err): (Value, Option<String>) =
            lua.load(r#"return ini.parse("[]")"#).eval().unwrap();
        assert!(matches!(val, Value::Nil));
        let err = err.expect("error expected");
        assert!(err.contains("empty section name"), "got: {err}");
    }

    #[test]
    fn parse_error_null_bytes() {
        let lua = engine();
        let (val, err): (Value, Option<String>) = lua
            .load(r#"return ini.parse("key=val\0ue")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let err = err.expect("error expected");
        assert!(err.contains("control bytes"), "got: {err}");
    }

    #[test]
    fn parse_error_sentinel_bytes() {
        let lua = engine();
        let input = lua.create_string(b"[s]\nkey=val\x01ue").unwrap();
        lua.globals().set("input", input).unwrap();
        let (val, err): (Value, Option<String>) =
            lua.load(r#"return ini.parse(input)"#).eval().unwrap();
        assert!(matches!(val, Value::Nil));
        let err = err.expect("error expected");
        assert!(err.contains("control bytes"), "got: {err}");

        let input2 = lua.create_string(b"[s]\nkey\x02=value").unwrap();
        lua.globals().set("input", input2).unwrap();
        let (val2, err2): (Value, Option<String>) =
            lua.load(r#"return ini.parse(input)"#).eval().unwrap();
        assert!(matches!(val2, Value::Nil));
        let err2 = err2.expect("error expected");
        assert!(err2.contains("control bytes"), "got: {err2}");
    }

    #[test]
    fn stringify_basic() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local t = { section = { key = "value" } }
                local s, err = ini.stringify(t)
                assert(err == nil)
                return s
                "#,
            )
            .eval()
            .unwrap();
        assert!(result.contains("[section]"), "got: {result}");
        assert!(result.contains("key=value"), "got: {result}");
    }

    #[test]
    fn stringify_bare_key() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local t = { section = { flag = true } }
                local s = ini.stringify(t)
                return s
                "#,
            )
            .eval()
            .unwrap();
        assert!(
            result.contains("flag\n") || result.contains("flag\r"),
            "got: {result}"
        );
        assert!(!result.contains("flag="), "got: {result}");
    }

    #[test]
    fn stringify_sectionless_keys() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local t = { [""] = { name = "world" }, section = { k = "v" } }
                local s = ini.stringify(t)
                return s
                "#,
            )
            .eval()
            .unwrap();
        assert!(result.contains("name=world"), "got: {result}");
        let name_pos = result.find("name=world").unwrap();
        let section_pos = result.find("[section]").unwrap();
        assert!(name_pos < section_pos, "sectionless keys should come first");
    }

    #[test]
    fn stringify_spacing_true() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local t = { section = { key = "value" } }
                local s = ini.stringify(t, { spacing = true })
                return s
                "#,
            )
            .eval()
            .unwrap();
        assert!(result.contains("key = value"), "got: {result}");
        assert!(!result.contains("key=value"), "got: {result}");
    }

    #[test]
    fn stringify_spacing_false() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local t = { section = { key = "value" } }
                local s = ini.stringify(t, { spacing = false })
                return s
                "#,
            )
            .eval()
            .unwrap();
        assert!(result.contains("key=value"), "got: {result}");
        assert!(!result.contains("key = value"), "got: {result}");
    }

    #[test]
    fn round_trip() {
        let lua = engine();
        let result: String = lua
            .load(
                r#"
                local input = "[db]\nhost=localhost\nport=5432\n[app]\ndebug=true"
                local parsed = ini.parse(input)
                local output = ini.stringify(parsed)
                return output
                "#,
            )
            .eval()
            .unwrap();
        assert!(result.contains("host=localhost"), "got: {result}");
        assert!(result.contains("port=5432"), "got: {result}");
    }

    #[test]
    fn stringify_scalars_and_rejects_non_scalar() {
        let lua = engine();
        // false / integer / number serialise as their literal form — never the
        // Rust `Debug` rendering (`Boolean(false)`, `Integer(42)`, …) a
        // catch-all `_ => format!("{v:?}")` arm would have leaked.
        let s: String = lua
            .load(r#"return ini.stringify({ s = { f = false, i = 42, n = 1.5 } })"#)
            .eval()
            .unwrap();
        assert!(
            s.contains("f=false") && s.contains("i=42") && s.contains("n=1.5"),
            "got: {s}"
        );
        assert!(
            !s.contains("Boolean") && !s.contains("Integer(") && !s.contains("Number("),
            "Debug leak: {s}"
        );

        // A non-scalar value (table/function/userdata) has no INI form and is
        // rejected, not silently serialised.
        let e: String = lua
            .load(
                r#"local ok, err = pcall(function() return ini.stringify({ s = { bad = {} } }) end)
                   assert(ok == false)
                   return tostring(err)"#,
            )
            .eval()
            .unwrap();
        assert!(
            e.contains("must be a string, number, or boolean"),
            "got: {e}"
        );
    }
}

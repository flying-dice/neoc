//! INI file parsing — `lib:ini`.

use mlua::{Lua, Table, Value};
use tracing::instrument;

use crate::lua::sandbox::{err, from_err, ok};

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum IniValue {
    Str(String),
    Bare,
}

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

struct IniDoc {
    sections: Vec<(Option<String>, Vec<(String, IniValue)>)>,
}

impl IniDoc {
    fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    fn ensure_section(&mut self, name: Option<&str>) -> usize {
        if let Some(pos) = self.sections.iter().position(|(n, _)| n.as_deref() == name) {
            pos
        } else {
            self.sections.push((name.map(|s| s.to_owned()), Vec::new()));
            self.sections.len() - 1
        }
    }

    fn set(&mut self, section_idx: usize, key: String, value: IniValue) {
        let props = &mut self.sections[section_idx].1;
        if let Some(existing) = props.iter_mut().find(|(k, _)| *k == key) {
            existing.1 = value;
        } else {
            props.push((key, value));
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn do_parse(text: &str, opts: &ParseOpts) -> Result<IniDoc, String> {
    let mut doc = IniDoc::new();
    let mut current_section: Option<String> = None;

    for (line_num, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();

        if line.is_empty() {
            continue;
        }

        // Comment check — only at line start.
        if let Some(first) = line.chars().next() {
            if opts.comment_symbols.contains(&first) {
                continue;
            }
        }

        // Section header.
        if line.starts_with('[') {
            let end = line
                .find(']')
                .ok_or_else(|| format!("line {}: unclosed section header", line_num + 1))?;
            let name = &line[1..end];
            if name.is_empty() {
                return Err(format!("line {}: empty section name", line_num + 1));
            }
            let name = if opts.lowercase_keys {
                name.to_lowercase()
            } else {
                name.to_owned()
            };
            current_section = Some(name);
            continue;
        }

        // Key-value or bare key.
        let section_idx = doc.ensure_section(current_section.as_deref());

        if let Some(delim_pos) = line.find(|c: char| opts.delimiters.contains(&c)) {
            let raw_key = line[..delim_pos].trim_end();
            let key = if opts.lowercase_keys {
                raw_key.to_lowercase()
            } else {
                raw_key.to_owned()
            };
            let val_start = delim_pos + line[delim_pos..].chars().next().unwrap().len_utf8();
            let value = line[val_start..].trim().to_owned();
            doc.set(section_idx, key, IniValue::Str(value));
        } else {
            let key = if opts.lowercase_keys {
                line.to_lowercase()
            } else {
                line.to_owned()
            };
            doc.set(section_idx, key, IniValue::Bare);
        }
    }

    Ok(doc)
}

// ---------------------------------------------------------------------------
// Lua entry points
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("parse", lua.create_function(parse)?)?;
    t.set("stringify", lua.create_function(stringify)?)?;
    Ok(t)
}

#[instrument(name = "ini.parse", target = "luau::lib::ini", skip_all)]
fn parse(
    lua: &Lua,
    (text, opts_table): (mlua::String, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let raw = text
        .to_str()
        .map_err(|e| mlua::Error::external(e.to_string()))?;

    if raw.contains('\0') {
        return err(lua, "ini.parse: input contains null bytes");
    }

    let opts = if let Some(tbl) = opts_table {
        let mut o = ParseOpts::default();
        if let Some(cs) = tbl.get::<Option<Table>>("comment_symbols")? {
            let mut syms = Vec::new();
            for pair in cs.pairs::<i64, mlua::String>() {
                let (_, v) = pair?;
                let s = v
                    .to_str()
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                if let Some(c) = s.chars().next() {
                    syms.push(c);
                }
            }
            o.comment_symbols = syms;
        }
        if let Some(ds) = tbl.get::<Option<Table>>("delimiters")? {
            let mut delims = Vec::new();
            for pair in ds.pairs::<i64, mlua::String>() {
                let (_, v) = pair?;
                let s = v
                    .to_str()
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                if let Some(c) = s.chars().next() {
                    delims.push(c);
                }
            }
            o.delimiters = delims;
        }
        if let Some(lk) = tbl.get::<Option<bool>>("lowercase_keys")? {
            o.lowercase_keys = lk;
        }
        o
    } else {
        ParseOpts::default()
    };

    let doc = match do_parse(&raw, &opts) {
        Ok(d) => d,
        Err(e) => return from_err(lua, "ini.parse", e),
    };

    let result = lua.create_table()?;
    for (section_name, props) in &doc.sections {
        let key = section_name.as_deref().unwrap_or("");
        let section_table: Table = match result.get::<Option<Table>>(key)? {
            Some(t) => t,
            None => {
                let t = lua.create_table()?;
                result.set(key, t.clone())?;
                t
            }
        };
        for (k, v) in props {
            match v {
                IniValue::Str(s) => section_table.set(k.as_str(), s.as_str())?,
                IniValue::Bare => section_table.set(k.as_str(), true)?,
            }
        }
    }

    ok(lua, result)
}

#[instrument(name = "ini.stringify", target = "luau::lib::ini", skip_all)]
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

    // Write sectionless keys first (under "" key).
    if let Some(global) = config.get::<Option<Table>>("")? {
        write_props(&global, &mut out)?;
        first_section = false;
    }

    // Write named sections.
    for pair in config.pairs::<mlua::String, Table>() {
        let (k, section_table) = pair?;
        let section_name = k
            .to_str()
            .map_err(|e| mlua::Error::external(e.to_string()))?;
        if section_name.is_empty() {
            continue;
        }
        if !first_section && spacing {
            out.push('\n');
        }
        if !first_section || !out.is_empty() {
            if !spacing && !out.is_empty() {
                out.push('\n');
            }
        }
        out.push('[');
        out.push_str(&section_name);
        out.push(']');
        out.push('\n');
        write_props(&section_table, &mut out)?;
        first_section = false;
    }

    ok(lua, out)
}

fn write_props(table: &Table, out: &mut String) -> mlua::Result<()> {
    for pair in table.pairs::<mlua::String, Value>() {
        let (k, v) = pair?;
        let key = k
            .to_str()
            .map_err(|e| mlua::Error::external(e.to_string()))?;
        match &v {
            Value::Boolean(true) => {
                out.push_str(&key);
                out.push('\n');
            }
            Value::String(s) => {
                out.push_str(&key);
                out.push('=');
                let val = s
                    .to_str()
                    .map_err(|e| mlua::Error::external(e.to_string()))?;
                out.push_str(&val);
                out.push('\n');
            }
            _ => {
                out.push_str(&key);
                out.push('=');
                out.push_str(&format!("{:?}", v));
                out.push('\n');
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
            let _ = do_parse(black_box(&payload), &opts);
        });
    });

    let ident: mlua::Function = lua
        .create_function(|_, s: mlua::String| Ok(s))
        .expect("ident");
    g.bench_function("into_out_lua", |b| {
        b.iter(|| {
            let _: mlua::String = ident.call(black_box(payload_lua.clone())).unwrap();
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
        assert!(err.contains("null bytes"), "got: {err}");
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
}

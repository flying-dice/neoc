//! Regular expressions — `lib:regex`.
//!
//! Dual surface: `Pattern` UserData (compiled, reusable) plus flat
//! `is_match` / `find` / `find_all` / `replace` / `replace_all` / `split` /
//! `escape` convenience functions. Backed by the `regex` crate —
//! **linear-time, no catastrophic backtracking**.
//!
//! Flags follow Perl/Python/JS letter conventions, accepted either as a
//! constructor argument (`Pattern.new(pat, "is")`) or inline in the pattern
//! itself (`(?is)…`) per the upstream regex crate. Unicode matching is on
//! by default and cannot be disabled.
//!
//! Lookaround and backreferences are deliberately unsupported — the
//! linear-time guarantee is the design point. Scripts that need PCRE
//! features should reach for a different tool.

use mlua::{Lua, MetaMethod, Table, UserData, UserDataMethods, Value};
use regex::{Regex, RegexBuilder};
use tracing::instrument;

use crate::lua::sandbox::{err, ok};

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    let pattern_sub = lua.create_table()?;
    pattern_sub.set("new", lua.create_function(pattern_new)?)?;
    t.set("Pattern", pattern_sub)?;

    t.set("is_match", lua.create_function(flat_is_match)?)?;
    t.set("find", lua.create_function(flat_find)?)?;
    t.set("find_all", lua.create_function(flat_find_all)?)?;
    t.set("replace", lua.create_function(flat_replace)?)?;
    t.set("replace_all", lua.create_function(flat_replace_all)?)?;
    t.set("split", lua.create_function(flat_split)?)?;
    t.set("escape", lua.create_function(flat_escape)?)?;

    Ok(t)
}

// ---------------------------------------------------------------------------
// Pattern UserData
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PatternHandle {
    source: String,
    compiled: Regex,
}

impl PatternHandle {
    fn compile(pattern: &str, flags: Option<&str>, prefix: &str) -> Result<Self, String> {
        let mut builder = RegexBuilder::new(pattern);
        if let Some(f) = flags {
            apply_flags(&mut builder, f, prefix)?;
        }
        let compiled = builder.build().map_err(|e| format!("{prefix}: {e}"))?;
        Ok(Self {
            source: pattern.to_owned(),
            compiled,
        })
    }
}

impl UserData for PatternHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_match", |_, this, text: String| {
            Ok(this.compiled.is_match(&text))
        });

        methods.add_method("find", |lua, this, text: String| {
            match this.compiled.captures(&text) {
                Some(caps) => Ok(Some(build_match(lua, &this.compiled, &caps)?)),
                None => Ok(None),
            }
        });

        methods.add_method("find_all", |lua, this, text: String| {
            let t = lua.create_table()?;
            for (i, caps) in this.compiled.captures_iter(&text).enumerate() {
                t.set(i + 1, build_match(lua, &this.compiled, &caps)?)?;
            }
            Ok(t)
        });

        methods.add_method(
            "replace",
            |_, this, (text, replacement): (String, String)| {
                Ok(this
                    .compiled
                    .replace(&text, replacement.as_str())
                    .into_owned())
            },
        );

        methods.add_method(
            "replace_all",
            |_, this, (text, replacement): (String, String)| {
                Ok(this
                    .compiled
                    .replace_all(&text, replacement.as_str())
                    .into_owned())
            },
        );

        methods.add_method(
            "split",
            |lua, this, (text, limit): (String, Option<i64>)| {
                split_into_table(lua, &this.compiled, &text, limit, "regex.Pattern.split")
            },
        );

        methods.add_meta_method(MetaMethod::ToString, |_, this, _: ()| {
            Ok(this.source.clone())
        });
    }
}

#[instrument(name = "regex.Pattern.new", target = "luau::lib::regex", skip_all)]
fn pattern_new(
    lua: &Lua,
    (pattern, flags): (String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    match PatternHandle::compile(&pattern, flags.as_deref(), "regex.Pattern.new") {
        Ok(p) => ok(lua, p),
        Err(msg) => err(lua, msg),
    }
}

// ---------------------------------------------------------------------------
// Flat functions
// ---------------------------------------------------------------------------

/// Compile `pattern` with optional `flags`, or return early as `(nil, msg)`.
/// Inlined at every flat call site via `?`-on-an-`Option` wouldn't work —
/// success carries the handle, failure carries a Lua error tuple — so the
/// macro keeps the early-return readable without inventing a closure.
macro_rules! compile_or_return {
    ($lua:expr, $pattern:expr, $flags:expr, $prefix:expr) => {
        match PatternHandle::compile($pattern, $flags, $prefix) {
            Ok(p) => p,
            Err(msg) => return err($lua, msg),
        }
    };
}

#[instrument(name = "regex.is_match", target = "luau::lib::regex", skip_all)]
fn flat_is_match(
    lua: &Lua,
    (pattern, text, flags): (String, String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.is_match");
    ok(lua, p.compiled.is_match(&text))
}

#[instrument(name = "regex.find", target = "luau::lib::regex", skip_all)]
fn flat_find(
    lua: &Lua,
    (pattern, text, flags): (String, String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.find");
    match p.compiled.captures(&text) {
        Some(caps) => ok(lua, build_match(lua, &p.compiled, &caps)?),
        None => Ok((Value::Nil, Value::Nil)),
    }
}

#[instrument(name = "regex.find_all", target = "luau::lib::regex", skip_all)]
fn flat_find_all(
    lua: &Lua,
    (pattern, text, flags): (String, String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.find_all");
    let t = lua.create_table()?;
    for (i, caps) in p.compiled.captures_iter(&text).enumerate() {
        t.set(i + 1, build_match(lua, &p.compiled, &caps)?)?;
    }
    ok(lua, t)
}

#[instrument(name = "regex.replace", target = "luau::lib::regex", skip_all)]
fn flat_replace(
    lua: &Lua,
    (pattern, text, replacement, flags): (String, String, String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.replace");
    ok(
        lua,
        p.compiled.replace(&text, replacement.as_str()).into_owned(),
    )
}

#[instrument(name = "regex.replace_all", target = "luau::lib::regex", skip_all)]
fn flat_replace_all(
    lua: &Lua,
    (pattern, text, replacement, flags): (String, String, String, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.replace_all");
    ok(
        lua,
        p.compiled
            .replace_all(&text, replacement.as_str())
            .into_owned(),
    )
}

#[instrument(name = "regex.split", target = "luau::lib::regex", skip_all)]
fn flat_split(
    lua: &Lua,
    (pattern, text, limit, flags): (String, String, Option<i64>, Option<String>),
) -> mlua::Result<(Value, Value)> {
    let p = compile_or_return!(lua, &pattern, flags.as_deref(), "regex.split");
    split_into_table(lua, &p.compiled, &text, limit, "regex.split")
}

#[instrument(name = "regex.escape", target = "luau::lib::regex", skip_all)]
fn flat_escape(_: &Lua, text: String) -> mlua::Result<String> {
    Ok(regex::escape(&text))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `Match` table from a `Captures`.
///
/// Positions are 1-based byte offsets matching `string.find`'s contract:
/// `start` is the first byte of the match, `finish` is the last byte
/// (inclusive). Empty matches (`""`) produce `finish = start - 1`.
fn build_match(lua: &Lua, re: &Regex, caps: &regex::Captures<'_>) -> mlua::Result<Table> {
    let whole = caps
        .get(0)
        .expect("regex::Captures always contains group 0");
    let t = lua.create_table()?;
    t.set("text", whole.as_str())?;
    let (start, finish) = byte_range_to_lua(whole.start(), whole.end());
    t.set("start", start)?;
    t.set("finish", finish)?;
    t.set("groups", positional_groups(lua, caps)?)?;
    t.set("named", named_groups(lua, re, caps)?)?;
    Ok(t)
}

/// 1-based array of positional captures. Group 0 is omitted (it's already
/// the `text`/`start`/`finish` triple on the parent). Unmatched optional
/// groups are filled with `false` so the array stays dense — Lua's `#`
/// and `ipairs` are undefined on tables with mid-array `nil` holes, and
/// every parallel runtime (Python `None`, Go `""`, JS `undefined`) returns
/// a dense container. `false` is the closest Lua equivalent that round-trips
/// through `ipairs`.
fn positional_groups(lua: &Lua, caps: &regex::Captures<'_>) -> mlua::Result<Table> {
    let groups = lua.create_table()?;
    for i in 1..caps.len() {
        match caps.get(i) {
            Some(m) => groups.set(i, m.as_str())?,
            None => groups.set(i, false)?,
        }
    }
    Ok(groups)
}

/// String-keyed table of named captures. Only matching names are populated —
/// `pairs(m.named)` iterates exactly the groups that fired.
fn named_groups(lua: &Lua, re: &Regex, caps: &regex::Captures<'_>) -> mlua::Result<Table> {
    let named = lua.create_table()?;
    for name in re.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            named.set(name, m.as_str())?;
        }
    }
    Ok(named)
}

fn byte_range_to_lua(start: usize, end: usize) -> (i64, i64) {
    // 1-based, inclusive. Empty match (start == end) collapses to
    // (start+1, start) — same convention `string.find` uses for empty
    // patterns like `""`.
    (start as i64 + 1, end as i64)
}

fn split_into_table(
    lua: &Lua,
    re: &Regex,
    text: &str,
    limit: Option<i64>,
    prefix: &str,
) -> mlua::Result<(Value, Value)> {
    let limit = match limit {
        None => None,
        Some(n) if n < 0 => return err(lua, format!("{prefix}: limit must be >= 0")),
        Some(0) => None,
        Some(n) => Some(n as usize),
    };
    let t = lua.create_table()?;
    let mut i = 0usize;
    match limit {
        None => {
            for part in re.split(text) {
                i += 1;
                t.set(i, part)?;
            }
        }
        Some(n) => {
            for part in re.splitn(text, n) {
                i += 1;
                t.set(i, part)?;
            }
        }
    }
    ok(lua, t)
}

/// Apply Perl/Python/JS-style letter flags onto a `RegexBuilder`.
///
/// Each char maps to one upstream method. Unknown chars surface as the
/// caller's prefixed error so typos don't silently no-op.
///
/// | Flag | Effect |
/// |------|--------|
/// | `i`  | case_insensitive |
/// | `m`  | multi_line (^/$ match line boundaries) |
/// | `s`  | dot_matches_new_line |
/// | `x`  | ignore_whitespace (free-spacing / commented patterns) |
fn apply_flags(b: &mut RegexBuilder, flags: &str, prefix: &str) -> Result<(), String> {
    for c in flags.chars() {
        match c {
            'i' => {
                b.case_insensitive(true);
            }
            'm' => {
                b.multi_line(true);
            }
            's' => {
                b.dot_matches_new_line(true);
            }
            'x' => {
                b.ignore_whitespace(true);
            }
            other => {
                return Err(format!(
                    "{prefix}: unknown flag {other:?} — expected any of i, m, s, x"
                ));
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

    let mut g = c.benchmark_group("regex");

    let haystack = "The quick brown fox jumps over the lazy dog. ".repeat(50);

    // Pure crate path — compile + match.
    g.bench_function("library/compile_and_match", |b| {
        b.iter(|| {
            let re = Regex::new(black_box(r"\b\w{5}\b")).unwrap();
            let _ = re.is_match(black_box(&haystack));
        });
    });

    // Compiled regex reused.
    let re = Regex::new(r"\b\w{5}\b").unwrap();
    g.bench_function("library/match_only", |b| {
        b.iter(|| {
            let _ = re.find_iter(black_box(&haystack)).count();
        });
    });

    // End-to-end through Lua.
    let find_all: mlua::Function = lua
        .load(r#"return function(p, t) return (require("lib:regex")).find_all(p, t) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua/find_all", |b| {
        b.iter(|| {
            let _: mlua::Value = find_all
                .call((black_box(r"\b\w{5}\b"), black_box(haystack.as_str())))
                .unwrap();
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
        let m: Table = lua.load(r#"return require("lib:regex")"#).eval().unwrap();
        lua.globals().set("regex", m).unwrap();
        lua
    }

    #[test]
    fn flat_is_match_basic() {
        let lua = engine();
        let yes: bool = lua
            .load(r#"return regex.is_match("\\d+", "abc 123 def")"#)
            .eval()
            .unwrap();
        assert!(yes);
        let no: bool = lua
            .load(r#"return regex.is_match("\\d+", "abc def")"#)
            .eval()
            .unwrap();
        assert!(!no);
    }

    #[test]
    fn flat_is_match_invalid_pattern_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return regex.is_match("(abc", "anything")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("regex.is_match:"), "got: {e}");
    }

    #[test]
    fn flat_find_returns_match_table_with_positions() {
        let lua = engine();
        let (text, start, finish): (String, i64, i64) = lua
            .load(
                r#"
                local m = regex.find("\\d+", "abc 123 def")
                return m.text, m.start, m.finish
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(text, "123");
        // "abc " is 4 bytes → match starts at byte 5 (1-based), ends at 7.
        assert_eq!((start, finish), (5, 7));
    }

    #[test]
    fn flat_find_no_match_returns_nil_nil() {
        let lua = engine();
        let (val, e): (Value, Value) = lua
            .load(r#"return regex.find("zzz", "abc")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        assert!(matches!(e, Value::Nil));
    }

    #[test]
    fn find_positions_match_string_find_contract() {
        // string.find on a literal should agree byte-for-byte with regex.find
        // on the same literal. Locks the 1-based inclusive contract.
        let lua = engine();
        let same: bool = lua
            .load(
                r#"
                local s = "abc 123 def 456"
                local s1, e1 = string.find(s, "123", 1, true)
                local m = regex.find("123", s)
                return s1 == m.start and e1 == m.finish
            "#,
            )
            .eval()
            .unwrap();
        assert!(same);
    }

    #[test]
    fn find_all_returns_every_match() {
        let lua = engine();
        let texts: Vec<String> = lua
            .load(
                r#"
                local out = {}
                for i, m in ipairs(regex.find_all("\\d+", "1 22 333 4444")) do
                    out[i] = m.text
                end
                return out
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(texts, vec!["1", "22", "333", "4444"]);
    }

    #[test]
    fn positional_groups() {
        let lua = engine();
        let (whole, first, second): (String, String, String) = lua
            .load(
                r#"
                local m = regex.find("(\\d+)-(\\d+)", "size 12-34 px")
                return m.text, m.groups[1], m.groups[2]
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(whole, "12-34");
        assert_eq!(first, "12");
        assert_eq!(second, "34");
    }

    #[test]
    fn optional_unmatched_group_is_false() {
        let lua = engine();
        let (has_first, second, len): (bool, Value, i64) = lua
            .load(
                r#"
                local m = regex.find("(a)(b)?", "a")
                return m.groups[1] == "a", m.groups[2], #m.groups
            "#,
            )
            .eval()
            .unwrap();
        assert!(has_first);
        assert!(matches!(second, Value::Boolean(false)));
        assert_eq!(len, 2);
    }

    #[test]
    fn sparse_groups_dense_after_unmatched() {
        // Regression: an unmatched early group followed by a matched later
        // group used to leave a `nil` hole, breaking `#` / `ipairs`. Now the
        // hole is filled with `false`, mirroring Python `None` / Go `""`.
        let lua = engine();
        let (first, second, len, iter_count): (Value, String, i64, i64) = lua
            .load(
                r#"
                local m = regex.find("(a)?(b)", "b")
                local n = 0
                for _, _ in ipairs(m.groups) do n = n + 1 end
                return m.groups[1], m.groups[2], #m.groups, n
            "#,
            )
            .eval()
            .unwrap();
        assert!(matches!(first, Value::Boolean(false)));
        assert_eq!(second, "b");
        assert_eq!(len, 2);
        assert_eq!(iter_count, 2);
    }

    #[test]
    fn named_groups() {
        let lua = engine();
        let (year, month, day): (String, String, String) = lua
            .load(
                r#"
                local m = regex.find(
                    "(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})",
                    "date 2026-05-19 done"
                )
                return m.named.year, m.named.month, m.named.day
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            (year.as_str(), month.as_str(), day.as_str()),
            ("2026", "05", "19")
        );
    }

    #[test]
    fn flag_case_insensitive() {
        let lua = engine();
        let yes: bool = lua
            .load(r#"return regex.is_match("hello", "Hello, world!", "i")"#)
            .eval()
            .unwrap();
        assert!(yes);
    }

    #[test]
    fn inline_flag_equivalent_to_argument() {
        let lua = engine();
        let (a, b): (bool, bool) = lua
            .load(
                r#"
                return
                    regex.is_match("hello", "Hello", "i"),
                    regex.is_match("(?i)hello", "Hello")
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(a, b);
        assert!(a);
    }

    #[test]
    fn flag_multi_line_anchors() {
        let lua = engine();
        let (m_no, m_yes): (bool, bool) = lua
            .load(
                r#"
                local s = "first\nsecond"
                return
                    regex.is_match("^second$", s),
                    regex.is_match("^second$", s, "m")
            "#,
            )
            .eval()
            .unwrap();
        assert!(!m_no, "default ^/$ are anchors of the whole string");
        assert!(m_yes, "with 'm' flag they anchor at line boundaries");
    }

    #[test]
    fn flag_dotall() {
        let lua = engine();
        let (default_no, dotall_yes): (bool, bool) = lua
            .load(
                r#"
                local s = "a\nb"
                return
                    regex.is_match("a.b", s),
                    regex.is_match("a.b", s, "s")
            "#,
            )
            .eval()
            .unwrap();
        assert!(!default_no);
        assert!(dotall_yes);
    }

    #[test]
    fn unknown_flag_errors_with_listing() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return regex.is_match("a", "a", "z")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("unknown flag"), "got: {e}");
        assert!(e.contains("i, m, s, x"), "got: {e}");
    }

    #[test]
    fn replace_first_only() {
        let lua = engine();
        let out: String = lua
            .load(r#"return regex.replace("a", "aaa", "X")"#)
            .eval()
            .unwrap();
        assert_eq!(out, "Xaa");
    }

    #[test]
    fn replace_all_replaces_every_match() {
        let lua = engine();
        let out: String = lua
            .load(r#"return regex.replace_all("a", "aaa", "X")"#)
            .eval()
            .unwrap();
        assert_eq!(out, "XXX");
    }

    #[test]
    fn replace_uses_capture_references() {
        // `$1` / `${name}` per the regex crate's replacement grammar — matches
        // PCRE/Rust convention; documented on the d.luau side.
        let lua = engine();
        let (positional, named): (String, String) = lua
            .load(
                r#"
                return
                    regex.replace_all("(\\w+)=(\\w+)", "a=1 b=2", "$2=$1"),
                    regex.replace_all("(?P<k>\\w+)=(?P<v>\\w+)", "x=10", "${v}=${k}")
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(positional, "1=a 2=b");
        assert_eq!(named, "10=x");
    }

    #[test]
    fn split_basic() {
        let lua = engine();
        let parts: Vec<String> = lua
            .load(r#"return regex.split("\\s+", "  one  two   three ")"#)
            .eval()
            .unwrap();
        // Leading whitespace produces an empty leading element — matches
        // Rust's `Regex::split` semantics. Documented; callers can filter.
        assert_eq!(parts, vec!["", "one", "two", "three", ""]);
    }

    #[test]
    fn split_limit_truncates_into_n_parts() {
        let lua = engine();
        let parts: Vec<String> = lua
            .load(r#"return regex.split(",", "a,b,c,d", 2)"#)
            .eval()
            .unwrap();
        assert_eq!(parts, vec!["a", "b,c,d"]);
    }

    #[test]
    fn split_limit_zero_unlimited() {
        let lua = engine();
        let parts: Vec<String> = lua
            .load(r#"return regex.split(",", "a,b,c", 0)"#)
            .eval()
            .unwrap();
        assert_eq!(parts, vec!["a", "b", "c"]);
    }

    #[test]
    fn split_negative_limit_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return regex.split(",", "a,b,c", -1)"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.contains("limit must be >= 0"), "got: {e}");
    }

    #[test]
    fn escape_neutralises_meta_chars() {
        let lua = engine();
        // After escape, the regex literally matches the raw input even though
        // it's full of meta chars. Round-trips through is_match.
        let ok: bool = lua
            .load(
                r#"
                local raw = "1.2.3+build (rc)"
                local pat = regex.escape(raw)
                return regex.is_match(pat, raw)
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn pattern_userdata_reusable() {
        let lua = engine();
        let total: i64 = lua
            .load(
                r#"
                local p = regex.Pattern.new("\\d+")
                local n = 0
                for _ in ipairs(p:find_all("1 22 333")) do n += 1 end
                if p:is_match("nope") then return -1 end
                return n
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(total, 3);
    }

    #[test]
    fn pattern_tostring_returns_source() {
        let lua = engine();
        let s: String = lua
            .load(r#"return tostring(regex.Pattern.new("\\d+"))"#)
            .eval()
            .unwrap();
        assert_eq!(s, r"\d+");
    }

    #[test]
    fn pattern_new_invalid_errors() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return regex.Pattern.new("(abc")"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(e.starts_with("regex.Pattern.new:"), "got: {e}");
    }

    /// `Pattern:find` is infallible — the regex is already compiled, so a
    /// no-match cannot raise. Return shape must be a bare `Match?`, not the
    /// `(Match?, string?)` tuple the flat `regex.find` uses. Lock the
    /// observable return arity so the d.luau signature stays accurate.
    #[test]
    fn pattern_find_returns_single_value() {
        let lua = engine();
        let (hit, miss): (i64, i64) = lua
            .load(
                r##"
                local p = regex.Pattern.new("\\d+")
                return
                    select("#", p:find("abc 123")),
                    select("#", p:find("abc"))
            "##,
            )
            .eval()
            .unwrap();
        assert_eq!(hit, 1, "Pattern:find must return exactly one value on hit");
        assert_eq!(
            miss, 1,
            "Pattern:find must return exactly one value on miss"
        );
    }

    /// UserData methods carry the full `module.Type.method` error prefix —
    /// matching `semver.Version.increment`, `glob.Glob.new`, etc.
    #[test]
    fn pattern_split_error_prefix_is_module_scoped() {
        let lua = engine();
        let (val, e): (Value, Option<String>) = lua
            .load(r#"return regex.Pattern.new(","):split("a,b", -1)"#)
            .eval()
            .unwrap();
        assert!(matches!(val, Value::Nil));
        let e = e.unwrap();
        assert!(
            e.starts_with("regex.Pattern.split:"),
            "got: {e} — expected 'regex.Pattern.split:' prefix"
        );
    }

    #[test]
    fn pattern_with_flags() {
        let lua = engine();
        let yes: bool = lua
            .load(r#"return regex.Pattern.new("hello", "i"):is_match("HELLO")"#)
            .eval()
            .unwrap();
        assert!(yes);
    }

    #[test]
    fn unicode_matching_on_by_default() {
        // `\w` includes Unicode letters in the regex crate's default mode.
        let lua = engine();
        let yes: bool = lua
            .load(r#"return regex.is_match("^\\w+$", "café")"#)
            .eval()
            .unwrap();
        assert!(yes, "default \\w must match non-ASCII letters");
    }

    #[test]
    fn unicode_position_uses_byte_offsets() {
        // 'é' is two UTF-8 bytes. In "café X" the bytes are
        // c=1, a=2, f=3, é=4..5, space=6 — so the match `é` starts at byte 4
        // and ends at byte 5. Confirm we report byte offsets, not char
        // offsets — matches Lua's own string convention.
        let lua = engine();
        let (start, finish): (i64, i64) = lua
            .load(
                r#"
                local m = regex.find("é", "café X")
                return m.start, m.finish
            "#,
            )
            .eval()
            .unwrap();
        // "caf" = 3 bytes → 'é' starts at byte 4 (1-based), ends at byte 5.
        assert_eq!((start, finish), (4, 5));
    }

    #[test]
    fn round_trip_invariant_find_all_each_is_match() {
        // For every match returned by find_all: re-running is_match on the
        // matched substring must be true. Locks the invariant independently
        // of the captures plumbing.
        let lua = engine();
        let ok: bool = lua
            .load(
                r#"
                local pat = "\\w{3,}"
                local text = "the quick brown fox"
                for _, m in ipairs(regex.find_all(pat, text)) do
                    if not regex.is_match(pat, m.text) then return false end
                end
                return true
            "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }
}

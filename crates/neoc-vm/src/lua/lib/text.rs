//! String utilities and case conversion — `lib:text`.

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutySnakeCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase,
};
use mlua::{Lua, Table, Value};
use strsim::levenshtein as strsim_levenshtein;
use tracing::instrument;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use crate::lua::facade::Module;
use crate::lua::limits;
use crate::lua::sandbox::{err, ok};

const ELLIPSIS: &str = "...";
const ELLIPSIS_WIDTH: usize = 3;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:text` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// each carries its own `#[instrument]` span, so routing through the facade's
/// `func` / `try_func` would open a second span. `word_wrap` / `closest` /
/// `truncate` / the pad family build their `(value, error)` returns directly
/// through `err` / `ok` with a baked `"text.<fn>: "` prefix; the rest return a
/// bare value. `clip` is among the rest although it refuses a `max` that is
/// not a count: it raises, as for an argument of the wrong type, so that the
/// common call is a plain `string` and not a `string?` to unwrap. Their
/// signatures are read by `neoc_macros` from the implementation.
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:text")?;

    m.declared(to_camel_case::lua(), lua.create_function(to_camel_case)?)?;

    m.declared(to_snake_case::lua(), lua.create_function(to_snake_case)?)?;

    m.declared(to_kebab_case::lua(), lua.create_function(to_kebab_case)?)?;

    m.declared(to_pascal_case::lua(), lua.create_function(to_pascal_case)?)?;

    m.declared(to_title_case::lua(), lua.create_function(to_title_case)?)?;

    m.declared(
        to_constant_case::lua(),
        lua.create_function(to_constant_case)?,
    )?;

    m.declared(word_wrap::lua(), lua.create_function(word_wrap)?)?;

    m.declared(levenshtein::lua(), lua.create_function(levenshtein)?)?;

    m.declared(closest::lua(), lua.create_function(closest)?)?;

    m.declared(truncate::lua(), lua.create_function(truncate)?)?;

    m.declared(pad_left::lua(), lua.create_function(pad_left)?)?;

    m.declared(pad_right::lua(), lua.create_function(pad_right)?)?;

    m.declared(center::lua(), lua.create_function(center)?)?;

    m.declared(slugify::lua(), lua.create_function(slugify)?)?;

    m.declared(trim::lua(), lua.create_function(trim)?)?;

    m.declared(trim_start::lua(), lua.create_function(trim_start)?)?;

    m.declared(trim_end::lua(), lua.create_function(trim_end)?)?;

    m.declared(clip::lua(), lua.create_function(clip)?)?;

    m.declared(to_valid_utf8::lua(), lua.create_function(to_valid_utf8)?)?;

    m.declared(strip_ansi::lua(), lua.create_function(strip_ansi)?)?;

    Ok(m)
}

// ---------------------------------------------------------------------------
// Case conversion — heck does the word-boundary detection and acronym handling.
// ---------------------------------------------------------------------------

/// Convert to `camelCase`. Acronym boundaries are detected by the backing `heck` crate.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_camel_case", target = "lua::lib::text", skip_all)]
fn to_camel_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_lower_camel_case())
}

/// Convert to `snake_case`.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_snake_case", target = "lua::lib::text", skip_all)]
fn to_snake_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_snake_case())
}

/// Convert to `kebab-case`.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_kebab_case", target = "lua::lib::text", skip_all)]
fn to_kebab_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_kebab_case())
}

/// Convert to `PascalCase`.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_pascal_case", target = "lua::lib::text", skip_all)]
fn to_pascal_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_upper_camel_case())
}

/// Convert to `Title Case`.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_title_case", target = "lua::lib::text", skip_all)]
fn to_title_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_title_case())
}

/// Convert to `CONSTANT_CASE`.
#[neoc_macros::lua_fn(names(s = "text"))]
#[instrument(name = "text.to_constant_case", target = "lua::lib::text", skip_all)]
fn to_constant_case(_: &Lua, s: mlua::LuaString) -> mlua::Result<String> {
    Ok(s.to_str()?.to_shouty_snake_case())
}

// ---------------------------------------------------------------------------
// word_wrap — greedy word-boundary wrap, display-width aware.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct WrapOpts {
    break_words: bool,
    preserve_newlines: bool,
}

impl Default for WrapOpts {
    fn default() -> Self {
        // `preserve_newlines = true` matches Python textwrap.fill on
        // pre-formatted paragraphs and is the safe default — flattening
        // newlines silently merges intentional paragraph breaks.
        Self {
            break_words: false,
            preserve_newlines: true,
        }
    }
}

fn read_bool_opt(t: &Table, key: &str, prefix: &str) -> Result<Option<bool>, String> {
    match t.get::<Value>(key).map_err(|e| format!("{prefix}: {e}"))? {
        Value::Nil => Ok(None),
        Value::Boolean(b) => Ok(Some(b)),
        _ => Err(format!("{prefix}: '{key}' must be a boolean")),
    }
}

fn parse_wrap_opts(opts: Option<&Table>) -> Result<WrapOpts, String> {
    let mut out = WrapOpts::default();
    let Some(t) = opts else { return Ok(out) };
    if let Some(v) = read_bool_opt(t, "break_words", "text.word_wrap")? {
        out.break_words = v;
    }
    if let Some(v) = read_bool_opt(t, "preserve_newlines", "text.word_wrap")? {
        out.preserve_newlines = v;
    }
    Ok(out)
}

/// Greedy word wrap, display-width aware. `width` must be > 0. Defaults: `break_words = false`, `preserve_newlines = true`.
#[neoc_macros::lua_fn(returns = "Result<string, string>")]
#[instrument(name = "text.word_wrap", target = "lua::lib::text", skip_all)]
fn word_wrap(
    lua: &Lua,
    (text, width, opts): (mlua::LuaString, i64, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    if width <= 0 {
        return err(lua, "text.word_wrap: width must be > 0");
    }
    let width = width as usize;
    let parsed = match parse_wrap_opts(opts.as_ref()) {
        Ok(o) => o,
        Err(m) => return err(lua, m),
    };
    let s_ref = text.to_str()?;
    ok(lua, wrap_text(&s_ref, width, parsed))
}

fn wrap_text(text: &str, width: usize, opts: WrapOpts) -> String {
    // Iterator chain — no intermediate Vec<String> for paragraphs.
    if opts.preserve_newlines {
        text.split('\n')
            .map(|p| wrap_paragraph(p, width, opts.break_words))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        wrap_paragraph(&text.replace('\n', " "), width, opts.break_words)
    }
}

fn wrap_paragraph(para: &str, width: usize, break_words: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0usize;

    for word in para.split_whitespace() {
        let word_w = display_width(word);
        let projected = if current.is_empty() {
            word_w
        } else {
            current_w + 1 + word_w
        };

        if projected <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_w = projected;
            continue;
        }

        // Word doesn't fit on the current line. Flush and reset; the
        // overwrite of `current_w` below guarantees no stale read.
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }

        if word_w <= width || !break_words {
            // Fits on its own line, or break_words=false → overflow allowed.
            current.push_str(word);
            current_w = word_w;
            continue;
        }

        // Break the oversize word into width-sized chunks in place — emit
        // completed chunks straight into `lines` and leave the final partial
        // chunk in `current` for possible coalescing with the next word.
        let (completed, tail, tail_w) = chunk_by_width(word, width);
        lines.extend(completed);
        current = tail;
        current_w = tail_w;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join("\n")
}

fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

// Splits `s` into display-width-bounded chunks. All but the last go into the
// returned `Vec`; the last (possibly partial) lives in the trailing tuple slot
// so the caller can coalesce it with following input on the same output line.
fn chunk_by_width(s: &str, width: usize) -> (Vec<String>, String, usize) {
    let mut completed = Vec::new();
    let mut buf = String::new();
    let mut buf_w = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if !buf.is_empty() && buf_w + cw > width {
            completed.push(std::mem::take(&mut buf));
            buf_w = 0;
        }
        buf.push(c);
        buf_w += cw;
    }
    (completed, buf, buf_w)
}

// ---------------------------------------------------------------------------
// levenshtein / closest
// ---------------------------------------------------------------------------

/// Codepoint-based Levenshtein edit distance.
#[neoc_macros::lua_fn]
#[instrument(name = "text.levenshtein", target = "lua::lib::text", skip_all)]
fn levenshtein(_: &Lua, (a, b): (mlua::LuaString, mlua::LuaString)) -> mlua::Result<i64> {
    let a = a.to_str()?;
    let b = b.to_str()?;
    Ok(strsim_levenshtein(&a, &b) as i64)
}

fn parse_closest_limit(opts: Option<&Table>) -> Result<Option<usize>, String> {
    let prefix = "text.closest";
    let Some(t) = opts else { return Ok(None) };
    match t
        .get::<Value>("limit")
        .map_err(|e| format!("{prefix}: invalid 'limit': {e}"))?
    {
        Value::Nil => Ok(None),
        Value::Integer(n) if n < 0 => Err(format!("{prefix}: 'limit' must be >= 0")),
        Value::Integer(n) => Ok(Some(n as usize)),
        Value::Number(n) if n.fract() == 0.0 && n >= 0.0 => Ok(Some(n as usize)),
        _ => Err(format!("{prefix}: 'limit' must be a non-negative integer")),
    }
}

/// Score each candidate against `input` by Levenshtein distance, returning the list sorted ascending (ties broken alphabetically).
#[neoc_macros::lua_fn(returns = "Result<LuaValue, string>")]
#[instrument(name = "text.closest", target = "lua::lib::text", skip_all)]
fn closest(
    lua: &Lua,
    (input, candidates, opts): (mlua::LuaString, Table, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let input_str = input.to_str()?.to_string();

    let mut cands: Vec<String> = Vec::new();
    for entry in candidates.sequence_values::<Value>() {
        match entry? {
            Value::String(s) => cands.push(s.to_str()?.to_string()),
            _ => return err(lua, "text.closest: candidates must be strings"),
        }
    }
    if cands.is_empty() {
        return ok(lua, Value::Nil);
    }

    let limit = match parse_closest_limit(opts.as_ref()) {
        Ok(v) => v,
        Err(m) => return err(lua, m),
    };

    let mut scored: Vec<(String, usize)> = cands
        .into_iter()
        .map(|c| {
            let d = strsim_levenshtein(&input_str, &c);
            (c, d)
        })
        .collect();
    // Ties broken alphabetically — deterministic ordering across runs.
    scored.sort_by(|(a, da), (b, db)| da.cmp(db).then_with(|| a.cmp(b)));
    if let Some(n) = limit {
        scored.truncate(n);
    }

    let tbl = lua.create_table()?;
    for (i, (m, d)) in scored.into_iter().enumerate() {
        let row = lua.create_table()?;
        row.set("match", m)?;
        row.set("distance", d as i64)?;
        tbl.raw_set(i + 1, row)?;
    }
    ok(lua, tbl)
}

// ---------------------------------------------------------------------------
// truncate
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Boundary {
    Char,
    Word,
}

fn parse_truncate_boundary(opts: Option<&Table>) -> Result<Boundary, String> {
    let prefix = "text.truncate";
    let Some(t) = opts else {
        return Ok(Boundary::Char);
    };
    match t
        .get::<Value>("boundary")
        .map_err(|e| format!("{prefix}: invalid 'boundary': {e}"))?
    {
        Value::Nil => Ok(Boundary::Char),
        Value::String(bs) => {
            let s = bs
                .to_str()
                .map_err(|e| format!("{prefix}: 'boundary' not utf-8: {e}"))?;
            match s.as_ref() {
                "char" => Ok(Boundary::Char),
                "word" => Ok(Boundary::Word),
                other => Err(format!(
                    "{prefix}: 'boundary' must be \"char\" or \"word\" (got {other:?})"
                )),
            }
        }
        _ => Err(format!("{prefix}: 'boundary' must be a string")),
    }
}

/// Truncate `text` so its display width is at most `max_len`. The `...` ellipsis counts toward the limit.
#[neoc_macros::lua_fn(returns = "Result<string, string>")]
#[instrument(name = "text.truncate", target = "lua::lib::text", skip_all)]
fn truncate(
    lua: &Lua,
    (text, max_len, opts): (mlua::LuaString, i64, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    if max_len < 0 {
        return err(lua, "text.truncate: max_len must be >= 0");
    }
    let max = max_len as usize;

    let boundary = match parse_truncate_boundary(opts.as_ref()) {
        Ok(b) => b,
        Err(m) => return err(lua, m),
    };

    let s_ref = text.to_str()?;
    let s: &str = &s_ref;
    if display_width(s) <= max {
        return ok(lua, s.to_string());
    }

    // When max can't fit the ellipsis, drop the ellipsis and take a prefix.
    if max < ELLIPSIS_WIDTH {
        return ok(lua, take_by_width(s, max));
    }

    let budget = max - ELLIPSIS_WIDTH;
    let prefix = match boundary {
        Boundary::Char => take_by_width(s, budget),
        Boundary::Word => take_at_word(s, budget),
    };
    ok(lua, format!("{prefix}{ELLIPSIS}"))
}

fn take_by_width(s: &str, budget: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > budget {
            break;
        }
        out.push(c);
        used += cw;
    }
    out
}

// Single-pass: track the position of the last whitespace as we accumulate.
// At budget exhaust, snap back to that position. Avoids a second `rfind` scan.
fn take_at_word(s: &str, budget: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    let mut last_ws_end: Option<usize> = None;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > budget {
            break;
        }
        if c.is_whitespace() {
            last_ws_end = Some(out.len());
        }
        out.push(c);
        used += cw;
    }
    match last_ws_end {
        Some(end) => out[..end].trim_end().to_string(),
        None => out,
    }
}

// ---------------------------------------------------------------------------
// pad_left / pad_right / center
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PadSide {
    Left,
    Right,
    Center,
}

/// Pad `text` on the left so total display width equals `width`. `fill_char` must be one display-width-1 character (default space).
#[neoc_macros::lua_fn(returns = "Result<string, string>", names(fill = "fill_char"))]
#[instrument(name = "text.pad_left", target = "lua::lib::text", skip_all)]
fn pad_left(
    lua: &Lua,
    (text, width, fill): (mlua::LuaString, i64, Option<mlua::LuaString>),
) -> mlua::Result<(Value, Value)> {
    pad_text(lua, "text.pad_left", text, width, fill, PadSide::Left)
}

/// Pad `text` on the right so total display width equals `width`. Same constraints as `pad_left`.
#[neoc_macros::lua_fn(returns = "Result<string, string>", names(fill = "fill_char"))]
#[instrument(name = "text.pad_right", target = "lua::lib::text", skip_all)]
fn pad_right(
    lua: &Lua,
    (text, width, fill): (mlua::LuaString, i64, Option<mlua::LuaString>),
) -> mlua::Result<(Value, Value)> {
    pad_text(lua, "text.pad_right", text, width, fill, PadSide::Right)
}

/// Pad `text` on both sides so total display width equals `width`. Uneven padding places the extra column on the right.
#[neoc_macros::lua_fn(returns = "Result<string, string>", names(fill = "fill_char"))]
#[instrument(name = "text.center", target = "lua::lib::text", skip_all)]
fn center(
    lua: &Lua,
    (text, width, fill): (mlua::LuaString, i64, Option<mlua::LuaString>),
) -> mlua::Result<(Value, Value)> {
    pad_text(lua, "text.center", text, width, fill, PadSide::Center)
}

// Validates and extracts a single-codepoint, single-column fill character.
// Default `" "` when no fill is supplied.
fn parse_fill_char(fill: Option<mlua::LuaString>, prefix: &str) -> Result<char, String> {
    let Some(s) = fill else { return Ok(' ') };
    let s_ref = s.to_str().map_err(|e| format!("{prefix}: {e}"))?;
    let mut chars = s_ref.chars();
    let Some(c) = chars.next() else {
        return Err(format!("{prefix}: fill char must be exactly one character"));
    };
    if chars.next().is_some() {
        return Err(format!("{prefix}: fill char must be exactly one character"));
    }
    if UnicodeWidthChar::width(c).unwrap_or(0) != 1 {
        return Err(format!("{prefix}: fill char must have display width 1"));
    }
    Ok(c)
}

fn pad_text(
    lua: &Lua,
    prefix: &str,
    text: mlua::LuaString,
    width: i64,
    fill: Option<mlua::LuaString>,
    side: PadSide,
) -> mlua::Result<(Value, Value)> {
    let width = match limits::size(width, "width") {
        Ok(w) => w,
        Err(m) => return err(lua, format!("{prefix}: {m}")),
    };
    let fill_char = match parse_fill_char(fill, prefix) {
        Ok(c) => c,
        Err(m) => return err(lua, m),
    };
    let s_ref = text.to_str()?;
    let s: &str = &s_ref;
    let s_w = display_width(s);
    if s_w >= width {
        return ok(lua, s.to_string());
    }
    let pad = width - s_w;
    let fill_str = |n: usize| -> String { std::iter::repeat_n(fill_char, n).collect() };
    let out = match side {
        PadSide::Left => format!("{}{s}", fill_str(pad)),
        PadSide::Right => format!("{s}{}", fill_str(pad)),
        PadSide::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{s}{}", fill_str(left), fill_str(right))
        }
    };
    ok(lua, out)
}

// ---------------------------------------------------------------------------
// slugify — strip diacritics (URL-safe default), lowercase, hyphens.
// ---------------------------------------------------------------------------

/// Convert `text` to a URL-safe ASCII slug via `deunicode` transliteration, then lowercase and hyphenate.
#[neoc_macros::lua_fn]
#[instrument(name = "text.slugify", target = "lua::lib::text", skip_all)]
fn slugify(_: &Lua, text: mlua::LuaString) -> mlua::Result<String> {
    let s_ref = text.to_str()?;
    Ok(slugify_str(&s_ref))
}

fn slugify_str(input: &str) -> String {
    let ascii = deunicode::deunicode(input);
    let mut out = String::with_capacity(ascii.len());
    let mut pending_hyphen = false;
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_hyphen && !out.is_empty() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
            pending_hyphen = false;
        } else {
            pending_hyphen = true;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// trim — std's `str::trim*`, which trim the Unicode `White_Space` property.
// ---------------------------------------------------------------------------

/// Remove Unicode white space (the `White_Space` property: spaces, tabs, line breaks, U+00A0, U+3000, ...) from both ends of `text`. Raises when `text` is not valid UTF-8 (repair it with `to_valid_utf8` first).
#[neoc_macros::lua_fn]
#[instrument(name = "text.trim", target = "lua::lib::text", skip_all)]
fn trim(_: &Lua, text: mlua::LuaString) -> mlua::Result<String> {
    Ok(text.to_str()?.trim().to_owned())
}

/// Remove Unicode white space from the start of `text`. Raises when `text` is not valid UTF-8 (repair it with `to_valid_utf8` first).
#[neoc_macros::lua_fn]
#[instrument(name = "text.trim_start", target = "lua::lib::text", skip_all)]
fn trim_start(_: &Lua, text: mlua::LuaString) -> mlua::Result<String> {
    Ok(text.to_str()?.trim_start().to_owned())
}

/// Remove Unicode white space from the end of `text`. Raises when `text` is not valid UTF-8 (repair it with `to_valid_utf8` first).
#[neoc_macros::lua_fn]
#[instrument(name = "text.trim_end", target = "lua::lib::text", skip_all)]
fn trim_end(_: &Lua, text: mlua::LuaString) -> mlua::Result<String> {
    Ok(text.to_str()?.trim_end().to_owned())
}

// ---------------------------------------------------------------------------
// clip — at most `max` extended grapheme clusters (unicode-segmentation).
// ---------------------------------------------------------------------------

/// Clip `text` to at most `max` characters. A character is an extended grapheme cluster (UAX #29, what `lib:unicode.graphemes` yields and a Swift `Character` is): a flag emoji or a letter with its combining accent is one, and is never cut apart. When `text` is cut and `ellipsis` is given, the ellipsis ends the result and counts toward `max`; an ellipsis longer than `max` is left out. The count bounds characters, not bytes: one character may carry any number of combining marks. Raises (it does not return an error) when `max` is negative, fractional or NaN, and when `text` or `ellipsis` is not valid UTF-8; a `max` beyond any text, infinity included, is the whole text.
#[neoc_macros::lua_fn]
#[instrument(name = "text.clip", target = "lua::lib::text", skip_all)]
fn clip(
    _: &Lua,
    (text, max, ellipsis): (mlua::LuaString, f64, Option<mlua::LuaString>),
) -> mlua::Result<String> {
    if max.is_nan() || max < 0.0 || (max.is_finite() && max.fract() != 0.0) {
        return Err(mlua::Error::external(
            "text.clip: max must be a whole number >= 0",
        ));
    }
    // A float-to-integer `as` saturates: a count beyond `usize` (infinity too) is one no text reaches.
    let max = max as usize;
    let text = text.to_str()?;
    let ellipsis = match &ellipsis {
        Some(e) => e.to_str()?.to_owned(),
        None => String::new(),
    };
    Ok(clip_str(&text, max, &ellipsis))
}

/// The byte offset where the grapheme at index `count` starts, or `None` when `s` has no more than `count`.
fn grapheme_offset(s: &str, count: usize) -> Option<usize> {
    s.grapheme_indices(true).nth(count).map(|(at, _)| at)
}

fn clip_str(text: &str, max: usize, ellipsis: &str) -> String {
    let Some(cut) = grapheme_offset(text, max) else {
        return text.to_owned();
    };
    // An ellipsis that does not fit in `max` is left out, as `truncate` leaves out its own. Counting it stops
    // one past `max`: that is enough to know it does not fit.
    let ellipsis_len = ellipsis.graphemes(true).take(max.saturating_add(1)).count();
    let Some(kept) = max.checked_sub(ellipsis_len) else {
        return text[..cut].to_owned();
    };
    let cut = grapheme_offset(text, kept).unwrap_or(cut);
    format!("{}{ellipsis}", &text[..cut])
}

// ---------------------------------------------------------------------------
// to_valid_utf8 — std's lossy decoding.
// ---------------------------------------------------------------------------

/// Make any byte string valid UTF-8: bytes that are not part of a character are replaced with U+FFFD (the replacement character). Valid input is returned as it is.
#[neoc_macros::lua_fn]
#[instrument(name = "text.to_valid_utf8", target = "lua::lib::text", skip_all)]
fn to_valid_utf8(lua: &Lua, bytes: mlua::LuaString) -> mlua::Result<mlua::LuaString> {
    let repaired = match String::from_utf8_lossy(&bytes.as_bytes()) {
        std::borrow::Cow::Borrowed(_) => None,
        std::borrow::Cow::Owned(repaired) => Some(repaired),
    };
    match repaired {
        None => Ok(bytes),
        Some(repaired) => lua.create_string(repaired),
    }
}

// ---------------------------------------------------------------------------
// strip_ansi — vte parses the terminal's language; what it would print stays.
// ---------------------------------------------------------------------------

/// What a terminal would show of the bytes `vte` parsed: the printed characters, line feeds and tabs.
struct Shown(String);

impl vte::Perform for Shown {
    fn print(&mut self, c: char) {
        // DEL and the C1 controls reach `print`; they are no more text than the C0 controls `execute` gets.
        if !c.is_control() {
            self.0.push(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if matches!(byte, b'\n' | b'\t') {
            self.0.push(char::from(byte));
        }
    }
}

/// Remove what a terminal would act on instead of show: escape sequences (colours, cursor movement, window titles) and control characters. Line feeds and tabs stay, so `\r\n` becomes `\n`. Each line is read on its own, so a sequence that is never ended takes the rest of its line and no more. A C1 control written as a character (U+0080 to U+009F) is removed without being read as the start of a sequence. Raises when `text` is not valid UTF-8 (repair it with `to_valid_utf8` first).
#[neoc_macros::lua_fn]
#[instrument(name = "text.strip_ansi", target = "lua::lib::text", skip_all)]
fn strip_ansi(_: &Lua, text: mlua::LuaString) -> mlua::Result<String> {
    Ok(strip_ansi_str(&text.to_str()?))
}

fn strip_ansi_str(text: &str) -> String {
    let mut shown = Shown(String::new());
    // A parser per line: a string sequence (a window title, say) that is never ended would otherwise take
    // every line after it, and one stray `ESC ]` would hide the rest of a log.
    // The line feed is put back by hand: inside such a sequence the parser would take it as well.
    for (at, line) in text.split('\n').enumerate() {
        if at > 0 {
            shown.0.push('\n');
        }
        vte::Parser::new().advance(&mut shown, line.as_bytes());
    }
    shown.0
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
pub fn benches(c: &mut criterion::Criterion) {
    use criterion::Throughput;
    use std::hint::black_box;

    let lua = crate::lua::sandbox::make_engine().expect("engine");
    let payload = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let payload_lua = lua.create_string(&payload).expect("payload");

    let mut g = c.benchmark_group("text");
    g.throughput(Throughput::Bytes(payload.len() as u64));

    g.bench_function("library_to_snake", |b| {
        b.iter(|| {
            let _ = black_box(&payload).to_snake_case();
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
        .load(r#"return function(p) return (require("lib:text")).to_snake_case(p) end"#)
        .eval()
        .expect("chunk");
    g.bench_function("through_lua", |b| {
        b.iter(|| {
            let _: String = chunk.call(black_box(payload_lua.clone())).unwrap();
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    fn engine() -> Lua {
        let lua = crate::lua::sandbox::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:text")"#).eval().unwrap();
        lua.globals().set("text", m).unwrap();
        lua
    }

    #[test]
    fn snake_pins() {
        assert_eq!("helloWorld".to_snake_case(), "hello_world");
        assert_eq!("HTTPSConnection".to_snake_case(), "https_connection");
        assert_eq!("getHTTPSURL".to_snake_case(), "get_httpsurl");
        assert_eq!("XMLParser".to_snake_case(), "xml_parser");
        assert_eq!("".to_snake_case(), "");
        assert_eq!("a".to_snake_case(), "a");
        assert_eq!("already_snake".to_snake_case(), "already_snake");
        assert_eq!("kebab-case".to_snake_case(), "kebab_case");
    }

    #[test]
    fn case_edge_inputs_pinned() {
        // Whitespace gets stripped, non-alphanumeric becomes a boundary.
        assert_eq!("  spaces  ".to_snake_case(), "spaces");
        assert_eq!("  spaces  ".to_lower_camel_case(), "spaces");
        assert_eq!("  spaces  ".to_upper_camel_case(), "Spaces");
        assert_eq!("  spaces  ".to_kebab_case(), "spaces");
        assert_eq!("  spaces  ".to_shouty_snake_case(), "SPACES");

        // Digits stay attached to adjacent letters (no digit-letter boundary).
        assert_eq!("123numeric".to_snake_case(), "123numeric");
        assert_eq!("123numeric".to_lower_camel_case(), "123numeric");
        assert_eq!("123numeric".to_upper_camel_case(), "123numeric");

        // Dots split as non-alphanumeric.
        assert_eq!("with.dots.in".to_snake_case(), "with_dots_in");
        assert_eq!("with.dots.in".to_lower_camel_case(), "withDotsIn");
        assert_eq!("with.dots.in".to_upper_camel_case(), "WithDotsIn");

        // Mixed: heck splits inside MIXed as UPPER-UPPER-lower → MI / Xed.
        assert_eq!("MIXed_CASE-stuff".to_snake_case(), "mi_xed_case_stuff");
        assert_eq!("MIXed_CASE-stuff".to_lower_camel_case(), "miXedCaseStuff");
        assert_eq!("MIXed_CASE-stuff".to_upper_camel_case(), "MiXedCaseStuff");

        // Title case behaviour for acronyms.
        assert_eq!("helloWorld".to_title_case(), "Hello World");
        assert_eq!("HTTPSConnection".to_title_case(), "Https Connection");
    }

    #[test]
    fn levenshtein_pins() {
        assert_eq!(strsim_levenshtein("kitten", "sitting"), 3);
        assert_eq!(strsim_levenshtein("", "abc"), 3);
        assert_eq!(strsim_levenshtein("abc", ""), 3);
        assert_eq!(strsim_levenshtein("abc", "abc"), 0);
        assert_eq!(strsim_levenshtein("", ""), 0);
        // Symmetry — property of the metric, sanity-check the backing impl.
        assert_eq!(
            strsim_levenshtein("kitten", "sitting"),
            strsim_levenshtein("sitting", "kitten")
        );
    }

    #[test]
    fn slugify_pins() {
        assert_eq!(slugify_str("Hello, World!"), "hello-world");
        assert_eq!(slugify_str("  leading trailing  "), "leading-trailing");
        assert_eq!(slugify_str("---multiple---hyphens---"), "multiple-hyphens");
        assert_eq!(slugify_str(""), "");
        assert_eq!(slugify_str("café résumé"), "cafe-resume");
    }

    #[test]
    fn word_wrap_basic() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.word_wrap("hello world", 5)))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "hello\nworld");
    }

    #[test]
    fn word_wrap_break_words() {
        let opts = WrapOpts {
            break_words: true,
            preserve_newlines: true,
        };
        assert_eq!(wrap_text("abcdefghij", 5, opts), "abcde\nfghij");
    }

    #[test]
    fn word_wrap_break_words_disabled_overflows() {
        let opts = WrapOpts {
            break_words: false,
            preserve_newlines: true,
        };
        assert_eq!(wrap_text("abcdefghij", 5, opts), "abcdefghij");
    }

    #[test]
    fn word_wrap_preserve_newlines_true() {
        let opts = WrapOpts {
            break_words: false,
            preserve_newlines: true,
        };
        assert_eq!(wrap_text("line1\nline2", 80, opts), "line1\nline2");
    }

    #[test]
    fn word_wrap_preserve_newlines_false() {
        let opts = WrapOpts {
            break_words: false,
            preserve_newlines: false,
        };
        assert_eq!(wrap_text("line1\nline2", 80, opts), "line1 line2");
    }

    #[test]
    fn word_wrap_zero_width_errs() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return text.word_wrap("hi", 0)"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().contains("width must be > 0"));
    }

    #[test]
    fn truncate_char_boundary() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.truncate("hello world", 8)))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "hello...");
    }

    #[test]
    fn truncate_word_boundary() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.truncate("hello world", 8, { boundary = "word" })))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "hello...");
    }

    #[test]
    fn truncate_no_op_when_short() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.truncate("hi", 8)))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "hi");
    }

    #[test]
    fn truncate_max_below_ellipsis_drops_ellipsis() {
        let lua = engine();
        let two: String = lua
            .load(r#"return (select(1, text.truncate("hello", 2)))"#)
            .eval()
            .unwrap();
        assert_eq!(two, "he");
        let three: String = lua
            .load(r#"return (select(1, text.truncate("hello", 3)))"#)
            .eval()
            .unwrap();
        assert_eq!(three, "...");
    }

    #[test]
    fn pad_left_default_fill() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.pad_left("42", 5)))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "   42");
    }

    #[test]
    fn pad_left_custom_fill() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.pad_left("42", 5, "0")))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "00042");
    }

    #[test]
    fn pad_right_default_fill() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.pad_right("name", 10)))"#)
            .eval()
            .unwrap();
        assert_eq!(s, "name      ");
    }

    #[test]
    fn center_basic() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.center("title", 11)))"#)
            .eval()
            .unwrap();
        // 11 - 5 = 6, split 3/3
        assert_eq!(s, "   title   ");
    }

    #[test]
    fn center_uneven_extra_goes_right() {
        let lua = engine();
        let s: String = lua
            .load(r#"return (select(1, text.center("ab", 5)))"#)
            .eval()
            .unwrap();
        // 5 - 2 = 3 → left=1, right=2
        assert_eq!(s, " ab  ");
    }

    #[test]
    fn pad_rejects_multi_char_fill() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return text.pad_left("x", 5, "ab")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().contains("exactly one character"));
    }

    #[test]
    fn pad_rejects_empty_fill() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return text.pad_left("x", 5, "")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().contains("exactly one character"));
    }

    #[test]
    fn pad_rejects_double_width_fill() {
        let lua = engine();
        let (val, err): (Option<String>, Option<String>) = lua
            .load(r#"return text.pad_left("x", 5, "あ")"#)
            .eval()
            .unwrap();
        assert!(val.is_none());
        assert!(err.unwrap().contains("display width 1"));
    }

    #[test]
    fn closest_basic() {
        let lua = engine();
        // "tea" and "ten" both at distance 1 from "teh" (single substitute);
        // "the" is distance 2 (e/h transpose costs two substitutions).
        // Alphabetical tie-break orders "tea" before "ten".
        let (first, second, third, d1, d3): (String, String, String, i64, i64) = lua
            .load(
                r#"
                local r = text.closest("teh", {"the", "tea", "ten"})
                return r[1].match, r[2].match, r[3].match, r[1].distance, r[3].distance
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(first, "tea");
        assert_eq!(second, "ten");
        assert_eq!(third, "the");
        assert_eq!(d1, 1);
        assert_eq!(d3, 2);
    }

    #[test]
    fn closest_limit() {
        let lua = engine();
        let len: i64 = lua
            .load(
                r#"
                local r = text.closest("teh", {"the", "tea", "ten"}, { limit = 2 })
                return #r
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(len, 2);
    }

    #[test]
    fn closest_empty_candidates_returns_nil() {
        let lua = engine();
        let nil: Option<mlua::Table> = lua.load(r#"return text.closest("x", {})"#).eval().unwrap();
        assert!(nil.is_none());
    }

    #[test]
    fn closest_limit_zero_returns_empty_table() {
        let lua = engine();
        let len: i64 = lua
            .load(
                r#"
                local r = text.closest("x", {"a","b"}, { limit = 0 })
                return #r
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(len, 0);
    }

    #[test]
    fn slugify_diacritics() {
        let lua = engine();
        let s: String = lua
            .load(r#"return text.slugify("Café résumé")"#)
            .eval()
            .unwrap();
        assert_eq!(s, "cafe-resume");
    }
}

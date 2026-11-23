//! Text differencing — `lib:diff`.
//!
//! Backed by the `similar` crate for the patience / Myers algorithms.
//! Format / parse / apply for the unified diff string is hand-rolled so we
//! own the round-trip invariants:
//!
//!   diff.apply(old, diff.unified(old, new):format()) == new
//!   diff.parse_unified(result:format()) ≡ result   (structurally)
//!
//! "No newline at end of file" handling follows GNU diff conventions: when
//! a value lacks a trailing `\n`, the formatter appends a synthetic newline
//! plus the `\ No newline at end of file` marker; the parser strips both
//! back out on the way in.

use mlua::{Lua, Table, Value};
use similar::{Algorithm, ChangeTag, TextDiff};
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{err, ok};

const DEFAULT_CONTEXT: usize = 3;
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file";

/// Short-circuit on `Err(String)` from an option-parser by returning the
/// `(nil, err)` tuple from the enclosing Lua function. Folds the 5x
/// repeated `match { Ok => v, Err(msg) => return err(lua, msg) }` block.
macro_rules! short_circuit {
    ($lua:expr, $expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(msg) => return err($lua, msg),
        }
    };
}

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `lib:diff` surface through the facade.
///
/// Every function is set verbatim with [`crate::lua::facade::Module::set`]:
/// each carries its own `#[instrument]` span, and the fallible ones build their
/// `(value, error)` returns directly through `err` / `ok` with a baked
/// `"diff.<fn>: "` prefix (the option parsers funnel through the module-local
/// `short_circuit!` macro), so they do not fit the facade's `func` / `try_func`
/// model. Their signatures are recorded via
/// [`crate::lua::facade::Module::record_fn`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "lib:diff")?;

    m.declared(lines::lua(), lua.create_function(lines)?)?;

    m.declared(words::lua(), lua.create_function(words)?)?;

    m.declared(chars::lua(), lua.create_function(chars)?)?;

    m.declared(unified::lua(), lua.create_function(unified)?)?;

    m.declared(parse_unified::lua(), lua.create_function(parse_unified)?)?;

    m.declared(apply::lua(), lua.create_function(apply)?)?;

    m.declared(apply_changes::lua(), lua.create_function(apply_changes)?)?;

    m.declared(similarity::lua(), lua.create_function(similarity)?)?;

    Ok(m)
}

// ---------------------------------------------------------------------------
// Mode + options parsing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Mode {
    Lines,
    Words,
    Chars,
}

fn parse_algorithm(opts: Option<&Table>, prefix: &str) -> Result<Algorithm, String> {
    let Some(t) = opts else {
        return Ok(Algorithm::Patience);
    };
    let v: Value = t.get("algorithm").map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(Algorithm::Patience),
        Value::String(s) => match s.to_str().map_err(|e| format!("{prefix}: {e}"))?.as_ref() {
            "patience" => Ok(Algorithm::Patience),
            "myers" => Ok(Algorithm::Myers),
            other => Err(format!(
                "{prefix}: unknown algorithm {other:?} (expected \"patience\" or \"myers\")"
            )),
        },
        _ => Err(format!("{prefix}: algorithm must be a string")),
    }
}

/// The most context lines a hunk can use. Larger values mean "the whole file" for any input a script can
/// build, and `similar` computes `radius * 2`, which overflows (a panic) for the huge ones a Lua number can
/// hold (`1e300`).
const MAX_CONTEXT: u64 = 1 << 31;

fn clamp_context(n: u64) -> usize {
    n.min(MAX_CONTEXT) as usize
}

fn parse_context(opts: Option<&Table>, prefix: &str) -> Result<usize, String> {
    let Some(t) = opts else {
        return Ok(DEFAULT_CONTEXT);
    };
    let v: Value = t.get("context").map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(DEFAULT_CONTEXT),
        Value::Integer(i) if i >= 0 => Ok(clamp_context(i as u64)),
        Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => Ok(clamp_context(n as u64)),
        _ => Err(format!("{prefix}: context must be a non-negative integer")),
    }
}

fn parse_label(
    opts: Option<&Table>,
    key: &str,
    default: &str,
    prefix: &str,
) -> Result<String, String> {
    let Some(t) = opts else {
        return Ok(default.into());
    };
    let v: Value = t.get(key).map_err(|e| format!("{prefix}: {e}"))?;
    match v {
        Value::Nil => Ok(default.into()),
        Value::String(s) => Ok(s.to_str().map_err(|e| format!("{prefix}: {e}"))?.to_owned()),
        _ => Err(format!("{prefix}: {key} must be a string")),
    }
}

// ---------------------------------------------------------------------------
// Change-list functions
// ---------------------------------------------------------------------------

/// Line-level change list between `old` and `new`. Default algorithm is patience.
#[neoc_macros::lua_fn(
    returns = "Result<LuaValue, string>",
    takes("old: string", "new: string", "opts: LuaValue?")
)]
#[instrument(name = "diff.lines", target = "lua::lib::diff", skip_all)]
fn lines(
    lua: &Lua,
    args: (mlua::LuaString, mlua::LuaString, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    change_list(lua, Mode::Lines, args, "diff.lines")
}

/// Word-level change list, tokenising on whitespace boundaries.
#[neoc_macros::lua_fn(
    returns = "Result<LuaValue, string>",
    takes("old: string", "new: string", "opts: LuaValue?")
)]
#[instrument(name = "diff.words", target = "lua::lib::diff", skip_all)]
fn words(
    lua: &Lua,
    args: (mlua::LuaString, mlua::LuaString, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    change_list(lua, Mode::Words, args, "diff.words")
}

/// Character-level change list, operating on Unicode codepoints.
#[neoc_macros::lua_fn(
    returns = "Result<LuaValue, string>",
    takes("old: string", "new: string", "opts: LuaValue?")
)]
#[instrument(name = "diff.chars", target = "lua::lib::diff", skip_all)]
fn chars(
    lua: &Lua,
    args: (mlua::LuaString, mlua::LuaString, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    change_list(lua, Mode::Chars, args, "diff.chars")
}

fn change_list(
    lua: &Lua,
    mode: Mode,
    (old, new, opts): (mlua::LuaString, mlua::LuaString, Option<Table>),
    prefix: &str,
) -> mlua::Result<(Value, Value)> {
    let old_s = old.to_str()?.to_owned();
    let new_s = new.to_str()?.to_owned();
    let alg = short_circuit!(lua, parse_algorithm(opts.as_ref(), prefix));
    let changes = compute_changes(mode, alg, &old_s, &new_s);
    let table = changes_to_table(lua, &changes)?;
    ok(lua, table)
}

#[derive(Clone)]
struct Change {
    op: ChangeOp,
    value: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChangeOp {
    Equal,
    Delete,
    Insert,
}

impl ChangeOp {
    fn as_str(self) -> &'static str {
        match self {
            ChangeOp::Equal => "equal",
            ChangeOp::Delete => "delete",
            ChangeOp::Insert => "insert",
        }
    }
}

fn compute_changes(mode: Mode, alg: Algorithm, old: &str, new: &str) -> Vec<Change> {
    let diff = match mode {
        Mode::Lines => TextDiff::configure().algorithm(alg).diff_lines(old, new),
        Mode::Words => TextDiff::configure().algorithm(alg).diff_words(old, new),
        Mode::Chars => TextDiff::configure().algorithm(alg).diff_chars(old, new),
    };
    diff.iter_all_changes()
        .map(|c| Change {
            op: match c.tag() {
                ChangeTag::Equal => ChangeOp::Equal,
                ChangeTag::Delete => ChangeOp::Delete,
                ChangeTag::Insert => ChangeOp::Insert,
            },
            value: c.value().to_string(),
        })
        .collect()
}

fn changes_to_table(lua: &Lua, changes: &[Change]) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(changes.len(), 0)?;
    for (i, c) in changes.iter().enumerate() {
        let row = lua.create_table_with_capacity(0, 2)?;
        row.set("op", c.op.as_str())?;
        row.set("value", c.value.as_str())?;
        t.set(i + 1, row)?;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Unified diff — hunk model + format / parse / apply
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct HunkLine {
    op: ChangeOp,
    value: String,
    /// True iff `value` does NOT end with `\n` (sourced from input that
    /// lacks a trailing newline). Drives the `\ No newline at end of file`
    /// marker on format / strip on parse.
    no_newline: bool,
}

#[derive(Clone, Debug)]
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<HunkLine>,
}

#[derive(Clone, Debug)]
struct UnifiedResult {
    hunks: Vec<Hunk>,
    old_label: String,
    new_label: String,
}

/// Compute a unified diff. The result carries `.hunks` and a `:format()` method producing the unified diff string.
#[neoc_macros::lua_fn(returns = "Result<LuaValue, string>")]
#[instrument(name = "diff.unified", target = "lua::lib::diff", skip_all)]
fn unified(
    lua: &Lua,
    (old, new, opts): (mlua::LuaString, mlua::LuaString, Option<Table>),
) -> mlua::Result<(Value, Value)> {
    let old_s = old.to_str()?.to_owned();
    let new_s = new.to_str()?.to_owned();
    let prefix = "diff.unified";
    let alg = short_circuit!(lua, parse_algorithm(opts.as_ref(), prefix));
    let ctx = short_circuit!(lua, parse_context(opts.as_ref(), prefix));
    let old_label = short_circuit!(lua, parse_label(opts.as_ref(), "old_label", "a", prefix));
    let new_label = short_circuit!(lua, parse_label(opts.as_ref(), "new_label", "b", prefix));

    let hunks = build_hunks(alg, &old_s, &new_s, ctx);
    let result = UnifiedResult {
        hunks,
        old_label,
        new_label,
    };
    let table = unified_to_table(lua, result)?;
    ok(lua, table)
}

fn build_hunks(alg: Algorithm, old: &str, new: &str, context_radius: usize) -> Vec<Hunk> {
    let diff = TextDiff::configure().algorithm(alg).diff_lines(old, new);
    let mut hunks = Vec::new();
    for group in diff.grouped_ops(context_radius) {
        if group.is_empty() {
            continue;
        }
        let first = &group[0];
        let last = group.last().unwrap();
        let old_range_start = first.old_range().start;
        let new_range_start = first.new_range().start;
        let old_range_end = last.old_range().end;
        let new_range_end = last.new_range().end;

        let mut lines = Vec::new();
        for op in &group {
            for change in diff.iter_changes(op) {
                let raw = change.value();
                let (value, no_newline) = strip_trailing_newline(raw);
                let op = match change.tag() {
                    ChangeTag::Equal => ChangeOp::Equal,
                    ChangeTag::Delete => ChangeOp::Delete,
                    ChangeTag::Insert => ChangeOp::Insert,
                };
                lines.push(HunkLine {
                    op,
                    value,
                    no_newline,
                });
            }
        }
        let old_count = old_range_end - old_range_start;
        let new_count = new_range_end - new_range_start;
        let old_start = encode_anchor(old_range_start, old_count);
        let new_start = encode_anchor(new_range_start, new_count);
        hunks.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        });
    }
    hunks
}

/// GNU unified-diff convention: when `count == 0` the `start` line names
/// the line BEFORE the insertion gap (with `0` meaning before-first-line).
/// When `count > 0`, `start` is the 1-indexed first line of the range.
/// `encode_anchor` and `decode_anchor` are the symmetric encode/decode
/// pair — keep them next to each other so format and apply stay aligned.
fn encode_anchor(start_zero_indexed: usize, count: usize) -> usize {
    if count == 0 {
        start_zero_indexed
    } else {
        start_zero_indexed + 1
    }
}

fn decode_anchor(start: usize, count: usize) -> usize {
    if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    }
}

/// Split `\n` off the tail of a line so format/parse can re-attach it
/// consistently. Returns `(value_without_trailing_newline, had_no_newline)`.
fn strip_trailing_newline(raw: &str) -> (String, bool) {
    if let Some(stripped) = raw.strip_suffix('\n') {
        (stripped.to_string(), false)
    } else {
        (raw.to_string(), true)
    }
}

fn unified_to_table(lua: &Lua, result: UnifiedResult) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    let hunks_t = lua.create_table_with_capacity(result.hunks.len(), 0)?;
    for (i, h) in result.hunks.iter().enumerate() {
        hunks_t.set(i + 1, hunk_to_table(lua, h)?)?;
    }
    t.set("hunks", hunks_t)?;
    t.set("old_label", result.old_label.as_str())?;
    t.set("new_label", result.new_label.as_str())?;

    // Pre-format the diff string. `:format()` returns a stored copy so the
    // table is self-contained — no recompute on each call.
    let formatted = format_unified(&result);
    t.set(
        "format",
        lua.create_function(move |_, _: Value| Ok(formatted.clone()))?,
    )?;
    Ok(t)
}

fn hunk_to_table(lua: &Lua, h: &Hunk) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, 5)?;
    t.set("old_start", h.old_start)?;
    t.set("old_count", h.old_count)?;
    t.set("new_start", h.new_start)?;
    t.set("new_count", h.new_count)?;
    let lines_t = lua.create_table_with_capacity(h.lines.len(), 0)?;
    for (i, ln) in h.lines.iter().enumerate() {
        let row = lua.create_table_with_capacity(0, 2)?;
        row.set("op", ln.op.as_str())?;
        // Surface the value WITH its trailing newline if present. That's
        // what callers want — round-trips via apply_changes(old, hunks) need
        // newlines preserved.
        let v = if ln.no_newline {
            ln.value.clone()
        } else {
            format!("{}\n", ln.value)
        };
        row.set("value", v.as_str())?;
        lines_t.set(i + 1, row)?;
    }
    t.set("lines", lines_t)?;
    Ok(t)
}

fn format_unified(r: &UnifiedResult) -> String {
    let mut out = String::new();
    out.push_str("--- ");
    out.push_str(&r.old_label);
    out.push('\n');
    out.push_str("+++ ");
    out.push_str(&r.new_label);
    out.push('\n');
    for h in &r.hunks {
        format_hunk_header(&mut out, h);
        for ln in &h.lines {
            let prefix = match ln.op {
                ChangeOp::Equal => ' ',
                ChangeOp::Delete => '-',
                ChangeOp::Insert => '+',
            };
            out.push(prefix);
            out.push_str(&ln.value);
            out.push('\n');
            if ln.no_newline {
                out.push_str(NO_NEWLINE_MARKER);
                out.push('\n');
            }
        }
    }
    out
}

fn format_hunk_header(out: &mut String, h: &Hunk) {
    out.push_str("@@ -");
    out.push_str(&fmt_range(h.old_start, h.old_count));
    out.push_str(" +");
    out.push_str(&fmt_range(h.new_start, h.new_count));
    out.push_str(" @@\n");
}

fn fmt_range(start: usize, count: usize) -> String {
    // Always emit `start,count` so the parser stays trivial. `start` already
    // encodes the GNU "line BEFORE the gap" convention for count==0 (set in
    // build_hunks); we just print what we have.
    format!("{},{}", start, count)
}

// ---------------------------------------------------------------------------
// Parse unified
// ---------------------------------------------------------------------------

/// Parse a unified diff string back into the same structured result shape as `unified`.
#[neoc_macros::lua_fn(returns = "Result<LuaValue, string>")]
#[instrument(name = "diff.parse_unified", target = "lua::lib::diff", skip_all)]
fn parse_unified(lua: &Lua, patch: mlua::LuaString) -> mlua::Result<(Value, Value)> {
    let s = patch.to_str()?.to_owned();
    match parse_unified_inner(&s) {
        Ok(r) => {
            let table = unified_to_table(lua, r)?;
            ok(lua, table)
        }
        Err(msg) => err(lua, msg),
    }
}

fn parse_unified_inner(patch: &str) -> Result<UnifiedResult, String> {
    let prefix = "diff.parse_unified";
    if patch.is_empty() {
        return Err(format!("{prefix}: not a unified diff"));
    }

    // Tokenize into (line_no, content) pairs. Patch input is 1-indexed
    // for human-facing errors.
    let mut iter = patch.split('\n').enumerate().peekable();

    let HeaderScan {
        old_label,
        new_label,
        saw_minus,
        saw_plus,
        found_hunk,
    } = skip_file_headers(&mut iter);
    if !found_hunk {
        return Err(if saw_minus || saw_plus {
            format!("{prefix}: truncated patch: no hunks after headers")
        } else {
            format!("{prefix}: not a unified diff")
        });
    }

    let hunks = parse_hunks(&mut iter, prefix)?;
    if hunks.is_empty() {
        return Err(format!("{prefix}: truncated patch: no hunks parsed"));
    }

    Ok(UnifiedResult {
        hunks,
        old_label,
        new_label,
    })
}

struct HeaderScan {
    old_label: String,
    new_label: String,
    saw_minus: bool,
    saw_plus: bool,
    found_hunk: bool,
}

/// Consume optional `--- a/...` / `+++ b/...` file-header lines from the
/// front of the patch and stop at the first `@@` hunk header. Empty lines
/// before any content are tolerated; non-header non-hunk content with
/// nothing parsed yet is a strong "not a unified diff" signal — surfaced
/// by the caller, not here.
fn skip_file_headers<'a, I>(iter: &mut std::iter::Peekable<I>) -> HeaderScan
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut scan = HeaderScan {
        old_label: "a".to_string(),
        new_label: "b".to_string(),
        saw_minus: false,
        saw_plus: false,
        found_hunk: false,
    };
    while let Some(&(_, line)) = iter.peek() {
        if let Some(rest) = line.strip_prefix("--- ") {
            scan.old_label = rest.to_string();
            scan.saw_minus = true;
            iter.next();
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            scan.new_label = rest.to_string();
            scan.saw_plus = true;
            iter.next();
        } else if line.starts_with("@@") {
            scan.found_hunk = true;
            break;
        } else if line.is_empty() && iter.peek().is_some() {
            // Trailing newline at end of patch splits into a final empty
            // chunk — skip silently.
            iter.next();
        } else {
            break;
        }
    }
    scan
}

fn parse_hunks<'a, I>(iter: &mut std::iter::Peekable<I>, prefix: &str) -> Result<Vec<Hunk>, String>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut hunks = Vec::new();
    while let Some(&(line_no, line)) = iter.peek() {
        if line.starts_with("@@") {
            let header_line_no = line_no + 1; // 1-indexed for humans
            iter.next();
            let header = parse_hunk_header(line, header_line_no, prefix)?;
            let body = parse_hunk_body(iter, prefix)?;
            check_hunk_count(
                "old_count",
                header.old_count,
                body.counted_old,
                header_line_no,
                prefix,
            )?;
            check_hunk_count(
                "new_count",
                header.new_count,
                body.counted_new,
                header_line_no,
                prefix,
            )?;
            hunks.push(Hunk {
                old_start: header.old_start,
                old_count: header.old_count,
                new_start: header.new_start,
                new_count: header.new_count,
                lines: body.lines,
            });
        } else if line.is_empty() {
            iter.next();
        } else {
            return Err(format!(
                "{prefix}: stray content between hunks at line {}: {:?}",
                line_no + 1,
                line
            ));
        }
    }
    Ok(hunks)
}

struct HunkBody {
    lines: Vec<HunkLine>,
    counted_old: usize,
    counted_new: usize,
}

fn parse_hunk_body<'a, I>(
    iter: &mut std::iter::Peekable<I>,
    prefix: &str,
) -> Result<HunkBody, String>
where
    I: Iterator<Item = (usize, &'a str)>,
{
    let mut lines = Vec::new();
    let mut counted_old = 0usize;
    let mut counted_new = 0usize;
    while let Some(&(body_line_no, body_line)) = iter.peek() {
        if body_line.starts_with("@@") {
            break;
        }
        // End of patch (trailing newline → terminal empty chunk).
        if body_line.is_empty() {
            iter.next();
            if iter.peek().is_some() {
                // Empty line inside a hunk body — treat as " " (empty equal line).
                lines.push(HunkLine {
                    op: ChangeOp::Equal,
                    value: String::new(),
                    no_newline: false,
                });
                counted_old += 1;
                counted_new += 1;
                continue;
            }
            break;
        }

        match body_line.as_bytes()[0] {
            b' ' => {
                lines.push(HunkLine {
                    op: ChangeOp::Equal,
                    value: body_line[1..].to_string(),
                    no_newline: false,
                });
                counted_old += 1;
                counted_new += 1;
                iter.next();
            }
            b'-' => {
                lines.push(HunkLine {
                    op: ChangeOp::Delete,
                    value: body_line[1..].to_string(),
                    no_newline: false,
                });
                counted_old += 1;
                iter.next();
            }
            b'+' => {
                lines.push(HunkLine {
                    op: ChangeOp::Insert,
                    value: body_line[1..].to_string(),
                    no_newline: false,
                });
                counted_new += 1;
                iter.next();
            }
            b'\\' => {
                // `\ No newline at end of file` — strip newline from the
                // previous line, whatever its op.
                let Some(last) = lines.last_mut() else {
                    return Err(format!(
                        "{prefix}: invalid marker at line {}: \\ marker with no preceding line",
                        body_line_no + 1
                    ));
                };
                last.no_newline = true;
                iter.next();
            }
            _ => {
                return Err(format!(
                    "{prefix}: invalid line {} in hunk: {:?}",
                    body_line_no + 1,
                    body_line
                ));
            }
        }
    }
    Ok(HunkBody {
        lines,
        counted_old,
        counted_new,
    })
}

/// Sanity-check declared header count vs observed body count. Both
/// `old_count` and `new_count` mismatches surface through this single
/// helper so the error vocabulary stays identical.
fn check_hunk_count(
    name: &str,
    declared: usize,
    observed: usize,
    line_no: usize,
    prefix: &str,
) -> Result<(), String> {
    if declared != observed {
        return Err(format!(
            "{prefix}: hunk header at line {line_no} declares {name}={declared} but body has {observed}"
        ));
    }
    Ok(())
}

struct ParsedHeader {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
}

fn parse_hunk_header(line: &str, line_no: usize, prefix: &str) -> Result<ParsedHeader, String> {
    // Format: `@@ -A,B +C,D @@` (B or D may be omitted ⇒ 1).
    let bad = || format!("{prefix}: invalid hunk header at line {line_no}: {line:?}");
    let body = line
        .strip_prefix("@@")
        .and_then(|s| s.trim_start().strip_prefix('-'))
        .ok_or_else(bad)?;
    let (old_part, rest) = body.split_once(" +").ok_or_else(bad)?;
    let new_part = rest
        .split_once("@@")
        .map(|(a, _)| a.trim())
        .ok_or_else(bad)?;

    let (old_start, old_count) = parse_range(old_part.trim(), line_no, prefix, line)?;
    let (new_start, new_count) = parse_range(new_part.trim(), line_no, prefix, line)?;
    Ok(ParsedHeader {
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

fn parse_range(
    part: &str,
    line_no: usize,
    prefix: &str,
    raw: &str,
) -> Result<(usize, usize), String> {
    let bad = || format!("{prefix}: invalid hunk header at line {line_no}: {raw:?}");
    let (start_str, count_str) = match part.split_once(',') {
        Some((a, b)) => (a, b),
        None => (part, "1"),
    };
    let start: usize = start_str.parse().map_err(|_| bad())?;
    let count: usize = count_str.parse().map_err(|_| bad())?;
    Ok((start, count))
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

/// Apply a unified patch string to `original`. Requires exact context match.
#[neoc_macros::lua_fn(returns = "Result<string, string>", names(orig = "original"))]
#[instrument(name = "diff.apply", target = "lua::lib::diff", skip_all)]
fn apply(
    lua: &Lua,
    (orig, patch): (mlua::LuaString, mlua::LuaString),
) -> mlua::Result<(Value, Value)> {
    let orig_s = orig.to_str()?.to_owned();
    let patch_s = patch.to_str()?.to_owned();
    match apply_inner(&orig_s, &patch_s) {
        Ok(s) => ok(lua, s),
        Err(msg) => err(lua, msg),
    }
}

fn apply_inner(orig: &str, patch: &str) -> Result<String, String> {
    let parsed = parse_unified_inner(patch).map_err(|e| {
        // Re-prefix parse errors to point at apply's contract (still
        // includes the original parse line number).
        e.replace("diff.parse_unified:", "diff.apply: malformed patch:")
    })?;

    // Split original on newlines, keeping the `no trailing newline` quirk:
    // a tail without `\n` becomes its own final entry; an empty tail
    // (`"a\n"`) does not.
    let orig_lines = split_keep_newline_flag(orig);
    apply_hunks(&orig_lines, &parsed.hunks)
}

/// Returns `(value_without_trailing_newline, no_newline_flag)` per line.
fn split_keep_newline_flag(s: &str) -> Vec<(String, bool)> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(idx) = rest.find('\n') {
        out.push((rest[..idx].to_string(), false));
        rest = &rest[idx + 1..];
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), true));
    }
    out
}

/// Verify that the line at `orig[cursor]` matches `ln` (value + newline
/// flag). `verb` selects the error vocabulary ("context" vs "delete") so
/// callers see which kind of hunk line failed. Equal and Delete both run
/// this check before consuming an original line — the only difference is
/// whether the line is also pushed to the output.
fn verify_orig_line(
    orig: &[(String, bool)],
    cursor: usize,
    ln: &HunkLine,
    hunk_no: usize,
    verb: &str,
) -> Result<(), String> {
    if cursor >= orig.len() {
        return Err(format!(
            "diff.apply: hunk {hunk_no} {verb} overruns end of original at line {}",
            cursor + 1
        ));
    }
    let (got, got_no_nl) = &orig[cursor];
    if got != &ln.value || *got_no_nl != ln.no_newline {
        return Err(format!(
            "diff.apply: hunk {hunk_no} {verb} mismatch at line {}: expected {:?}, found {:?}",
            cursor + 1,
            ln.value,
            got
        ));
    }
    Ok(())
}

fn apply_hunks(orig: &[(String, bool)], hunks: &[Hunk]) -> Result<String, String> {
    let mut out_lines: Vec<(String, bool)> = Vec::new();
    let mut cursor: usize = 0; // 0-indexed position in `orig`

    for (idx, hunk) in hunks.iter().enumerate() {
        let hunk_no = idx + 1;
        let anchor = decode_anchor(hunk.old_start, hunk.old_count);

        if anchor > orig.len() {
            return Err(format!(
                "diff.apply: hunk {hunk_no} starts past end of original (line {} of {} total)",
                hunk.old_start,
                orig.len()
            ));
        }
        if anchor < cursor {
            return Err(format!(
                "diff.apply: hunk {hunk_no} starts at line {} but cursor is already at line {}",
                hunk.old_start,
                cursor + 1
            ));
        }
        out_lines.extend(orig[cursor..anchor].iter().cloned());
        cursor = anchor;

        for ln in &hunk.lines {
            match ln.op {
                ChangeOp::Equal => {
                    verify_orig_line(orig, cursor, ln, hunk_no, "context")?;
                    out_lines.push((ln.value.clone(), ln.no_newline));
                    cursor += 1;
                }
                ChangeOp::Delete => {
                    verify_orig_line(orig, cursor, ln, hunk_no, "delete")?;
                    cursor += 1;
                }
                ChangeOp::Insert => {
                    out_lines.push((ln.value.clone(), ln.no_newline));
                }
            }
        }
    }

    // Tail of original after last hunk.
    out_lines.extend(orig[cursor..].iter().cloned());

    Ok(join_apply_lines(&out_lines))
}

/// Re-join `apply_hunks` output back into a single string, preserving the
/// "no trailing newline" quirk: a final no-newline line is emitted without
/// the synthetic `\n`. All other lines get their `\n` separator.
fn join_apply_lines(lines: &[(String, bool)]) -> String {
    let mut out = String::new();
    for (i, (val, no_nl)) in lines.iter().enumerate() {
        out.push_str(val);
        let is_last = i + 1 == lines.len();
        if !is_last || !*no_nl {
            out.push('\n');
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Apply structured change list
// ---------------------------------------------------------------------------

/// Apply a structured change list directly, validating that the equal + delete lines reconstruct `original` first.
#[neoc_macros::lua_fn(returns = "Result<string, string>", names(orig = "original"))]
#[instrument(name = "diff.apply_changes", target = "lua::lib::diff", skip_all)]
fn apply_changes(
    lua: &Lua,
    (orig, changes): (mlua::LuaString, Table),
) -> mlua::Result<(Value, Value)> {
    let prefix = "diff.apply_changes";
    let orig_s = orig.to_str()?.to_owned();
    let changes = short_circuit!(lua, read_changes(&changes, prefix));

    if let Err(msg) = validate_change_list_reconstructs(&changes, &orig_s, prefix) {
        return err(lua, msg);
    }

    ok(lua, emit_change_list(&changes))
}

/// Catches misaligned change lists before they silently corrupt output:
/// the `equal + delete` half of any valid change list must equal the
/// original input.
fn validate_change_list_reconstructs(
    changes: &[Change],
    orig: &str,
    prefix: &str,
) -> Result<(), String> {
    let mut reconstructed = String::new();
    for c in changes {
        if c.op == ChangeOp::Equal || c.op == ChangeOp::Delete {
            reconstructed.push_str(&c.value);
        }
    }
    if reconstructed != orig {
        return Err(format!(
            "{prefix}: change list does not reconstruct original (got {} bytes, expected {})",
            reconstructed.len(),
            orig.len()
        ));
    }
    Ok(())
}

/// Emit the `equal + insert` half of a change list — the new text built
/// from a list whose `equal + delete` half already matched the original.
fn emit_change_list(changes: &[Change]) -> String {
    let mut out = String::new();
    for c in changes {
        if c.op == ChangeOp::Equal || c.op == ChangeOp::Insert {
            out.push_str(&c.value);
        }
    }
    out
}

fn read_changes(t: &Table, prefix: &str) -> Result<Vec<Change>, String> {
    let len = t.raw_len();
    // not presized: `#t` is whatever border Lua finds, so a forged table can report 2^30
    let mut out = Vec::new();
    for i in 1..=len {
        let row: Table = t
            .get(i)
            .map_err(|e| format!("{prefix}: change #{i}: {e}"))?;
        let op_s: String = row
            .get("op")
            .map_err(|e| format!("{prefix}: change #{i}: missing op: {e}"))?;
        let value: String = row
            .get("value")
            .map_err(|e| format!("{prefix}: change #{i}: missing value: {e}"))?;
        let op = match op_s.as_str() {
            "equal" => ChangeOp::Equal,
            "delete" => ChangeOp::Delete,
            "insert" => ChangeOp::Insert,
            other => {
                return Err(format!(
                    "{prefix}: change #{i}: invalid op {other:?} (expected equal/delete/insert)"
                ));
            }
        };
        out.push(Change { op, value });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Similarity
// ---------------------------------------------------------------------------

/// Edit-ratio similarity score in `[0.0, 1.0]`, computed from a character-level patience diff.
#[neoc_macros::lua_fn]
#[instrument(name = "diff.similarity", target = "lua::lib::diff", skip_all)]
fn similarity(_: &Lua, (a, b): (mlua::LuaString, mlua::LuaString)) -> mlua::Result<f64> {
    let a_s = a.to_str()?;
    let b_s = b.to_str()?;
    // Fast paths: identical (incl. both empty) → 1.0; one empty, other not → 0.0.
    if a_s == b_s {
        return Ok(1.0);
    }
    if a_s.is_empty() || b_s.is_empty() {
        return Ok(0.0);
    }
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_chars(a_s.as_ref(), b_s.as_ref());
    Ok(diff.ratio() as f64)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn line_changes(old: &str, new: &str) -> Vec<Change> {
        compute_changes(Mode::Lines, Algorithm::Patience, old, new)
    }

    #[test]
    fn lines_emits_equal_delete_insert_in_order() {
        let cs = line_changes("a\nb\nc\n", "a\nB\nc\n");
        let tags: Vec<&str> = cs.iter().map(|c| c.op.as_str()).collect();
        assert_eq!(tags, vec!["equal", "delete", "insert", "equal"]);
    }

    #[test]
    fn lines_identical_returns_only_equal() {
        let cs = line_changes("a\nb\n", "a\nb\n");
        for c in &cs {
            assert_eq!(c.op, ChangeOp::Equal);
        }
    }

    #[test]
    fn lines_empty_old_returns_only_inserts() {
        let cs = line_changes("", "a\nb\n");
        for c in &cs {
            assert!(matches!(c.op, ChangeOp::Insert | ChangeOp::Equal));
        }
        assert!(cs.iter().any(|c| c.op == ChangeOp::Insert));
    }

    #[test]
    fn lines_empty_new_returns_only_deletes() {
        let cs = line_changes("a\nb\n", "");
        assert!(cs.iter().any(|c| c.op == ChangeOp::Delete));
    }

    #[test]
    fn unified_round_trip_simple() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 3);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let parsed = parse_unified_inner(&formatted).expect("parse");
        let reformatted = format_unified(&parsed);
        assert_eq!(formatted, reformatted, "round-trip format identical");
    }

    #[test]
    fn apply_round_trip_simple() {
        let old = "alpha\nbravo\ncharlie\n";
        let new = "alpha\nBRAVO\ncharlie\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 3);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let applied = apply_inner(old, &formatted).expect("apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn apply_round_trip_no_trailing_newline() {
        let old = "alpha\nbravo";
        let new = "alpha\nBRAVO";
        let hunks = build_hunks(Algorithm::Patience, old, new, 3);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        assert!(formatted.contains(NO_NEWLINE_MARKER));
        let applied = apply_inner(old, &formatted).expect("apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn apply_round_trip_myers_matches_patience() {
        let old = "1\n2\n3\n4\n5\n";
        let new = "1\n2\nX\n4\n5\n";
        let p = build_hunks(Algorithm::Patience, old, new, 3);
        let m = build_hunks(Algorithm::Myers, old, new, 3);
        let pf = format_unified(&UnifiedResult {
            hunks: p,
            old_label: "a".into(),
            new_label: "b".into(),
        });
        let mf = format_unified(&UnifiedResult {
            hunks: m,
            old_label: "a".into(),
            new_label: "b".into(),
        });
        assert_eq!(apply_inner(old, &pf).unwrap(), new);
        assert_eq!(apply_inner(old, &mf).unwrap(), new);
    }

    #[test]
    fn parse_rejects_non_unified_diff() {
        let r = parse_unified_inner("this is not a patch\nat all\n");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("not a unified diff"));
    }

    #[test]
    fn parse_truncated_patch_reports_line() {
        let r = parse_unified_inner("--- a\n+++ b\n");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("truncated"));
    }

    #[test]
    fn parse_malformed_header_reports_line() {
        let r = parse_unified_inner("--- a\n+++ b\n@@@ -1 +1 @@\n");
        assert!(r.is_err());
        // Either invalid hunk header or stray content — both are line-numbered.
        let msg = r.unwrap_err();
        assert!(msg.contains("line"), "got: {msg}");
    }

    #[test]
    fn apply_context_mismatch_errors_with_line() {
        let old = "alpha\nbravo\ncharlie\n";
        let new = "alpha\nBRAVO\ncharlie\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 3);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let drifted = "alpha\nBRAVO_NOT\ncharlie\n";
        let err = apply_inner(drifted, &formatted).expect_err("must mismatch");
        assert!(err.contains("hunk 1"));
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn similarity_identical_is_one() {
        let s = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_chars("abc", "abc")
            .ratio() as f64;
        // Empty case is handled in `similarity` itself; verify equal-input parity here.
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn similarity_symmetry() {
        let a = "the quick brown fox";
        let b = "the lazy red fox";
        let ab = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_chars(a, b)
            .ratio() as f64;
        let ba = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_chars(b, a)
            .ratio() as f64;
        assert!((ab - ba).abs() < 1e-9, "ab={ab} ba={ba}");
    }

    #[test]
    fn similarity_bounded() {
        let s = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_chars("hello", "world")
            .ratio() as f64;
        assert!((0.0..=1.0).contains(&s), "out of range: {s}");
    }

    #[test]
    fn strip_trailing_newline_works() {
        assert_eq!(strip_trailing_newline("foo\n"), ("foo".to_string(), false));
        assert_eq!(strip_trailing_newline("foo"), ("foo".to_string(), true));
        assert_eq!(strip_trailing_newline(""), ("".to_string(), true));
    }

    #[test]
    fn split_keep_newline_flag_works() {
        assert_eq!(
            split_keep_newline_flag("a\nb\n"),
            vec![("a".to_string(), false), ("b".to_string(), false)]
        );
        assert_eq!(
            split_keep_newline_flag("a\nb"),
            vec![("a".to_string(), false), ("b".to_string(), true)]
        );
        assert_eq!(split_keep_newline_flag(""), Vec::<(String, bool)>::new());
    }

    #[test]
    fn validate_change_list_reconstructs_rejects_misaligned() {
        let changes = vec![
            Change {
                op: ChangeOp::Equal,
                value: "a\n".to_string(),
            },
            Change {
                op: ChangeOp::Delete,
                value: "WRONG\n".to_string(),
            },
        ];
        // Original is "a\nb\n", but equal+delete would reconstruct "a\nWRONG\n".
        let err = validate_change_list_reconstructs(&changes, "a\nb\n", "diff.apply_changes")
            .expect_err("must reject");
        assert!(err.contains("reconstruct"));
    }

    #[test]
    fn parse_alg_default_is_patience() {
        assert!(matches!(
            parse_algorithm(None, "x"),
            Ok(Algorithm::Patience)
        ));
    }

    #[test]
    fn a_huge_context_is_clamped_not_an_overflow_panic() {
        for src in ["1e300", "2^63", "2^53", "2^53 + 2", "1.8e19"] {
            let lua = crate::lua::sandbox::make_engine().expect("engine");
            let (hunks, _): (mlua::Value, mlua::Value) = lua
                .load(format!(
                    r#"return require("lib:diff").unified("a\nb\nc\n", "a\nB\nc\n", {{ context = {src} }})"#
                ))
                .eval()
                .unwrap_or_else(|e| panic!("context = {src}: {e}"));
            assert!(
                matches!(hunks, mlua::Value::Table(_)),
                "context = {src}: {hunks:?}"
            );
        }
        assert_eq!(clamp_context(u64::MAX), MAX_CONTEXT as usize);
    }

    #[test]
    fn parse_context_default_is_three() {
        assert_eq!(parse_context(None, "x"), Ok(DEFAULT_CONTEXT));
    }

    #[test]
    fn parse_alg_rejects_unknown() {
        let lua = mlua::Lua::new();
        let t = lua.create_table().unwrap();
        t.set("algorithm", "wagner").unwrap();
        let r = parse_algorithm(Some(&t), "x");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("unknown algorithm"));
    }

    #[test]
    fn unified_zero_hunks_when_identical() {
        let hunks = build_hunks(Algorithm::Patience, "a\nb\n", "a\nb\n", 3);
        assert!(hunks.is_empty());
    }

    #[test]
    fn apply_empty_patch_round_trip() {
        // A patch with only headers + zero hunks is treated as truncated.
        // Confirmed elsewhere; here we verify the identity transform works
        // through unified() when there are no diffs.
        let old = "a\nb\n";
        let new = "a\nb\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 3);
        assert!(hunks.is_empty());
        // An empty patch (no hunks) is malformed for parse — confirmed in
        // `parse_truncated_patch_reports_line`.
    }

    #[test]
    fn apply_pure_insert_hunk_middle_with_zero_context() {
        // With context=0 the inserted line forms its own pure-insert hunk
        // (`@@ -2,0 +3,1 @@`). Regression for the GNU "line BEFORE the gap"
        // convention: anchor must be `old_start`, not `old_start - 1`.
        let old = "a\nb\n";
        let new = "a\nb\nc\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 0);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let applied = apply_inner(old, &formatted).expect("apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn apply_pure_insert_hunk_start_with_zero_context() {
        let old = "a\nb\n";
        let new = "X\na\nb\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 0);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let applied = apply_inner(old, &formatted).expect("apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn apply_pure_insert_hunk_end_with_zero_context() {
        let old = "a\nb\n";
        let new = "a\nb\nZ\n";
        let hunks = build_hunks(Algorithm::Patience, old, new, 0);
        let r = UnifiedResult {
            hunks,
            old_label: "a".into(),
            new_label: "b".into(),
        };
        let formatted = format_unified(&r);
        let applied = apply_inner(old, &formatted).expect("apply");
        assert_eq!(applied, new);
    }

    #[test]
    fn long_line_diff_does_not_panic() {
        let a: String = "x".repeat(20_000);
        let mut b = a.clone();
        b.push('y');
        let cs = compute_changes(Mode::Chars, Algorithm::Patience, &a, &b);
        assert!(!cs.is_empty());
        // Insert at end — last change is insert "y"
        let last = cs.last().unwrap();
        assert_eq!(last.op, ChangeOp::Insert);
        assert_eq!(last.value, "y");
    }
}

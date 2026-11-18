//! A budgeted, depth-limited implementation of Lua 5.1's pattern-matching string functions
//! (`string.find`, `match`, `gmatch`, `gsub`).
//!
//! # Why
//!
//! PUC Lua's matcher is C code that never polls the VM's count hook, so a pattern such as
//! `string.find(string.rep("a", 1e5), ".-.-.-.-b")` (cubic or worse in the subject length) runs for hours
//! inside one VM instruction: the CPU budget cannot interrupt it. Lua 5.1 also has no recursion limit in
//! `match`, so a pattern of a hundred thousand `a*` items overflows the C stack and takes the host down.
//!
//! This module is a line-for-line port of `lstrlib.c` (Lua 5.1.5) in safe Rust, with two additions:
//!
//! - every [`STEP_BLOCK`] matcher steps it calls the embedder's `charge` callback (one CPU tick), which
//!   returns an error once a budget is spent, so a catastrophic pattern ends like a runaway loop does;
//! - recursion is capped at [`MAX_DEPTH`] ("pattern too complex", as Lua 5.2+ does), and `gsub` output is
//!   capped at [`PatternLimits::max_output`], because a Rust buffer is outside the Lua heap's memory limit.
//!
//! Everything else follows the C source, quirks included (a NUL byte in a pattern ends the pattern; `^` is
//! literal in `gmatch`; `%` before a non-digit in a `gsub` replacement is that character). The tests compare
//! this implementation with the real C functions on thousands of generated cases.
//!
//! [`install`] replaces the four functions in the engine's `string` table, which is also where the string
//! metatable's `__index` points, so both `string.find(s, p)` and `s:find(p)` come here.

use std::{cell::Cell, sync::Arc};

use mlua::{Function, Lua, LuaString, MultiValue, Table, Value};

/// Matcher steps per CPU tick charged to the embedder's budget.
pub const STEP_BLOCK: u64 = 64;

/// Deepest recursion of the matcher (Lua 5.2's `MAXCCALLS`). Honest patterns nest a handful of items.
pub const MAX_DEPTH: usize = 200;

const MAX_CAPTURES: usize = 32;
const CAP_UNFINISHED: isize = -1;
const CAP_POSITION: isize = -2;
const L_ESC: u8 = b'%';
/// Characters that make a `find` pattern a real pattern rather than a plain substring.
const SPECIALS: &[u8] = b"^$*+?.([%-";

/// The embedder's side of the budget: how matcher work is paid for, and how much `gsub` may build.
#[derive(Clone)]
pub struct PatternLimits {
    /// Called with a number of CPU ticks; an error aborts the match (and, being the embedder's, may keep
    /// failing until the call is over).
    pub charge: Arc<dyn Fn(u64) -> mlua::Result<()> + Send + Sync>,
    /// Most bytes one `gsub` may produce.
    pub max_output: usize,
}

type Charge = dyn Fn(u64) -> mlua::Result<()> + Send + Sync;

fn error(message: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(message.into())
}

struct Matcher<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, isize); MAX_CAPTURES],
    depth: usize,
    steps: u64,
    charge: &'a Charge,
}

type Found = mlua::Result<Option<usize>>;

impl<'a> Matcher<'a> {
    fn new(src: &'a [u8], pat: &'a [u8], charge: &'a Charge) -> Self {
        Self { src, pat, level: 0, capture: [(0, 0); MAX_CAPTURES], depth: 0, steps: 0, charge }
    }

    /// `*s`: the subject byte, or the C string's NUL terminator past the end.
    fn s(&self, i: usize) -> u8 {
        self.src.get(i).copied().unwrap_or(0)
    }

    /// `*p`: the pattern byte, or NUL past the end (a NUL inside the pattern ends it too, as in C).
    fn p(&self, i: usize) -> u8 {
        self.pat.get(i).copied().unwrap_or(0)
    }

    fn step(&mut self) -> mlua::Result<()> {
        self.steps += 1;
        if self.steps.is_multiple_of(STEP_BLOCK) {
            (self.charge)(1)?;
        }
        Ok(())
    }

    fn check_capture(&self, l: u8) -> mlua::Result<usize> {
        let l = usize::from(l.wrapping_sub(b'1'));
        if l >= self.level || self.capture[l].1 == CAP_UNFINISHED {
            return Err(error("invalid capture index"));
        }
        Ok(l)
    }

    fn capture_to_close(&self) -> mlua::Result<usize> {
        (0..self.level)
            .rev()
            .find(|&level| self.capture[level].1 == CAP_UNFINISHED)
            .ok_or_else(|| error("invalid pattern capture"))
    }

    fn class_end(&self, mut p: usize) -> mlua::Result<usize> {
        let c = self.p(p);
        p += 1;
        match c {
            L_ESC => {
                if self.p(p) == 0 {
                    return Err(error("malformed pattern (ends with '%')"));
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.p(p) == b'^' {
                    p += 1;
                }
                loop {
                    // look for a `]`
                    if self.p(p) == 0 {
                        return Err(error("malformed pattern (missing ']')"));
                    }
                    let current = self.p(p);
                    p += 1;
                    if current == L_ESC && self.p(p) != 0 {
                        p += 1; // skip escapes (e.g. `%]`)
                    }
                    if self.p(p) == b']' {
                        return Ok(p + 1);
                    }
                }
            }
            _ => Ok(p),
        }
    }

    fn match_bracket_class(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.p(p + 1) == b'^' {
            sig = false;
            p += 1; // skip the `^`
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.p(p) == L_ESC {
                p += 1;
                if match_class(c, self.p(p)) {
                    return sig;
                }
            } else if self.p(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.p(p - 2) <= c && c <= self.p(p) {
                    return sig;
                }
            } else if self.p(p) == c {
                return sig;
            }
        }
        !sig
    }

    fn single_match(&self, c: u8, p: usize, ep: usize) -> bool {
        match self.p(p) {
            b'.' => true,
            L_ESC => match_class(c, self.p(p + 1)),
            b'[' => self.match_bracket_class(c, p, ep - 1),
            other => other == c,
        }
    }

    fn match_balance(&self, mut s: usize, p: usize) -> mlua::Result<Option<usize>> {
        if self.p(p) == 0 || self.p(p + 1) == 0 {
            return Err(error("unbalanced pattern"));
        }
        if self.s(s) != self.p(p) {
            return Ok(None);
        }
        let (b, e) = (self.p(p), self.p(p + 1));
        let mut cont = 1;
        loop {
            s += 1;
            if s >= self.src.len() {
                return Ok(None); // string ends out of balance
            }
            if self.s(s) == e {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if self.s(s) == b {
                cont += 1;
            }
        }
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> Found {
        let mut i = 0usize;
        while s + i < self.src.len() && self.single_match(self.s(s + i), p, ep) {
            self.step()?;
            i += 1;
        }
        // keeps trying to match with the maximum repetitions
        loop {
            self.step()?;
            if let Some(res) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(res));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1; // didn't match: reduce one repetition to try again
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> Found {
        loop {
            self.step()?;
            if let Some(res) = self.do_match(s, ep + 1)? {
                return Ok(Some(res));
            }
            if s < self.src.len() && self.single_match(self.s(s), p, ep) {
                s += 1; // try with one more repetition
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: isize) -> Found {
        let level = self.level;
        if level >= MAX_CAPTURES {
            return Err(error("too many captures"));
        }
        self.capture[level] = (s, what);
        self.level = level + 1;
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.level -= 1; // undo capture
        }
        Ok(res)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> Found {
        let l = self.capture_to_close()?;
        self.capture[l].1 = isize::try_from(s - self.capture[l].0).unwrap_or(isize::MAX); // close capture
        let res = self.do_match(s, p)?;
        if res.is_none() {
            self.capture[l].1 = CAP_UNFINISHED; // undo capture
        }
        Ok(res)
    }

    fn match_capture(&self, s: usize, l: u8) -> mlua::Result<Option<usize>> {
        let l = self.check_capture(l)?;
        let (init, len) = self.capture[l];
        // a position capture has no text to match (C reads its length as a huge unsigned number)
        let Ok(len) = usize::try_from(len) else { return Ok(None) };
        let wanted = self.src.get(init..init + len);
        let here = self.src.get(s..s + len);
        Ok(match (wanted, here) {
            (Some(a), Some(b)) if a == b => Some(s + len),
            _ => None,
        })
    }

    /// `match` in `lstrlib.c`: the end of the match of the pattern from `p` at subject offset `s`.
    fn do_match(&mut self, s: usize, p: usize) -> Found {
        if self.depth >= MAX_DEPTH {
            return Err(error("pattern too complex"));
        }
        self.depth += 1;
        let result = self.match_items(s, p);
        self.depth -= 1;
        result
    }

    fn match_items(&mut self, mut s: usize, mut p: usize) -> Found {
        loop {
            self.step()?;
            match self.p(p) {
                b'(' => {
                    return if self.p(p + 1) == b')' {
                        self.start_capture(s, p + 2, CAP_POSITION)
                    } else {
                        self.start_capture(s, p + 1, CAP_UNFINISHED)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                L_ESC if self.p(p + 1) == b'b' => {
                    match self.match_balance(s, p + 2)? {
                        Some(next) => s = next,
                        None => return Ok(None),
                    }
                    p += 4;
                    continue;
                }
                L_ESC if self.p(p + 1) == b'f' => {
                    p += 2;
                    if self.p(p) != b'[' {
                        return Err(error("missing '[' after '%f' in pattern"));
                    }
                    let ep = self.class_end(p)?;
                    let previous = if s == 0 { 0 } else { self.s(s - 1) };
                    if self.match_bracket_class(previous, p, ep - 1)
                        || !self.match_bracket_class(self.s(s), p, ep - 1)
                    {
                        return Ok(None);
                    }
                    p = ep;
                    continue;
                }
                L_ESC if self.p(p + 1).is_ascii_digit() => {
                    match self.match_capture(s, self.p(p + 1))? {
                        Some(next) => s = next,
                        None => return Ok(None),
                    }
                    p += 2;
                    continue;
                }
                0 => return Ok(Some(s)),
                b'$' if self.p(p + 1) == 0 => {
                    return Ok((s == self.src.len()).then_some(s));
                }
                _ => {}
            }
            // a pattern item
            let ep = self.class_end(p)?;
            let m = s < self.src.len() && self.single_match(self.s(s), p, ep);
            match self.p(ep) {
                b'?' => {
                    if m {
                        if let Some(res) = self.do_match(s + 1, ep + 1)? {
                            return Ok(Some(res));
                        }
                    }
                    p = ep + 1;
                }
                b'*' => return self.max_expand(s, p, ep),
                b'+' => return if m { self.max_expand(s + 1, p, ep) } else { Ok(None) },
                b'-' => return self.min_expand(s, p, ep),
                _ => {
                    if !m {
                        return Ok(None);
                    }
                    s += 1;
                    p = ep;
                }
            }
        }
    }

    /// One capture as a Lua value (`push_onecapture`).
    fn one_capture(&self, lua: &Lua, i: usize, s: Option<usize>, e: usize) -> mlua::Result<Value> {
        if i >= self.level {
            return match (i, s) {
                (0, Some(s)) => Ok(Value::String(lua.create_string(&self.src[s..e])?)),
                _ => Err(error("invalid capture index")),
            };
        }
        let (init, len) = self.capture[i];
        match len {
            CAP_UNFINISHED => Err(error("unfinished capture")),
            CAP_POSITION => Ok(Value::Integer(i64::try_from(init + 1).unwrap_or(i64::MAX))),
            len => {
                let len = usize::try_from(len).unwrap_or(0);
                Ok(Value::String(lua.create_string(&self.src[init..init + len])?))
            }
        }
    }

    /// All captures, or the whole match when the pattern has none and `s` is given (`push_captures`).
    fn captures(&self, lua: &Lua, s: Option<usize>, e: usize) -> mlua::Result<Vec<Value>> {
        let levels = if self.level == 0 && s.is_some() { 1 } else { self.level };
        (0..levels).map(|i| self.one_capture(lua, i, s, e)).collect()
    }
}

/// The C `ctype` classes for the "C" locale (ASCII).
fn match_class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        // C `isspace`: space, \t \n \v \f \r (Rust's `is_ascii_whitespace` leaves out \v)
        b's' => matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        b'z' => c == 0, // Lua 5.1's `%z`: the NUL byte
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() {
        !res
    } else {
        res
    }
}

// ---- argument handling (luaL_check*) --------------------------------------------------------------

fn bad_argument(position: usize, function: &str, expected: &str, got: Option<&Value>) -> mlua::Error {
    let got = got.map_or("no value", Value::type_name);
    error(format!("bad argument #{position} to '{function}' ({expected} expected, got {got})"))
}

fn arg_string(lua: &Lua, args: &MultiValue, index: usize, function: &str) -> mlua::Result<LuaString> {
    let value = args.get(index);
    match value {
        Some(v @ (Value::String(_) | Value::Number(_) | Value::Integer(_))) => {
            lua.coerce_string(v.clone())?.ok_or_else(|| bad_argument(index + 1, function, "string", value))
        }
        _ => Err(bad_argument(index + 1, function, "string", value)),
    }
}

/// `luaL_optinteger`: a number (or numeric string), truncated toward zero; `default` when absent or nil.
fn arg_integer(
    lua: &Lua,
    args: &MultiValue,
    index: usize,
    function: &str,
    default: Option<isize>,
) -> mlua::Result<isize> {
    match args.get(index) {
        None | Some(Value::Nil) if default.is_some() => Ok(default.unwrap_or_default()),
        Some(v @ (Value::Number(_) | Value::Integer(_) | Value::String(_))) => {
            match lua.coerce_number(v.clone())? {
                #[allow(clippy::cast_possible_truncation)]
                Some(n) => Ok(n as isize),
                None => Err(bad_argument(index + 1, function, "number", Some(v))),
            }
        }
        other => Err(bad_argument(index + 1, function, "number", other)),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Nil | Value::Boolean(false)))
}

/// `posrelat`: a negative position counts back from the end.
fn pos_relative(pos: isize, len: usize) -> isize {
    let len = isize::try_from(len).unwrap_or(isize::MAX);
    let pos = if pos < 0 { pos.saturating_add(len).saturating_add(1) } else { pos };
    pos.max(0)
}

fn position(n: usize) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

// ---- the four functions ---------------------------------------------------------------------------

/// `string.find` (`find == true`) and `string.match`.
fn find_aux(lua: &Lua, limits: &PatternLimits, args: &MultiValue, find: bool) -> mlua::Result<MultiValue> {
    let name = if find { "find" } else { "match" };
    let subject = arg_string(lua, args, 0, name)?;
    let pattern = arg_string(lua, args, 1, name)?;
    let (s, p) = (subject.as_bytes(), pattern.as_bytes());
    let init = pos_relative(arg_integer(lua, args, 2, name, Some(1))?, s.len()) - 1;
    let init = usize::try_from(init.max(0)).unwrap_or(0).min(s.len());
    // `strpbrk` stops at the first NUL of the C string
    let specials = p.iter().take_while(|&&b| b != 0).any(|b| SPECIALS.contains(b));
    if find && (truthy(args.get(3)) || !specials) {
        // a plain search
        (limits.charge)(1 + u64::try_from(s.len() / 4096).unwrap_or(u64::MAX))?;
        if let Some(at) = memchr::memmem::find(&s[init..], &p) {
            let start = init + at;
            return Ok(MultiValue::from_vec(vec![position(start + 1), position(start + p.len())]));
        }
        return Ok(MultiValue::from_vec(vec![Value::Nil]));
    }
    let anchor = p.first() == Some(&b'^');
    let pat = if anchor { &p[1..] } else { &p[..] };
    let mut m = Matcher::new(&s, pat, limits.charge.as_ref());
    let mut start = init;
    loop {
        m.level = 0;
        if let Some(end) = m.do_match(start, 0)? {
            if find {
                let mut out = vec![position(start + 1), position(end)];
                out.extend(m.captures(lua, None, end)?);
                return Ok(MultiValue::from_vec(out));
            }
            return Ok(MultiValue::from_vec(m.captures(lua, Some(start), end)?));
        }
        let more = start < s.len();
        start += 1;
        if !more || anchor {
            break;
        }
    }
    Ok(MultiValue::from_vec(vec![Value::Nil]))
}

fn gmatch(lua: &Lua, limits: &PatternLimits, args: &MultiValue) -> mlua::Result<Function> {
    let subject = arg_string(lua, args, 0, "gmatch")?;
    let pattern = arg_string(lua, args, 1, "gmatch")?;
    let limits = limits.clone();
    let next = Cell::new(0usize);
    // the iterator holds the Lua strings (no copy outside the Lua heap)
    lua.create_function(move |lua, ()| {
        let (s, p) = (subject.as_bytes(), pattern.as_bytes());
        let mut m = Matcher::new(&s, &p, limits.charge.as_ref());
        let mut src = next.get();
        while src <= s.len() {
            m.level = 0;
            if let Some(end) = m.do_match(src, 0)? {
                next.set(if end == src { end + 1 } else { end }); // an empty match advances one position
                return Ok(MultiValue::from_vec(m.captures(lua, Some(src), end)?));
            }
            src += 1;
        }
        next.set(s.len() + 1);
        Ok(MultiValue::new())
    })
}

/// How `gsub` replaces a match: the third argument.
enum Replacement {
    Template(LuaString),
    Function(Function),
    Table(Table),
}

fn gsub(lua: &Lua, limits: &PatternLimits, args: &MultiValue) -> mlua::Result<MultiValue> {
    let subject = arg_string(lua, args, 0, "gsub")?;
    let pattern = arg_string(lua, args, 1, "gsub")?;
    let replacement = match args.get(2) {
        Some(v @ (Value::String(_) | Value::Number(_) | Value::Integer(_))) => Replacement::Template(
            lua.coerce_string(v.clone())?
                .ok_or_else(|| bad_argument(3, "gsub", "string/function/table", Some(v)))?,
        ),
        Some(Value::Function(f)) => Replacement::Function(f.clone()),
        Some(Value::Table(t)) => Replacement::Table(t.clone()),
        _ => return Err(error("bad argument #3 to 'gsub' (string/function/table expected)")),
    };
    let (s, p) = (subject.as_bytes(), pattern.as_bytes());
    let max_s = arg_integer(
        lua,
        args,
        3,
        "gsub",
        Some(isize::try_from(s.len()).unwrap_or(isize::MAX).saturating_add(1)),
    )?;
    let anchor = p.first() == Some(&b'^');
    let pat = if anchor { &p[1..] } else { &p[..] };
    let mut m = Matcher::new(&s, pat, limits.charge.as_ref());
    let mut out: Vec<u8> = Vec::new();
    let too_big = || mlua::Error::MemoryError("not enough memory".into());
    let (mut src, mut n) = (0usize, 0isize);
    while n < max_s {
        m.level = 0;
        let end = m.do_match(src, 0)?;
        if let Some(e) = end {
            n += 1;
            add_value(lua, &m, &replacement, src, e, &mut out)?;
        }
        match end {
            Some(e) if e > src => src = e, // a non-empty match: skip it
            _ if src < s.len() => {
                out.push(s[src]);
                src += 1;
            }
            _ => break,
        }
        if out.len() > limits.max_output {
            return Err(too_big());
        }
        if anchor {
            break;
        }
    }
    if out.len().saturating_add(s.len() - src) > limits.max_output {
        return Err(too_big());
    }
    out.extend_from_slice(&s[src..]);
    Ok(MultiValue::from_vec(vec![
        Value::String(lua.create_string(&out)?),
        Value::Integer(i64::try_from(n).unwrap_or(0)),
    ]))
}

fn add_value(
    lua: &Lua,
    m: &Matcher<'_>,
    replacement: &Replacement,
    s: usize,
    e: usize,
    out: &mut Vec<u8>,
) -> mlua::Result<()> {
    let value = match replacement {
        Replacement::Template(template) => return add_template(lua, m, &template.as_bytes(), s, e, out),
        Replacement::Function(f) => f.call::<Value>(MultiValue::from_vec(m.captures(lua, Some(s), e)?))?,
        Replacement::Table(t) => t.get::<Value>(m.one_capture(lua, 0, Some(s), e)?)?,
    };
    match &value {
        Value::Nil | Value::Boolean(false) => out.extend_from_slice(&m.src[s..e]), // keep the original text
        Value::String(_) | Value::Number(_) | Value::Integer(_) => {
            if let Some(text) = lua.coerce_string(value.clone())? {
                out.extend_from_slice(&text.as_bytes());
            }
        }
        other => return Err(error(format!("invalid replacement value (a {})", other.type_name()))),
    }
    Ok(())
}

fn add_template(
    lua: &Lua,
    m: &Matcher<'_>,
    template: &[u8],
    s: usize,
    e: usize,
    out: &mut Vec<u8>,
) -> mlua::Result<()> {
    let mut i = 0;
    while i < template.len() {
        if template[i] == L_ESC {
            i += 1; // skip the escape
            let c = template.get(i).copied().unwrap_or(0);
            if !c.is_ascii_digit() {
                out.push(c);
            } else if c == b'0' {
                out.extend_from_slice(&m.src[s..e]);
            } else {
                match m.one_capture(lua, usize::from(c - b'1'), Some(s), e)? {
                    Value::String(text) => out.extend_from_slice(&text.as_bytes()),
                    other => {
                        // a position capture: its number as text
                        if let Some(text) = lua.coerce_string(other)? {
                            out.extend_from_slice(&text.as_bytes());
                        }
                    }
                }
            }
        } else {
            out.push(template[i]);
        }
        i += 1;
    }
    Ok(())
}

/// `string.rep`. The C version loops `n` times even when the string is empty (`string.rep("", 1e10)` runs
/// for minutes inside one VM instruction), and builds its result outside the memory limit's reach until it
/// is too late. This one sizes the result first and refuses one past the cap.
fn rep(lua: &Lua, limits: &PatternLimits, args: &MultiValue) -> mlua::Result<Value> {
    let subject = arg_string(lua, args, 0, "rep")?;
    let n = arg_integer(lua, args, 1, "rep", None)?;
    let s = subject.as_bytes();
    let times = usize::try_from(n).unwrap_or(0);
    if times == 0 || s.is_empty() {
        return Ok(Value::String(lua.create_string("")?));
    }
    if s.len().checked_mul(times).is_none_or(|total| total > limits.max_output) {
        return Err(mlua::Error::MemoryError("not enough memory".into()));
    }
    Ok(Value::String(lua.create_string(s.repeat(times))?))
}

/// Replace `find`, `match`, `gmatch`, `gsub` and `rep` in the engine's `string` table with the budgeted
/// versions.
///
/// # Errors
/// Fails when the `string` table or a function cannot be set up.
pub fn install(lua: &Lua, limits: PatternLimits) -> mlua::Result<()> {
    let string: Table = lua.globals().get("string")?;
    let find = {
        let limits = limits.clone();
        lua.create_function(move |lua, args: MultiValue| find_aux(lua, &limits, &args, true))?
    };
    let matcher = {
        let limits = limits.clone();
        lua.create_function(move |lua, args: MultiValue| find_aux(lua, &limits, &args, false))?
    };
    let gmatch_fn = {
        let limits = limits.clone();
        lua.create_function(move |lua, args: MultiValue| gmatch(lua, &limits, &args))?
    };
    let gsub_fn = {
        let limits = limits.clone();
        lua.create_function(move |lua, args: MultiValue| gsub(lua, &limits, &args))?
    };
    let rep_fn = lua.create_function(move |lua, args: MultiValue| rep(lua, &limits, &args))?;
    string.raw_set("rep", rep_fn)?;
    string.raw_set("find", find)?;
    string.raw_set("match", matcher)?;
    string.raw_set("gmatch", gmatch_fn)?;
    string.raw_set("gsub", gsub_fn)?;
    Ok(())
}

#[cfg(test)]
#[path = "patterns_tests.rs"]
mod tests;

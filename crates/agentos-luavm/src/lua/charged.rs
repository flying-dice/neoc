//! The C library functions whose work the VM's count hook cannot see, charged to the CPU budget by size.
//!
//! The hook runs every hundred VM instructions. A library function is one instruction, however much it does:
//! `string.upper` of a 16 MiB string, `table.concat` of a million pieces, `table.insert(t, 1, v)` on a big
//! array and `string.format("%s", big)` each cost milliseconds that no tick pays for, so a loop of them runs
//! thousands of times longer than the budget means. Here each is wrapped (or reimplemented, where the C
//! version cannot be trusted with the arguments) so that it charges ticks in proportion to its input or
//! output: an honest call costs a tick or two, a call on a huge value costs what the time it takes is worth.
//!
//! Every function here keeps its Lua 5.1 behaviour, except `table.insert`: a position outside `1..=#t + 1`
//! is an error ("position out of bounds", as in Lua 5.3) because the C version shifts elements by walking
//! from the end of the table down to a position that may be 2^31 below it, in one uninterruptible call.
//!
//! [`install`] replaces them in the engine's `string` and `table` tables, before an environment copies them.

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::patterns::{arg_integer, arg_string, bad_argument, pos_relative, PatternLimits};

/// Bytes a per-byte loop (case mapping, reversal, formatting) handles per CPU tick charged.
const BYTES_PER_TICK: usize = 256;
/// Bytes a copy (`sub`, `concat`) handles per CPU tick charged.
const COPY_BYTES_PER_TICK: usize = 2048;
/// Elements (pieces, arguments, results) handled per CPU tick charged.
const ELEMENTS_PER_TICK: usize = 32;
/// Array slots moved (a `get` and a `set` each) or keys visited per CPU tick charged.
const MOVES_PER_TICK: usize = 8;

fn ticks(amount: usize, per_tick: usize) -> u64 {
    1 + u64::try_from(amount / per_tick).unwrap_or(u64::MAX)
}

fn length_of(values: &MultiValue) -> usize {
    match values.front() {
        Some(Value::String(s)) => s.as_bytes().len(),
        _ => 0,
    }
}

/// Replace `string[name]` / `table[name]` with a function that calls the original, then charges
/// `cost(args, results)` ticks.
fn wrap(
    lua: &Lua,
    library: &Table,
    name: &str,
    limits: &PatternLimits,
    cost: impl Fn(&MultiValue, &MultiValue) -> u64 + Send + Sync + 'static,
) -> mlua::Result<()> {
    let original: Function = library.raw_get(name)?;
    let charge = limits.charge.clone();
    let wrapper = lua.create_function(move |_, args: MultiValue| {
        let results: MultiValue = original.call(args.clone())?;
        charge(cost(&args, &results))?;
        Ok(results)
    })?;
    library.raw_set(name, wrapper)
}

/// An integer argument the way `luaL_checkint` reads it (truncated toward zero), saturating instead of
/// wrapping for a value no integer holds.
#[allow(clippy::cast_possible_truncation)] // `as` saturates, and NaN is 0
fn integer(lua: &Lua, value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Integer(i) => Some(*i),
        v @ (Value::Number(_) | Value::String(_)) => {
            lua.coerce_number(v.clone()).ok().flatten().map(|n| n as i64)
        }
        _ => None,
    }
}

/// `i` / `j` of `table.concat` for the cost only (the C function validates them): saturating, `default` when
/// absent or not a number.
#[allow(clippy::cast_possible_truncation)] // `as` saturates, and NaN is 0
fn bound(value: Option<&Value>, default: i64) -> i64 {
    match value {
        Some(Value::Integer(i)) => *i,
        Some(Value::Number(n)) => *n as i64,
        _ => default,
    }
}

fn table_arg<'a>(args: &'a MultiValue, function: &str) -> mlua::Result<&'a Table> {
    match args.front() {
        Some(Value::Table(t)) => Ok(t),
        other => Err(bad_argument(1, function, "table", other)),
    }
}

/// `string.sub(s, i [, j])`, a copy charged by its length. (Native rather than a wrapper: `byte` and `char`
/// take thousands of arguments, which a wrapper that passes them on would push twice.)
fn sub(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |lua, args: MultiValue| {
        let subject = arg_string(lua, &args, 0, "sub")?;
        let s = subject.as_bytes();
        let len = isize::try_from(s.len()).unwrap_or(isize::MAX);
        let start = pos_relative(arg_integer(lua, &args, 1, "sub", None)?, s.len()).max(1);
        let end = pos_relative(arg_integer(lua, &args, 2, "sub", Some(-1))?, s.len()).min(len);
        let piece: &[u8] = match (usize::try_from(start - 1), usize::try_from(end)) {
            (Ok(from), Ok(to)) if start <= end => &s[from..to],
            _ => &[],
        };
        charge(ticks(piece.len(), COPY_BYTES_PER_TICK))?;
        lua.create_string(piece)
    })
}

/// Most values `string.byte` may return at once (Lua's C stack limit).
const MAX_BYTES_RETURNED: usize = 8000;

/// `string.byte(s [, i [, j]])`, charged by the number of values returned.
fn byte(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |lua, args: MultiValue| {
        let subject = arg_string(lua, &args, 0, "byte")?;
        let s = subject.as_bytes();
        let len = isize::try_from(s.len()).unwrap_or(isize::MAX);
        let first = pos_relative(arg_integer(lua, &args, 1, "byte", Some(1))?, s.len());
        let last = pos_relative(arg_integer(lua, &args, 2, "byte", Some(first))?, s.len());
        let (first, last) = (first.max(1), last.min(len));
        if first > last {
            return Ok(MultiValue::new());
        }
        let count = usize::try_from(last - first + 1).unwrap_or(usize::MAX);
        if count > MAX_BYTES_RETURNED {
            return Err(mlua::Error::runtime("string slice too long"));
        }
        charge(ticks(count, ELEMENTS_PER_TICK))?;
        let from = usize::try_from(first - 1).unwrap_or(0);
        Ok(s[from..from + count].iter().map(|&b| Value::Integer(i64::from(b))).collect())
    })
}

/// `string.char(...)`, charged by the number of arguments.
fn char(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |lua, args: MultiValue| {
        charge(ticks(args.len(), ELEMENTS_PER_TICK))?;
        let mut out = Vec::with_capacity(args.len());
        for index in 0..args.len() {
            let c = arg_integer(lua, &args, index, "char", None)?;
            out.push(u8::try_from(c).map_err(|_| {
                mlua::Error::runtime(format!("bad argument #{} to 'char' (invalid value)", index + 1))
            })?);
        }
        lua.create_string(&out)
    })
}

/// `table.insert(t, [pos,] value)`: appends, or shifts `t[pos..]` up by one (charged by the shift's length).
fn insert(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |lua, args: MultiValue| {
        let t = table_arg(&args, "insert")?;
        let size = t.raw_len();
        let after = i64::try_from(size).unwrap_or(i64::MAX).saturating_add(1);
        match args.len() {
            2 => t.raw_set(after, args[1].clone()),
            3 => {
                let pos = integer(lua, args.get(1))
                    .ok_or_else(|| bad_argument(2, "insert", "number", args.get(1)))?;
                if !(1..=after).contains(&pos) {
                    return Err(mlua::Error::runtime("bad argument #2 to 'insert' (position out of bounds)"));
                }
                charge(ticks(usize::try_from(after - pos).unwrap_or(usize::MAX), MOVES_PER_TICK))?;
                for i in (pos + 1..=after).rev() {
                    t.raw_set(i, t.raw_get::<Value>(i - 1)?)?;
                }
                t.raw_set(pos, args[2].clone())
            }
            _ => Err(mlua::Error::runtime("wrong number of arguments to 'insert'")),
        }
    })
}

/// `table.remove(t [, pos])`: the element at `pos` (default the last), the ones above it shifted down; nothing
/// when `pos` is outside the table (as in Lua 5.1).
fn remove(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |lua, args: MultiValue| {
        let t = table_arg(&args, "remove")?;
        let size = i64::try_from(t.raw_len()).unwrap_or(i64::MAX);
        let pos = match args.get(1) {
            None | Some(Value::Nil) => size,
            given => integer(lua, given).ok_or_else(|| bad_argument(2, "remove", "number", given))?,
        };
        if !(1..=size).contains(&pos) {
            return Ok(MultiValue::new());
        }
        charge(ticks(usize::try_from(size - pos).unwrap_or(usize::MAX), MOVES_PER_TICK))?;
        let removed: Value = t.raw_get(pos)?;
        for i in pos..size {
            t.raw_set(i, t.raw_get::<Value>(i + 1)?)?;
        }
        t.raw_set(size, Value::Nil)?;
        Ok(MultiValue::from_iter([removed]))
    })
}

/// `table.maxn(t)`: the largest positive numeric key, found by walking the whole table (charged as it goes, so
/// a table of millions of keys cannot be walked for free in a loop).
fn maxn(lua: &Lua, limits: &PatternLimits) -> mlua::Result<Function> {
    let charge = limits.charge.clone();
    lua.create_function(move |_, args: MultiValue| {
        let t = table_arg(&args, "maxn")?;
        let mut max = 0.0_f64;
        for (seen, pair) in t.pairs::<Value, Value>().enumerate() {
            if seen % MOVES_PER_TICK == 0 {
                charge(1)?;
            }
            match pair?.0 {
                Value::Integer(i) if i > 0 => max = max.max(i as f64),
                Value::Number(n) if n > max => max = n,
                _ => {}
            }
        }
        Ok(max)
    })
}

/// Charge the library functions of the engine's `string` and `table` by size (see the module docs).
///
/// # Errors
/// Fails when a library or a function cannot be found or replaced.
pub fn install(lua: &Lua, limits: &PatternLimits) -> mlua::Result<()> {
    let string: Table = lua.globals().get("string")?;
    let table: Table = lua.globals().get("table")?;
    for name in ["upper", "lower", "reverse", "format"] {
        wrap(lua, &string, name, limits, |args, results| {
            // a format reads its arguments as well as writing its result
            let read: usize =
                args.iter().map(|v| if let Value::String(s) = v { s.as_bytes().len() } else { 0 }).sum();
            ticks(length_of(results).max(read), BYTES_PER_TICK)
        })?;
    }
    string.raw_set("sub", sub(lua, limits)?)?;
    string.raw_set("byte", byte(lua, limits)?)?;
    string.raw_set("char", char(lua, limits)?)?;
    wrap(lua, &table, "concat", limits, |args, results| {
        // the pieces joined (`i..j`, no more than the table holds) and the bytes copied
        let pieces = match args.front() {
            Some(Value::Table(t)) => {
                let len = i64::try_from(t.raw_len()).unwrap_or(i64::MAX);
                let low = bound(args.get(2), 1).max(1);
                let high = bound(args.get(3), len).min(len);
                usize::try_from(high.saturating_sub(low).saturating_add(1)).unwrap_or(0)
            }
            _ => 0,
        };
        ticks(pieces, ELEMENTS_PER_TICK) + ticks(length_of(results), COPY_BYTES_PER_TICK)
    })?;
    table.raw_set("insert", insert(lua, limits)?)?;
    table.raw_set("remove", remove(lua, limits)?)?;
    table.raw_set("maxn", maxn(lua, limits)?)?;
    Ok(())
}

#[cfg(test)]
#[path = "charged_tests.rs"]
mod tests;

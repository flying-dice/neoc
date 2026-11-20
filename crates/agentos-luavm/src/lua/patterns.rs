//! Guards for Lua 5.1's C pattern matcher.
//!
//! `lstrlib.c` of Lua 5.1 matches a pattern by recursion with no depth limit (5.2 added `MAXCCALLS`): every
//! `*`, `+`, `?` and every capture group (`(` and `)`) that has to be tried adds a C frame. A pattern with
//! tens of thousands of such items (`("a?"):rep(20000)`) overflows the thread's stack, and a stack overflow
//! aborts the whole host. [`install`] wraps `string.find`, `string.match`, `string.gmatch` and `string.gsub`
//! so that a pattern with more than [`MAX_ITEMS`] of those characters is a Lua error instead; every other
//! pattern, and everything else about the four functions, is untouched.
//!
//! The count is of the characters, not of the items the matcher would recurse on: `%*` (an escaped star), a `-`
//! inside a set (`[a-z]`) and so on count too, which only ever refuses a pattern early. Depth is bounded by
//! the count (each recursion consumes one item of the pattern), and [`MAX_ITEMS`] times the largest frame
//! (about 200 bytes in an unoptimised build) is far below the 2 MiB of a default worker thread.
//!
//! The wrappers go on the engine's own `string` table, because every string's metatable (`("x"):find(...)`)
//! indexes that table rather than the copy an app environment holds.

use mlua::{Function, Lua, MultiValue, Table, Value};

/// The most pattern characters that make the matcher recurse (`* + - ? ( )`) that a pattern may hold.
pub const MAX_ITEMS: usize = 512;

/// The functions of `string` that take a pattern as their second argument.
const PATTERN_FUNCTIONS: &[&str] = &["find", "match", "gmatch", "gsub"];

/// How many characters of `pattern` the matcher may recurse on.
fn items(pattern: &[u8]) -> usize {
    pattern.iter().filter(|b| matches!(b, b'*' | b'+' | b'-' | b'?' | b'(' | b')')).count()
}

/// Refuse a pattern the matcher could overflow the stack on.
fn check(name: &str, pattern: &[u8]) -> mlua::Result<()> {
    // fewer bytes than the limit cannot hold more items than the limit
    if pattern.len() > MAX_ITEMS && items(pattern) > MAX_ITEMS {
        return Err(mlua::Error::runtime(format!(
            "bad argument #2 to '{name}' (pattern too complex: more than {MAX_ITEMS} of `* + - ? ( )`)"
        )));
    }
    Ok(())
}

/// Replace `find`, `match`, `gmatch` and `gsub` of the engine's `string` table by checked versions.
///
/// # Errors
/// Fails when `string` or one of the functions is not there.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let string: Table = lua.globals().raw_get("string")?;
    for &name in PATTERN_FUNCTIONS {
        let original: Function = string.raw_get(name)?;
        let wrapper = lua.create_function(move |_, args: MultiValue| {
            // a plain `string.find` does not interpret its pattern
            let plain = name == "find"
                && args.get(3).is_some_and(|v| !matches!(v, Value::Nil | Value::Boolean(false)));
            if !plain {
                if let Some(Value::String(pattern)) = args.get(1) {
                    check(name, &pattern.as_bytes())?;
                }
            }
            original.call::<MultiValue>(args)
        })?;
        string.raw_set(name, wrapper)?;
    }
    Ok(())
}

//! The `bit` global: `LuaJIT`'s bit-operations API on standard Lua 5.1, implemented in Rust.
//!
//! Lua 5.1 ships no bit library, and every number in it is an IEEE double. The semantics follow
//! <https://bitop.luajit.org/api.html>: each operand is rounded to the nearest integer (ties to even),
//! wrapped modulo 2^32 and treated as a signed 32-bit value; every result is a signed 32-bit value (as
//! a Lua number). Shift counts use only their low 5 bits. NaN and infinities wrap to 0.

use mlua::{Lua, Table, Variadic};

/// `tobit`: round to the nearest integer, wrap modulo 2^32, reinterpret as signed 32-bit.
#[must_use]
pub fn tobit(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    // `rem_euclid` of an integral f64 by a power of two is exact, so this is exact for any magnitude.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let wrapped = x.round_ties_even().rem_euclid(4_294_967_296.0) as u32;
    wrapped.cast_signed()
}

fn shift_count(n: f64) -> u32 {
    tobit(n).cast_unsigned() & 31
}

/// `tohex`: the low `|n|` hex digits of `x` (default 8, at most 8); upper case when `n` is negative.
#[must_use]
pub fn tohex(x: i32, n: Option<f64>) -> String {
    let n = n.map_or(8, tobit);
    let digits = n.unsigned_abs().min(8) as usize;
    let word = x.cast_unsigned();
    let full = if n < 0 { format!("{word:08X}") } else { format!("{word:08x}") };
    full[8 - digits..].to_string()
}

fn fold(name: &'static str, args: &[f64], op: fn(i32, i32) -> i32) -> mlua::Result<f64> {
    let (first, rest) = args.split_first().ok_or_else(|| {
        mlua::Error::runtime(format!("bad argument #1 to '{name}' (number expected, got no value)"))
    })?;
    Ok(f64::from(rest.iter().fold(tobit(*first), |acc, v| op(acc, tobit(*v)))))
}

/// Build the `bit` table.
///
/// # Errors
/// Fails when a function cannot be created in `lua`.
pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("tobit", lua.create_function(|_, x: f64| Ok(f64::from(tobit(x))))?)?;
    t.set("bnot", lua.create_function(|_, x: f64| Ok(f64::from(!tobit(x))))?)?;
    t.set("band", lua.create_function(|_, a: Variadic<f64>| fold("band", &a, |x, y| x & y))?)?;
    t.set("bor", lua.create_function(|_, a: Variadic<f64>| fold("bor", &a, |x, y| x | y))?)?;
    t.set("bxor", lua.create_function(|_, a: Variadic<f64>| fold("bxor", &a, |x, y| x ^ y))?)?;
    t.set(
        "lshift",
        lua.create_function(|_, (x, n): (f64, f64)| {
            Ok(f64::from(tobit(x).cast_unsigned().wrapping_shl(shift_count(n)).cast_signed()))
        })?,
    )?;
    t.set(
        "rshift",
        lua.create_function(|_, (x, n): (f64, f64)| {
            Ok(f64::from(tobit(x).cast_unsigned().wrapping_shr(shift_count(n)).cast_signed()))
        })?,
    )?;
    t.set(
        "arshift",
        lua.create_function(|_, (x, n): (f64, f64)| Ok(f64::from(tobit(x).wrapping_shr(shift_count(n)))))?,
    )?;
    t.set(
        "rol",
        lua.create_function(|_, (x, n): (f64, f64)| {
            Ok(f64::from(tobit(x).cast_unsigned().rotate_left(shift_count(n)).cast_signed()))
        })?,
    )?;
    t.set(
        "ror",
        lua.create_function(|_, (x, n): (f64, f64)| {
            Ok(f64::from(tobit(x).cast_unsigned().rotate_right(shift_count(n)).cast_signed()))
        })?,
    )?;
    t.set("bswap", lua.create_function(|_, x: f64| Ok(f64::from(tobit(x).swap_bytes())))?)?;
    t.set("tohex", lua.create_function(|_, (x, n): (f64, Option<f64>)| Ok(tohex(tobit(x), n)))?)?;
    Ok(t)
}

/// Install the `bit` table as a global.
///
/// # Errors
/// Fails when the table cannot be built or set.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    lua.globals().set("bit", module(lua)?)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // the results are exactly representable, integer-valued doubles
mod tests {
    use super::*;

    fn eval<T: mlua::FromLua>(src: &str) -> T {
        let lua = Lua::new();
        install(&lua).unwrap();
        lua.load(src).eval().unwrap()
    }

    #[test]
    fn tobit_wraps_modulo_2_pow_32_and_rounds_to_nearest_even() {
        assert_eq!(tobit(0.0), 0);
        assert_eq!(tobit(4_294_967_295.0), -1);
        assert_eq!(tobit(4_294_967_296.0), 0);
        assert_eq!(tobit(2_147_483_648.0), i32::MIN);
        assert_eq!(tobit(-2_147_483_649.0), i32::MAX);
        assert_eq!(tobit(0.5), 0);
        assert_eq!(tobit(1.5), 2);
        assert_eq!(tobit(2.5), 2);
        assert_eq!(tobit(-1.5), -2);
        assert_eq!(tobit(f64::from(1_u32 << 31) * 4_096.0 + 7.0), 7); // 2^43 + 7: beyond i32, still exact
        assert_eq!(tobit(f64::NAN), 0);
        assert_eq!(tobit(f64::INFINITY), 0);
    }

    #[test]
    fn matches_the_luajit_documentation_examples() {
        assert_eq!(eval::<f64>("return bit.tobit(0xffffffff)"), -1.0);
        assert_eq!(eval::<String>("return bit.tohex(1)"), "00000001");
        assert_eq!(eval::<String>("return bit.tohex(-1)"), "ffffffff");
        assert_eq!(eval::<String>("return bit.tohex(0x1f3, -4)"), "01F3");
        assert_eq!(eval::<String>("return bit.tohex(0xabcdef12, 3)"), "f12");
        assert_eq!(eval::<String>("return bit.tohex(1, 20)"), "00000001");
        assert_eq!(eval::<f64>("return bit.bnot(0)"), -1.0);
        assert_eq!(eval::<f64>("return bit.bnot(-1)"), 0.0);
        assert_eq!(eval::<f64>("return bit.band(0x12345678, 0xff)"), f64::from(0x78));
        assert_eq!(eval::<f64>("return bit.band(0xff, 0x0f, 0x03)"), 3.0);
        assert_eq!(eval::<f64>("return bit.bor(1, 2, 4)"), 7.0);
        assert_eq!(eval::<f64>("return bit.bxor(5, 3)"), 6.0);
        assert_eq!(eval::<f64>("return bit.lshift(1, 31)"), f64::from(i32::MIN));
        assert_eq!(eval::<f64>("return bit.lshift(1, 32)"), 1.0, "counts use the low 5 bits");
        assert_eq!(eval::<f64>("return bit.rshift(-1, 1)"), f64::from(i32::MAX));
        assert_eq!(eval::<f64>("return bit.rshift(256, 4)"), 16.0);
        assert_eq!(eval::<f64>("return bit.arshift(-256, 4)"), -16.0);
        assert_eq!(eval::<f64>("return bit.arshift(0x80000000, 31)"), -1.0);
        assert_eq!(eval::<f64>("return bit.rol(0x12345678, 8)"), f64::from(0x3456_7812_i32));
        assert_eq!(eval::<f64>("return bit.ror(0x12345678, 8)"), f64::from(0x7812_3456_i32));
        assert_eq!(eval::<f64>("return bit.bswap(0x12345678)"), f64::from(0x7856_3412_i32));
    }

    #[test]
    fn operands_are_coerced_like_lua_numbers() {
        assert_eq!(eval::<f64>(r#"return bit.band("255", 15)"#), 15.0);
    }

    #[test]
    fn a_variadic_op_with_no_operand_is_an_error_not_a_panic() {
        let lua = Lua::new();
        install(&lua).unwrap();
        let err = lua.load("return bit.band()").eval::<f64>().unwrap_err().to_string();
        assert!(err.contains("bad argument #1 to 'band'"), "got: {err}");
    }
}

//! Tests of `lua::charged`: the wrapped functions behave as the C originals, and what they cost grows with
//! the size of their input.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use mlua::Lua;

use super::install;
use crate::lua::{host_bytes::ByteBudget, patterns::PatternLimits, sandbox::make_bare_engine};

/// An engine whose library is charged, the ticks it was charged so far, and a global `orig` of the originals.
fn engine(limit: u64) -> (Lua, Arc<AtomicU64>) {
    let lua = make_bare_engine().expect("engine");
    lua.load(
        "orig = { upper = string.upper, lower = string.lower, reverse = string.reverse, format = string.format,
                  sub = string.sub, byte = string.byte, char = string.char, concat = table.concat,
                  insert = table.insert, remove = table.remove, maxn = table.maxn }",
    )
    .exec()
    .expect("stash the originals");
    let spent = Arc::new(AtomicU64::new(0));
    let charge = {
        let spent = spent.clone();
        Arc::new(move |n: u64| {
            if spent.fetch_add(n, Ordering::Relaxed) + n > limit {
                Err(mlua::Error::runtime("cpu budget exceeded"))
            } else {
                Ok(())
            }
        })
    };
    let limits = PatternLimits { charge, bytes: ByteBudget::new(256 << 20) };
    install(&lua, &limits).expect("install");
    (lua, spent)
}

fn ticks_of(lua: &Lua, spent: &AtomicU64, source: &str) -> u64 {
    spent.store(0, Ordering::Relaxed);
    lua.load(source).exec().unwrap_or_else(|e| panic!("{source}: {e}"));
    spent.load(Ordering::Relaxed)
}

#[test]
fn the_wrapped_functions_return_what_the_c_originals_return() {
    let (lua, _) = engine(u64::MAX);
    lua.load(
        r##"
        local function pack(...) return { n = select("#", ...), ... } end
        local function same(a, b)
          if a.n ~= b.n then return false end
          for i = 1, a.n do if a[i] ~= b[i] then return false end end
          return true
        end
        local function check(name, ...)
          local want = pack(pcall(orig[name], ...))
          local got = pack(pcall((name == "upper" or name == "lower" or name == "reverse" or name == "format" or name == "sub"
            or name == "byte" or name == "char") and string[name] or table[name], ...))
          assert(want[1] == got[1], name .. ": success differs")
          if want[1] then assert(same(want, got), name .. " differs: " .. tostring(want[2]) .. " vs " .. tostring(got[2])) end
        end
        for _, s in ipairs { "", "a", "Hello, World", "\0\1\2", "ÀÉ" } do
          check("upper", s) check("lower", s) check("reverse", s)
          check("byte", s) check("byte", s, 2) check("byte", s, 1, -1) check("byte", s, -3, 100)
          for _, i in ipairs { -100, -2, 0, 1, 2, 3, 100 } do
            check("sub", s, i) check("sub", s, i, i + 1) check("sub", s, 1, i)
          end
        end
        check("char") check("char", 65, 66, 67) check("char", 256) check("char", -1) check("char", "x")
        check("format", "%d-%s-%5.2f", 7, "x", 3.14159) check("format", "%q", 'a "b"\n') check("format", "%x %X %o", 255, 255, 8)
        check("format", "%s", {}) check("format", "%d", "x") check("format", "%", 1) check("format", "%10.3s|", "abcdef")
        check("concat", { "a", "b", "c" }) check("concat", { "a", "b", "c" }, ", ") check("concat", { 1, 2, 3 }, "-", 2, 3)
        check("concat", { "a", {} }) check("concat", {}) check("concat", { "a" }, "x", 3, 2) check("concat", "nope")
        assert(string.upper("abc") == "ABC" and ("abc"):upper() == "ABC" and ("x"):rep(3) == "xxx")
        "##,
    )
    .exec()
    .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn what_a_call_costs_grows_with_the_size_of_its_input_or_output() {
    let (lua, spent) = engine(u64::MAX);
    lua.load("big = string.rep('x', 1048576) pieces = {} for i = 1, 100000 do pieces[i] = 'ab' end")
        .exec()
        .unwrap();
    let small = ticks_of(
        &lua,
        &spent,
        "local _ = ('abc'):upper() .. ('ABC'):lower() .. ('abc'):reverse() .. ('x'):sub(1, 1)",
    );
    assert!(small <= 8, "an honest call costs a tick or so: {small}");
    for (call, at_least) in [
        ("big:upper()", 4000),
        ("big:lower()", 4000),
        ("big:reverse()", 4000),
        ("string.format('%s', big)", 4000),
        ("big:sub(2)", 400),
        ("table.concat(pieces)", 3200),
        ("table.concat(pieces, ', ')", 3200),
        ("string.char(unpack({ string.byte(big, 1, 7000) }))", 200),
        ("string.byte(big, 1, 7000)", 200),
    ] {
        let cost = ticks_of(&lua, &spent, &format!("local _ = {call}"));
        assert!(cost >= at_least, "{call} charged {cost}, expected at least {at_least}");
    }
}

#[test]
fn a_loop_of_calls_on_big_values_runs_into_the_budget_though_each_is_one_instruction() {
    let (lua, _) = engine(50_000);
    lua.load("big = string.rep('x', 1048576) pieces = {} for i = 1, 100000 do pieces[i] = 'ab' end")
        .exec()
        .unwrap();
    for body in [
        "local _ = big:upper()",
        "local _ = big:lower()",
        "local _ = big:reverse()",
        "local _ = string.format('%s', big)",
        "local _ = big:sub(2)",
        "local _ = table.concat(pieces)",
        "local _ = string.char(string.byte(big, 1, 7000))",
    ] {
        let err = lua.load(format!("for _ = 1, 1e9 do {body} end")).exec().expect_err(body).to_string();
        assert!(err.contains("cpu budget exceeded"), "{body}: {err}");
    }
}

#[test]
fn table_insert_and_remove_behave_like_lua_5_1_inside_the_table() {
    let (lua, _) = engine(u64::MAX);
    lua.load(
        r##"
        local function dump(t) local out = {} for i = 1, #t do out[#out + 1] = tostring(t[i]) end return table.concat(out, ",") end
        for _, pos in ipairs { 1, 2, 3, 5, 6 } do
          local a, b = { 1, 2, 3, 4, 5 }, { 1, 2, 3, 4, 5 }
          orig.insert(a, pos, "v") table.insert(b, pos, "v")
          assert(dump(a) == dump(b), "insert at " .. pos .. ": " .. dump(a) .. " vs " .. dump(b))
        end
        local a, b = { 1, 2 }, { 1, 2 }
        orig.insert(a, "tail") table.insert(b, "tail")
        assert(dump(a) == dump(b))
        for _, pos in ipairs { 1, 2, 3, 5, 0, 6, -1, 100 } do
          local a, b = { 1, 2, 3, 4, 5 }, { 1, 2, 3, 4, 5 }
          local ra, rb = orig.remove(a, pos), table.remove(b, pos)
          assert(ra == rb and dump(a) == dump(b), "remove at " .. pos)
        end
        local a, b = { 1, 2, 3 }, { 1, 2, 3 }
        assert(orig.remove(a) == table.remove(b) and dump(a) == dump(b))
        assert(select("#", table.remove({})) == 0, "nothing to remove from an empty table, like 5.1")
        assert(select("#", table.remove({ 1 }, 5)) == 0)
        assert(table.maxn({ 1, 2, [10] = 3, [2.5] = 1, x = 9 }) == 10 and orig.maxn({ 1, 2, [10] = 3 }) == 10 and table.maxn({}) == 0)
        -- both work on a table the app gave a metatable (raw access, as the C versions)
        local m = setmetatable({ 1, 2 }, { __index = function() error("no") end, __newindex = function() error("no") end })
        table.insert(m, 3) table.insert(m, 1, 0) assert(table.remove(m) == 3 and #m == 3)
        "##,
    )
    .exec()
    .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn table_insert_at_a_position_no_array_has_is_an_error_not_a_two_billion_step_loop() {
    let (lua, _) = engine(u64::MAX);
    let started = std::time::Instant::now();
    for position in
        ["-2^31", "2^31", "-2^63", "2^63", "1e300", "-1e300", "0/0", "1/0", "-1/0", "0", "-1", "7"]
    {
        let message: String = lua
            .load(format!("local ok, e = pcall(table.insert, {{ 1, 2, 3 }}, {position}, 'v') return tostring(ok) .. tostring(e)"))
            .eval()
            .unwrap();
        assert!(
            message.starts_with("false") && message.contains("position out of bounds"),
            "{position}: {message}"
        );
        // remove at such a position is "nothing to remove", as in 5.1, and just as quick
        let removed: i64 =
            lua.load(format!("return select('#', table.remove({{ 1, 2, 3 }}, {position}))")).eval().unwrap();
        assert_eq!(removed, 0, "{position}");
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

#[test]
fn a_shift_is_charged_by_the_number_of_elements_it_moves() {
    let (lua, spent) = engine(u64::MAX);
    lua.load("t = {} for i = 1, 100000 do t[i] = i end").exec().unwrap();
    let front = ticks_of(&lua, &spent, "table.insert(t, 1, 0) table.remove(t, 1)");
    let back = ticks_of(&lua, &spent, "table.insert(t, 0) table.remove(t)");
    assert!(front >= 2 * (100_000 / 8), "{front}");
    assert!(back <= 4, "{back}");
    let maxn = ticks_of(&lua, &spent, "return table.maxn(t)");
    assert!(maxn >= 100_000 / 8, "{maxn}");
}

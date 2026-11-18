//! Tests of `lua::patterns`: the port is compared with the real C implementation, and the budget and
//! depth limits are exercised.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use mlua::Lua;

use super::{install, PatternLimits};
use crate::lua::sandbox::make_bare_engine;

fn unlimited() -> PatternLimits {
    PatternLimits { charge: Arc::new(|_| Ok(())), max_output: 64 * 1024 * 1024 }
}

/// An engine whose `string` functions are ours, and a global `orig` holding the C originals.
fn engine_with(limits: PatternLimits) -> Lua {
    let lua = make_bare_engine().expect("engine");
    lua.load("orig = { find = string.find, match = string.match, gmatch = string.gmatch, gsub = string.gsub, rep = string.rep }")
        .exec()
        .expect("stash the originals");
    install(&lua, limits).expect("install");
    lua
}

/// Runs the C original and ours on generated subjects and patterns and fails on the first difference.
const DIFFERENTIAL: &str = r##"
    local seed = 20240607
    local function rnd(n) seed = (seed * 1103515245 + 12345) % 2147483648 return seed % n + 1 end
    local function pack(...) return { n = select("#", ...), ... } end
    local function same(a, b)
      if a.n ~= b.n then return false end
      for i = 1, a.n do if a[i] ~= b[i] then return false end end
      return true
    end
    local function show(r) local t = {} for i = 1, r.n do t[i] = tostring(r[i]) end return "(" .. table.concat(t, ", ") .. ")" end

    local tokens = { "a", "b", "c", ".", "%a", "%d", "%s", "%w", "%A", "%S", "[ab]", "[^a]", "[a-c]", "[%d]", "[%a_]",
      "*", "+", "-", "?", "(", ")", "()", "%1", "%2", "%b()", "%b<>", "%f[%w]", "%f[^%w]", "^", "$", "%%", "%(", "[]]", " ", "1", "%z", "%p", "%c", "%x", "%l", "%u", "%L", "[%w-]", "[a-]", "[^%s]", "%.", "%f[a]", "%3", "[a-b-c]" }
    local alphabet = { "a", "b", "c", "1", " ", "(", ")", "a", "b", "<", ">" }
    local function pattern()
      local t = {}
      for i = 1, rnd(7) do t[i] = tokens[rnd(#tokens)] end
      return table.concat(t)
    end
    local function subject()
      local t = {}
      for i = 1, rnd(13) - 1 do t[i] = alphabet[rnd(#alphabet)] end
      return table.concat(t)
    end

    local function gather(iter)
      local all, n = {}, 0
      for _ = 1, 40 do
        local r = pack(iter())
        if r.n == 0 or r[1] == nil then break end
        n = n + 1
        all[n] = show(r)
      end
      return table.concat(all, ";")
    end

    local repls = {
      "X", "%0%0", "%1", "<%1>", "%", "%%", "a%2b",
      function(...) return select("#", ...) end,
      function(a) return a end,
      function() return false end,
      { a = "A", b = 7, ["1"] = "one" },
    }

    local checked, errors = 0, 0
    local function compare(label, f, g, ...)
      local a, b = pack(pcall(f, ...)), pack(pcall(g, ...))
      checked = checked + 1
      if a[1] ~= b[1] or (a[1] and not same(a, b)) then
        local args = pack(...)
        for i = 1, args.n do if type(args[i]) == "string" then args[i] = string.format("%q", args[i]) end end
        error(label .. show(args) .. ": C says " .. show(a) .. ", ours says " .. show(b), 0)
      end
      if not a[1] then errors = errors + 1 end
    end

    local init = { false, false, 1, 2, -2, 0, 5, 99, -99 }
    for _ = 1, __CASES__ do
      local s, p = subject(), pattern()
      local i = init[rnd(#init)] or nil
      compare("find", orig.find, string.find, s, p, i)
      compare("find plain", orig.find, string.find, s, p, i, true)
      compare("match", orig.match, string.match, s, p, i)
      compare("gmatch", function(...) return gather(orig.gmatch(...)) end,
                        function(...) return gather(string.gmatch(...)) end, s, p)
      local r = repls[rnd(#repls)]
      local limit = ({ false, false, 0, 1, 2, 100 })[rnd(6)] or nil
      compare("gsub", orig.gsub, string.gsub, s, p, r, limit)
    end
    return checked, errors
"##;

fn differential(cases: u32) -> (i64, i64) {
    let lua = engine_with(unlimited());
    let script = DIFFERENTIAL.replace("__CASES__", &cases.to_string());
    lua.load(script).eval::<(i64, i64)>().unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn generated_patterns_behave_exactly_like_the_c_implementation() {
    let (checked, errors) = differential(8000);
    assert!(checked >= 20_000, "{checked} comparisons");
    // the generator must exercise both the matching paths and the malformed-pattern errors
    assert!(errors > 100 && errors < checked - 1000, "{errors} of {checked} cases errored");
}

#[test]
fn hand_written_cases_behave_like_the_c_implementation() {
    let lua = engine_with(unlimited());
    lua.load(
        r#"
        local cases = {
          { "hello world", "o w" }, { "hello world", "(o)(%s)(w)" }, { "hello", "" }, { "", "" }, { "", "x*" },
          { "abc", "^abc$" }, { "abc", "^b" }, { "aaa", "a-" }, { "aaa", "a-$" }, { "THE (quick) fox", "%((%a+)%)" },
          { "key = value", "^(%w+)%s*=%s*(%w+)$" }, { "f(a(b)c)d", "%b()" }, { "THE (quick) fox", "%f[%a]%a+" },
          { "a,b,,c", "([^,]*)" }, { "x=1, y=2", "(%w+)=(%w+)" }, { "abc", "()b()" }, { "aXb", "%u" }, { "a.b", "%." },
          { "a\0b", "%z" }, { "a\0b", "a\0b" }, { "abc", "a\0c" }, { "\n\v\f\r\t ", "^%s+$" }, { "abc", "[a-]" },
          { "abc", "[%a-z]" }, { "a]b", "[]]" }, { "a^b", "[%^]" }, { "abab", "(ab)%1" }, { "aaa", "((a)(a))" },
        }
        for _, c in ipairs(cases) do
          for _, f in ipairs { "find", "match" } do
            local a = { pcall(orig[f], c[1], c[2]) }
            local b = { pcall(string[f], c[1], c[2]) }
            assert(#a == #b, f .. " " .. c[2])
            for i = 1, #a do assert(a[i] == b[i], f .. " " .. c[2] .. " #" .. i) end
          end
        end
        -- gsub keeps a position capture as a number, and `%` before a non-digit is that character
        assert(string.gsub("abc", "()b", "%1") == "a2c")
        assert(string.gsub("abc", "b", "%x") == "axc")
        assert(("x"):rep(3):gsub("x", { x = "y" }) == "yyy")
        -- methods on strings reach the budgeted functions
        assert(("abc"):find("b", 1, true) == 2)
        assert(select(2, ("a,b"):gsub(",", ";")) == 1)
        "#,
    )
    .exec()
    .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn a_catastrophic_pattern_is_stopped_by_the_budget_instead_of_hanging() {
    let spent = Arc::new(AtomicU64::new(0));
    let limit = 10_000;
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
    let lua = engine_with(PatternLimits { charge, max_output: 1 << 20 });
    let started = std::time::Instant::now();
    // cubic or worse: far more than `limit` ticks (a tick is STEP_BLOCK matcher steps)
    for source in [
        r#"return string.find(string.rep("a", 100000), ".-.-.-.-b")"#,
        r#"return string.find(string.rep("a", 3000), "a*a*a*a*a*a*a*a*b")"#,
        r#"return string.match(string.rep("a", 40), string.rep("a?", 40) .. string.rep("a", 40) .. "b")"#,
        r#"return string.gsub(string.rep("a", 20000), "(a*)*b", "x")"#,
        r#"for _ in string.gmatch(string.rep("a", 50000), "a*a*b") do end"#,
    ] {
        spent.store(0, Ordering::Relaxed);
        let err = lua.load(source).exec().expect_err(source).to_string();
        assert!(err.contains("cpu budget exceeded"), "{source}: {err}");
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
}

#[test]
fn a_pattern_of_a_hundred_thousand_items_is_too_complex_not_a_stack_overflow() {
    let lua = engine_with(unlimited());
    let err = lua
        .load(r#"return string.find("aaaa", string.rep("a*", 100000))"#)
        .exec()
        .expect_err("too deep")
        .to_string();
    assert!(err.contains("pattern too complex"), "{err}");
}

#[test]
fn gsub_output_is_capped_because_a_rust_buffer_is_outside_the_lua_heap_limit() {
    let lua = engine_with(PatternLimits { charge: Arc::new(|_| Ok(())), max_output: 1000 });
    let err = lua
        .load(r#"return string.gsub(string.rep("a", 500), "a", "xyz")"#)
        .exec()
        .expect_err("too big")
        .to_string();
    assert!(err.contains("memory"), "{err}");
    let (_, n): (String, i64) = lua.load(r#"return string.gsub("aaa", "a", "x")"#).eval().expect("small");
    assert_eq!(n, 3);
}

#[test]
fn gsub_replacement_functions_propagate_their_errors() {
    let lua = engine_with(unlimited());
    let err = lua
        .load(r#"return string.gsub("abc", "%w", function(c) error("nope " .. c, 0) end)"#)
        .exec()
        .expect_err("propagates")
        .to_string();
    assert!(err.contains("nope a"), "{err}");
}

#[test]
fn rep_matches_the_c_implementation_and_never_loops_on_an_empty_string() {
    let lua = engine_with(PatternLimits { charge: Arc::new(|_| Ok(())), max_output: 1000 });
    lua.load(
        r#"
        for _, s in ipairs { "", "a", "ab", "\0" } do
          for _, n in ipairs { -3, 0, 1, 2, 7, 2.9, "3" } do
            assert(orig.rep(s, n) == string.rep(s, n), s .. " x " .. tostring(n))
          end
        end
        assert(pcall(string.rep) == false and pcall(string.rep, "a") == false)
        -- the C loop would run 10^10 times here
        assert(string.rep("", 1e10) == "")
        assert(("ab"):rep(3) == "ababab")
        assert(not pcall(string.rep, "ab", 1e9), "past the cap")
        "#,
    )
    .exec()
    .unwrap_or_else(|e| panic!("{e}"));
}

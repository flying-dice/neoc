-- Compares the prelude's polyfills with the native implementations of a Lua 5.3+ interpreter:
--   lua scripts/prelude-polyfill-check.lua <prelude.lua>
-- The prelude runs in a fresh globals table that lacks utf8 and typeof (the app environment copies neither from the
-- engine), then each call is made on both and the results must agree (an error must be an error on
-- both; messages differ). Exits 0 and prints "ok <n> comparisons", or raises with the first difference.

local native = { utf8 = utf8 }

local env = {}
for name, value in pairs(_G) do
  env[name] = value
end
env.utf8, env.typeof = nil, nil
env.unpack = table.unpack

local source = assert(io.open(arg[1], "rb")):read("a")
assert(load(source, "prelude", "t", env))(env) -- the prelude installs into its `...` argument
local poly = { utf8 = env.utf8 }

local comparisons = 0
local function same(a, b)
  if type(a) == "table" and type(b) == "table" then
    for key, value in pairs(a) do
      if not same(value, b[key]) then
        return false
      end
    end
    for key in pairs(b) do
      if a[key] == nil then
        return false
      end
    end
    return true
  end
  return a == b
end

-- `pick` limits the results compared (Lua 5.5's utf8.offset also returns an end position).
local function agree(label, fn_native, fn_poly, pick, ...)
  local args = table.pack(...)
  local ok_n, r_n = pcall(function()
    return table.pack(fn_native(table.unpack(args, 1, args.n)))
  end)
  local ok_p, r_p = pcall(function()
    return table.pack(fn_poly(table.unpack(args, 1, args.n)))
  end)
  comparisons = comparisons + 1
  local function show()
    local parts = {}
    for i = 1, args.n do
      parts[#parts + 1] = tostring(args[i])
    end
    return label .. "(" .. table.concat(parts, ", ") .. ")"
  end
  if ok_n ~= ok_p then
    error(
      show() .. ": native " .. (ok_n and "returns" or "errors") .. ", polyfill " .. (ok_p and "returns" or "errors"),
      0
    )
  end
  if ok_n then
    if pick then
      r_n, r_p = table.pack(table.unpack(r_n, 1, pick)), table.pack(table.unpack(r_p, 1, pick))
    end
    if not same(r_n, r_p) then
      error(show() .. ": results differ: " .. table.concat(r_n, "|") .. " vs " .. table.concat(r_p, "|"), 0)
    end
  end
end

local samples = {
  "",
  "hello",
  "h\xc3\xa9llo",
  "\xe6\x97\xa5\xe6\x9c\xac\xe8\xaa\x9e",
  "a\xf0\x9f\x98\x80z",
  "\xff",
  "ab\xe2\x82",
  "\xc0\x80",
  "\xed\xa0\x80",
  "\xf4\x90\x80\x80",
  "x\xc3",
  "\xe2\x82\xac\xe2\x82\xac",
}
local positions = { -9, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 9 }

for _, text in ipairs(samples) do
  agree("len", native.utf8.len, poly.utf8.len, 2, text)
  agree("codepoint", native.utf8.codepoint, poly.utf8.codepoint, nil, text, 1, -1)
  for _, i in ipairs(positions) do
    agree("len", native.utf8.len, poly.utf8.len, 2, text, i)
    agree("codepoint", native.utf8.codepoint, poly.utf8.codepoint, nil, text, i)
    for _, j in ipairs(positions) do
      agree("len", native.utf8.len, poly.utf8.len, 2, text, i, j)
      agree("codepoint", native.utf8.codepoint, poly.utf8.codepoint, nil, text, i, j)
    end
    for n = -3, 4 do
      agree("offset", native.utf8.offset, poly.utf8.offset, 1, text, n, i)
    end
  end
  -- (n = 0 without a position is left out: Lua 5.5 answers 1 where 5.3, 5.4 and Luau default the position to #s + 1)
  for n = -3, 4 do
    if n ~= 0 then
      agree("offset", native.utf8.offset, poly.utf8.offset, 1, text, n)
    end
  end
end
assert(poly.utf8.offset("hello", 0) == 6, "utf8.offset(s, 0) defaults the position to #s + 1")

-- utf8.codes over whole strings: the (position, code point) pairs, or an error at the same point.
for _, text in ipairs(samples) do
  local function walk(utf8lib)
    local pairs_seen = {}
    for position, code in utf8lib.codes(text) do
      pairs_seen[#pairs_seen + 1] = position .. ":" .. code
    end
    return table.concat(pairs_seen, ",")
  end
  agree("codes", function()
    return walk(native.utf8)
  end, function()
    return walk(poly.utf8)
  end, nil)
end

for _, code in ipairs { 0, 65, 127, 128, 233, 2047, 2048, 8364, 55295, 57344, 65535, 65536, 128512, 1114111 } do
  agree("char", native.utf8.char, poly.utf8.char, nil, code)
end
agree("char", native.utf8.char, poly.utf8.char, nil, 72, 233, 8364, 128512)
agree("char", native.utf8.char, poly.utf8.char, nil)
agree("char", native.utf8.char, poly.utf8.char, nil, -1)
-- above U+10FFFF native Lua 5.3+ encodes it; Luau (and the polyfill) refuse
assert(not pcall(poly.utf8.char, 1114112), "utf8.char refuses code points above U+10FFFF")
assert(native.utf8.charpattern ~= nil and poly.utf8.charpattern ~= nil)
-- charpattern splits a string into characters (5.1 spells NUL `%z`, so the strings differ but the splitting must not)
for _, text in ipairs { "h\xc3\xa9llo", "\xe6\x97\xa5\xe6\x9c\xac", "a\xf0\x9f\x98\x80z" } do
  local a, b = {}, {}
  for character in text:gmatch(native.utf8.charpattern) do
    a[#a + 1] = character
  end
  for character in text:gmatch((poly.utf8.charpattern:gsub("%%z", "\0"))) do
    b[#b + 1] = character
  end
  assert(same(a, b), "charpattern splits " .. text .. " differently")
end

assert(env.typeof(1) == "number" and env.typeof {} == "table" and env.typeof(nil) == "nil")

print("ok " .. comparisons .. " comparisons")

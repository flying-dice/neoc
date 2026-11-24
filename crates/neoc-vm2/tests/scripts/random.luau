--!strict
-- lib:random as a script sees it: its answers (random.json), then what it asserts. Random values are checked by their
-- shape.
local random = require("lib:random")
local out = {}

out.bytes = {
  #assert(random.random_bytes(32)),
  #assert(random.random_bytes(0)),
  random.random_bytes(32) ~= random.random_bytes(32),
} :: { any }

local ints = {}
for _ = 1, 200 do
  local n = random.random_int(1, 6)
  ints[#ints + 1] = n >= 1 and n <= 6
end
local all_in_range = true
for _, ok in ints do
  all_in_range = all_in_range and ok
end
out.int = { all_in_range, random.random_int(7, 7), (random.random_int(-5, -5)) } :: { any }

local s = assert(random.random_string(20))
out.string = {
  #s,
  s:match("^%w+$") ~= nil,
  random.random_string(0),
  (assert(random.random_string(50, "01")):match("^[01]+$")) ~= nil,
} :: { any }
out.multibyte = #assert(random.random_string(64, "αβγ")) -- 64 two-byte characters

local token = assert(random.token_urlsafe(32))
out.token = { #token, token:match("^[%w_-]+$") ~= nil, random.token_urlsafe(0) } :: { any }

local v4, v7 = random.uuid_v4(), random.uuid_v7()
out.uuid = {
  #v4,
  v4:sub(15, 15),
  #v7,
  v7:sub(15, 15),
  v4 ~= random.uuid_v4(),
  v4:match("^%x%x%x%x%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x$") ~= nil,
} :: { any }

local bytes, err = random.uuid_parse(v4)
out.round_trip = { #assert(bytes), err == nil, random.uuid_format(assert(bytes)) == v4 } :: { any }
out.parse_forms = {
  (random.uuid_format((assert(random.uuid_parse("550E8400-E29B-41D4-A716-446655440000"))))),
  (random.uuid_format((assert(random.uuid_parse("{550e8400-e29b-41d4-a716-446655440000}"))))),
}

-- failures are `(nil, err)`
for name, call in
  {
    min_over_max = function()
      return random.random_int(10, 5)
    end,
    empty_alphabet = function()
      return random.random_string(8, "")
    end,
    bad_uuid = function()
      return random.uuid_parse("not-a-uuid")
    end,
    short_uuid = function()
      return random.uuid_format("too short")
    end,
  } :: { [string]: () -> (unknown, unknown) }
do
  local value, why = call()
  out["fails_" .. name] = { value == nil, why ~= nil }
end

-- what it asserts
do
  local _, range = random.random_int(10, 5)
  assert(range ~= nil)
  assert(range.kind == "empty_range" and range.min == 10 and range.max == 5 and range.op == "lib:random.random_int")
  assert(tostring(range):find("^lib:random%.random_int: empty_range: "))

  local _, alphabet = random.random_string(8, "")
  assert(alphabet ~= nil and alphabet.kind == "empty_alphabet")

  local _, big = random.random_bytes(2 ^ 30 + 1)
  assert(big ~= nil and big.kind == "too_large" and big.requested == 2 ^ 30 + 1 and big.max == 2 ^ 30)

  local _, bad = random.uuid_parse("not-a-uuid")
  assert(bad ~= nil and bad.kind == "invalid_uuid" and bad.op == "lib:random.uuid_parse")

  local _, short = random.uuid_format("too short")
  assert(short ~= nil and short.kind == "wrong_length" and short.length == 9)

  local ok, negative = pcall(function()
    random.random_bytes(-1)
  end)
  assert(
    not ok
      and tostring(negative):find("lib:random.random_bytes: argument #1 'n_bytes': expected a whole number", 1, true)
  )

  local ok2, utf8 = pcall(function()
    random.random_string(8, "\xff\xfe")
  end)
  assert(not ok2 and tostring(utf8):find("argument #2 'alphabet'", 1, true))
end

return out

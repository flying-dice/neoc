--!strict
-- intentional: failures are typed errors (too_large, empty_range, empty_alphabet, invalid_uuid, wrong_length); a negative count and a non-UTF-8 alphabet are refused as bad arguments (they were returned error strings).
local random = require("lib:random")

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
assert(not ok and tostring(negative):find("lib:random.random_bytes: argument #1 'n_bytes': expected a whole number", 1, true))

local ok2, utf8 = pcall(function()
	random.random_string(8, "\xff\xfe")
end)
assert(not ok2 and tostring(utf8):find("argument #2 'alphabet'", 1, true))

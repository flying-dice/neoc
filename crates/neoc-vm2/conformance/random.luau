-- lib:random on neoc-vm and neoc-vm2: what both must answer the same. Random values are checked by their shape.
local random = require("lib:random")
local out = {}

out.bytes = { #random.random_bytes(32), #random.random_bytes(0), random.random_bytes(32) ~= random.random_bytes(32) }

local ints = {}
for _ = 1, 200 do
	local n = random.random_int(1, 6)
	ints[#ints + 1] = n >= 1 and n <= 6
end
local all_in_range = true
for _, ok in ints do
	all_in_range = all_in_range and ok
end
out.int = { all_in_range, random.random_int(7, 7), (random.random_int(-5, -5)) }

local s = random.random_string(20)
out.string = { #s, s:match("^%w+$") ~= nil, random.random_string(0), (random.random_string(50, "01"):match("^[01]+$")) ~= nil }
out.multibyte = #random.random_string(64, "αβγ") -- 64 two-byte characters

local token = random.token_urlsafe(32)
out.token = { #token, token:match("^[%w_-]+$") ~= nil, random.token_urlsafe(0) }

local v4, v7 = random.uuid_v4(), random.uuid_v7()
out.uuid = {
	#v4, v4:sub(15, 15), #v7, v7:sub(15, 15), v4 ~= random.uuid_v4(),
	v4:match("^%x%x%x%x%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x$") ~= nil,
}

local bytes, err = random.uuid_parse(v4)
out.round_trip = { #bytes, err == nil, random.uuid_format(bytes) == v4 }
out.parse_forms = {
	(random.uuid_format((random.uuid_parse("550E8400-E29B-41D4-A716-446655440000")))),
	(random.uuid_format((random.uuid_parse("{550e8400-e29b-41d4-a716-446655440000}")))),
}

-- failures are `(nil, err)`; what `err` is differs (see random.vm2.luau)
for name, call in {
	min_over_max = function() return random.random_int(10, 5) end,
	empty_alphabet = function() return random.random_string(8, "") end,
	bad_uuid = function() return random.uuid_parse("not-a-uuid") end,
	short_uuid = function() return random.uuid_format("too short") end,
} do
	local value, why = call()
	out["fails_" .. name] = { value == nil, why ~= nil }
end

return out

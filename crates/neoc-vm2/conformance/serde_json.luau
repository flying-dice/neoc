-- vnd:serde_json on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- A null document, the errors and what is refused differ on purpose: see serde_json.vm2.luau.
local serde_json = require("vnd:serde_json")
local out = {}

local object, err = serde_json.from_str('{"name":"alice","age":30,"ratio":0.5,"on":true,"tags":["a","b"],"user":{"id":7}}')
out.object = {
	err == nil,
	object:get("name"),
	object:get("age"),
	object:get("ratio"),
	object:get("on"),
	object:get("tags"):len(),
	object:get("tags"):get(2),
	object:get("user"):get("id"),
	object:keys(),
}
local array = serde_json.from_str(' [1, "two", false, [3], {"k": "v"}] ')
out.array = { array:len(), array:get(1), array:get(2), array:get(3), array:get(4):get(1), array:get(5):get("k") }
out.scalars = {
	serde_json.from_str('"hello"'),
	serde_json.from_str("42"),
	serde_json.from_str("-1.5e3"),
	serde_json.from_str("true"),
	serde_json.from_str("false"),
	serde_json.from_str('"\\u00e9\\n"'),
}
out.to_table = serde_json.from_str('{"a":[1,2,{"b":"c"}],"d":"e"}'):to_table()

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	bad_key = "{bad}",
	empty = "",
	unclosed = "[1, 2",
	trailing = "1 2",
	not_utf8 = '"\255"',
	deep = ("["):rep(100000) .. ("]"):rep(100000),
} do
	local value, why = serde_json.from_str(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

local value = {
	name = "bob",
	age = 25,
	ratio = 1.5,
	on = true,
	text = 'quote " and \n newline é',
	scores = { 1, 2, 3 },
	nested = { deep = { list = { "x", { y = 1 } } } },
	empty = {},
}
out.to_string = { serde_json.to_string(value) }
out.to_string_pretty = { serde_json.to_string_pretty(value) }
out.scalars_out = {
	serde_json.to_string(42),
	serde_json.to_string(-0.25),
	serde_json.to_string(1e20),
	serde_json.to_string("hello"),
	serde_json.to_string(true),
	serde_json.to_string(nil),
	serde_json.to_string({}),
	serde_json.to_string({ 1, "two" }),
}

-- a handle is written as what it holds
object:get("tags"):push("c")
out.handle = { serde_json.to_string(object), serde_json.to_string(object:get("tags")) }
out.round_trip = serde_json.from_str((serde_json.to_string(value))):to_table()

-- 127 levels are written and read back
local function nested(depth)
	local v = {}
	for _ = 2, depth do
		v = { v }
	end
	return v
end
local text = serde_json.to_string(nested(127))
out.deepest = { #text, serde_json.from_str(text) ~= nil }

return out

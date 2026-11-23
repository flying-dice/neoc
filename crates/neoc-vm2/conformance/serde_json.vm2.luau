-- intentional: a JSON null document reads as nil (there is no json.null sentinel), a failure is a typed error (DecodeError syntax or eof with line and column, EncodeError too_deep), a value that is not plain data (a function, a buffer anywhere inside, a table that contains itself) is refused as an argument where some were returned errors, and from_reader(reader) / to_writer(value, writer) wait for std:io.
local serde_json = require("vnd:serde_json")

-- null is nil, with no error
local value, none = serde_json.from_str("null")
assert(value == nil and none == nil)
local doc = assert(serde_json.from_str('{"gone":null}'))
assert(doc:get("gone") == nil and doc:kind("gone") == "null" and doc:has("gone"))
assert(serde_json.from_reader == nil and serde_json.to_writer == nil)

-- typed errors
local _, syntax = serde_json.from_str('{"ok": 1,\n bad}')
assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.column == 2 and syntax.reason == "key must be a string")
assert(syntax.op == "vnd:serde_json.from_str")
assert(tostring(syntax) == "vnd:serde_json.from_str: syntax: line 2, column 2: key must be a string")
for _, text in { "", '{"a":', "[1, 2", '"open' } do
	local _, eof = serde_json.from_str(text)
	assert(eof.kind == "eof" and eof.line == 1)
end

local function nested(depth)
	local v = {}
	for _ = 2, depth do
		v = { v }
	end
	return v
end
for name, encode in { to_string = serde_json.to_string, to_string_pretty = serde_json.to_string_pretty } do
	local text, deep = encode(nested(128))
	assert(text == nil and deep.kind == "too_deep" and deep.max_depth == 127 and deep.op == "vnd:serde_json." .. name)
end

-- what is not plain data is refused as an argument
local cyclic = {}
cyclic.self = cyclic
for _, bad in { print, { f = print }, buffer.create(2), { data = { buffer.create(2) } }, cyclic } do
	for name, encode in { to_string = serde_json.to_string, to_string_pretty = serde_json.to_string_pretty } do
		local ok, refused = pcall(encode, bad)
		assert(not ok and tostring(refused):find("vnd:serde_json." .. name .. ": argument #1 'value'", 1, true))
	end
end
assert(not pcall(serde_json.from_str, 42))

-- a number that is not finite is written null
assert(serde_json.to_string({ n = 0 / 0, big = math.huge }) == '{"big":null,"n":null}')

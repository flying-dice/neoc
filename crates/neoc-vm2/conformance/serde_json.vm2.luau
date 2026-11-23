-- intentional: a JSON null document reads as nil (there is no json.null sentinel), a failure is a typed error (DecodeError syntax or eof with line and column, EncodeError too_deep), a value that is not plain data (a function, a buffer anywhere inside, a table that contains itself) is refused as an argument where some were returned errors, and from_reader(reader) / to_writer(value, writer) take std:io's Reader and Writer, refuse anything else as an argument (it was a returned error) and fail with a typed ReadError / WriteError (the kinds above and io).
local serde_json = require("vnd:serde_json")

-- null is nil, with no error
local value, none = serde_json.from_str("null")
assert(value == nil and none == nil)
local doc = assert(serde_json.from_str('{"gone":null}'))
assert(doc:get("gone") == nil and doc:kind("gone") == "null" and doc:has("gone"))

-- a std:io reader
local io = require("std:io")
local read_null, read_none = serde_json.from_reader(io.from_string("null"))
assert(read_null == nil and read_none == nil)
local nothing, read_syntax = serde_json.from_reader(io.from_string('{"ok": 1,\n bad}'))
assert(nothing == nil and read_syntax.kind == "syntax" and read_syntax.line == 2 and read_syntax.op == "vnd:serde_json.from_reader")
local _, read_eof = serde_json.from_reader(io.from_string(""))
assert(read_eof.kind == "eof")
local not_reader_ok, not_reader = pcall(serde_json.from_reader, "1")
assert(not not_reader_ok and tostring(not_reader):find("vnd:serde_json.from_reader: argument #1 'reader': expected a Reader", 1, true))
local not_writer_ok, not_writer = pcall(serde_json.to_writer, { a = 1 }, io.from_string(""))
assert(not not_writer_ok and tostring(not_writer):find("vnd:serde_json.to_writer: argument #2 'writer': expected a Writer", 1, true))

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

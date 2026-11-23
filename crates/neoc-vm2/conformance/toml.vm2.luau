-- intentional: a date or time decodes to its text (it was a table with one private key), a failure is a typed error (DecodeError syntax with line and column, EncodeError unsupported), a document is a table with string keys so encode refuses anything else as an argument (a list, a string, a buffer anywhere inside; these were returned errors and a raised one for the buffer), and decode(reader) / encode_to(value, writer) wait for std:io.
local toml = require("vnd:toml")

-- dates and times are text
local doc = assert(toml.decode_string("t = 07:32\nd = 1979-05-27\nat = 1979-05-27T07:32:00Z"))
assert(doc.t == "07:32" and doc.d == "1979-05-27" and doc.at == "1979-05-27T07:32:00Z")
assert(toml.decode == nil and toml.encode_to == nil)

-- typed errors
local _, syntax = toml.decode_string("ok = 1\nbad = [unclosed")
assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.column > 1 and syntax.op == "vnd:toml.decode_string")
assert(type(syntax.reason) == "string" and tostring(syntax):find("vnd:toml.decode_string: syntax: line 2, column ", 1, true))
local text, unsupported = toml.encode({ n = 0 / 0 })
assert(text == nil and unsupported.kind == "unsupported" and unsupported.op == "vnd:toml.encode")

-- a document is a table with string keys
for _, value in { "text", 42, { 1, 2, 3 }, { f = print }, { data = buffer.create(2) }, { nested = { data = buffer.create(2) } } } do
	for name, encode in { encode = toml.encode, encode_pretty = toml.encode_pretty } do
		local ok, refused = pcall(encode, value)
		assert(not ok and tostring(refused):find("vnd:toml." .. name .. ": argument #1 'value'", 1, true))
	end
end
assert(not pcall(toml.decode_string, 42))

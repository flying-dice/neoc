-- intentional: a YAML null reads as nil (it was mlua's null sentinel, a userdata), a failure is a typed error (DecodeError syntax with line and column or invalid, EncodeError unsupported), a value that is not plain data (a function, a buffer anywhere inside, a table that contains itself) is refused as an argument where some were returned errors, and decode(reader) / encode_to(value, writer) wait for std:io.
local serde_yaml = require("vnd:serde_yaml")

-- null is nil
local doc = assert(serde_yaml.decode_string("key: null\nother: ~\nkept: 1\nnan: .nan\n"))
assert(doc.key == nil and doc.other == nil and doc.nan == nil and doc.kept == 1)
for _, text in { "", "~", "null" } do
	local value, none = serde_yaml.decode_string(text)
	assert(value == nil and none == nil)
end
assert(serde_yaml.decode == nil and serde_yaml.encode_to == nil)

-- typed errors
local _, syntax = serde_yaml.decode_string("ok: 1\nkey: *nope")
assert(syntax.kind == "syntax" and syntax.line == 2 and syntax.column == 6 and syntax.reason == "unknown anchor")
assert(syntax.op == "vnd:serde_yaml.decode_string")
assert(tostring(syntax) == "vnd:serde_yaml.decode_string: syntax: line 2, column 6: unknown anchor")
local _, tagged = serde_yaml.decode_string("!Tag x")
assert(tagged.kind == "syntax" and tagged.line == 1)
local _, several = serde_yaml.decode_string("a: 1\n---\nb: 2")
assert(several.kind == "invalid" and several.reason:find("more than one document", 1, true))

-- what is not plain data is refused as an argument
local cyclic = {}
cyclic.self = cyclic
for _, bad in { print, { f = print }, buffer.create(2), { list = { 1, buffer.create(2) } }, cyclic } do
	local ok, refused = pcall(serde_yaml.encode, bad)
	assert(not ok and tostring(refused):find("vnd:serde_yaml.encode: argument #1 'value'", 1, true))
end
assert(not pcall(serde_yaml.decode_string, 42))

-- a number that is not finite is written null
assert(serde_yaml.encode({ n = 0 / 0 }) == "n: null\n")

-- intentional: a YAML null reads as nil (it was mlua's null sentinel, a userdata), a failure is a typed error (DecodeError syntax with line and column or invalid, EncodeError unsupported), a value that is not plain data (a function, a buffer anywhere inside, a table that contains itself) is refused as an argument where some were returned errors, and decode(reader) / encode_to(value, writer) take std:io's Reader and Writer, refuse anything else as an argument (it was a returned error) and fail with a typed ReadError / WriteError (the kinds above and io).
local serde_yaml = require("vnd:serde_yaml")

-- null is nil
local doc = assert(serde_yaml.decode_string("key: null\nother: ~\nkept: 1\nnan: .nan\n"))
assert(doc.key == nil and doc.other == nil and doc.nan == nil and doc.kept == 1)
for _, text in { "", "~", "null" } do
	local value, none = serde_yaml.decode_string(text)
	assert(value == nil and none == nil)
end

-- a std:io reader
local io = require("std:io")
local read = assert(serde_yaml.decode(io.from_string("key: null\nkept: 1\n")))
assert(read.key == nil and read.kept == 1)
local nothing, read_syntax = serde_yaml.decode(io.from_string("key: [unclosed"))
assert(nothing == nil and read_syntax.kind == "syntax" and read_syntax.op == "vnd:serde_yaml.decode")
local not_reader_ok, not_reader = pcall(serde_yaml.decode, "a: 1")
assert(not not_reader_ok and tostring(not_reader):find("vnd:serde_yaml.decode: argument #1 'reader': expected a Reader", 1, true))
local not_writer_ok, not_writer = pcall(serde_yaml.encode_to, { a = 1 }, io.from_string(""))
assert(not not_writer_ok and tostring(not_writer):find("vnd:serde_yaml.encode_to: argument #2 'writer': expected a Writer", 1, true))

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

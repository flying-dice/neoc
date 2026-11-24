--!strict
-- intentional: elements of one name that repeat decode to a list (only the last was kept), encode takes the root's name as an option (it was always root) and a document is a table with string keys (a scalar or a list as the whole value is refused as an argument; a list wrote several roots), an empty element or attribute name is an error (it wrote <>), a failure is a typed error (DecodeError syntax, unexpected_eof, too_deep, invalid_utf8, unsupported; EncodeError unsupported), a value that is not plain data is refused as an argument, and decode(reader) / encode_to(value, writer, options?) take std:io's Reader and Writer, refuse anything else as an argument (it was a returned error) and fail with a typed ReadError / WriteError (the kinds above and io).
local quick_xml = require("vnd:quick_xml")

-- repeats are a list, so a list reads back
local doc = assert(quick_xml.decode_string('<root><item>a</item><other>x</other><item>b</item><user id="1"/><user id="2"/></root>'))
assert(#doc.item == 2 and doc.item[1] == "a" and doc.item[2] == "b" and doc.other == "x")
assert(doc.user[1]["@id"] == "1" and doc.user[2]["@id"] == "2")
local back = assert(quick_xml.decode_string((assert(quick_xml.encode({ tags = { "a", "b", "c" } })))))
assert(#back.tags == 3 and back.tags[3] == "c")

-- a std:io reader
local io = require("std:io")
local read = assert(quick_xml.decode(io.from_string("<r><a>1</a><a>2</a></r>")))
assert(read.a[1] == "1" and read.a[2] == "2")
local nothing, read_syntax = quick_xml.decode(io.from_string("<a><b></a>"))
assert(nothing == nil and read_syntax ~= nil)
assert(read_syntax.kind == "syntax" and read_syntax.op == "vnd:quick_xml.decode")
local _, read_utf8 = quick_xml.decode(io.from_string("<a>\255</a>"))
assert(read_utf8 ~= nil and read_utf8.kind == "invalid_utf8" and read_utf8.offset == 3)
local not_reader_ok, not_reader = pcall(function()
	quick_xml.decode("<a/>" :: any) -- deliberate refusal test: text is not a reader
end)
assert(not not_reader_ok and tostring(not_reader):find("vnd:quick_xml.decode: argument #1 'reader': expected a Reader", 1, true))
local not_writer_ok, not_writer = pcall(function()
	quick_xml.encode_to({ a = "1" }, io.from_string("") :: any) -- deliberate refusal test: a reader is not a writer
end)
assert(not not_writer_ok and tostring(not_writer):find("vnd:quick_xml.encode_to: argument #2 'writer': expected a Writer", 1, true))

-- the root's name
assert(quick_xml.encode({ name = "ada" }, { root = "user" }) == "<user><name>ada</name></user>")
assert(quick_xml.encode({ name = "ada" }) == "<root><name>ada</name></root>")
assert(not pcall(function()
	quick_xml.encode({ name = "ada" }, { rot = "user" } :: any) -- deliberate refusal test: not an option
end))

-- typed errors
local _, syntax = quick_xml.decode_string("<unclosed")
assert(syntax ~= nil)
assert(syntax.kind == "syntax" and type(syntax.reason) == "string" and syntax.op == "vnd:quick_xml.decode_string")
assert(tostring(syntax):find("vnd:quick_xml.decode_string: syntax: ", 1, true))
local _, eof = quick_xml.decode_string("")
assert(eof ~= nil and eof.kind == "unexpected_eof")
local _, deep = quick_xml.decode_string(("<a>"):rep(1000) .. ("</a>"):rep(1000))
assert(deep ~= nil and deep.kind == "too_deep" and deep.max_depth == 128)
local _, bytes = quick_xml.decode_string("<a>ok\255</a>")
assert(bytes ~= nil and bytes.kind == "invalid_utf8" and bytes.offset == 5)
local attempts: { { value: { [string]: any }, options: vnd_quick_xml_EncodeOptions? } } = {
	{ value = { ["bad key"] = 1 } },
	{ value = { [""] = 1 } },
	{ value = { a = { ["@"] = "x" } } },
	{ value = { a = 1 }, options = { root = "" } },
}
for _, attempt in attempts do
	local text, unsupported = quick_xml.encode(attempt.value, attempt.options)
	assert(text == nil and unsupported ~= nil)
	assert(unsupported.kind == "unsupported" and unsupported.op == "vnd:quick_xml.encode")
end

-- a document is a table with string keys
-- deliberate refusal test: none is a document
for _, value in { "text", 42, { 1, 2, 3 }, { f = print }, { data = buffer.create(2) }, { nested = { data = buffer.create(2) } } } :: { any } do
	local ok, refused = pcall(function()
		quick_xml.encode(value)
	end)
	assert(not ok and tostring(refused):find("vnd:quick_xml.encode: argument #1 'value'", 1, true))
end
assert(not pcall(function()
	quick_xml.decode_string(42 :: any) -- deliberate refusal test: a number is not text
end))

-- intentional: elements of one name that repeat decode to a list (only the last was kept), encode takes the root's name as an option (it was always root) and a document is a table with string keys (a scalar or a list as the whole value is refused as an argument; a list wrote several roots), an empty element or attribute name is an error (it wrote <>), a failure is a typed error (DecodeError syntax, unexpected_eof, too_deep, invalid_utf8, unsupported; EncodeError unsupported), a value that is not plain data is refused as an argument, and decode(reader) / encode_to(value, writer) wait for std:io.
local quick_xml = require("vnd:quick_xml")

-- repeats are a list, so a list reads back
local doc = assert(quick_xml.decode_string('<root><item>a</item><other>x</other><item>b</item><user id="1"/><user id="2"/></root>'))
assert(#doc.item == 2 and doc.item[1] == "a" and doc.item[2] == "b" and doc.other == "x")
assert(doc.user[1]["@id"] == "1" and doc.user[2]["@id"] == "2")
local back = assert(quick_xml.decode_string((assert(quick_xml.encode({ tags = { "a", "b", "c" } })))))
assert(#back.tags == 3 and back.tags[3] == "c")
assert(quick_xml.decode == nil and quick_xml.encode_to == nil)

-- the root's name
assert(quick_xml.encode({ name = "ada" }, { root = "user" }) == "<user><name>ada</name></user>")
assert(quick_xml.encode({ name = "ada" }) == "<root><name>ada</name></root>")
assert(not pcall(quick_xml.encode, { name = "ada" }, { rot = "user" }))

-- typed errors
local _, syntax = quick_xml.decode_string("<unclosed")
assert(syntax.kind == "syntax" and type(syntax.reason) == "string" and syntax.op == "vnd:quick_xml.decode_string")
assert(tostring(syntax):find("vnd:quick_xml.decode_string: syntax: ", 1, true))
local _, eof = quick_xml.decode_string("")
assert(eof.kind == "unexpected_eof")
local _, deep = quick_xml.decode_string(("<a>"):rep(1000) .. ("</a>"):rep(1000))
assert(deep.kind == "too_deep" and deep.max_depth == 128)
local _, bytes = quick_xml.decode_string("<a>ok\255</a>")
assert(bytes.kind == "invalid_utf8" and bytes.offset == 5)
for _, attempt in { { { ["bad key"] = 1 } }, { { [""] = 1 } }, { { a = { ["@"] = "x" } } }, { { a = 1 }, { root = "" } } } do
	local text, unsupported = quick_xml.encode(attempt[1], attempt[2])
	assert(text == nil and unsupported.kind == "unsupported" and unsupported.op == "vnd:quick_xml.encode")
end

-- a document is a table with string keys
for _, value in { "text", 42, { 1, 2, 3 }, { f = print }, { data = buffer.create(2) }, { nested = { data = buffer.create(2) } } } do
	local ok, refused = pcall(quick_xml.encode, value)
	assert(not ok and tostring(refused):find("vnd:quick_xml.encode: argument #1 'value'", 1, true))
end
assert(not pcall(quick_xml.decode_string, 42))

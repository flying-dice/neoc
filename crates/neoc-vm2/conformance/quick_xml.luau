-- vnd:quick_xml on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Repeated elements, the root's name, the errors and what is refused differ on purpose: see quick_xml.vm2.luau.
local quick_xml = require("vnd:quick_xml")
local out = {}

local doc, err = quick_xml.decode_string([=[<?xml version="1.0" encoding="UTF-8"?>
<!-- a comment -->
<config version="2">
  <name>alice</name>
  <age>30</age>
  <empty/>
  <escaped>&lt;b&gt; &amp; &quot;q&quot;</escaped>
  <raw><![CDATA[<not a tag>]]></raw>
  <server port="8080">
    <host>localhost</host>
    <tls enabled="false"/>
  </server>
  <note lang="en">hello</note>
</config>]=])
out.decode = { doc, err == nil }
out.text_only = quick_xml.decode_string("<root>only text</root>")
out.empty_root = quick_xml.decode_string("<root/>")
out.unicode = quick_xml.decode_string("<root><キー>値</キー></root>")
out.prefixed = quick_xml.decode_string('<root xmlns:x="urn:x"><x:a>1</x:a></root>')

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	unclosed = "<unclosed",
	mismatched = "<root><a></root>",
	missing_end = "<root><a>",
	bad_entity = "<root><a>&bogus;</a></root>",
	empty = "",
	not_utf8 = "<a>\255</a>",
	deep = ("<a>"):rep(100000) .. ("</a>"):rep(100000),
} do
	local value, why = quick_xml.decode_string(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

local config = {
	name = "bob",
	age = 25,
	ratio = 1.5,
	on = true,
	text = 'x < y & "z"',
	tags = { "a", "b" },
	server = { ["@port"] = "3000", host = "localhost", limits = { cpu = 2 } },
	jobs = { { id = 1 }, { id = 2 } },
	note = { ["@lang"] = "en", ["$text"] = "hello" },
}
out.encode = { quick_xml.encode(config) }
out.encode_empty = quick_xml.encode({})
for name, value in { space = { ["bad key"] = 1 }, digit = { ["1"] = 1 } } do
	local text, why = quick_xml.encode(value)
	out["refuses_" .. name] = { text == nil, why ~= nil }
end
out.round_trip = quick_xml.decode_string((quick_xml.encode({ name = "eve", age = "42", user = { ["@id"] = "7", city = "x" } })))

return out

-- vnd:serde_yaml on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- A null, the errors and what is refused differ on purpose: see serde_yaml.vm2.luau.
local serde_yaml = require("vnd:serde_yaml")
local out = {}

local doc, err = serde_yaml.decode_string([=[
title: demo
count: 30
negative: -7
ratio: 0.5
enabled: true
nums: [1, 2, 3]
mixed: [a, 1, true, [2, 3]]
point: { x: 1, y: 2 }
multi: |
  two
  lines
folded: >
  one
  line
quoted: "tab\there"
server:
  host: localhost
  port: 8080
  tls:
    enabled: false
jobs:
  - id: 1
  - id: 2
    tags: [x]
]=])
out.decode = { doc, err == nil }
out.sequence = serde_yaml.decode_string("- a\n- b\n")
out.scalars = { serde_yaml.decode_string("hello\n"), serde_yaml.decode_string("true\n"), serde_yaml.decode_string("42") }
out.flattened = serde_yaml.decode_string("1: one\ntrue: yes\nat: 2001-12-14t21:59:43.10-05:00\nhex: 0x10\nbase: &x [1]\ncopy: *x\n")
out.unicode = serde_yaml.decode_string("キー: 値\n")

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	unclosed = "key: [unclosed",
	bad_indent = "a:\n  b: 1\n c: 2",
	no_anchor = "a: *nope",
	tagged = "a: !Tag {b: 1}",
	several = "a: 1\n---\nb: 2",
	not_utf8 = "a: \255",
	deep = ("["):rep(100000) .. ("]"):rep(100000),
} do
	local value, why = serde_yaml.decode_string(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

local config = {
	name = "bob",
	age = 25,
	ratio = 1.5,
	on = true,
	text = "two\nlines",
	tags = { "a", "b" },
	server = { host = "localhost", port = 3000, limits = { cpu = 2 } },
	jobs = { { id = 1 }, { id = 2 } },
}
out.encode = { serde_yaml.encode(config) }
out.encode_scalars = { serde_yaml.encode(42), serde_yaml.encode(true), serde_yaml.encode("hello"), serde_yaml.encode(nil) }
out.encode_list = serde_yaml.encode({ 1, "two", { 3 } })
out.round_trip = serde_yaml.decode_string((serde_yaml.encode(config)))

return out

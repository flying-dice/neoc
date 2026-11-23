-- vnd:toml on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Dates and times, the errors, and what is refused differ on purpose: see toml.vm2.luau.
local toml = require("vnd:toml")
local out = {}

local doc, err = toml.decode_string([=[
title = "demo"
count = 30
negative = -7
ratio = 0.5
enabled = true
nums = [1, 2, 3]
mixed = ["a", 1, true, [2, 3]]
point = { x = 1, y = 2 }
multi = """
two
lines"""
literal = 'C:\path'

[server]
host = "localhost"
port = 8080

[server.tls]
enabled = false

[[jobs]]
id = 1

[[jobs]]
id = 2
tags = ["x"]
]=])
out.decode = { doc, err == nil }
out.empty = { toml.decode_string("") }
out.comment_only = toml.decode_string("# nothing here\n")
out.toml_1_1 = { toml.decode_string("x = {\n  a = 1,\n  b = 2,\n}\n"), toml.decode_string('x = "\\e"') }
out.unicode = toml.decode_string('"キー" = "値"\n')

-- a failure is `(nil, err)`; what `err` is differs
for name, text in {
	unclosed = "key = [unclosed",
	duplicate = "a = 1\na = 2",
	no_value = "a =",
	deep = "a = " .. ("["):rep(100000) .. ("]"):rep(100000),
} do
	local value, why = toml.decode_string(text)
	out["fails_" .. name] = { value == nil, why ~= nil }
end

local config = {
	name = "bob",
	age = 25,
	ratio = 1.5,
	on = true,
	tags = { "a", "b" },
	server = { host = "localhost", port = 3000, limits = { cpu = 2 } },
	jobs = { { id = 1 }, { id = 2 } },
}
out.encode = { toml.encode(config) }
out.encode_pretty = { toml.encode_pretty(config) }
out.encode_empty = toml.encode({})
out.round_trip = toml.decode_string((toml.encode(config)))

-- a std:io reader gives what its text gives (writers come from std:fs, which neoc-vm2 does not have yet)
local io = require("std:io")
out.reader = { toml.decode(io.from_string('title = "demo"\nnums = [1, 2]\n\n[server]\nport = 8080\n')) }
out.reader_empty = { toml.decode(io.from_string("")) }
local read, read_why = toml.decode(io.from_string("key = [unclosed"))
out.reader_fails = { read == nil, read_why ~= nil }

return out

-- std:io on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Writers come from std:fs: `write`, `flush` and `copy` are in files.luau. Lines that are not UTF-8 and what an
-- error is differ on purpose: see io.vm2.luau.
local io = require("std:io")
local out = {}

-- nil answers are kept apart from strings
local function maybe(value: string?, err: unknown): { string | boolean }
	return { if value == nil then false else value, err == nil }
end

local reader = io.from_string("hello world")
out.read = { maybe(reader:read(5)), maybe(reader:read(1)), maybe(reader:read(100)), maybe(reader:read(1)), maybe(reader:read(1)) }
out.read_empty = maybe(io.from_string(""):read(4))
out.read_huge = maybe(io.from_string("hello"):read(2 ^ 40))
-- bytes that are not text are compared here: the harness carries text
out.read_binary = io.from_string("\0\1\2\254\255"):read(5) == "\0\1\2\254\255"
out.read_clamped = #assert(io.from_string(string.rep("x", (16 * 1024 * 1024) + 5)):read(2 ^ 40))

local lines = io.from_string("foo\nbar\r\n\nlast")
out.read_line = { maybe(lines:read_line()), maybe(lines:read_line()), maybe(lines:read_line()), maybe(lines:read_line()), maybe(lines:read_line()) }
out.read_line_unicode = maybe(io.from_string("naïve 値\n"):read_line())

local rest = io.from_string("hello world")
rest:read(6)
out.read_all = { maybe(rest:read_all()), maybe(rest:read_all()), maybe(io.from_string(""):read_all()) }
out.read_all_binary = io.from_string("\0\1\2\254\255"):read_all() == "\0\1\2\254\255"

local mixed = io.from_string("head\nbody bytes")
out.mixed = { maybe(mixed:read_line()), maybe(mixed:read(4)), maybe(mixed:read_all()) }

out.wrong_arguments = { (pcall(io.from_string, {})), (pcall(io.from_string)), (pcall(reader.read, reader, "many")) }

return out

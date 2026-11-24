--!strict
-- std:io as a script sees it: its answers (io.json), then what it asserts. Writers come from std:fs: `write`, `flush`
-- and `copy` are in files.luau.
local io = require("std:io")
local out = {}

-- nil answers are kept apart from strings
local function maybe(value: string?, err: unknown): { string | boolean }
  return { if value == nil then false else value, err == nil }
end

local reader = io.from_string("hello world")
out.read = {
  maybe(reader:read(5)),
  maybe(reader:read(1)),
  maybe(reader:read(100)),
  maybe(reader:read(1)),
  maybe(reader:read(1)),
}
out.read_empty = maybe(io.from_string(""):read(4))
out.read_huge = maybe(io.from_string("hello"):read(2 ^ 40))
-- bytes that are not text are compared here: the harness carries text
out.read_binary = io.from_string("\0\1\2\254\255"):read(5) == "\0\1\2\254\255"
out.read_clamped = #assert(io.from_string(string.rep("x", (16 * 1024 * 1024) + 5)):read(2 ^ 40))

local lines = io.from_string("foo\nbar\r\n\nlast")
out.read_line = {
  maybe(lines:read_line()),
  maybe(lines:read_line()),
  maybe(lines:read_line()),
  maybe(lines:read_line()),
  maybe(lines:read_line()),
}
out.read_line_unicode = maybe(io.from_string("naïve 値\n"):read_line())

local rest = io.from_string("hello world")
rest:read(6)
out.read_all = { maybe(rest:read_all()), maybe(rest:read_all()), maybe(io.from_string(""):read_all()) }
out.read_all_binary = io.from_string("\0\1\2\254\255"):read_all() == "\0\1\2\254\255"

local mixed = io.from_string("head\nbody bytes")
out.mixed = { maybe(mixed:read_line()), maybe(mixed:read(4)), maybe(mixed:read_all()) }

out.wrong_arguments =
  { (pcall(io.from_string :: any, {})), (pcall(io.from_string :: any)), (pcall(reader.read :: any, reader, "many")) }

-- what it asserts
do
  -- a line is bytes
  local reader = io.from_string("\255\254\n\0x\r\r\n")
  assert(reader:read_line() == "\255\254" and reader:read_line() == "\0x\r" and reader:read_line() == nil)

  -- a count is a whole number from 0
  local again = io.from_string("abc")
  for _, count in { -1, 1.5, 0 / 0, "2" :: any } do -- deliberate refusal test: text is not a count
    local ok, refused = pcall(function()
      again:read(count)
    end)
    assert(not ok and tostring(refused):find("std:io.Reader:read: argument #1 'n'", 1, true))
  end
  assert(again:read_all() == "abc")

  -- copy takes a Reader and a Writer
  -- deliberate refusal tests: a Reader is not a Writer, and text is not a Reader
  local ok, refused = pcall(function()
    io.copy(again, again :: any)
  end)
  assert(not ok and tostring(refused):find("std:io.copy: argument #2 'writer': expected a Writer", 1, true))
  local first_ok, first = pcall(function()
    io.copy("text" :: any, again :: any)
  end)
  assert(not first_ok and tostring(first):find("std:io.copy: argument #1 'reader': expected a Reader", 1, true))
end

return out

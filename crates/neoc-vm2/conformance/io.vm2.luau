--!strict
-- intentional: the handle types are Reader and Writer (they were ReaderHandle and WriterHandle), a failure is a typed IoError whose kind is the operating system's (not_found, broken_pipe, ...), read_line gives a line's bytes whatever they are (a line that is not UTF-8 was an error), read refuses a count that is not a whole number from 0, and copy refuses a wrong argument by raising with its place (it returned a string).
local io = require("std:io")

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

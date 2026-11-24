--!strict
-- intentional: with_extension returns a typed error (separator) for an extension that holds a separator instead of raising a string, and absolute's failure is a typed error (empty_path / current_dir).
local path = require("std:path")

-- join is a base and any number of parts; each part is a path
local sep = path.separator
assert(path.join("a") == "a")
assert(path.join("a", "b") == "a" .. sep .. "b")
assert(path.join("a", "b", "c.txt") == "a" .. sep .. "b" .. sep .. "c.txt")
local no_base = pcall(function()
	(path.join :: any)()
end)
assert(not no_base)
local ok, not_a_part = pcall(function()
	path.join("a", "b", nil :: any)
end)
assert(not ok and tostring(not_a_part):find("std:path.join: argument #3 'parts': expected a string", 1, true))

-- typed errors
local value, separator = path.with_extension("a/b.txt", "x/y")
assert(value == nil and separator ~= nil)
assert(separator.kind == "separator" and separator.op == "std:path.with_extension")
assert(tostring(separator) == "std:path.with_extension: separator: an extension cannot contain a path separator")
local absolute, empty = path.absolute("")
assert(absolute == nil and empty ~= nil)
assert(empty.kind == "empty_path" and empty.op == "std:path.absolute")

-- a wrong argument is refused with its place
local refused_ok, refused = pcall(function()
	path.normalize({} :: any)
end)
assert(not refused_ok and tostring(refused):find("std:path.normalize: argument #1 'path': expected a string", 1, true))

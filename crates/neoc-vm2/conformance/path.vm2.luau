-- intentional: join takes a base and one part as Path::join does (it took any number of parts; variadic parameters wait for mlua-ir), with_extension returns a typed error (separator) for an extension that holds a separator instead of raising a string, and absolute's failure is a typed error (empty_path / current_dir).
local path = require("std:path")

-- join is two paths
assert(path.join("a", "b") == "a" .. path.separator .. "b")
assert(not pcall(path.join, "a"))
local ok, extra = pcall(path.join, "a", "b", "c")
assert(not ok and tostring(extra):find("std:path.join", 1, true))

-- typed errors
local value, separator = path.with_extension("a/b.txt", "x/y")
assert(value == nil and separator.kind == "separator" and separator.op == "std:path.with_extension")
assert(tostring(separator) == "std:path.with_extension: separator: an extension cannot contain a path separator")
local absolute, empty = path.absolute("")
assert(absolute == nil and empty.kind == "empty_path" and empty.op == "std:path.absolute")

-- a wrong argument is refused with its place
local refused_ok, refused = pcall(path.normalize, {})
assert(not refused_ok and tostring(refused):find("std:path.normalize: argument #1 'path': expected a string", 1, true))

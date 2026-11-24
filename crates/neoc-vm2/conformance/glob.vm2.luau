--!strict
-- intentional: a failure is a typed error (invalid_pattern with the pattern and why; for a scan also not_found and not_a_directory with the directory), a scan returns one value { paths, errors } (it returned the paths and the unreadable entries as two values, the second where every other function has its error), and a wrong option is refused as a wrong argument with its place (it was a returned string).
local glob = require("lib:glob")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

-- a pattern that is not one
local function invalid(op: string, value: unknown, err: lib_glob_ScanError?)
	assert(value == nil and err ~= nil)
	assert(err.kind == "invalid_pattern" and err.pattern == "src/[*.rs" and err.op == op)
	assert(tostring(err) == op .. ": invalid_pattern: src/[*.rs: " .. err.reason)
end
invalid("lib:glob.is_match", glob.is_match("src/[*.rs", "a"))
invalid("lib:glob.Glob.new", glob.Glob.new("src/[*.rs"))
invalid("lib:glob.GlobSet.new", glob.GlobSet.new({ "ok", "src/[*.rs" }))
invalid("lib:glob.scan", glob.scan("src/[*.rs"))
invalid("lib:glob.scan", glob.scan("*", { ignore = { "src/[*.rs" } }))

-- the bounds: the pattern is cut in the error
local _, long = glob.Glob.new(string.rep("a", 4097))
assert(long ~= nil and long.kind == "invalid_pattern" and #long.pattern == 128 and long.reason:find("too long", 1, true))
local _, deep = glob.is_match(string.rep("{a,", 33) .. "a" .. string.rep("}", 33), "a")
assert(deep ~= nil and deep.reason:find("nests", 1, true))
local _, many = glob.is_match(string.rep("{a,b}", 257), "a")
assert(many ~= nil and many.reason:find("more than 256", 1, true))

-- a scan: one value, and typed failures
local dir = path.join(env.temp_dir(), "neoc-conformance-glob-vm2-" .. random.uuid_v4())
local function scans(): ...unknown
	assert(fs.create_dir(dir))
	assert(fs.write(path.join(dir, "b.txt"), ""))
	assert(fs.write(path.join(dir, "a.txt"), ""))
	local found, err = glob.scan("*.txt", { cwd = dir })
	assert(found ~= nil and err == nil)
	assert(table.concat(found.paths, ",") == "a.txt,b.txt" and #found.errors == 0)
	local again = assert(assert(glob.Glob.new("*.txt")):scan({ cwd = dir, limit = 1 }))
	assert(#again.paths == 1 and again.paths[1] == "a.txt" and #again.errors == 0)

	local missing = path.join(dir, "nope")
	local none, not_found = glob.scan("*", { cwd = missing })
	assert(none == nil and not_found ~= nil and not_found.kind == "not_found" and not_found.path == missing)
	assert(tostring(not_found) == "lib:glob.scan: not_found: directory not found: " .. missing)
	local file = path.join(dir, "a.txt")
	local _, not_dir = assert(glob.Glob.new("*")):scan({ cwd = file })
	assert(not_dir ~= nil and not_dir.kind == "not_a_directory" and not_dir.path == file and not_dir.op == "lib:glob.Glob:scan")
end
local ok, failure = pcall(scans)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, failure)

-- a wrong option is a wrong argument
for _, opts in { { type = "block" }, { limit = -1 }, { limit = 1.5 }, { cwd = 5 }, { ignore = "x" }, { absolut = true } } :: { any } do -- deliberate refusal test: none is the options
	local refused_ok, refused = pcall(function()
		glob.scan("*", opts)
	end)
	assert(not refused_ok and tostring(refused):find("lib:glob.scan: argument #2 'opts'", 1, true), tostring(refused))
end

--!strict
-- intentional: a failure is a typed BundleError (entrypoint_unreadable with the path, stub_unreadable, too_large, output_unwritable with the path) where it was a string; a field that does not exist is refused with the other wrong arguments (it was ignored).
local compile = require("lib:compile")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local dir = path.join(env.temp_dir(), "neoc-conformance-compile-vm2-" .. random.uuid_v4())
local entrypoint = path.join(dir, "nope.lua")

local built, err = compile.bundle({ entrypoint = entrypoint, output = path.join(dir, "app") })
assert(built == nil and err ~= nil)
assert(err.kind == "entrypoint_unreadable" and err.op == "lib:compile.bundle")
if err.kind == "entrypoint_unreadable" then
	assert(err.path == entrypoint and #err.reason > 0)
	assert(err.message == `cannot read {entrypoint}: {err.reason}`)
end
assert(tostring(err) == `lib:compile.bundle: entrypoint_unreadable: {err.message}`)

-- arguments of the wrong shape are refused, with their place
for _, bundle in
	{
		{ output = "app" },
		{ entrypoint = "main.lua" },
		{ entrypoint = "main.lua", output = 5 },
		{ entrypoint = "main.lua", output = "app", minify = true },
		"main.lua",
	} :: { any } -- deliberate refusal test: none is a bundle
do
	local ok, refused = pcall(function()
		compile.bundle(bundle)
	end)
	assert(not ok and tostring(refused):find("lib:compile.bundle: argument #1 'bundle'", 1, true), tostring(refused))
end

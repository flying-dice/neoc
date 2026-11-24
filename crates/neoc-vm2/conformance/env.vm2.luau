--!strict
-- intentional: a failure is a typed error (var: not_present / not_unicode with the name; home_dir: not_found; current_dir: not_found / permission_denied / other), and a malformed _NEOC_ARGS raises a typed error (malformed) instead of a string.
local env = require("std:env")

local value, missing = env.var("_NEOC_DEFINITELY_MISSING_XYZ_42")
assert(value == nil and missing ~= nil and missing.kind == "not_present" and missing.name == "_NEOC_DEFINITELY_MISSING_XYZ_42")
assert(missing.op == "std:env.var")
assert(tostring(missing) == "std:env.var: not_present: _NEOC_DEFINITELY_MISSING_XYZ_42: environment variable not found")

-- a wrong argument is refused with its place
local ok, refused = pcall(function()
	env.var(42 :: any) -- deliberate refusal test: a number is not a name
end)
assert(not ok and tostring(refused):find("std:env.var: argument #1 'key'", 1, true))

-- the module table is read-only
assert(not pcall(function()
	(env :: any).os = "other" -- deliberate refusal test: a write to the module
end))

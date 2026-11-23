-- std:env on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- What an error is differs on purpose: see env.vm2.luau.
local env = require("std:env")
local out = {}

out.constants = { env.os, env.arch, env.family, env.exe_suffix, env.dll_prefix, env.dll_suffix }
out.home_dir = { env.home_dir() }
out.temp_dir = env.temp_dir()
out.current_dir = { env.current_dir() }

local name = if env.family == "windows" then "USERPROFILE" else "HOME"
out.var = { env.var(name) }
local missing, why = env.var("_NEOC_DEFINITELY_MISSING_XYZ_42")
out.var_missing = { missing == nil, why ~= nil }
out.vars = env.vars()[name] == env.var(name)
for key, value in env.vars() do
	assert(type(key) == "string" and type(value) == "string")
end

out.args_none = #env.args()

return out

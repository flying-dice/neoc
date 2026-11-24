--!strict
-- std:env as a script sees it: its answers, then what it asserts. The answers are a file per platform (env.unix.json,
-- env.windows.json).
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

-- what it asserts
do
  local value, missing = env.var("_NEOC_DEFINITELY_MISSING_XYZ_42")
  assert(
    value == nil
      and missing ~= nil
      and missing.kind == "not_present"
      and missing.name == "_NEOC_DEFINITELY_MISSING_XYZ_42"
  )
  assert(missing.op == "std:env.var")
  assert(
    tostring(missing) == "std:env.var: not_present: _NEOC_DEFINITELY_MISSING_XYZ_42: environment variable not found"
  )

  -- a wrong argument is refused with its place
  local ok, refused = pcall(function()
    env.var(42 :: any) -- deliberate refusal test: a number is not a name
  end)
  assert(not ok and tostring(refused):find("std:env.var: argument #1 'key'", 1, true))

  -- the module table is read-only
  assert(not pcall(function()
    (env :: any).os = "other" -- deliberate refusal test: a write to the module
  end))
end

return out

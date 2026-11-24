--!strict
-- lib:compile as a script sees it: its answers (compile.json), then what it asserts. A successful bundle copies the
-- running binary (hundreds of megabytes for a test binary), so only the failures run here; the bundle itself is covered
-- by the module's Rust tests.
local compile = require("lib:compile")
local path = require("std:path")
local env = require("std:env")
local fs = require("std:fs")
local random = require("lib:random")

local out = {}

local dir = path.join(env.temp_dir(), "neoc-scripts-compile-" .. random.uuid_v4())
local output = path.join(dir, "app")
local value, err = compile.bundle { entrypoint = path.join(dir, "nope.lua"), output = output }
-- a failure is `(nil, err)`, and nothing was written
out.missing_entrypoint = { value == nil, err ~= nil, fs.exists(output) } :: { any }

-- both fields are required
out.wrong_arguments = {
  (pcall(compile.bundle :: any, { output = output })),
  (pcall(compile.bundle :: any, { entrypoint = "main.lua" })),
  (pcall(compile.bundle :: any, { entrypoint = "main.lua", output = {} })),
  (pcall(compile.bundle :: any)),
}

-- what it asserts
do
  local dir = path.join(env.temp_dir(), "neoc-scripts-compile-asserts-" .. random.uuid_v4())
  local entrypoint = path.join(dir, "nope.lua")

  local built, err = compile.bundle { entrypoint = entrypoint, output = path.join(dir, "app") }
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
end

return out

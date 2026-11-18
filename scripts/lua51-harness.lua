-- Loads the shipped Lua 5.1 pieces the way the host does, under a real Lua 5.1 interpreter:
--   lua5.1 scripts/lua51-harness.lua <prelude.lua> <ui.lua> <app.lua>...
-- The prelude runs first in the globals, `@agentos/ui` is the real ui.lua, `@agentos/app` is a stub that accepts any
-- use, then each app bundle's top level runs. Prints "ok <n>" and exits 0, or raises.
-- Plain Lua 5.1 on purpose (no goto, no integer division, no load with an environment).

local function chunk(path)
  local loaded, problem = loadfile(path)
  if not loaded then
    error("cannot load " .. path .. ": " .. tostring(problem), 0)
  end
  return loaded
end

assert(utf8 == nil and table.move == nil and typeof == nil, "this is not a plain Lua 5.1: the prelude would be skipped")
chunk(arg[1])(_G) -- the prelude installs into its `...` argument (the host passes the app's environment)

-- The polyfills do what Lua 5.3 does (a few of the cases the apps rely on; the full comparison is
-- scripts/lua51.test.ts against a native Lua 5.3+).
assert(typeof {} == "table" and typeof(1) == "number")
assert(utf8.len("h\195\169llo") == 5)
assert(utf8.offset("h\195\169llo", 3) == 4)
assert(utf8.len("\255") == nil)
assert(table.concat(table.move({ 1, 2, 3 }, 1, 3, 2), ",") == "1,1,2,3")
assert(table.unpack { 1, 2 } == 1)
assert(table.clone({ 7 })[1] == 7)

local stub = {}
setmetatable(stub, {
  __index = function()
    return stub
  end,
  __call = function()
    return stub
  end,
})
package.loaded["@agentos/app"] = stub
package.loaded["@agentos/ui"] = chunk(arg[2])()
assert(type(package.loaded["@agentos/ui"]) == "table", "ui.lua must return the ui table")

for index = 3, #arg do
  chunk(arg[index])()
end
print("ok " .. (#arg - 2))

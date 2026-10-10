-- The application host, in raw Lua — the entry point and the driver.
--
-- `luajit` has no HTTP or database, so this is a console app: the same
-- ports-and-adapters model as the http-sqlite examples, fitted to the runtime.
-- neoc owns the controllers + the `Store` port; this host implements the port
-- (an in-memory table) and drives, then prints. It imports the controllers from
-- the transpiled neoc library.
--
-- LuaJIT resolves `require` through `package.path` relative to the working
-- directory, not the requiring file, so anchor the generated module's
-- directory on this script's own path.
local here = arg[0]:match("^(.*[/\\])") or "./"
package.path = here .. "../../auto/lua/?.lua;" .. package.path
local app = require("main")

-- The `Store` port, implemented over an in-memory table. neoc's `store.add(b)`
-- lowers to `store:add(b)` (colon → implicit self), so the methods take self.
local store = { items = {} }
function store:add(body)
  table.insert(self.items, body)
  return #self.items
end
function store:count()
  return #self.items
end
function store:at(index) -- neoc is 0-based; Lua tables are 1-based
  return self.items[index + 1]
end

print(app.create(store, "hello"))
print(app.create(store, "world"))
print(app.create(store, "")) -- the neoc business rule
print(app.list(store))

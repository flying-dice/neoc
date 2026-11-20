-- Correct use of the `std:`, `lib:` and `vnd:` modules. `bun run lint:lua` requires this file to check clean against
-- the generated module definitions (types/luals/<namespace>/<name>.lua, through types/luals-plugin.lua); the matching wrong uses are in
-- negative/wrong_module_use.lua. If the module definitions stop loading or stop matching a module's real surface,
-- this file (or that one) fails the gate.
local json = require("lib:json")
local base64 = require("lib:base64")
local hash = require("lib:hash")
local collections = require("std:collections")
local time = require("std:time")
local fs = require("std:fs")
local rusqlite = require("vnd:rusqlite")
local task = require("std:task")

-- a fallible function returns the value, then an error message
local encoded = base64.encode("hello")
local decoded, decode_err = base64.decode(encoded)
if decoded == nil then
  error(decode_err)
end
print(decoded:upper())

-- a resource type has methods
local object = json.object()
object:set("name", "Ada")
local count = object:len()
print(count + 1, object:has("name"), object:keys())

-- a constructor on a module field, and the methods it returns
local map = collections.Map.new()
map:set("k", 1)
print(map:size() + 1)
local counter = collections.Counter.new(10)
print(counter:increment())

-- a type with static members and methods
local timeout = time.Duration.from_secs(5)
print(timeout:as_millis() + 1, timeout:is_zero())

-- the optional argument of a fallible function
local digest, hash_err = hash.hash("sha256", "data")
print(digest and digest:len(), hash_err)
local text, read_err = fs.read("/tmp/x")
print(text, read_err)

-- functions that take any number of trailing arguments: SQL parameters, a JSON path, a task's arguments
local db = rusqlite.open(":memory:")
if db then
  db:execute("CREATE TABLE t (a, b)")
  print(db:execute("INSERT INTO t VALUES (?, ?)", 1, 2))
  print(db:query("SELECT * FROM t WHERE a = ? AND b = ?", 1, 2))
  print(db:query_one("SELECT * FROM t"))
  local statement = db:prepare("SELECT * FROM t WHERE a = ? AND b = ?")
  print(statement:query(1, 2))
end
print(object:get("a", "b", 1))
task.spawn(function(a, b)
  print(a, b)
end, 1, 2)
task.defer(print, "deferred", 1)
task.delay(1, print, "later", 1, 2)

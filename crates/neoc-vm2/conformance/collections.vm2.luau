-- intentional: Map:set(key, nil) removes the key (it stored a null that get gave back as a sentinel), Map:keys() is sorted, a value that is not plain data (a buffer, a function) and a number that is not whole for a Counter are refused as arguments with their place, and Mutex is not ported (with_lock needs an awaitable callback mlua-ir does not have yet).
local collections = require("std:collections")

local map = collections.Map.new()
map:set("k", 1)
map:set("k", nil)
assert(map:get("k") == nil and not map:has("k") and map:size() == 0)

map:set("b", 1)
map:set("c", 2)
map:set("a", 3)
assert(table.concat(map:keys(), ",") == "a,b,c")

local ok, refused = pcall(map.set, map, "buf", { data = buffer.create(2) })
assert(not ok and tostring(refused):find("std:collections.Map:set: argument #2 'value'", 1, true))
assert(not pcall(map.set, map, "f", print))
assert(not pcall(map.set, map, 1, "numeric key"))

local counter_ok, not_whole = pcall(collections.Counter.new, 1.5)
assert(not counter_ok and tostring(not_whole):find("std:collections.Counter.new: argument #1 'initial': expected a whole number", 1, true))
local counter = collections.Counter.new()
assert(not pcall(counter.add, counter, 0.5))
assert(not pcall(counter.set, counter, 2 ^ 63))

assert((collections :: any).Mutex == nil)

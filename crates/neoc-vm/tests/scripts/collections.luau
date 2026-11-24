--!strict
-- std:collections as a script sees it: its answers (collections.json), then what it asserts.
local collections = require("std:collections")
local out = {}

local map = collections.Map.new()
out.empty = { map:size(), map:has("k"), map:get("k") == nil, #map:keys() } :: { any }
map:set("s", "hello")
map:set("n", 42)
map:set("f", 1.5)
map:set("b", false)
map:set("list", { 1, 2, 3 })
map:set("user", { name = "alice", age = 30, tags = { "admin", "ops" } })
out.values = { map:get("s"), map:get("n"), map:get("f"), map:get("b"), map:get("list"), map:get("user") }
local keys = map:keys()
table.sort(keys)
out.keys = keys
out.size = map:size()

-- what `get` gives is a copy
local user = map:get("user")
assert(user).name = "bob"
out.copy = assert(map:get("user")).name

map:set("s", "replaced")
out.replaced = map:get("s")
out.delete = { map:delete("s"), map:delete("s"), map:has("s"), map:get("s") == nil, map:size() } :: { any }
map:clear()
out.cleared = { map:size(), map:has("n") } :: { any }

local other = collections.Map.new()
map:set("k", "mine")
out.independent = other:get("k") == nil

-- a buffer is refused, however deep
local deep = { buffer.create(2) }
for _ = 2, 100 do
  deep = { inner = deep }
end
out.buffer =
  { (pcall(map.set, map, "buf", { data = buffer.create(2) })), (pcall(map.set, map, "buf", deep)), map:has("buf") }

local zero, counter = collections.Counter.new(), collections.Counter.new(42)
out.counter = {
  zero:get(),
  counter:get(),
  zero:increment(),
  zero:increment(),
  zero:get(),
  counter:decrement(),
  counter:add(5),
  counter:add(-2),
}
counter:set(-7)
out.counter_set = counter:get()
counter:set(2 ^ 62)
out.wraps_up = counter:add(2 ^ 62)
counter:set(-2 ^ 63)
out.wraps_down = counter:decrement()

-- what it asserts
do
  local map = collections.Map.new()
  map:set("k", 1)
  map:set("k", nil)
  assert(map:get("k") == nil and not map:has("k") and map:size() == 0)

  map:set("b", 1)
  map:set("c", 2)
  map:set("a", 3)
  assert(table.concat(map:keys(), ",") == "a,b,c")

  local ok, refused = pcall(function(): ...unknown
    map:set("buf", { data = buffer.create(2) })
  end)
  assert(not ok and tostring(refused):find("std:collections.Map:set: argument #2 'value'", 1, true))
  assert(not pcall(map.set, map, "f", print))
  assert(not pcall(map.set, map, 1 :: any, "numeric key")) -- deliberate refusal test: a key is text

  local counter_ok, not_whole = pcall(collections.Counter.new, 1.5)
  assert(
    not counter_ok
      and tostring(not_whole):find(
        "std:collections.Counter.new: argument #1 'initial': expected a whole number",
        1,
        true
      )
  )
  local counter = collections.Counter.new()
  assert(not pcall(counter.add, counter, 0.5))
  assert(not pcall(counter.set, counter, 2 ^ 63))

  assert((collections :: any).Mutex == nil)
end

return out

-- std:collections on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Storing nil, the order of keys, what is refused and `Mutex` differ on purpose: see collections.vm2.luau.
local collections = require("std:collections")
local out = {}

local map = collections.Map.new()
out.empty = { map:size(), map:has("k"), map:get("k") == nil, #map:keys() }
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
user.name = "bob"
out.copy = map:get("user").name

map:set("s", "replaced")
out.replaced = map:get("s")
out.delete = { map:delete("s"), map:delete("s"), map:has("s"), map:get("s") == nil, map:size() }
map:clear()
out.cleared = { map:size(), map:has("n") }

local other = collections.Map.new()
map:set("k", "mine")
out.independent = other:get("k") == nil

-- a buffer is refused, however deep
local deep = { buffer.create(2) }
for _ = 2, 100 do
	deep = { inner = deep }
end
out.buffer = { (pcall(map.set, map, "buf", { data = buffer.create(2) })), (pcall(map.set, map, "buf", deep)), map:has("buf") }

local zero, counter = collections.Counter.new(), collections.Counter.new(42)
out.counter = { zero:get(), counter:get(), zero:increment(), zero:increment(), zero:get(), counter:decrement(), counter:add(5), counter:add(-2) }
counter:set(-7)
out.counter_set = counter:get()
counter:set(2 ^ 62)
out.wraps_up = counter:add(2 ^ 62)
counter:set(-2 ^ 63)
out.wraps_down = counter:decrement()

return out

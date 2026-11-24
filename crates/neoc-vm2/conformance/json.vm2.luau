--!strict
-- intentional: there is no json.null sentinel (a JSON null reads as nil, kind(key) says "null", set(key, nil) writes one, to_table leaves nulls out), get takes one step and a longer path is at({ ... }) (get was variadic), failures are typed JsonError tables that are raised and read back with json.error_of (zero_index, stale, too_far, too_deep), the nesting limit counts the handle's own depth, and a handle nested inside a plain table is refused.
local json = require("lib:json")

-- null without a sentinel
assert((json :: any).null == nil)
local o = json.object()
o:set("gone", nil)
assert(o:get("gone") == nil and o:has("gone") and o:kind("gone") == "null" and o:kind("never") == nil)
-- a list of several types is annotated: the checker takes a literal's first item for the type of all
local items: { any } = { 1, "two", true, { x = 1 } }
o:set("list", items)
-- what a read gives is told apart by its type's name
local list = o:get("list")
assert(typeof(list) == "lib_json_Array")
list:push(nil)
assert(list:len() == 5 and list:get(5) == nil and list:kind(5) == "null" and list:kind(6) == nil)
assert(list:kind(1) == "number" and list:kind(2) == "string" and list:kind(3) == "bool" and list:kind(4) == "object")
assert(o:kind("list") == "array")
assert(o:to_table().gone == nil)

-- paths
assert(o:at({ "list", 4, "x" }) == 1 and o:at({ "list", 9 }) == nil and list:at({ 4, "x" }) == 1)
local ok, extra = pcall(o.get :: any, o, "list", 4) -- deliberate refusal test: an argument too many
assert(not ok and tostring(extra):find("takes 1 argument", 1, true))

-- typed, raised errors
local _, caught = pcall(function(): ...unknown
	list:set(0, "x")
end)
local zero = json.error_of(caught)
assert(zero ~= nil and zero.kind == "zero_index" and zero.op == "lib:json.Array:set")
assert(tostring(caught):find("lib:json.Array:set: zero_index:", 1, true))
assert(json.error_of("not ours") == nil)

local _, far_caught = pcall(function(): ...unknown
	list:set(2 ^ 30, "x")
end)
local far = json.error_of(far_caught)
assert(far ~= nil and far.kind == "too_far" and far.index == 2 ^ 30 and far.length == 5 and far.max_gap == json.MAX_ARRAY_GAP)

o:set("list", "replaced")
local _, stale_caught = pcall(function(): ...unknown
	list:push(1)
end)
local stale = json.error_of(stale_caught)
assert(stale ~= nil and stale.kind == "stale" and stale.expected == "array" and list:len() == 0)

-- the nesting limit counts where the handle is
local cursor = json.array()
local levels = 1
while pcall(cursor.push, cursor, json.array()) do
	local inner = cursor:get(1)
	assert(typeof(inner) == "lib_json_Array")
	cursor = inner
	levels += 1
end
assert(levels == json.MAX_DEPTH + 1)
local _, deep_caught = pcall(function(): ...unknown
	cursor:push(json.array())
end)
local deep = json.error_of(deep_caught)
assert(deep ~= nil and deep.kind == "too_deep" and deep.max_depth == json.MAX_DEPTH)

-- arguments are checked by their types
local refused_ok, refused = pcall(list.get, list, -1)
assert(not refused_ok and tostring(refused):find("lib:json.Array:get: argument #1 'index'", 1, true))
assert(not pcall(list.get, list, 1.5))
local nested_ok, nested = pcall(function(): ...unknown
	o:set("x", { json.array() })
end)
assert(not nested_ok and tostring(nested):find("lib:json.Object:set: argument #2 'value'", 1, true))

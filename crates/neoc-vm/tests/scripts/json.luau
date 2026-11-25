--!strict
-- lib:json as a script sees it: its answers (json.json), then what it asserts.
local json = require("lib:json")
local out = {}

local o = json.object()
o:set("name", "ada")
o:set("age", 36)
o:set("ratio", 0.5)
o:set("active", false)
o:set("empty", "")
o:set("tags", { "a", "b" })
o:set("address", { city = "London", geo = { lat = 51.5 } })
out.scalars = { o:get("name"), o:get("age"), o:get("ratio"), o:get("active"), o:get("empty") }
out.missing = o:get("nope") == nil
out.has = { o:has("name"), o:has("nope") }
out.keys = o:keys()
out.len = o:len()

-- containers come out as live handles
local tags = o:get("tags") :: lib_json_Array
tags:push("c")
tags:set(1, "z")
out.tags =
  { tags:len(), tags:get(1), tags:get(3), (o:get("tags") :: lib_json_Array):get(3), tags:get(4) == nil } :: { any }
local address = o:get("address") :: lib_json_Object
address:set("city", "Paris")
out.address = {
  (o:get("address") :: lib_json_Object):get("city"),
  (address:get("geo") :: lib_json_Object):get("lat"),
  address:len(),
}
(address:get("geo") :: lib_json_Object):set("lon", -0.1)
out.deep_write = ((o:get("address") :: lib_json_Object):get("geo") :: lib_json_Object):get("lon")

-- overwriting changes what a key holds
o:set("age", "thirty-six")
out.overwritten = o:get("age")

-- a handle given as a value is copied in
local inner = json.array()
inner:push(1)
local holder = json.object()
holder:set("inner", inner)
inner:push(2)
out.copied = { (holder:get("inner") :: lib_json_Array):len(), inner:len() }

-- a handle whose place was retyped reads as empty
local root = json.object()
root:set("a", json.object())
local stale = root:get("a") :: lib_json_Object
root:set("a", { 10, 20, 30 })
out.stale = { stale:len(), #stale:keys(), stale:has("x"), (root:get("a") :: lib_json_Array):len() } :: { any }

-- plain tables
local t = o:to_table()
out.to_table = { t.name, t.tags[1], #t.tags, t.address.geo.lat, t.active }
local list = (o:get("tags") :: lib_json_Array):to_table()
out.list = list

-- an array pads a small gap; an empty table is an object
local a = json.array()
a:push(1)
a:set(3, "x")
a:push {}
out.array =
  { a:len(), a:get(1), a:get(3), (a:get(4) :: lib_json_Object):len(), #(a:get(4) :: lib_json_Object):keys() } :: { any }

-- refusals: both raise
local function nested(depth)
  local v = {}
  for _ = 2, depth do
    v = { v }
  end
  return v
end
local cyclic = {}
cyclic[1] = cyclic
local function ok(...)
  return (pcall(...))
end
out.refused = {
  ok(a.set, a, 0, "x"),
  ok(a.get, a, 0),
  ok(a.get, a, -1),
  ok(a.set, a, 2 ^ 40, "x"),
  ok(a.push, a, nested(128)),
  ok(a.push, a, cyclic),
  ok(a.push, a, print),
}
local fresh = json.array()
out.accepted = ok(fresh.push, fresh, nested(127))

-- what it asserts
do
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
  assert(o:at { "list", 4, "x" } == 1 and o:at { "list", 9 } == nil and list:at { 4, "x" } == 1)
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
  assert(
    far ~= nil
      and far.kind == "too_far"
      and far.index == 2 ^ 30
      and far.length == 5
      and far.max_gap == json.MAX_ARRAY_GAP
  )

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
end

return out

-- lib:json on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- JSON null, paths longer than one step and the errors differ on purpose: see json.vm2.luau.
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
local tags = o:get("tags")
tags:push("c")
tags:set(1, "z")
out.tags = { tags:len(), tags:get(1), tags:get(3), o:get("tags"):get(3), tags:get(4) == nil }
local address = o:get("address")
address:set("city", "Paris")
out.address = { o:get("address"):get("city"), address:get("geo"):get("lat"), address:len() }
address:get("geo"):set("lon", -0.1)
out.deep_write = o:get("address"):get("geo"):get("lon")

-- overwriting changes what a key holds
o:set("age", "thirty-six")
out.overwritten = o:get("age")

-- a handle given as a value is copied in
local inner = json.array()
inner:push(1)
local holder = json.object()
holder:set("inner", inner)
inner:push(2)
out.copied = { holder:get("inner"):len(), inner:len() }

-- a handle whose place was retyped reads as empty
local root = json.object()
root:set("a", json.object())
local stale = root:get("a")
root:set("a", { 10, 20, 30 })
out.stale = { stale:len(), #stale:keys(), stale:has("x"), root:get("a"):len() }

-- plain tables
local t = o:to_table()
out.to_table = { t.name, t.tags[1], #t.tags, t.address.geo.lat, t.active }
local list = o:get("tags"):to_table()
out.list = list

-- an array pads a small gap; an empty table is an object
local a = json.array()
a:push(1)
a:set(3, "x")
a:push({})
out.array = { a:len(), a:get(1), a:get(3), a:get(4):len(), #a:get(4):keys() }

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

return out

-- lib:cache on neoc-vm and neoc-vm2: what both must answer the same. Making a cache was reshaped (cache.vm2.luau):
-- neoc-vm takes an options table and neoc-vm2 the size, and each refuses the other's. `new` below is the only
-- place that knows; everything a cache then does is compared. Times to live and on_evict go through the reshaped
-- constructor and are in the Rust tests and cache.vm2.luau.
local cache = require("lib:cache")
local out = {}

local function new(max_size: number)
	local ok, made = pcall(cache.Cache.new, max_size) -- neoc-vm2
	if ok and made ~= nil then
		return made
	end
	return assert(cache.Cache.new({ max_size = max_size })) -- neoc-vm
end

-- set, get, overwrite
local c = new(8)
c:set("k", "v")
out.get = { c:get("k"), c:get("absent") == nil }
c:set("k", "second")
out.overwrite = { c:get("k"), c:stats().evictions }
out.set_returns = select("#", c:set("other", 1))

-- a stored value comes back as it is
local t = { 1, 2, 3 }
local f = function() end
c:set("table", t)
c:set("function", f)
c:set("false", false)
out.identity = { c:get("table") == t, c:get("function") == f, c:get("false") }

-- keys
local keys = new(8)
keys:set("s", "string")
keys:set(1, "one")
keys:set(true, "yes")
keys:set(false, "no")
keys:set(1.5, "float")
keys:set("1", "text one")
keys:set(0, "zero")
out.keys = {
	keys:get("s"), keys:get(1), keys:get(1.0), keys:get(true), keys:get(false), keys:get(1.5), keys:get("1"),
	keys:get(-0), keys:len(),
}
out.key_refused = {
	not pcall(function() return keys:get({}) end),
	not pcall(function() keys:set(function() end, 1) end),
	not pcall(function() return keys:delete(nil) end),
}
out.nil_value_refused = { not pcall(function() keys:set("k", nil) end), keys:get("k") == nil }

-- the least recently used entry goes
local lru = new(2)
lru:set("a", "A")
lru:set("b", "B")
lru:get("a")
lru:set("c", "C")
out.lru = { lru:get("a"), lru:get("b") == nil, lru:get("c"), lru:len(), lru:stats().evictions }

-- delete, clear, len
local d = new(8)
d:set("k", "v")
out.delete = { d:delete("k"), d:delete("k"), d:get("k") == nil }
d:set("a", 1)
d:set("b", 2)
d:get("a")
out.len = d:len()
d:clear()
out.clear = { d:len(), d:stats().hits, d:get("a") == nil }

-- stats
local s = new(1)
s:set("a", 1)
s:get("a")
s:get("a")
s:get("nope")
s:set("b", 2)
local stats = s:stats()
out.stats = { stats.hits, stats.misses, stats.evictions }

out.tostring = { tostring(s), tostring(new(4)) }

return out

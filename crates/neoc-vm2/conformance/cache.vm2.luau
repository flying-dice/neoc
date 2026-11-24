--!strict
-- intentional: Cache.new(max_size, ttl?, on_evict?) replaces Cache.new({ max_size, ttl, on_evict }); a time to live is a std:time Duration (it was seconds) and Duration.zero() is "never expires"; a size or a key of the wrong type is refused as an argument, a size of 0 or past the limit is the typed NewError (out_of_range), storing nil raises the typed SetError (nil_value); stats() is a declared record.
local cache = require("lib:cache")
local time = require("std:time")

-- the size is the first argument
local c = assert(cache.Cache.new(2))
c:set("a", "A")
c:set("b", "B")
assert(c:get("a") == "A")
c:set("c", "C")
assert(c:get("b") == nil and c:len() == 2 and tostring(c) == "Cache(len=2, max_size=2)")

-- a size out of range is a typed error
local none, zero = cache.Cache.new(0)
assert(none == nil and zero ~= nil and zero.kind == "out_of_range" and zero.max_size == 0 and zero.max == 2 ^ 24)
assert(zero.op == "lib:cache.Cache.new")
local _, huge = cache.Cache.new(1000000000)
assert(huge ~= nil and huge.kind == "out_of_range")
assert(tostring(huge) == "lib:cache.Cache.new: out_of_range: max_size must be from 1 to 16777216, got 1000000000")

-- a size that is not a whole number, and the options table that was, are refused as arguments
for _, size in { -1, 1.5, "8", { max_size = 8 } } :: { any } do -- deliberate refusal test: none is a size
	local ok, refused = pcall(function()
		cache.Cache.new(size)
	end)
	assert(not ok and tostring(refused):find("lib:cache.Cache.new: argument #1 'max_size'", 1, true))
end
local ok_ttl, refused_ttl = pcall(function()
	cache.Cache.new(8, 60 :: any) -- deliberate refusal test: seconds are not a Duration
end)
assert(not ok_ttl and tostring(refused_ttl):find("lib:cache.Cache.new: argument #2 'ttl'", 1, true))

-- a time to live is a Duration; zero is "never expires"
local long = assert(cache.Cache.new(8, time.Duration.from_secs(3600)))
long:set("default", 1)
long:set("own", 2, time.Duration.from_secs(60))
long:set("pinned", 3, time.Duration.zero())
assert(long:get("default") == 1 and long:get("own") == 2 and long:get("pinned") == 3)
local ok_set, refused_set = pcall(function(): ...unknown
	long:set("k", "v", 60 :: any) -- deliberate refusal test: seconds are not a Duration
end)
assert(not ok_set and tostring(refused_set):find("lib:cache.Cache:set: argument #3 'ttl'", 1, true))

-- an entry past its time is dropped when it is seen: a miss and an eviction
local seen = {}
local short = assert(cache.Cache.new(8, time.Duration.from_millis(1), function(key, value)
	table.insert(seen, `{key}={value}`)
end))
short:set("gone", "soon")
short:set("kept", "always", time.Duration.zero())
time.sleep(time.Duration.from_millis(20))
assert(short:get("gone") == nil and short:get("kept") == "always")
assert(short:len() == 1 and seen[1] == "gone=soon" and #seen == 1)
local stats = short:stats()
assert(stats.hits == 1 and stats.misses == 1 and stats.evictions == 1)

-- on_evict hears of the entry pushed out, with the lock released: it may use the cache
local evicted: { string } = {}
local small
small = assert(cache.Cache.new(1, nil, function(key, value)
	table.insert(evicted, `{key}={value} len={small:len()}`)
end))
small:set("a", "A")
small:set("a", "A2")
small:delete("a")
small:set(1, "one")
small:set(true, "yes")
small:clear()
assert(#evicted == 1 and evicted[1] == "1=one len=1")

-- keys and values are refused as arguments, with their place
local ok_key, refused_key = pcall(c.get, c, {} :: any) -- deliberate refusal test: a table is not a key
assert(not ok_key and tostring(refused_key):find("lib:cache.Cache:get: argument #1 'key'", 1, true))
local ok_nil, refused_nil = pcall(function(): ...unknown
	c:set("k", nil)
end)
assert(not ok_nil and tostring(refused_nil):find("lib:cache.Cache:set: nil_value: ", 1, true))

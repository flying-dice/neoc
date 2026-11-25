--!strict
-- lib:cache as a script sees it: its answers (cache.json), then what it asserts.
local cache = require("lib:cache")
local out = {}

local function new(max_size: number)
  return assert(cache.Cache.new(max_size))
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
out.identity = { c:get("table") == t, c:get("function") == f, c:get("false") } :: { any }

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
  keys:get("s"),
  keys:get(1),
  keys:get(1.0),
  keys:get(true),
  keys:get(false),
  keys:get(1.5),
  keys:get("1"),
  keys:get(-0),
  keys:len(),
}
out.key_refused = {
  not pcall(function()
    return keys:get({} :: any)
  end),
  not pcall(function()
    keys:set(function() end :: any, 1)
  end),
  not pcall(function()
    return keys:delete(nil :: any)
  end),
}
out.nil_value_refused = { not pcall(function()
  keys:set("k", nil)
end), keys:get("k") == nil }

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
out.clear = { d:len(), d:stats().hits, d:get("a") == nil } :: { any }

-- stats
local s = new(1)
s:set("a", 1)
s:get("a")
s:get("a")
s:get("nope")
s:set("b", 2)
local stats = s:stats()
out.stats = { stats.hits, stats.misses, stats.evictions }

out.tostring = { tostring(s), tostring((new(4))) }

-- what it asserts
do
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
end

return out

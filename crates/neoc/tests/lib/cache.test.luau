local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local cache = require("lib:cache")

describe("cache", function()

    -- Cache.new -------------------------------------------------------------

    describe("Cache.new", function()
        test("builds a usable cache from max_size", function()
            local c, err = cache.Cache.new({ max_size = 8 })
            expect(err).toBeNil()
            expect(c).toBeTruthy()
        end)

        test("missing max_size returns (nil, err)", function()
            local c, err = cache.Cache.new({})
            expect(c).toBeNil()
            expect(err).toContain("Cache.new:")
            expect(err).toContain("max_size is required")
        end)

        test("max_size below 1 returns (nil, err)", function()
            local c, err = cache.Cache.new({ max_size = 0 })
            expect(c).toBeNil()
            expect(err).toContain("max_size must be at least 1")
        end)

        test("max_size above the ceiling returns (nil, err)", function()
            -- A value large enough to overflow the backing index must reach the
            -- (nil, err) channel, not panic in hashbrown.
            local c, err = cache.Cache.new({ max_size = 1000000000000000000 })
            expect(c).toBeNil()
            expect(err).toContain("max_size must be at most")
        end)
    end)

    -- get / set -------------------------------------------------------------

    describe("get and set", function()
        test("a stored value reads back", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("k", "v")
            expect(c:get("k")).toBe("v")
        end)

        test("a miss returns nil", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            expect(c:get("absent")).toBeNil()
        end)

        test("setting an existing key overwrites it", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("k", "first")
            c:set("k", "second")
            expect(c:get("k")).toBe("second")
            expect(c:stats().evictions).toBe(0)
        end)

        test("string, number, and boolean keys all work", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("s", "str")
            c:set(42, "num")
            c:set(true, "bool")
            expect(c:get("s")).toBe("str")
            expect(c:get(42)).toBe("num")
            expect(c:get(true)).toBe("bool")
        end)

        test("an integer and its float spelling are the same key", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set(1, "one")
            expect(c:get(1.0)).toBe("one")
        end)

        test("an unhashable key throws a prefixed error", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            local ok, err = pcall(function() return c:get({}) end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("cache.get:")
            expect(tostring(err)).toContain("must be a string, number, or boolean")
        end)

        test("a nil value throws rather than corrupting miss semantics", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            local ok, err = pcall(function() c:set("k", nil) end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("cache.set:")
            expect(tostring(err)).toContain("must not be nil")
        end)
    end)

    -- eviction --------------------------------------------------------------

    describe("LRU eviction", function()
        test("the least-recently-used entry is evicted at capacity", function()
            local c = assert(cache.Cache.new({ max_size = 2 }))
            c:set("a", "A")
            c:set("b", "B")
            c:get("a")          -- a becomes most-recently-used
            c:set("c", "C")     -- evicts b, the untouched entry
            expect(c:get("a")).toBe("A")
            expect(c:get("b")).toBeNil()
            expect(c:get("c")).toBe("C")
        end)
    end)

    -- ttl -------------------------------------------------------------------

    describe("ttl", function()
        test("a far-future ttl keeps the entry live", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("k", "v", 3600)
            expect(c:get("k")).toBe("v")
        end)

        test("a non-positive per-entry ttl pins past the cache default", function()
            -- Default expiry is tiny, but ttl <= 0 overrides it to "never".
            local c = assert(cache.Cache.new({ max_size = 8, ttl = 0.001 }))
            c:set("pinned", "v", -1)
            expect(c:get("pinned")).toBe("v")
        end)

        test("an overflowing ttl never expires instead of crashing", function()
            -- A ttl past what a Duration can hold folds to "no expiry" — like
            -- inf/NaN — rather than panicking, via per-call and default ttl.
            local perCall = assert(cache.Cache.new({ max_size = 8 }))
            perCall:set("k", "v", 2e19)
            expect(perCall:get("k")).toBe("v")

            local byDefault = assert(cache.Cache.new({ max_size = 8, ttl = 2e19 }))
            byDefault:set("k", "v")
            expect(byDefault:get("k")).toBe("v")
        end)
    end)

    -- delete / clear --------------------------------------------------------

    describe("delete", function()
        test("reports presence and removes the entry", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("k", "v")
            expect(c:delete("k")).toBe(true)
            expect(c:delete("k")).toBe(false)
            expect(c:get("k")).toBeNil()
        end)
    end)

    describe("clear", function()
        test("empties the cache but preserves statistics", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            c:set("a", 1)
            c:get("a")
            c:clear()
            expect(c:len()).toBe(0)
            expect(c:stats().hits).toBe(1)
        end)
    end)

    -- len -------------------------------------------------------------------

    describe("len", function()
        test("counts live entries", function()
            local c = assert(cache.Cache.new({ max_size = 8 }))
            expect(c:len()).toBe(0)
            c:set("a", 1)
            c:set("b", 2)
            expect(c:len()).toBe(2)
        end)
    end)

    -- stats -----------------------------------------------------------------

    describe("stats", function()
        test("tracks hits, misses, and evictions", function()
            local c = assert(cache.Cache.new({ max_size = 1 }))
            c:set("a", 1)
            c:get("a")          -- hit
            c:get("a")          -- hit
            c:get("nope")       -- miss
            c:set("b", 2)       -- evicts a
            local s = c:stats()
            expect(s.hits).toBe(2)
            expect(s.misses).toBe(1)
            expect(s.evictions).toBe(1)
        end)
    end)

    -- on_evict --------------------------------------------------------------

    describe("on_evict", function()
        test("fires on capacity eviction with (key, value)", function()
            local seen = {}
            local c = assert(cache.Cache.new({
                max_size = 1,
                on_evict = function(k, v) seen[k] = v end,
            }))
            c:set("a", "A")
            c:set("b", "B")     -- evicts a
            expect(seen.a).toBe("A")
        end)

        test("does not fire on delete, clear, or overwrite", function()
            local count = 0
            local c = assert(cache.Cache.new({
                max_size = 4,
                on_evict = function() count = count + 1 end,
            }))
            c:set("a", "A")
            c:set("a", "A2")    -- overwrite
            c:delete("a")       -- delete
            c:set("b", "B")
            c:clear()           -- clear
            expect(count).toBe(0)
        end)
    end)

    -- tostring --------------------------------------------------------------

    describe("tostring", function()
        test("reports length and capacity", function()
            local c = assert(cache.Cache.new({ max_size = 4 }))
            c:set("a", 1)
            expect(tostring(c)).toBe("Cache(len=1, max_size=4)")
        end)
    end)
end)

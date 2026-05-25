local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local collections = require("std:collections")

local Map     = collections.Map
local Counter = collections.Counter
local Mutex   = collections.Mutex

describe("collections", function()

    -- map ----------------------------------------------------------------------

    describe("Map", function()
        test("set/get round-trips a string", function()
            local m = Map.new()
            m:set("k", "hello")
            expect(m:get("k")).toBe("hello")
        end)

        test("set/get round-trips a number", function()
            local m = Map.new()
            m:set("k", 42)
            expect(m:get("k")).toBe(42)
        end)

        test("set/get round-trips a boolean", function()
            local m = Map.new()
            m:set("k", true)
            expect(m:get("k")).toBe(true)
        end)

        test("set/get round-trips a nested table", function()
            local m = Map.new()
            m:set("user", { name = "alice", age = 30, tags = { "admin", "ops" } })
            local u = m:get("user")
            expect(u.name).toBe("alice")
            expect(u.age).toBe(30)
            expect(u.tags[1]).toBe("admin")
            expect(u.tags[2]).toBe("ops")
        end)

        test("get returns nil for missing key", function()
            local m = Map.new()
            expect(m:get("nope")).toBeNil()
        end)

        test("delete removes entry and reports prior presence", function()
            local m = Map.new()
            m:set("k", "v")
            expect(m:delete("k")).toBe(true)
            expect(m:get("k")).toBeNil()
            expect(m:delete("k")).toBe(false)
        end)

        test("has reflects presence", function()
            local m = Map.new()
            expect(m:has("k")).toBe(false)
            m:set("k", 1)
            expect(m:has("k")).toBe(true)
        end)

        test("size reflects entry count", function()
            local m = Map.new()
            expect(m:size()).toBe(0)
            m:set("a", 1); m:set("b", 2)
            expect(m:size()).toBe(2)
        end)

        test("keys returns array of keys", function()
            local m = Map.new()
            m:set("a", 1); m:set("b", 2)
            local keys = m:keys()
            expect(#keys).toBe(2)
        end)

        test("constructed maps are independent unless shared", function()
            local m1 = Map.new()
            local m2 = Map.new()
            m1:set("k", "from_a")
            expect(m2:get("k")).toBeNil()
        end)
    end)

    -- counter ------------------------------------------------------------------

    describe("Counter", function()
        test("new() creates a zero counter", function()
            local c = Counter.new()
            expect(c:get()).toBe(0)
        end)

        test("new(initial) starts at the given value", function()
            local c = Counter.new(42)
            expect(c:get()).toBe(42)
        end)

        test("increment returns new value", function()
            local c = Counter.new()
            expect(c:increment()).toBe(1)
            expect(c:increment()).toBe(2)
            expect(c:get()).toBe(2)
        end)

        test("decrement returns new value", function()
            local c = Counter.new(10)
            expect(c:decrement()).toBe(9)
            expect(c:get()).toBe(9)
        end)

        test("add accepts arbitrary signed delta", function()
            local c = Counter.new()
            expect(c:add(5)).toBe(5)
            expect(c:add(-2)).toBe(3)
        end)
    end)

    -- mutex --------------------------------------------------------------------

    describe("Mutex", function()
        test("with_lock runs the body and returns its value", function()
            local m = Mutex.new()
            local result = m:with_lock(function() return "ok" end)
            expect(result).toBe("ok")
        end)

        test("with_lock serializes a counter increment", function()
            local mu = Mutex.new()
            local c  = Counter.new()
            for _ = 1, 10 do
                mu:with_lock(function() c:increment() end)
            end
            expect(c:get()).toBe(10)
        end)
    end)
end)

-- TcpListener.bind -----------------------------------------------------------

describe("TcpListener.bind", function()
    local TcpListener = require("std:net").TcpListener

    test("binds and returns a listener with addr", function()
        local l = TcpListener.bind("127.0.0.1:0")
        local addr = l:addr()
        expect(addr).toContain("127.0.0.1:")
    end)
end)

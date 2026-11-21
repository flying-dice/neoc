local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local collections = require("std:collections")
local thread = require("std:thread")

local Map     = collections.Map
local Counter = collections.Counter


describe("thread", function()

    -- spawn + await ------------------------------------------------------------

    describe("spawn + await", function()
        test("returns a value via :await()", function()
            local w, err = thread.spawn({
                source = "local args = ...; return args.v * 2",
                args   = { v = 21 },
            })
            expect(err).toBeNil()
            local result, aerr = w:await()
            expect(aerr).toBeNil()
            expect(result).toBe(42)
        end)

        test("works with no args", function()
            local w = thread.spawn({ source = "return 'ok'" })
            expect(w:await()).toBe("ok")
        end)

        test("missing source/file returns error", function()
            local w, err = thread.spawn({})
            expect(w).toBeNil()
            expect(err).toContain("thread.spawn:")
        end)

        test("worker errors propagate via :await()", function()
            local w = thread.spawn({ source = "error('boom')" })
            local r, err = w:await()
            expect(r).toBeNil()
            expect(err).toContain("boom")
        end)

        test("spawn from file with bidirectional messaging", function()
            local w = thread.spawn({
                file = "crates/neoc/tests/std/thread_worker.lua",
                args = { n = 7 },
            })
            -- Worker sends a greeting, waits for "ack", replies, then returns.
            expect(w:receive()).toBe("hello from file worker")
            w:send("ack")
            expect(w:receive()).toBe("acknowledged")
            expect(w:await()).toBe(49)
        end)

        test("missing file returns clear error", function()
            local w, err = thread.spawn({ file = "tests/__does_not_exist.lua" })
            expect(w).toBeNil()
            expect(err).toContain("read tests/__does_not_exist.lua:")
        end)

        test("workers have full lib access and shared state", function()
            local m = Map.new()

            local w = thread.spawn({
                source = [[
                    local base64 = require("lib:base64")
                    local shared = (require("std:thread")).shared
                    shared.m:set("k", "from-worker")
                    return base64.encode("hi")
                ]],
                shared = { m = m },
            })
            local r = w:await()
            expect(r).toBe("aGk=")
            expect(m:get("k")).toBe("from-worker")
        end)
    end)

    -- messaging ----------------------------------------------------------------

    describe("messaging", function()
        test("worker can send, parent can receive", function()
            local w = thread.spawn({
                source = [[
                    local parent = (require("std:thread")).parent
                    parent:send("hello")
                    parent:send("world")
                    return "done"
                ]],
            })
            local m1 = w:receive()
            local m2 = w:receive()
            expect(m1).toBe("hello")
            expect(m2).toBe("world")
            expect(w:await()).toBe("done")
        end)

        test("parent can send, worker can receive", function()
            local w = thread.spawn({
                source = [[
                    local parent = (require("std:thread")).parent
                    local msg = parent:receive()
                    return msg .. "!"
                ]],
            })
            w:send("hi")
            expect(w:await()).toBe("hi!")
        end)

        test("bidirectional ping-pong", function()
            local w = thread.spawn({
                source = [[
                    local parent = (require("std:thread")).parent
                    parent:send("ping")
                    local pong = parent:receive()
                    return pong .. "!"
                ]],
            })
            expect(w:receive()).toBe("ping")
            w:send("pong")
            expect(w:await()).toBe("pong!")
        end)

        test("messages can be nested tables", function()
            local w = thread.spawn({
                source = [[
                    local parent = (require("std:thread")).parent
                    local msg = parent:receive()
                    return {
                        received = msg.text,
                        double   = msg.n * 2,
                        tags     = msg.tags,
                    }
                ]],
            })
            w:send({ text = "hi", n = 21, tags = { "a", "b" } })
            local r = w:await()
            expect(r.received).toBe("hi")
            expect(r.double).toBe(42)
            expect(r.tags[1]).toBe("a")
            expect(r.tags[2]).toBe("b")
        end)
    end)

    -- lifecycle ----------------------------------------------------------------

    describe("lifecycle", function()
        test("terminate aborts a worker stuck on receive", function()
            local w = thread.spawn({
                source = [[
                    local parent = (require("std:thread")).parent
                    parent:receive()        -- never sent
                    return "should-not-reach"
                ]],
            })
            w:terminate()
            local r, err = w:await()
            expect(r).toBeNil()
            expect(err).toContain("terminated")
        end)

        test("await twice returns error on second call", function()
            local w = thread.spawn({ source = "return 1" })
            w:await()
            local r, err = w:await()
            expect(r).toBeNil()
            expect(err).toContain("already awaited")
        end)

        test("send to an exited worker errors clearly", function()
            local w = thread.spawn({ source = "return 1" })
            w:await()
            local ok, err = pcall(function() w:send("late") end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("worker has exited")
        end)
    end)

    -- parent global ------------------------------------------------------------

    describe("parent", function()
        test("thread.parent is nil at top level", function()
            expect(thread.parent).toBeNil()
        end)
    end)

    -- shared (spawn-time handoff) ---------------------------------------------

    describe("shared", function()
        test("thread.shared is nil at top level", function()
            expect(thread.shared).toBeNil()
        end)

        test("map passed via shared is visible in worker", function()
            local m = Map.new()
            m:set("origin", "parent")

            local w = thread.spawn({
                source = [[
                    local shared = (require("std:thread")).shared
                    shared.m:set("from_worker", "hello")
                    return shared.m:get("origin")
                ]],
                shared = { m = m },
            })

            local worker_saw = w:await()
            expect(worker_saw).toBe("parent")
            expect(m:get("from_worker")).toBe("hello")
        end)

        test("two threads increment a shared counter to 2", function()
            local c = Counter.new(0)

            local w1 = thread.spawn({
                source = [[
                    local shared = (require("std:thread")).shared
                    shared.counter:increment()
                ]],
                shared = { counter = c },
            })
            local w2 = thread.spawn({
                source = [[
                    local shared = (require("std:thread")).shared
                    shared.counter:increment()
                ]],
                shared = { counter = c },
            })

            w1:await()
            w2:await()
            expect(c:get()).toBe(2)
        end)

        test("worker writes to shared map, parent reads it back", function()
            local m = Map.new()

            local w = thread.spawn({
                source = [[
                    local shared = (require("std:thread")).shared
                    shared.state:set("status", "done")
                    shared.state:set("count", 42)
                ]],
                shared = { state = m },
            })

            w:await()
            expect(m:get("status")).toBe("done")
            expect(m:get("count")).toBe(42)
        end)

        test("four threads each increment, counter reaches 4", function()
            local c = Counter.new(0)
            local workers = {}

            for i = 1, 4 do
                workers[i] = thread.spawn({
                    source = [[
                        local shared = (require("std:thread")).shared
                        shared.c:increment()
                    ]],
                    shared = { c = c },
                })
            end

            for i = 1, 4 do
                workers[i]:await()
            end
            expect(c:get()).toBe(4)
        end)

        test("non-Sendable userdata in shared errors at spawn", function()
            local json = require("lib:json")
            local w, err = thread.spawn({
                source = "return 1",
                shared = { obj = json.object() },
            })
            expect(w).toBeNil()
            expect(err).toContain("not Sendable")
        end)

        test("plain table in shared errors at spawn", function()
            local w, err = thread.spawn({
                source = "return 1",
                shared = { t = {} },
            })
            expect(w).toBeNil()
            expect(err).toContain("not Sendable")
        end)

        test("empty shared is valid", function()
            local w = thread.spawn({
                source = [[
                    return (require("std:thread")).shared == nil
                ]],
                shared = {},
            })
            expect(w:await()).toBe(true)
        end)

        test("omitting shared is backward compatible", function()
            local w = thread.spawn({ source = "return 42" })
            expect(w:await()).toBe(42)
        end)
    end)

    -- available_parallelism -------------------------------------------------------

    describe("available_parallelism", function()
        test("returns a positive integer", function()
            local n = thread.available_parallelism()
            expect(type(n)).toBe("number")
            expect(n >= 1).toBe(true)
            expect(math.floor(n) == n).toBe(true)
        end)
    end)

    -- a buffer (refused, never sent as an array of numbers) --------------------

    describe("a buffer", function()
        local refused = "a buffer cannot be serialized; convert it explicitly first"

        test("spawn refuses one in its args", function()
            local ok, err = pcall(thread.spawn, { source = "return 1", args = { buffer.create(2) } })
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("thread.spawn: " .. refused)
        end)

        test("send refuses one to a worker, and a worker's send refuses one to its parent", function()
            local echo = assert(thread.spawn({ source = [[
                local parent = require("std:thread").parent
                local ok, err = pcall(parent.send, parent, { buffer.create(2) })
                parent:send(tostring(err))
            ]] }))
            local ok, err = pcall(echo.send, echo, buffer.create(2))
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("thread.send: " .. refused)
            expect(echo:receive()).toContain("channel.send: " .. refused)
            echo:await()
        end)

        test("await gives (nil, err) for a worker that returns one", function()
            local returner = assert(thread.spawn({ source = "return { buffer.create(2) }" }))
            local value, err = returner:await()
            expect(value).toBeNil()
            expect(err).toContain("thread.await: " .. refused)
        end)
    end)
end)

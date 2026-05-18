--!strict
local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local task = require("std:task")

describe("std:task", function()

    -- task.wait ----------------------------------------------------------------

    describe("task.wait", function()
        test("returns elapsed >= requested", function()
            local h = task.spawn(function()
                local start = os.clock()
                local elapsed = task.wait(0.05)
                expect(elapsed >= 0.05).toBe(true)
                expect(elapsed < 0.15).toBe(true)  -- soft: CI jitter 100ms budget
                expect(os.clock() - start >= 0.05).toBe(true)
            end)
            h:await()
        end)

        test("wait(0) yields to next tick", function()
            local order = {}
            local h = task.spawn(function()
                table.insert(order, "task_before_wait")
                task.wait(0)
                table.insert(order, "task_after_wait")
            end)
            h:await()
            expect(order[1]).toBe("task_before_wait")
            expect(order[2]).toBe("task_after_wait")
        end)

        test("negative duration clamped to 0", function()
            local h = task.spawn(function()
                local elapsed = task.wait(-1)
                expect(elapsed >= 0).toBe(true)
            end)
            h:await()
        end)

        test("returns a number", function()
            local h = task.spawn(function()
                local e = task.wait(0)
                expect(type(e)).toBe("number")
            end)
            h:await()
        end)
    end)

    -- task.spawn ---------------------------------------------------------------

    describe("task.spawn", function()
        test("returns a Task handle", function()
            local h = task.spawn(function() end)
            expect(type(h)).toBe("userdata")
        end)

        test("spawned function executes", function()
            local ran = false
            local h = task.spawn(function() ran = true end)
            h:await()
            expect(ran).toBe(true)
        end)

        test("spawn executes before caller resumes (ordering)", function()
            local order = {}
            local h = task.spawn(function()
                table.insert(order, "spawned")
            end)
            table.insert(order, "caller")
            h:await()
            expect(order[1]).toBe("spawned")
            expect(order[2]).toBe("caller")
        end)

        test("varargs: 0 args", function()
            local h = task.spawn(function(...)
                expect(select("#", ...)).toBe(0)
            end)
            h:await()
        end)

        test("varargs: N args forwarded correctly", function()
            local h = task.spawn(function(a, b, c)
                expect(a).toBe(1)
                expect(b).toBe(2)
                expect(c).toBe(3)
            end, 1, 2, 3)
            h:await()
        end)

        test("varargs with nil holes preserves count", function()
            local h = task.spawn(function(a, b, c)
                expect(a).toBe(1)
                expect(b).toBeNil()
                expect(c).toBe(3)
                expect(select("#", a, b, c)).toBe(3)
            end, 1, nil, 3)
            h:await()
        end)

        test("spawn dead coroutine errors", function()
            local co = coroutine.create(function() end)
            coroutine.resume(co)  -- run to completion — now dead
            local ok, err = pcall(function()
                task.spawn(co)
            end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("dead coroutine")
        end)

        test("multiple concurrent spawns complete", function()
            local count = 0
            local handles = {}
            for i = 1, 10 do
                handles[i] = task.spawn(function()
                    task.wait(0.01)
                    count = count + 1
                end)
            end
            for _, h in ipairs(handles) do
                h:await()
            end
            expect(count).toBe(10)
        end)
    end)

    -- task.defer ---------------------------------------------------------------

    describe("task.defer", function()
        test("returns a Task handle", function()
            local h = task.defer(function() end)
            expect(type(h)).toBe("userdata")
            h:await()
        end)

        test("deferred function executes after current batch", function()
            local order = {}
            local dh = task.defer(function()
                table.insert(order, "deferred")
            end)
            table.insert(order, "immediate")
            dh:await()
            expect(order[1]).toBe("immediate")
            expect(order[2]).toBe("deferred")
        end)

        test("multiple defers execute in FIFO order", function()
            local order = {}
            local h1 = task.defer(function() table.insert(order, 1) end)
            local h2 = task.defer(function() table.insert(order, 2) end)
            local h3 = task.defer(function() table.insert(order, 3) end)
            h1:await(); h2:await(); h3:await()
            expect(order[1]).toBe(1)
            expect(order[2]).toBe(2)
            expect(order[3]).toBe(3)
        end)

        test("varargs forwarded correctly", function()
            local h = task.defer(function(a, b)
                expect(a).toBe("x")
                expect(b).toBe("y")
            end, "x", "y")
            h:await()
        end)
    end)

    -- task.delay ---------------------------------------------------------------

    describe("task.delay", function()
        test("returns a Task handle", function()
            local h = task.delay(0, function() end)
            expect(type(h)).toBe("userdata")
            h:await()
        end)

        test("delay(0) equivalent to defer", function()
            local order = {}
            local dh = task.delay(0, function()
                table.insert(order, "delayed")
            end)
            table.insert(order, "immediate")
            dh:await()
            expect(order[1]).toBe("immediate")
            expect(order[2]).toBe("delayed")
        end)

        test("delay fires after secs", function()
            local h = task.spawn(function()
                local start = os.clock()
                local dh = task.delay(0.05, function() end)
                dh:await()
                expect(os.clock() - start >= 0.05).toBe(true)
            end)
            h:await()
        end)

        test("varargs forwarded to function", function()
            local h = task.delay(0, function(a)
                expect(a).toBe(42)
            end, 42)
            h:await()
        end)
    end)

    -- task.cancel --------------------------------------------------------------

    describe("task.cancel", function()
        test("cancel prevents pending delay from executing", function()
            local ran = false
            local h = task.delay(0.1, function() ran = true end)
            task.cancel(h)
            task.wait(0.15)  -- must be inside a coroutine; wrap in spawn
            -- Hoist into a spawned task since task.wait needs coroutine context
        end)

        test("cancel a pending delay via flat function", function()
            local ran = false
            local h = task.spawn(function()
                local dh = task.delay(0.2, function() ran = true end)
                task.cancel(dh)
                task.wait(0.05)
            end)
            h:await()
            expect(ran).toBe(false)
        end)

        test("cancel completed task is no-op (returns false)", function()
            local h = task.spawn(function() end)
            h:await()
            local result = h:cancel()
            expect(result).toBe(false)
        end)

        test("cancel cancelled task is idempotent (returns false)", function()
            local h = task.delay(1, function() end)
            local first  = h:cancel()
            local second = h:cancel()
            expect(first).toBe(true)
            expect(second).toBe(false)
        end)

        test("cancel(nil) raises type error", function()
            local ok, err = pcall(function() task.cancel(nil) end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("task.cancel")
        end)

        test("cancel(non-task) raises type error", function()
            local ok, err = pcall(function() task.cancel(42) end)
            expect(ok).toBe(false)
            expect(tostring(err)).toContain("task.cancel")
        end)
    end)

    -- Task handle: is_finished -------------------------------------------------

    describe("handle:is_finished", function()
        test("false before task completes", function()
            local h = task.delay(0.1, function() end)
            expect(h:is_finished()).toBe(false)
            h:cancel()
        end)

        test("true after task completes", function()
            local h = task.spawn(function() end)
            h:await()
            expect(h:is_finished()).toBe(true)
        end)

        test("true after cancel", function()
            local h = task.delay(1, function() end)
            h:cancel()
            expect(h:is_finished()).toBe(true)
        end)
    end)

    -- Task handle: await -------------------------------------------------------

    describe("handle:await", function()
        test("returns task result on success", function()
            local h = task.spawn(function()
                return 42
            end)
            local result = h:await()
            expect(result).toBe(42)
        end)

        test("returns (nil, err) on task error", function()
            local h = task.spawn(function()
                error("boom")
            end)
            local result, err = h:await()
            expect(result).toBeNil()
            expect(err).toContain("boom")
        end)

        test("cancel then await returns (nil, 'cancelled')", function()
            local h = task.delay(1, function() end)
            h:cancel()
            local result, err = h:await()
            expect(result).toBeNil()
            expect(err).toBe("cancelled")
        end)

        test("await already-completed returns cached result", function()
            local h = task.spawn(function() return "done" end)
            local r1 = h:await()
            local r2 = h:await()
            expect(r1).toBe("done")
            expect(r2).toBe("done")
        end)

        test("multiple return values", function()
            local h = task.spawn(function()
                return 1, 2, 3
            end)
            local a, b, c = h:await()
            expect(a).toBe(1)
            expect(b).toBe(2)
            expect(c).toBe(3)
        end)
    end)

    -- Error isolation ----------------------------------------------------------

    describe("error isolation", function()
        test("error in spawned task does not crash scheduler", function()
            local h1 = task.spawn(function() error("boom") end)
            local h2 = task.spawn(function() return "ok" end)
            local _, err = h1:await()
            local r2 = h2:await()
            expect(err).toContain("boom")
            expect(r2).toBe("ok")
        end)
    end)

    -- Stress test --------------------------------------------------------------

    describe("stress", function()
        test("100 concurrent spawns complete without deadlock", function()
            local count = 0
            local handles = {}
            for i = 1, 100 do
                handles[i] = task.spawn(function()
                    task.wait(0)
                    count = count + 1
                end)
            end
            for _, h in ipairs(handles) do
                h:await()
            end
            expect(count).toBe(100)
        end)
    end)

end)

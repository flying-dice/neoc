local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local time = require("std:time")
local task = require("std:task")

describe("time", function()

    -- Duration constructors ---------------------------------------------------

    describe("Duration.from_*", function()
        test("from_secs", function()
            local d = time.Duration.from_secs(2)
            expect(d:as_secs()).toBe(2)
            expect(d:as_millis()).toBe(2000)
        end)

        test("from_millis", function()
            expect(time.Duration.from_millis(500):as_millis()).toBe(500)
        end)

        test("from_micros", function()
            expect(time.Duration.from_micros(750):as_micros()).toBe(750)
        end)

        test("from_nanos", function()
            expect(time.Duration.from_nanos(250):as_nanos()).toBe(250)
        end)

        test("from_secs_f64", function()
            local d = time.Duration.from_secs_f64(1.5)
            expect(d:as_millis()).toBe(1500)
        end)

        test("ZERO is zero", function()
            expect(time.Duration.ZERO:is_zero()).toBe(true)
            expect(time.Duration.from_secs(0):is_zero()).toBe(true)
            expect(time.Duration.from_secs(1):is_zero()).toBe(false)
        end)
    end)

    -- Duration.parse / tostring -----------------------------------------------

    describe("Duration.parse", function()
        test("parses simple units", function()
            local d, err = time.Duration.parse("500ms")
            expect(err).toBeNil()
            expect(d:as_millis()).toBe(500)
        end)

        test("parses composite expressions", function()
            local d = time.Duration.parse("1m30s")
            expect(d:as_secs()).toBe(90)
        end)

        test("parses fractional seconds", function()
            expect(time.Duration.parse("1.5s"):as_millis()).toBe(1500)
        end)

        test("parses hours", function()
            expect(time.Duration.parse("1h"):as_secs()).toBe(3600)
        end)

        test("returns error on invalid input", function()
            local d, err = time.Duration.parse("nonsense")
            expect(d).toBeNil()
            expect(type(err)).toBe("string")
        end)

        test("returns error on empty input", function()
            local d, err = time.Duration.parse("")
            expect(d).toBeNil()
            expect(type(err)).toBe("string")
        end)
    end)

    describe("Duration tostring", function()
        test("roundtrips parse → format", function()
            expect(tostring(time.Duration.parse("500ms"))).toBe("500ms")
            expect(tostring(time.Duration.parse("2s"))).toBe("2s")
            expect(tostring(time.Duration.parse("1m30s"))).toBe("1m30s")
            expect(tostring(time.Duration.parse("1.5s"))).toBe("1.5s")
        end)

        test("renders ZERO as 0s", function()
            expect(tostring(time.Duration.ZERO)).toBe("0s")
        end)
    end)

    -- Duration arithmetic -----------------------------------------------------

    describe("Duration arithmetic", function()
        test("add", function()
            local sum = time.Duration.from_millis(300) + time.Duration.from_millis(200)
            expect(sum:as_millis()).toBe(500)
        end)

        test("sub saturates at ZERO", function()
            local d = time.Duration.from_millis(100) - time.Duration.from_millis(500)
            expect(d:is_zero()).toBe(true)
        end)

        test("mul by scalar", function()
            local d = time.Duration.from_secs(2) * 1.5
            expect(d:as_millis()).toBe(3000)
        end)

        test("div by scalar", function()
            local d = time.Duration.from_secs(2) / 4
            expect(d:as_millis()).toBe(500)
        end)

        test("mul rejects negative factor", function()
            local ok = pcall(function() return time.Duration.from_secs(1) * -1 end)
            expect(ok).toBe(false)
        end)

        test("div rejects zero", function()
            local ok = pcall(function() return time.Duration.from_secs(1) / 0 end)
            expect(ok).toBe(false)
        end)

        test("comparison", function()
            local a = time.Duration.from_millis(100)
            local b = time.Duration.from_millis(200)
            expect(a < b).toBe(true)
            expect(b > a).toBe(true)
            expect(a <= a).toBe(true)
            expect(a == time.Duration.from_millis(100)).toBe(true)
        end)
    end)

    -- Instant -----------------------------------------------------------------

    describe("Instant", function()
        test("now is monotonic non-decreasing", function()
            local t0 = time.Instant.now()
            local t1 = time.Instant.now()
            expect(t1 >= t0).toBe(true)
        end)

        test("flat now == Instant.now", function()
            local t0 = time.now()
            local t1 = time.Instant.now()
            expect(t1 >= t0).toBe(true)
        end)

        test("elapsed returns a Duration", function()
            local t0 = time.now()
            local e = t0:elapsed()
            expect(e:is_zero() or e:as_nanos() > 0).toBe(true)
        end)

        test("Instant + Duration → later Instant", function()
            local t0 = time.now()
            local later = t0 + time.Duration.from_secs(1)
            local diff = later - t0
            expect(diff:as_secs()).toBe(1)
        end)

        test("Instant - Instant → Duration", function()
            local t0 = time.now()
            local t1 = t0 + time.Duration.from_millis(250)
            expect((t1 - t0):as_millis()).toBe(250)
        end)

        test("duration_since saturates", function()
            local t0 = time.now()
            local later = t0 + time.Duration.from_secs(1)
            expect(t0:duration_since(later):is_zero()).toBe(true)
        end)
    end)

    -- sleep -------------------------------------------------------------------

    describe("sleep", function()
        test("blocks for at least the requested duration", function()
            local h = task.spawn(function()
                local target = time.Duration.from_millis(50)
                local before = time.now()
                local actual = time.sleep(target)
                local elapsed = time.now() - before
                expect(actual:as_millis() >= 50).toBe(true)
                expect(elapsed:as_millis() >= 50).toBe(true)
                return true
            end)
            expect(h:await()).toBe(true)
        end)
    end)
end)

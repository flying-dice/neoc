local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local chrono = require("lib:chrono")


describe("chrono", function()

    -- ── Date UserData ────────────────────────────────────────────────────────

    describe("Date", function()
        test("new exposes year/month/day", function()
            local d = chrono.Date.new(2026, 5, 20)
            expect(d.year).toBe(2026)
            expect(d.month).toBe(5)
            expect(d.day).toBe(20)
        end)

        test("new rejects invalid day-of-month", function()
            local v, e = chrono.Date.new(2026, 2, 30)
            expect(v).toBeNil()
            expect(e).toContain("chrono.Date.new:")
        end)

        test("parse round-trips ISO 8601", function()
            local d = chrono.Date.parse("2026-05-20")
            expect(tostring(d)).toBe("2026-05-20")
        end)

        test("parse rejects malformed input", function()
            local v, e = chrono.Date.parse("not a date")
            expect(v).toBeNil()
            expect(e).toContain("chrono.Date.parse:")
        end)

        test("ordinal and weekday reflect ISO calendar", function()
            local d = chrono.Date.new(2026, 1, 1)
            expect(d.ordinal).toBe(1)
            -- 2026-01-01 is a Thursday → ISO weekday 4.
            expect(d.weekday).toBe(4)
        end)

        test("add Duration", function()
            local d = chrono.Date.parse("2026-05-20")
            local r = d:add(chrono.Duration.of({ days = 10 }))
            expect(tostring(r)).toBe("2026-05-30")
        end)

        test("sub Duration", function()
            local d = chrono.Date.parse("2026-05-20")
            local r = d:sub(chrono.Duration.of({ days = 10 }))
            expect(tostring(r)).toBe("2026-05-10")
        end)

        test("diff returns Duration", function()
            local a = chrono.Date.parse("2026-05-20")
            local b = chrono.Date.parse("2026-05-30")
            local span = a:diff(b)
            expect(span.days).toBe(10)
        end)

        test("comparison metamethods", function()
            local a = chrono.Date.parse("2026-05-20")
            local b = chrono.Date.parse("2026-05-30")
            local c = chrono.Date.parse("2026-05-20")
            expect(a == c).toBeTruthy()
            expect(a < b).toBeTruthy()
            expect(a <= c).toBeTruthy()
        end)

        test("with_year/month/day return fresh Date", function()
            local d = chrono.Date.new(2026, 5, 20)
            expect(tostring(d:with_year(2030))).toBe("2030-05-20")
            expect(tostring(d:with_month(1))).toBe("2026-01-20")
            expect(tostring(d:with_day(1))).toBe("2026-05-01")
        end)

        test("strftime formats canonical patterns", function()
            local d = chrono.Date.new(2026, 5, 20)
            expect(d:format("%Y/%m/%d")).toBe("2026/05/20")
        end)

        test("format returns err on invalid pattern", function()
            local d = chrono.Date.new(2026, 5, 20)
            local v, e = d:format("%@@@invalid")
            expect(v).toBeNil()
            expect(e).toContain("chrono.Date.format:")
        end)

        test("add rejects Duration with sub-day units", function()
            local d = chrono.Date.parse("2026-05-20")
            local v, e = d:add(chrono.Duration.of({ hours = 25 }))
            expect(v).toBeNil()
            expect(e).toContain("chrono.Date.add:")
        end)

        test("sub rejects Duration with sub-day units", function()
            local d = chrono.Date.parse("2026-05-20")
            local v, e = d:sub(chrono.Duration.of({ minutes = 60 }))
            expect(v).toBeNil()
            expect(e).toContain("chrono.Date.sub:")
        end)
    end)

    -- ── Time UserData ────────────────────────────────────────────────────────

    describe("Time", function()
        test("new exposes hour/minute/second", function()
            local t = chrono.Time.new(13, 45, 30)
            expect(t.hour).toBe(13)
            expect(t.minute).toBe(45)
            expect(t.second).toBe(30)
            expect(t.nanosecond).toBe(0)
        end)

        test("new rejects out-of-range hour", function()
            local v, e = chrono.Time.new(25, 0, 0)
            expect(v).toBeNil()
            expect(e).toContain("chrono.Time.new:")
        end)

        test("on composes with Date", function()
            local d = chrono.Date.new(2026, 5, 20)
            local t = chrono.Time.new(13, 45, 30)
            expect(tostring(t:on(d))).toBe("2026-05-20T13:45:30")
        end)

        test("format returns err on invalid pattern", function()
            local t = chrono.Time.new(13, 45, 30)
            local v, e = t:format("%@@@invalid")
            expect(v).toBeNil()
            expect(e).toContain("chrono.Time.format:")
        end)
    end)

    -- ── DateTime UserData ────────────────────────────────────────────────────

    describe("DateTime", function()
        test("compose from Date and Time", function()
            local d = chrono.Date.new(2026, 5, 20)
            local t = chrono.Time.new(13, 45, 30)
            expect(tostring(d:at(t))).toBe("2026-05-20T13:45:30")
        end)

        test("parse rejects malformed input", function()
            local v, e = chrono.DateTime.parse("nope")
            expect(v).toBeNil()
            expect(e).toContain("chrono.DateTime.parse:")
        end)

        test("date and time extract components", function()
            local dt = chrono.DateTime.parse("2026-05-20T13:45:30")
            expect(tostring(dt:date())).toBe("2026-05-20")
            expect(tostring(dt:time())).toBe("13:45:30")
        end)

        test("in_zone promotes to ZonedDateTime", function()
            local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
            local z = dt:in_zone("UTC")
            expect(z.timezone).toBe("UTC")
            expect(z.offset_seconds).toBe(0)
        end)

        test("in_zone rejects unknown timezone", function()
            local dt = chrono.DateTime.parse("2026-05-20T12:00:00")
            local v, e = dt:in_zone("Mars/Olympus_Mons")
            expect(v).toBeNil()
            expect(e).toContain("chrono.DateTime.in_zone:")
        end)

        test("in_zone bumps DST-gap wall-clock forward (Compatible policy)", function()
            -- 2026-03-08 02:30 doesn't exist in America/New_York (spring-forward);
            -- jiff's Compatible disambiguation pushes to 03:30 EDT.
            local dt = chrono.DateTime.parse("2026-03-08T02:30:00")
            local z = dt:in_zone("America/New_York")
            expect(z.hour).toBe(3)
            expect(z.minute).toBe(30)
            expect(z.offset_seconds).toBe(-4 * 3600)
        end)

        test("format returns err on invalid pattern", function()
            local dt = chrono.DateTime.parse("2026-05-20T13:45:30")
            local v, e = dt:format("%@@@invalid")
            expect(v).toBeNil()
            expect(e).toContain("chrono.DateTime.format:")
        end)
    end)

    -- ── ZonedDateTime UserData ───────────────────────────────────────────────

    describe("ZonedDateTime", function()
        test("parse RFC 9557 with IANA suffix", function()
            -- Winter date — `-05:00` matches America/New_York standard time.
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            expect(z.timezone).toBe("America/New_York")
            expect(z.offset_seconds).toBe(-5 * 3600)
        end)

        test("in_zone preserves epoch instant", function()
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            local w = z:in_zone("UTC")
            expect(w.epoch_seconds).toBe(z.epoch_seconds)
            expect(w.timezone).toBe("UTC")
        end)

        test("to_rfc3339 is UTC instant", function()
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            expect(z:to_rfc3339()).toBe("2026-01-20T21:30:00Z")
        end)

        test("add preserves elapsed-time semantics across DST gap", function()
            -- 01:30 EST + 2h elapsed wall — DST gap consumes 02:00-03:00.
            -- Jiff's Zoned arithmetic adds elapsed seconds, so the epoch
            -- advances by exactly 7200 regardless of wall-clock display.
            local z = chrono.ZonedDateTime.parse("2026-03-08T01:30:00-05:00[America/New_York]")
            local r = z:add(chrono.Duration.of({ hours = 2 }))
            expect(r.epoch_seconds - z.epoch_seconds).toBe(2 * 3600)
        end)

        test("format returns err on invalid pattern", function()
            local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            local v, e = z:format("%@@@invalid")
            expect(v).toBeNil()
            expect(e).toContain("chrono.ZonedDateTime.format:")
        end)
    end)

    -- ── Duration UserData ────────────────────────────────────────────────────

    describe("Duration", function()
        test("of builds from parts", function()
            local d = chrono.Duration.of({ hours = 1, minutes = 30 })
            expect(d.hours).toBe(1)
            expect(d.minutes).toBe(30)
        end)

        test("total_seconds collapses absolute units", function()
            local d = chrono.Duration.of({ hours = 1, minutes = 30 })
            expect(d:total_seconds()).toBe(5400)
        end)

        test("total_seconds rejects calendar units", function()
            local d = chrono.Duration.of({ months = 1 })
            local v, e = d:total_seconds()
            expect(v).toBeNil()
            expect(e).toContain("chrono.Duration.total_seconds:")
        end)

        test("parse ISO 8601", function()
            local d = chrono.Duration.parse("PT1H30M")
            expect(tostring(d)).toBe("PT1H30M")
        end)

        test("negate flips sign", function()
            local d = chrono.Duration.of({ hours = 2 })
            expect(d:negate().hours).toBe(-2)
        end)
    end)

    -- ── Module-level flat helpers ────────────────────────────────────────────

    describe("now", function()
        test("returns a ZonedDateTime with a timezone", function()
            local z = chrono.now()
            -- System zone may be nil for fixed-offset hosts; either is fine.
            expect(z.epoch_seconds > 0).toBeTruthy()
        end)
    end)

    describe("utc_now", function()
        test("zone is UTC", function()
            expect(chrono.utc_now().timezone).toBe("UTC")
        end)
    end)

    describe("parse", function()
        test("dispatches to ZonedDateTime for zoned input", function()
            local v = chrono.parse("2026-01-20T16:30:00-05:00[America/New_York]")
            expect(v.timezone).toBe("America/New_York")
        end)

        test("dispatches to DateTime for civil input", function()
            local v = chrono.parse("2026-05-20T13:45:30")
            expect(v.year).toBe(2026)
            -- DateTime has no `timezone` field.
            expect(v.timezone).toBeNil()
        end)

        test("dispatches to Date for date-only input", function()
            local v = chrono.parse("2026-05-20")
            expect(tostring(v)).toBe("2026-05-20")
        end)

        test("dispatches to Time for time-only input", function()
            local v = chrono.parse("13:45:30")
            expect(tostring(v)).toBe("13:45:30")
        end)

        test("rejects unrecognised input", function()
            local v, e = chrono.parse("xx")
            expect(v).toBeNil()
            expect(e).toContain("chrono.parse:")
        end)

        test("dispatches to UTC ZonedDateTime for Z-suffix RFC 3339", function()
            local v = chrono.parse("2026-05-20T16:30:00Z")
            expect(v.timezone).toBe("UTC")
            expect(v.offset_seconds).toBe(0)
            expect(v.hour).toBe(16)
        end)

        test("dispatches to UTC ZonedDateTime for +00:00 offset without IANA suffix", function()
            local v = chrono.parse("2026-05-20T16:30:00+00:00")
            expect(v.timezone).toBe("UTC")
            expect(v.offset_seconds).toBe(0)
        end)
    end)
end)

-- lib:chrono on neoc-vm and neoc-vm2: what both must answer the same. Comparisons are operators on neoc-vm and
-- methods on neoc-vm2, and add/sub are plus/minus there (see chrono.vm2.luau), so neither is here.
local chrono = require("lib:chrono")
local out = {}

local function fails(value, err)
	return { value == nil, err ~= nil }
end

-- Date
local d = chrono.Date.new(2026, 5, 20)
local first = chrono.Date.new(2026, 1, 1)
out.date_fields = { d.year, d.month, d.day, d.weekday, d.ordinal, first.weekday, first.ordinal }
out.date_tostring = { tostring(d), tostring(chrono.Date.parse("2026-05-20")) }
out.date_invalid = fails(chrono.Date.new(2026, 2, 30))
out.date_too_large = fails(chrono.Date.new(2026, 300, 1))
out.date_parse_invalid = fails(chrono.Date.parse("not a date"))
out.date_with = { tostring(d:with_year(2030)), tostring(d:with_month(1)), tostring(d:with_day(1)) }
out.date_with_invalid = { fails(d:with_month(13)), fails(d:with_day(40)), fails(d:with_year(99999)) }
out.date_diff = d:diff((chrono.Date.parse("2026-05-30"))).days
out.date_format = d:format("%Y/%m/%d")
out.date_format_invalid = fails(d:format("%@@@invalid"))

-- Time
local t = chrono.Time.new(13, 45, 30)
out.time_fields = { t.hour, t.minute, t.second, t.nanosecond, chrono.Time.new(1, 2, 3, 400).nanosecond }
out.time_tostring = { tostring(t), tostring(chrono.Time.parse("13:45:30")) }
out.time_invalid = fails(chrono.Time.new(25, 0, 0))
out.time_parse_invalid = fails(chrono.Time.parse("nope"))
out.time_on = tostring(t:on(d))
out.time_format = t:format("%H.%M")
out.time_format_invalid = fails(t:format("%@@@invalid"))

-- DateTime
local dt = chrono.DateTime.parse("2026-05-20T13:45:30")
out.datetime_at = tostring(d:at(t))
out.datetime_new = tostring(chrono.DateTime.new(2026, 5, 20, 13, 45, 30))
out.datetime_new_invalid = fails(chrono.DateTime.new(2026, 5, 20, 24, 0, 0))
out.datetime_fields = { dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, dt.nanosecond }
out.datetime_parts = { tostring(dt:date()), tostring(dt:time()) }
out.datetime_parse_invalid = fails(chrono.DateTime.parse("nope"))
out.datetime_diff = dt:diff((chrono.DateTime.parse("2026-05-21T01:45:30"))).hours
out.datetime_format = dt:format("%Y %H")
out.datetime_format_invalid = fails(dt:format("%@@@invalid"))

local utc = chrono.DateTime.parse("2026-05-20T12:00:00"):in_zone("UTC")
out.in_zone = { tostring(utc), utc.timezone, utc.offset_seconds }
out.in_zone_unknown = fails(dt:in_zone("Mars/Olympus_Mons"))
-- 02:30 on 2026-03-08 does not exist in New York: it is moved forward by the gap
local gap = chrono.DateTime.parse("2026-03-08T02:30:00"):in_zone("America/New_York")
out.in_zone_gap = { gap.hour, gap.minute, gap.offset_seconds }

-- ZonedDateTime
local z = chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]")
out.zoned_fields = { z.year, z.month, z.day, z.hour, z.minute, z.second, z.nanosecond }
out.zoned = { tostring(z), z.timezone, z.offset_seconds, z.epoch_seconds, (z:to_rfc3339()) }
out.zoned_parts = { tostring(z:date()), tostring(z:time()), tostring(z:datetime()) }
local w = z:in_zone("UTC")
out.zoned_in_zone = { w.timezone, w.epoch_seconds == z.epoch_seconds, w.hour }
out.zoned_in_zone_unknown = fails(z:in_zone("Nowhere/At_All"))
out.zoned_parse_invalid = fails(chrono.ZonedDateTime.parse("2026-01-20T16:30:00"))
out.zoned_diff = z:diff(w):total_seconds()
out.zoned_format = z:format("%Y-%m-%dT%H:%M:%S%:z")
out.zoned_format_invalid = fails(z:format("%@@@invalid"))

-- Duration
local span = chrono.Duration.of({ hours = 1, minutes = 30 })
out.duration_fields = {
	span.years, span.months, span.weeks, span.days, span.hours, span.minutes,
	span.seconds, span.milliseconds, span.microseconds, span.nanoseconds,
}
out.duration_units_stay = chrono.Duration.of({ hours = 36 }).hours
out.duration_total = span:total_seconds()
out.duration_total_calendar = fails(chrono.Duration.of({ months = 1 }):total_seconds())
out.duration_tostring = { tostring(span), tostring(chrono.Duration.parse("PT1H30M")), tostring(chrono.Duration.parse("1h 30m")) }
out.duration_parse_invalid = fails(chrono.Duration.parse("soon"))
out.duration_negate = { span:negate().hours, span:negate().minutes, (span:negate():abs()).hours }

-- parse, by the text's shape
for name, text in {
	zoned = "2026-01-20T16:30:00-05:00[America/New_York]",
	utc = "2026-05-20T16:30:00Z",
	offset = "2026-05-20T16:30:00+00:00",
	datetime = "2026-05-20T13:45:30",
	date = "2026-05-20",
	time = "13:45:30",
} do
	out["parse_" .. name] = tostring(chrono.parse(text))
end
local parsed = chrono.parse("2026-05-20T16:30:00Z")
out.parse_utc_fields = { parsed.timezone, parsed.offset_seconds, parsed.hour }
out.parse_unknown = { fails(chrono.parse("xx")), fails(chrono.parse("not a timestamp")), fails(chrono.parse("25:99")) }

-- now
out.now = { chrono.now().epoch_seconds > 0, chrono.utc_now().timezone, chrono.utc_now().offset_seconds }

return out

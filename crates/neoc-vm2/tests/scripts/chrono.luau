--!strict
-- lib:chrono as a script sees it: its answers (chrono.json), then what it asserts.
local chrono = require("lib:chrono")
local out = {}

local function fails(value, err)
  return { value == nil, err ~= nil }
end

-- Date
local d = assert(chrono.Date.new(2026, 5, 20))
local first = assert(chrono.Date.new(2026, 1, 1))
out.date_fields = { d.year, d.month, d.day, d.weekday, d.ordinal, first.weekday, first.ordinal }
out.date_tostring = { tostring(d), tostring((chrono.Date.parse("2026-05-20"))) }
out.date_invalid = fails(chrono.Date.new(2026, 2, 30))
out.date_too_large = fails(chrono.Date.new(2026, 300, 1))
out.date_parse_invalid = fails(chrono.Date.parse("not a date"))
out.date_with = { tostring((d:with_year(2030))), tostring((d:with_month(1))), tostring((d:with_day(1))) }
out.date_with_invalid = { fails(d:with_month(13)), fails(d:with_day(40)), fails(d:with_year(99999)) }
out.date_diff = assert(d:diff((assert(chrono.Date.parse("2026-05-30"))))).days
out.date_format = d:format("%Y/%m/%d")
out.date_format_invalid = fails(d:format("%@@@invalid"))

-- Time
local t = assert(chrono.Time.new(13, 45, 30))
out.time_fields = { t.hour, t.minute, t.second, t.nanosecond, assert(chrono.Time.new(1, 2, 3, 400)).nanosecond }
out.time_tostring = { tostring(t), tostring((chrono.Time.parse("13:45:30"))) }
out.time_invalid = fails(chrono.Time.new(25, 0, 0))
out.time_parse_invalid = fails(chrono.Time.parse("nope"))
out.time_on = tostring(t:on(d))
out.time_format = t:format("%H.%M")
out.time_format_invalid = fails(t:format("%@@@invalid"))

-- DateTime
local dt = assert(chrono.DateTime.parse("2026-05-20T13:45:30"))
out.datetime_at = tostring(d:at(t))
out.datetime_new = tostring((chrono.DateTime.new(2026, 5, 20, 13, 45, 30)))
out.datetime_new_invalid = fails(chrono.DateTime.new(2026, 5, 20, 24, 0, 0))
out.datetime_fields = { dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, dt.nanosecond }
out.datetime_parts = { tostring(dt:date()), tostring(dt:time()) }
out.datetime_parse_invalid = fails(chrono.DateTime.parse("nope"))
out.datetime_diff = assert(dt:diff((assert(chrono.DateTime.parse("2026-05-21T01:45:30"))))).hours
out.datetime_format = dt:format("%Y %H")
out.datetime_format_invalid = fails(dt:format("%@@@invalid"))

local utc = assert(assert(chrono.DateTime.parse("2026-05-20T12:00:00")):in_zone("UTC"))
out.in_zone = { tostring(utc), utc.timezone, utc.offset_seconds } :: { any }
out.in_zone_unknown = fails(dt:in_zone("Mars/Olympus_Mons"))
-- 02:30 on 2026-03-08 does not exist in New York: it is moved forward by the gap
local gap = assert(assert(chrono.DateTime.parse("2026-03-08T02:30:00")):in_zone("America/New_York"))
out.in_zone_gap = { gap.hour, gap.minute, gap.offset_seconds }

-- ZonedDateTime
local z = assert(chrono.ZonedDateTime.parse("2026-01-20T16:30:00-05:00[America/New_York]"))
out.zoned_fields = { z.year, z.month, z.day, z.hour, z.minute, z.second, z.nanosecond }
out.zoned = { tostring(z), z.timezone, z.offset_seconds, z.epoch_seconds, (z:to_rfc3339()) } :: { any }
out.zoned_parts = { tostring(z:date()), tostring(z:time()), tostring(z:datetime()) }
local w = assert(z:in_zone("UTC"))
out.zoned_in_zone = { w.timezone, w.epoch_seconds == z.epoch_seconds, w.hour } :: { any }
out.zoned_in_zone_unknown = fails(z:in_zone("Nowhere/At_All"))
out.zoned_parse_invalid = fails(chrono.ZonedDateTime.parse("2026-01-20T16:30:00"))
out.zoned_diff = assert(z:diff(w)):total_seconds()
out.zoned_format = z:format("%Y-%m-%dT%H:%M:%S%:z")
out.zoned_format_invalid = fails(z:format("%@@@invalid"))

-- Duration
local span = chrono.Duration.of { hours = 1, minutes = 30 }
out.duration_fields = {
  span.years,
  span.months,
  span.weeks,
  span.days,
  span.hours,
  span.minutes,
  span.seconds,
  span.milliseconds,
  span.microseconds,
  span.nanoseconds,
}
out.duration_units_stay = chrono.Duration.of({ hours = 36 }).hours
out.duration_total = span:total_seconds()
out.duration_total_calendar = fails(chrono.Duration.of({ months = 1 }):total_seconds())
out.duration_tostring =
  { tostring(span), tostring((chrono.Duration.parse("PT1H30M"))), tostring((chrono.Duration.parse("1h 30m"))) }
out.duration_parse_invalid = fails(chrono.Duration.parse("soon"))
out.duration_negate = { span:negate().hours, span:negate().minutes, (span:negate():abs()).hours }

-- parse, by the text's shape
for name, text in
  {
    zoned = "2026-01-20T16:30:00-05:00[America/New_York]",
    utc = "2026-05-20T16:30:00Z",
    offset = "2026-05-20T16:30:00+00:00",
    datetime = "2026-05-20T13:45:30",
    date = "2026-05-20",
    time = "13:45:30",
  } :: { [string]: string }
do
  out["parse_" .. name] = tostring((chrono.parse(text)))
end
local parsed = assert(chrono.parse("2026-05-20T16:30:00Z")) :: lib_chrono_ZonedDateTime
out.parse_utc_fields = { parsed.timezone, parsed.offset_seconds, parsed.hour } :: { any }
out.parse_unknown = { fails(chrono.parse("xx")), fails(chrono.parse("not a timestamp")), fails(chrono.parse("25:99")) }

-- now
out.now = { chrono.now().epoch_seconds > 0, chrono.utc_now().timezone, chrono.utc_now().offset_seconds } :: { any }

-- what it asserts
do
  local function date(text: string)
    return (assert(chrono.Date.parse(text)))
  end

  -- order is a method, and a word
  local a, b, c = date("2026-05-20"), date("2026-05-30"), date("2026-05-20")
  assert(a:compare(b) == "less" and b:compare(a) == "greater" and a:compare(c) == "equal")
  assert(a:equals(c) and not a:equals(b))
  assert(a ~= c, "== asks whether two dates are the same object")

  local early = assert(chrono.Time.new(1, 0, 0))
  local late = assert(chrono.Time.new(2, 0, 0))
  assert(early:compare(late) == "less" and early:equals((assert(chrono.Time.new(1, 0, 0)))))

  -- plus and minus
  local ten = chrono.Duration.of { days = 10 }
  assert(tostring((assert(a:plus(ten)))) == "2026-05-30")
  assert(tostring((assert(a:minus(ten)))) == "2026-05-10")
  assert(assert(a:plus(ten)):equals(b))
  local _, sub_day = a:plus(chrono.Duration.of { hours = 25 })
  assert(sub_day ~= nil and sub_day.kind == "sub_day_units" and sub_day.op == "lib:chrono.Date:plus")
  local _, sub_day_minus = a:minus(chrono.Duration.of { minutes = 60 })
  assert(sub_day_minus ~= nil and sub_day_minus.kind == "sub_day_units")
  local _, past = a:plus(chrono.Duration.of { years = 19000 })
  assert(past ~= nil and past.kind == "overflow")

  local dt = assert(chrono.DateTime.new(2026, 5, 20, 13, 45, 30))
  local later = assert(dt:plus(chrono.Duration.of { hours = 12 }))
  assert(tostring(later) == "2026-05-21T01:45:30" and later:compare(dt) == "greater")
  assert(assert(later:minus(chrono.Duration.of { hours = 12 })):equals(dt))
  assert(assert(dt:diff(later)).hours == 12)

  -- a zoned plus is elapsed time: two hours across the spring-forward gap are 7200 seconds
  local z = assert(chrono.ZonedDateTime.parse("2026-03-08T01:30:00-05:00[America/New_York]"))
  local two = chrono.Duration.of { hours = 2 }
  local after = assert(z:plus(two))
  assert(after.epoch_seconds - z.epoch_seconds == 2 * 3600 and after.hour == 4)
  assert(assert(after:minus(two)):equals(z))
  -- the same instant in two zones is equal
  local utc = assert(z:in_zone("UTC"))
  assert(z:compare(utc) == "equal" and z:equals(utc) and z:compare(after) == "less")

  -- typed errors
  local _, range = chrono.Date.new(2026, 2, 30)
  assert(range ~= nil and range.kind == "out_of_range" and range.op == "lib:chrono.Date.new")
  assert(tostring(range):find("lib:chrono.Date.new: out_of_range: ", 1, true) == 1)
  local _, large = chrono.Date.new(2026, 300, 1)
  assert(large ~= nil and large.kind == "out_of_range" and large.reason == "month 300 is out of range")
  local _, hour = chrono.Time.new(25, 0, 0)
  assert(hour ~= nil and hour.kind == "out_of_range")

  local _, invalid = chrono.Date.parse("not a date")
  assert(invalid ~= nil and invalid.kind == "invalid" and invalid.text == "not a date")
  local _, unrecognised = chrono.parse("xx")
  assert(unrecognised ~= nil and unrecognised.kind == "unrecognised" and unrecognised.text == "xx")
  assert(
    tostring(unrecognised)
      == 'lib:chrono.parse: unrecognised: cannot recognise "xx" as a Date, Time, DateTime or ZonedDateTime'
  )

  local _, zone = dt:in_zone("Mars/Olympus_Mons")
  assert(zone ~= nil and zone.kind == "unknown_timezone" and zone.name == "Mars/Olympus_Mons")
  assert(zone.op == "lib:chrono.DateTime:in_zone")

  local _, pattern = a:format("%@@@invalid")
  assert(pattern ~= nil and pattern.kind == "invalid_pattern" and pattern.pattern == "%@@@invalid")

  local _, calendar = chrono.Duration.of({ months = 1 }):total_seconds()
  assert(calendar ~= nil and calendar.kind == "calendar_units")

  -- Duration.of: its units are whole numbers it knows, and a unit too large is raised
  for _, units in { { hours = 1.5 }, { hours = "one" }, { hour = 1 }, 5 } :: { any } do
    local ok, refused = pcall(chrono.Duration.of, units)
    assert(not ok and tostring(refused):find("lib:chrono.Duration.of: argument #1 'units'", 1, true))
  end
  local ok, raised = pcall(chrono.Duration.of, { years = 1000000 })
  assert(not ok and tostring(raised):find("lib:chrono.Duration.of: out_of_range: years: ", 1, true))

  -- durations compare by their units
  local span = chrono.Duration.of { hours = 2 }
  assert(span:equals(chrono.Duration.of { hours = 2 }) and not span:equals(chrono.Duration.of { minutes = 120 }))
  assert(span:negate():abs():equals(span))
  assert(select("#", span:abs()) == 1 and select("#", utc:to_rfc3339()) == 1)

  -- what parse gave is told by its kind
  local function describe(text: string): string
    local value = assert(chrono.parse(text))
    if value.kind == "zoned_date_time" then
      return `zoned {value.timezone} {value.epoch_seconds}`
    elseif value.kind == "date_time" then
      return `datetime {value.hour}`
    elseif value.kind == "date" then
      return `date {value.weekday}`
    else
      return `time {value.minute}`
    end
  end
  assert(describe("2026-05-20T16:30:00Z") == "zoned UTC 1779294600")
  assert(describe("2026-05-20T13:45:30") == "datetime 13")
  assert(describe("2026-05-20") == "date 3")
  assert(describe("13:45:30") == "time 45")
  assert(a.kind == "date" and early.kind == "time" and dt.kind == "date_time" and z.kind == "zoned_date_time")
end

return out

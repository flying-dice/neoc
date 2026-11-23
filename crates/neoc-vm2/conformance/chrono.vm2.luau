-- intentional: operators (== < <=) are methods (a:compare(b) gives "less" | "equal" | "greater", a:equals(b)); add and sub are plus and minus; every failure is a typed error (out_of_range, invalid, unrecognised, sub_day_units, overflow, unknown_timezone, invalid_pattern, calendar_units); Duration.of refuses a unit it does not know or that is not a whole number and raises one that is too large, so its result is passed on as it is; Duration:abs and ZonedDateTime:to_rfc3339 return one value; every date and time value has a kind field that says which one chrono.parse gave.
local chrono = require("lib:chrono")

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
local ten = chrono.Duration.of({ days = 10 })
assert(tostring(assert(a:plus(ten))) == "2026-05-30")
assert(tostring(assert(a:minus(ten))) == "2026-05-10")
assert(assert(a:plus(ten)):equals(b))
local _, sub_day = a:plus(chrono.Duration.of({ hours = 25 }))
assert(sub_day ~= nil and sub_day.kind == "sub_day_units" and sub_day.op == "lib:chrono.Date:plus")
local _, sub_day_minus = a:minus(chrono.Duration.of({ minutes = 60 }))
assert(sub_day_minus ~= nil and sub_day_minus.kind == "sub_day_units")
local _, past = a:plus(chrono.Duration.of({ years = 19000 }))
assert(past ~= nil and past.kind == "overflow")

local dt = assert(chrono.DateTime.new(2026, 5, 20, 13, 45, 30))
local later = assert(dt:plus(chrono.Duration.of({ hours = 12 })))
assert(tostring(later) == "2026-05-21T01:45:30" and later:compare(dt) == "greater")
assert(assert(later:minus(chrono.Duration.of({ hours = 12 }))):equals(dt))
assert(assert(dt:diff(later)).hours == 12)

-- a zoned plus is elapsed time: two hours across the spring-forward gap are 7200 seconds
local z = assert(chrono.ZonedDateTime.parse("2026-03-08T01:30:00-05:00[America/New_York]"))
local two = chrono.Duration.of({ hours = 2 })
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
assert(tostring(unrecognised) == 'lib:chrono.parse: unrecognised: cannot recognise "xx" as a Date, Time, DateTime or ZonedDateTime')

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
local span = chrono.Duration.of({ hours = 2 })
assert(span:equals(chrono.Duration.of({ hours = 2 })) and not span:equals(chrono.Duration.of({ minutes = 120 })))
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

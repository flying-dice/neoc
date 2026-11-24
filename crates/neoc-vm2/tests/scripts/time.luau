--!strict
-- std:time as a script sees it: its answers (time.json), then what it asserts.
local time = require("std:time")
local D = time.Duration
local out = {}

out.from_secs = { D.from_secs(2):as_secs(), D.from_secs(2):as_millis() }
out.from_millis = D.from_millis(500):as_millis()
out.from_micros = D.from_micros(750):as_micros()
out.from_nanos = D.from_nanos(250):as_nanos()
out.from_secs_f64 = assert(D.from_secs_f64(1.5)):as_millis()
out.zero = { D.from_secs(0):is_zero(), D.from_secs(1):is_zero() }
out.as_secs_f64 = D.from_millis(1500):as_secs_f64()

out.parse = {
  assert(D.parse("500ms")):as_millis(),
  assert(D.parse("1m30s")):as_secs(),
  assert(D.parse("1.5s")):as_millis(),
  assert(D.parse("1h")):as_secs(),
  assert(D.parse("500us")):as_micros(),
  assert(D.parse("250ns")):as_nanos(),
  assert(D.parse("0")):is_zero(),
} :: { any }
for i, text in { "nonsense", "", "5", "s", "5xs", "-5s", "99999999999999999999h" } do
  local value, err = D.parse(text)
  out["parse_fails_" .. i] = { value == nil, err ~= nil }
end

out.tostring = {
  tostring((D.parse("500ms"))),
  tostring((D.parse("2s"))),
  tostring((D.parse("1m30s"))),
  tostring((D.parse("1.5s"))),
  tostring((D.parse("1h30m"))),
  tostring((D.parse("0"))),
  tostring(D.from_nanos(500)),
}

-- a huge duration's counts stay positive
local huge = assert(D.parse("9000h"))
out.huge = { huge:as_nanos() > 0, huge:as_micros() > 0, huge:as_millis() > 0 }

out.instant = {
  type(time.now()),
  type(time.Instant.now()),
  time.now():elapsed():as_secs() < 5,
  time.Instant.now():duration_since(time.Instant.now()):is_zero(),
} :: { any }

-- what it asserts
do
  local D = time.Duration

  -- arithmetic
  assert(D.from_millis(300):plus(D.from_millis(200)):as_millis() == 500)
  assert(D.from_millis(100):minus(D.from_millis(500)):is_zero(), "minus saturates at zero")
  assert(assert(D.from_secs(2):times(1.5)):as_millis() == 3000)
  assert(assert(D.from_secs(2):divided_by(4)):as_millis() == 500)
  local _, negative = D.from_secs(1):times(-1)
  assert(negative ~= nil and negative.kind == "invalid_factor")
  local _, zero = D.from_secs(1):divided_by(0)
  assert(zero ~= nil and zero.kind == "division_by_zero" and zero.op == "std:time.Duration:divided_by")

  -- order
  local a, b = D.from_millis(100), D.from_millis(200)
  assert(a:compare(b) == "less" and b:compare(a) == "greater" and a:compare(D.from_millis(100)) == "equal")
  assert(D.zero():is_zero())

  -- parse errors say why
  local _, empty = D.parse("")
  assert(empty ~= nil and empty.kind == "empty")
  local _, unit = D.parse("5xs")
  assert(unit ~= nil and unit.kind == "unknown_unit" and unit.unit == "xs")
  local _, missing = D.parse("5")
  assert(missing ~= nil and missing.kind == "missing_unit")
  local _, overflow = D.parse("99999999999999999999h")
  assert(
    overflow ~= nil
      and overflow.kind == "overflow"
      and tostring(overflow) == "std:time.Duration.parse: overflow: duration overflow"
  )

  -- a negative count is refused before the call
  local ok, refused = pcall(D.from_secs, -1)
  assert(not ok and tostring(refused):find("argument #1 'secs'", 1, true))
  local _, range = D.from_secs_f64(-1)
  assert(range ~= nil and range.kind == "out_of_range")

  -- counts saturate at 2^53
  assert(assert(D.parse("9000h")):as_nanos() == 2 ^ 53)

  -- instants
  local t0 = time.now()
  local later = assert(t0:plus(D.from_millis(250)))
  assert(later:duration_since(t0):as_millis() == 250)
  assert(t0:compare(later) == "less")
  assert(assert(later:minus(D.from_millis(250))):compare(t0) == "equal")
  -- the longest duration there is (plus saturates): 2^53 seconds back is still an instant on Linux
  local longest = D.from_secs(2 ^ 53)
  for _ = 1, 12 do
    longest = longest:plus(longest)
  end
  local _, past = t0:minus(longest)
  assert(past ~= nil and past.kind == "instant_overflow")
  assert(tostring(t0) == "Instant")
end

return out

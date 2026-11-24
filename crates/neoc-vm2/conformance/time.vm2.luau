--!strict
-- intentional: operators are methods (plus, minus, times, divided_by, compare); Duration.ZERO is Duration.zero(); a negative count is refused as an argument; every failure is a typed error table; counts saturate at 2^53.
local time = require("std:time")
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
assert(overflow ~= nil and overflow.kind == "overflow" and tostring(overflow) == "std:time.Duration.parse: overflow: duration overflow")

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
local _, past = t0:minus(D.from_secs(2 ^ 53))
assert(past ~= nil and past.kind == "instant_overflow")
assert(tostring(t0) == "Instant")

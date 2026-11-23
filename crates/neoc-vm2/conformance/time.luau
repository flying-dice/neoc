-- std:time on neoc-vm and neoc-vm2: what both must answer the same. Operators are neoc-vm's and methods are
-- neoc-vm2's (see time.vm2.luau), so arithmetic is not here.
local time = require("std:time")
local D = time.Duration
local out = {}

out.from_secs = { D.from_secs(2):as_secs(), D.from_secs(2):as_millis() }
out.from_millis = D.from_millis(500):as_millis()
out.from_micros = D.from_micros(750):as_micros()
out.from_nanos = D.from_nanos(250):as_nanos()
out.from_secs_f64 = D.from_secs_f64(1.5):as_millis()
out.zero = { D.from_secs(0):is_zero(), D.from_secs(1):is_zero() }
out.as_secs_f64 = D.from_millis(1500):as_secs_f64()

out.parse = {
	D.parse("500ms"):as_millis(),
	D.parse("1m30s"):as_secs(),
	D.parse("1.5s"):as_millis(),
	D.parse("1h"):as_secs(),
	D.parse("500us"):as_micros(),
	D.parse("250ns"):as_nanos(),
	D.parse("0"):is_zero(),
}
for i, text in { "nonsense", "", "5", "s", "5xs", "-5s", "99999999999999999999h" } do
	local value, err = D.parse(text)
	out["parse_fails_" .. i] = { value == nil, err ~= nil }
end

out.tostring = {
	tostring(D.parse("500ms")),
	tostring(D.parse("2s")),
	tostring(D.parse("1m30s")),
	tostring(D.parse("1.5s")),
	tostring(D.parse("1h30m")),
	tostring(D.parse("0")),
	tostring(D.from_nanos(500)),
}

-- a huge duration's counts stay positive
local huge = D.parse("9000h")
out.huge = { huge:as_nanos() > 0, huge:as_micros() > 0, huge:as_millis() > 0 }

out.instant = {
	type(time.now()),
	type(time.Instant.now()),
	time.now():elapsed():as_secs() < 5,
	time.Instant.now():duration_since(time.Instant.now()):is_zero(),
}

return out

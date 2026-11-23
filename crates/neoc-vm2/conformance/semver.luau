-- lib:semver on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Comparison, which arguments take text and which take objects, coerce's miss and the errors differ on purpose: see
-- semver.vm2.luau.
local semver = require("lib:semver")
local out = {}

local v = semver.Version.parse("1.2.3-beta.1+build.42")
out.parts = { v.major, v.minor, v.patch, v.pre, v.build, tostring(v) }
local plain = semver.Version.parse("10.20.30")
out.plain = { plain.major, plain.minor, plain.patch, plain.pre == nil, plain.build == nil, tostring(plain) }

out.canonical = {}
for i, text in { "0.0.0", "1.2.3-rc.1+meta", "1.0.0-alpha.beta", "1.0.0+20130313144700", "1.0.0-0.3.7", "1.0.0-x.7.z.92" } do
	out.canonical[i] = tostring(semver.Version.parse(text))
end

-- a failure is `(nil, err)`; what `err` is differs
out.invalid = {}
for i, text in { "1.2", "v1.2.3", "1.2.3-01", "1.2.3.4", "", "not.a.ver", "01.2.3", " 1.2.3" } do
	local version, why = semver.Version.parse(text)
	out.invalid[i] = { version == nil, why ~= nil, semver.valid(text) }
end
out.valid = { semver.valid("1.2.3"), semver.valid("1.2.3-rc.1+b"), semver.valid("0.0.0") }

out.coerce = {}
for i, text in { "v1.2.3", "V1.2.3", "1.2", "1", "release-1.2.3", "v1.2.3-rc1", "1.2.3.4", "1.2.3+build", "1.2.3-01", "  v2" } do
	out.coerce[i] = tostring(semver.coerce(text))
end
out.coerce_miss = { semver.coerce("abc") == nil, semver.coerce("") == nil, semver.coerce("  ") == nil }

out.satisfies = {
	semver.satisfies("1.2.3", "^1.0.0"),
	semver.satisfies("2.0.0", "^1.0.0"),
	semver.satisfies("1.2.3", "~1.2.0"),
	semver.satisfies("1.3.0", "~1.2.0"),
	semver.satisfies("1.0.0-alpha", "^1.0.0"),
	semver.satisfies("0.0.1", "^0.0.1"),
	semver.satisfies("0.0.2", "^0.0.1"),
	semver.satisfies("1.5.0", ">=1.0.0, <2.0.0"),
	semver.satisfies("2.0.0", ">=1.0.0, <2.0.0"),
	semver.satisfies("1.5.0", "1.*"),
	semver.satisfies("1.5.0", "=1.5.0"),
	semver.satisfies("1.5.0-rc.1", ">=1.5.0-rc.0"),
	semver.satisfies("1.5.0", "*"),
}
for name, case in {
	bad_range = { "1.0.0", "<<bad>>" },
	bad_version = { "nope", "^1" },
	npm_or = { "1.0.0", "^1 || ^2" },
	npm_hyphen = { "1.0.0", "1.0.0 - 2.0.0" },
} do
	local answer, why = semver.satisfies(case[1], case[2])
	out["satisfies_fails_" .. name] = { answer == nil, why ~= nil }
end

local req = semver.VersionReq.parse(">=1.0.0, <2.0.0")
out.req = {
	tostring(req),
	req:matches((semver.Version.parse("1.0.0"))),
	req:matches((semver.Version.parse("1.5.0"))),
	req:matches((semver.Version.parse("2.0.0"))),
	req:matches((semver.Version.parse("0.9.9"))),
	tostring(semver.VersionReq.parse("^1.2")),
	tostring(semver.VersionReq.parse("~1")),
}
local no_req, req_why = semver.VersionReq.parse("not-a-range")
out.req_invalid = { no_req == nil, req_why ~= nil }

out.increment = {}
for i, case in {
	{ "0.0.0", "major" },
	{ "1.2.3-rc.1", "major" },
	{ "1.2.3", "minor" },
	{ "1.2.3", "patch" },
	{ "1.2.3-beta.1", "patch" },
	{ "1.2.3", "prerelease" },
	{ "1.2.3-beta", "prerelease" },
	{ "1.2.3-beta.1", "prerelease" },
	{ "1.2.3-0.alpha", "prerelease" },
	{ "1.2.3-rc.1.beta", "prerelease" },
	{ "1.2.3+build", "minor" },
	{ "1.2.3+meta", "patch" },
} do
	local before = semver.Version.parse(case[1])
	local after = before:increment(case[2])
	out.increment[i] = { tostring(after), tostring(before) }
end

return out

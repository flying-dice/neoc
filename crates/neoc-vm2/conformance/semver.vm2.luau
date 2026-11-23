-- intentional: operators (< <= ==) are methods (a:compare(b) gives "less" | "equal" | "greater" where it gave -1, 0, 1; a:equals(b)), each argument has one type (methods take objects, the module functions take text; each took either), increment takes its level as a string enum and is a method only (the functions increment, parse and format are gone), coerce gives nil when the text holds no version (it gave nil and a message), and a failure is a typed error (ParseError invalid_version or invalid_range, IncrementError overflow).
local semver = require("lib:semver")

local function parse(text: string)
	return (assert(semver.Version.parse(text)))
end

-- comparison is a method, and an order is a word
local a, b, c = parse("1.0.0"), parse("2.0.0"), parse("1.0.0+build")
assert(a:compare(b) == "less" and b:compare(a) == "greater" and a:compare(c) == "equal")
assert(a:equals(c) and not a:equals(b))
assert(a ~= c, "== asks whether two versions are the same object")
assert(not pcall(function()
	return a < b
end))
assert(semver.compare("1.2.3", "1.2.4") == "less" and semver.compare("1.0.0", "1.0.0-rc.1") == "greater")
assert(semver.compare("1.0.0+a", "1.0.0+b") == "equal")

local list = { parse("2.0.0"), parse("1.0.0"), parse("1.0.1-rc.1"), parse("1.0.1") }
table.sort(list, function(x, y)
	return x:compare(y) == "less"
end)
assert(tostring(list[1]) == "1.0.0" and tostring(list[2]) == "1.0.1-rc.1" and tostring(list[4]) == "2.0.0")

-- one type for each argument
local req = assert(semver.VersionReq.parse("^1"))
for _, call in {
	{ a.compare, a, "1.0.0" },
	{ a.equals, a, "1.0.0" },
	{ req.matches, req, "1.0.0" },
	{ semver.compare, a, "1.0.0" },
	{ semver.satisfies, a, "^1" },
	{ semver.satisfies, "1.0.0", req },
	{ a.increment, a, "bogus" },
} do
	assert(not pcall(table.unpack(call)))
end
local _, refused = pcall(a.increment, a, "bogus")
assert(tostring(refused):find("lib:semver.Version:increment: argument #1 'level'", 1, true))
assert(semver.parse == nil and semver.format == nil and semver.increment == nil)

-- coerce misses with nil alone
assert(select("#", semver.coerce("abc")) == 1 and semver.coerce("abc") == nil)

-- typed errors
local _, version = semver.Version.parse("1.2")
assert(version.kind == "invalid_version" and version.text == "1.2" and type(version.reason) == "string")
assert(version.op == "lib:semver.Version.parse")
assert(tostring(version):find('lib:semver.Version.parse: invalid_version: invalid version "1.2": ', 1, true) == 1)
local _, range = semver.VersionReq.parse("not-a-range")
assert(range.kind == "invalid_range" and range.text == "not-a-range" and range.op == "lib:semver.VersionReq.parse")
local _, either = semver.satisfies("1.0.0", "<<bad>>")
assert(either.kind == "invalid_range" and either.op == "lib:semver.satisfies")
_, either = semver.satisfies("nope", "^1")
assert(either.kind == "invalid_version")
_, either = semver.compare("1.0.0", "nope")
assert(either.kind == "invalid_version" and either.text == "nope")

local next, overflow = parse("18446744073709551615.0.0"):increment("major")
assert(next == nil and overflow.kind == "overflow" and overflow.level == "major")
assert(overflow.op == "lib:semver.Version:increment")

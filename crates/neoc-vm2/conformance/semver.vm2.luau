--!strict
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
	return (a :: any) < b -- deliberate refusal test: a version has no operators
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
-- deliberate refusal tests: each cast is an argument of the other type, or a level there is not
local wrong: { () -> ...unknown } = {
	function() return a:compare("1.0.0" :: any) end,
	function() return a:equals("1.0.0" :: any) end,
	function() return req:matches("1.0.0" :: any) end,
	function() return semver.compare(a :: any, "1.0.0") end,
	function() return semver.satisfies(a :: any, "^1") end,
	function() return semver.satisfies("1.0.0", req :: any) end,
	function() return a:increment("bogus" :: any) end,
}
for _, call in wrong do
	assert(not pcall(call))
end
local _, refused = pcall(function()
	a:increment("bogus" :: any) -- deliberate refusal test: not a level
end)
assert(tostring(refused):find("lib:semver.Version:increment: argument #1 'level'", 1, true))
local module = semver :: any
assert(module.parse == nil and module.format == nil and module.increment == nil)

-- coerce misses with nil alone
assert(select("#", semver.coerce("abc")) == 1 and semver.coerce("abc") == nil)

-- typed errors
local _, version = semver.Version.parse("1.2")
assert(version ~= nil)
assert(version.kind == "invalid_version" and version.text == "1.2" and type(version.reason) == "string")
assert(version.op == "lib:semver.Version.parse")
assert(tostring(version):find('lib:semver.Version.parse: invalid_version: invalid version "1.2": ', 1, true) == 1)
local _, range = semver.VersionReq.parse("not-a-range")
assert(range ~= nil)
assert(range.kind == "invalid_range" and range.text == "not-a-range" and range.op == "lib:semver.VersionReq.parse")
local _, bad_range = semver.satisfies("1.0.0", "<<bad>>")
assert(bad_range ~= nil and bad_range.kind == "invalid_range" and bad_range.op == "lib:semver.satisfies")
local _, bad_version = semver.satisfies("nope", "^1")
assert(bad_version ~= nil and bad_version.kind == "invalid_version")
local _, bad_other = semver.compare("1.0.0", "nope")
assert(bad_other ~= nil and bad_other.kind == "invalid_version" and bad_other.text == "nope")

local next, overflow = parse("18446744073709551615.0.0"):increment("major")
assert(next == nil and overflow ~= nil)
assert(overflow.kind == "overflow" and overflow.level == "major")
assert(overflow.op == "lib:semver.Version:increment")

--!strict
-- lib:semver as a script sees it: its answers (semver.json), then what it asserts.
local semver = require("lib:semver")
local out = {}

local v = assert(semver.Version.parse("1.2.3-beta.1+build.42"))
out.parts = { v.major, v.minor, v.patch, v.pre, v.build, tostring(v) } :: { any }
local plain = assert(semver.Version.parse("10.20.30"))
out.plain = { plain.major, plain.minor, plain.patch, plain.pre == nil, plain.build == nil, tostring(plain) } :: { any }

out.canonical = {}
for i, text in
  { "0.0.0", "1.2.3-rc.1+meta", "1.0.0-alpha.beta", "1.0.0+20130313144700", "1.0.0-0.3.7", "1.0.0-x.7.z.92" }
do
  out.canonical[i] = tostring((semver.Version.parse(text)))
end

-- a failure is `(nil, err)`
out.invalid = {}
for i, text in { "1.2", "v1.2.3", "1.2.3-01", "1.2.3.4", "", "not.a.ver", "01.2.3", " 1.2.3" } do
  local version, why = semver.Version.parse(text)
  out.invalid[i] = { version == nil, why ~= nil, semver.valid(text) }
end
out.valid = { semver.valid("1.2.3"), semver.valid("1.2.3-rc.1+b"), semver.valid("0.0.0") }

out.coerce = {}
for i, text in
  { "v1.2.3", "V1.2.3", "1.2", "1", "release-1.2.3", "v1.2.3-rc1", "1.2.3.4", "1.2.3+build", "1.2.3-01", "  v2" }
do
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
for name, case in
  {
    bad_range = { "1.0.0", "<<bad>>" },
    bad_version = { "nope", "^1" },
    npm_or = { "1.0.0", "^1 || ^2" },
    npm_hyphen = { "1.0.0", "1.0.0 - 2.0.0" },
  } :: { [string]: { string } }
do
  local answer, why = semver.satisfies(case[1], case[2])
  out["satisfies_fails_" .. name] = { answer == nil, why ~= nil }
end

local req = assert(semver.VersionReq.parse(">=1.0.0, <2.0.0"))
out.req = {
  tostring(req),
  req:matches((assert(semver.Version.parse("1.0.0")))),
  req:matches((assert(semver.Version.parse("1.5.0")))),
  req:matches((assert(semver.Version.parse("2.0.0")))),
  req:matches((assert(semver.Version.parse("0.9.9")))),
  tostring((semver.VersionReq.parse("^1.2"))),
  tostring((semver.VersionReq.parse("~1"))),
} :: { any }
local no_req, req_why = semver.VersionReq.parse("not-a-range")
out.req_invalid = { no_req == nil, req_why ~= nil }

out.increment = {}
for i, case in
  {
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
  }
do
  local before = assert(semver.Version.parse(case[1]))
  local after = before:increment(case[2] :: any)
  out.increment[i] = { tostring(after), tostring(before) }
end

-- what it asserts
do
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
    function()
      return a:compare("1.0.0" :: any)
    end,
    function()
      return a:equals("1.0.0" :: any)
    end,
    function()
      return req:matches("1.0.0" :: any)
    end,
    function()
      return semver.compare(a :: any, "1.0.0")
    end,
    function()
      return semver.satisfies(a :: any, "^1")
    end,
    function()
      return semver.satisfies("1.0.0", req :: any)
    end,
    function()
      return a:increment("bogus" :: any)
    end,
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
end

return out

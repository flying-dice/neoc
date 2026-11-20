-- Deliberately wrong uses of the `std:`, `lib:` and `vnd:` modules. The luals test requires lua-language-server to
-- report a diagnostic on every line marked `expect-error`, and none elsewhere. If the module definitions
-- (types/luals/<ns>/<name>.lua) or the plugin that finds them stop loading, `require("lib:json")` is `any` again, nothing here is flagged and the
-- test fails.
local base64 = require("lib:base64")
local chrono = require("lib:chrono")
local collections = require("std:collections")
local json = require("lib:json")
local rusqlite = require("vnd:rusqlite")
local semver = require("lib:semver")
local task = require("std:task")
local test = require("lib:test")
local time = require("std:time")
local url = require("lib:url")

-- an unknown function, a missing argument, an argument of the wrong type
base64.encod("x") -- expect-error
base64.encode() -- expect-error
base64.encode {} -- expect-error
time.sleep("soon") -- expect-error
collections.Counter.new("ten") -- expect-error

-- a result of the wrong type: a fallible function returns an optional value
---@type number
local _encoded = base64.encode("x") -- expect-error
---@type string
local _maybe = base64.decode("eA==") -- expect-error

-- an unknown method, an unknown constructor, a wrong argument to a method
json.object():nope() -- expect-error
json.object():set(1, 2) -- expect-error
collections.Map.create() -- expect-error
time.Duration.from_secs(5):as_years() -- expect-error
chrono.Date.new("2026", 5, 20) -- expect-error

-- a field that does not exist, and a field read as the wrong type
local _parsed = url.Url.parse("https://example.com")
print(assert(_parsed).nope.x) -- expect-error
---@type string
local _major = assert(semver.Version.parse("1.0.0")).major -- expect-error

-- trailing arguments are only for the functions that take them
rusqlite.open(":memory:"):query() -- expect-error
task.wait() -- expect-error
test.describe() -- expect-error
test.expect(1, 2).toBe(1) -- expect-error
test.expect(1).toBeAbout(1) -- expect-error

local _fine = base64.encode("this line is correct")
local _object = json.object()

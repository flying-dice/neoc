-- Deliberately wrong uses of the `std:`, `lib:` and `vnd:` modules. The luals test requires lua-language-server to
-- report, on every marked line, the diagnostic whose code follows the marker (e.g. `missing-parameter`), and no
-- diagnostic on any other line. If the module definitions (types/luals/<ns>/<name>.lua) or the plugin that finds
-- them stop loading, `require("lib:json")` is `any` again, nothing here is flagged and the test fails.
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
base64.encod("x") -- expect-error: undefined-field
base64.encode() -- expect-error: missing-parameter
base64.encode {} -- expect-error: param-type-mismatch
time.sleep("soon") -- expect-error: param-type-mismatch
collections.Counter.new("ten") -- expect-error: param-type-mismatch

-- a result of the wrong type: a fallible function returns an optional value
---@type number
local _encoded = base64.encode("x") -- expect-error: assign-type-mismatch
---@type string
local _maybe = base64.decode("eA==") -- expect-error: assign-type-mismatch

-- an unknown method, an unknown constructor, a wrong argument to a method
json.object():nope() -- expect-error: undefined-field
json.object():set(1, 2) -- expect-error: param-type-mismatch
collections.Map.create() -- expect-error: undefined-field
time.Duration.from_secs(5):as_years() -- expect-error: undefined-field
chrono.Date.new("2026", 5, 20) -- expect-error: param-type-mismatch

-- a field that does not exist, and a field read as the wrong type
local _parsed = url.Url.parse("https://example.com")
print(assert(_parsed).nope.x) -- expect-error: undefined-field
---@type string
local _major = assert(semver.Version.parse("1.0.0")).major -- expect-error: assign-type-mismatch

-- trailing arguments are only for the functions that take them
rusqlite.open(":memory:"):query() -- expect-error: missing-parameter
task.wait() -- expect-error: missing-parameter
test.describe() -- expect-error: missing-parameter
test.expect(1, 2).toBe(1) -- expect-error: redundant-parameter
test.expect(1).toBeAbout(1) -- expect-error: undefined-field

-- an optional result used without a check, a handler that is not a function, a listener that is not one
---@param req vnd_hyper.Request
local function _handler(req)
  local accept = req:header("accept")
  return accept:upper() -- expect-error: need-check-nil
end
require("vnd:hyper").serve(42, _handler) -- expect-error: param-type-mismatch
require("vnd:hyper").serve(assert(require("std:net").TcpListener.bind("127.0.0.1:0")), "handler") -- expect-error: param-type-mismatch

local _fine = base64.encode("this line is correct")
local _object = json.object()

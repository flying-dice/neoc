-- Deliberately wrong uses of the `std:`, `lib:` and `vnd:` modules. `bun run lint:lua` requires lua-language-server
-- to report a diagnostic on every line marked `expect-error`, and none elsewhere. If the module definitions
-- (types/luals/<namespace>/<name>.lua) stop loading, `require("lib:json")` is `any` again, nothing here is flagged and
-- the gate fails.
local json = require("lib:json")
local base64 = require("lib:base64")
local collections = require("std:collections")
local time = require("std:time")
local rusqlite = require("vnd:rusqlite")

base64.encod("x") -- expect-error
base64.encode {} -- expect-error
base64.encode() -- expect-error
---@type number
local _encoded = base64.encode("x") -- expect-error
json.object():nope() -- expect-error
json.object():set(1, 2) -- expect-error
collections.Map.create() -- expect-error
collections.Counter.new("ten") -- expect-error
time.sleep("soon") -- expect-error
rusqlite.open(":memory:"):query() -- expect-error
local _timeout = time.Duration.from_secs(5):as_years() -- expect-error
local _fine = base64.encode("this line is correct")
local _object = json.object()

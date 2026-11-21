--!strict
-- A module an app is not offered, and a global its environment does not have, are declared as unusable: the first
-- use is a type error that names it. (`require("lib:test")` itself gives a value of an unusable type: the last
-- overload of `require` is `(path: string) -> any`, for the app's own bundled modules, so a misspelt module name is
-- caught by `bun run lint:luau` itself, not by the types: see scripts/lint-luau.ts, `allowedRequire`.)
local test = require("lib:test")
local compile = require("lib:compile")
local workers = require("std:workers")
local base64 = require("lib:base64")

test.describe("x", function() end) -- expect-error: Key 'describe' not found in table 'vos_NotForApps_lib_test'
compile.build() -- expect-error: Key 'build' not found in table 'vos_NotForApps_lib_compile'
workers.shutdown() -- expect-error: Key 'shutdown' not found in table 'vos_NotForApps_std_workers'
local _encoded: number = base64.encode("x") -- expect-error: Expected this to be 'number'

-- globals that Luau has and an app's environment does not
print("hello") -- expect-error: Cannot call a value of type vos_NotForApps_print
local _now = os.time() -- expect-error: Key 'time' not found in table 'vos_NotForApps_os'
local _trace = debug.traceback() -- expect-error: Key 'traceback' not found in table 'vos_NotForApps_debug'
local _code = loadstring("return 1") -- expect-error: Cannot call a value of type vos_NotForApps_loadstring
local _bytes = buffer.create(8) -- expect-error: Key 'create' not found in table 'vos_NotForApps_buffer'
collectgarbage() -- expect-error: Cannot call a value of type vos_NotForApps_collectgarbage
local _env = getfenv() -- expect-error: Cannot call a value of type vos_NotForApps_getfenv

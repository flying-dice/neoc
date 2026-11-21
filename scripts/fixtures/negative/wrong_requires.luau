--!strict
-- What is a type error. A module an app is not offered is declared as a table with one `never` field: the first member
-- access is a type error that names it. The `require("lib:test")` call itself is not one in the first pass (a bare
-- require that is never used checks clean, and so does a misspelt module name: the last overload of `require` is
-- `(path: string) -> any`, for the app's own modules). The second pass of `bun run lint:luau`, which has no `declare
-- require`, reports it as an "Unknown require" (the lines below), and the require scan fails it as well
-- (scripts/lint-luau.ts, `allowedRequire`). A global the app's environment does not have is a table (`os`) or a function taking `never`
-- (`print`): any use of it, including `pcall(print, "x")` and an alias, is an error. Naming `print` without calling it is not.
local test = require("lib:test") -- expect-error: Unknown require
local compile = require("lib:compile") -- expect-error: Unknown require
local workers = require("std:workers") -- expect-error: Unknown require
local base64 = require("lib:base64")

test.describe("x", function() end) -- expect-error: Key 'describe' not found in table 'vos_NotForApps_lib_test'
compile.build() -- expect-error: Key 'build' not found in table 'vos_NotForApps_lib_compile'
workers.shutdown() -- expect-error: Key 'shutdown' not found in table 'vos_NotForApps_std_workers'
local _encoded: number = base64.encode("x") -- expect-error: Expected this to be 'number'

-- globals that Luau has and an app's environment does not
print("hello") -- expect-error: Expected this to be unreachable, but got 'string'
pcall(print, "x") -- expect-error: Expected this to be unreachable, but got 'string'
local alias = loadstring
alias("return 1") -- expect-error: Expected this to be unreachable, but got 'string'
local _now = os.time() -- expect-error: Key 'time' not found in table 'vos_NotForApps_os'
local _trace = debug.traceback() -- expect-error: Key 'traceback' not found in table 'vos_NotForApps_debug'
local _code = loadstring("return 1") -- expect-error: Expected this to be unreachable, but got 'string'
local _bytes = buffer.create(8) -- expect-error: Key 'create' not found in table 'vos_NotForApps_buffer'
collectgarbage() -- expect-error: Argument count mismatch. Function 'collectgarbage' expects 1 argument, but none are specified
local _env = getfenv() -- expect-error: Argument count mismatch. Function 'getfenv' expects 1 argument, but none are specified

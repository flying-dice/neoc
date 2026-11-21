--!strict
-- Wrong calls into an app's own module (`../lib/shout`). The editor sees `any` here; `bun run lint:luau` runs a second
-- pass in which `require` is luau-lsp's built-in, so the module has its real type and each line below is flagged.
local shout = require("../lib/shout")
local _missing = require("../lib/missing") -- expect-error: Unknown require

local _a = shout.loud(5) -- expect-error: Expected this to be 'string', but got 'number'
local _b: number = shout.loud("a") -- expect-error: Expected this to be 'number', but got 'string'
shout.whisper("x") -- expect-error: Key 'whisper' not found in table
local _c = shout.count() -- expect-error: expects 1 argument, but none are specified
local _fine = shout.count("this line is correct")

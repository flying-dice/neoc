--!strict
-- Correct use of an app's own module (`./lib/shout`). `bun run lint:luau` checks it against the module's real types in
-- its second pass; the wrong calls are in negative/wrong_module_use.luau.
local shout = require("./lib/shout")

local loud: string = shout.loud("hey")
local length: number = shout.count(loud)

return { loud = loud, length = length }

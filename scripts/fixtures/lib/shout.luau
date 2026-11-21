--!strict
-- An app's own module, for the fixtures that check calls into one (`../module_use.luau` correct,
-- `../negative/wrong_module_use.luau` wrong). The editor sees `require("./lib/shout")` as `any`; `bun run lint:luau`
-- type-checks it in its second pass.
local shout = {}

function shout.loud(text: string): string
  return string.upper(text) .. "!"
end

function shout.count(text: string): number
  return #text
end

return shout

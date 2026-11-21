-- A relative module of the package: darklua bundles it into the app file, while the neoc modules it requires
-- stay runtime `require`s that the host resolves (the same one the app requires, which it shares).
local base64 = require("lib:base64")

local M = {}

function M.encode(text)
  return base64.encode(text)
end

return M

local collections = require("std:collections")

local function main()
  print(math.floor(7.9))
  local f = collections.Map.new()
  f:set("a", 1)
  f:set("b", 2)
  print(f:size())
end
return { main = main }

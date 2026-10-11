local hex = require("lib:hex")
local collections = require("std:collections")

local function main()
  println(math.floor(7.9))
  println(hex.encode("hi"))
  local m = collections.Map.new()
  m:set("a", 1)
  m:set("b", 2)
  println(m:has("a"))
  println(m:has("z"))
  println(m:size())
end
return { main = main }

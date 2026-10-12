local Greet = require("greet").Greet
local D = { __struct = "D" }
D.__traits = { "Greet" }
function D.name(self)
  return "d"
end
D.hi = Greet.hi
local function main()
  local d = D
  println(d:hi())
end
return { main = main }

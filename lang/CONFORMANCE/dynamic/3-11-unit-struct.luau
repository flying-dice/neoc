type Sun = { __struct: "Sun" }
type Moon = { __struct: "Moon" }

local Sun = { __struct = "Sun" }
local Moon = { __struct = "Moon" }
function Sun.shine(self): string
  return "day"
end
function Moon.shine(self): string
  return "night"
end
local function main()
  local a = Sun
  local b = Moon
  println(a:shine())
  println(b:shine())
end

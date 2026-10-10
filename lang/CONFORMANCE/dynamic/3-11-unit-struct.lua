local Sun = { __struct = "Sun" }
local Moon = { __struct = "Moon" }
function Sun.shine(self)
  return "day"
end
function Moon.shine(self)
  return "night"
end
local function main()
  local a = Sun
  local b = Moon
  println(a:shine())
  println(b:shine())
end

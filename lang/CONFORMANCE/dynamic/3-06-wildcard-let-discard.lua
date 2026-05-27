local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return setmetatable(fields, Point)
end
local function main()
  local _ = Point.new({ x = 1, y = 2 })
  println(0)
end

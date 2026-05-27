local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return setmetatable(fields, Point)
end
function Point.manhattan(self)
  return (self.x + self.y)
end
local function main()
  local p = Point.new({ x = 3, y = 4 })
  println(p.x)
  println(p:manhattan())
end

local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return setmetatable(fields, Point)
end
function Point.display(self)
  return tostring(self.x) .. ", " .. tostring(self.y)
end
Point.__traits = { "Display" }
local function main()
  local p = Point.new({ x = 1, y = 2 })
  println(p:display())
end

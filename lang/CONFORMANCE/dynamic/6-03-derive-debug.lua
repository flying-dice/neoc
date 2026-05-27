local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return setmetatable(fields, Point)
end
function Point.debug(self)
  return "Point { " .. "x = " .. tostring(self.x) .. ", " .. "y = " .. tostring(self.y) .. " }"
end
Point.__traits = { "Debug" }
local function main()
  local p = Point.new({ x = 1, y = 2 })
  println(p:debug())
end

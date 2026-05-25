local Point = {}
Point.__index = Point
function Point.new(fields)
  fields._struct = "Point"
  return setmetatable(fields, Point)
end
local function main()
  local p = Point.new({ x = 3, y = 4 })
  local __d = p
  local x = __d.x
  local y = __d.y
  println((x + y))
end

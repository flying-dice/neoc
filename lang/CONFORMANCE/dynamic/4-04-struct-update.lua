local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return setmetatable(fields, Point)
end
local function main()
  local p = Point.new({ x = 1, y = 2 })
  local q = Point.new((function()
    local __b = {}
    for k, v in pairs(p) do __b[k] = v end
    __b.x = 9
    return __b
  end)())
  println(q.x)
  println(q.y)
end

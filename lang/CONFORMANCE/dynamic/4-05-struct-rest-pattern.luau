type Point = { __struct: "Point", x: number, y: number, z: number }

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Point
function __check_Point(v, path)
  if type(v) ~= "table" or v.__struct ~= "Point" then __type_error(path, "Point", v) end
  __check_number(v.x, path .. ".x")
  __check_number(v.y, path .. ".y")
  __check_number(v.z, path .. ".z")
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return __check_Point(setmetatable(fields, Point), "Point")
end
local function classify(p: Point): number
  return (function()
    local __s = p
    if __s.__struct == "Point" and __s.x == 0 then
      local y = __s.y
      return y
    end
    if __s.__struct == "Point" then
      local x = __s.x
      return x
    end
  end)()
end
local function main()
  println(classify(Point.new({ x = 0, y = 7, z = 99 })))
  println(classify(Point.new({ x = 5, y = 8, z = 99 })))
end

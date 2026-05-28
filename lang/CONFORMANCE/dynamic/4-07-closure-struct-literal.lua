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
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return __check_Point(setmetatable(fields, Point), "Point")
end
local function main()
  local mk = function(n) return Point.new({ x = n, y = n }) end
  local p = mk(7)
  println(p.x)
  println(p.y)
  local sq = function(n) return { __enum = "Shape", __tag = "Square", side = n } end
  local s = sq(5)
  return (function()
    local __s = s
    if __s.__tag == "Square" then
      local side = __s.side
      return println(side)
    end
  end)()
end

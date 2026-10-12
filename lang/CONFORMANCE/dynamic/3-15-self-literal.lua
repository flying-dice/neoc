local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Point, __check_Wrap
function __check_Point(v, path)
  if type(v) ~= "table" or v.__struct ~= "Point" then __type_error(path, "Point", v) end
  __check_number(v.x, path .. ".x")
  __check_number(v.y, path .. ".y")
  return v
end
function __check_Wrap(v, path)
  if type(v) ~= "table" or v.__struct ~= "Wrap" then __type_error(path, "Wrap", v) end
  __check_number(v.n, path .. ".n")
  return v
end
local Point = {}
Point.__index = Point
function Point.new(fields)
  fields.__struct = "Point"
  return __check_Point(setmetatable(fields, Point), "Point")
end
Point.__traits = { "Make" }
function Point.origin()
  return Point.new({ x = 0, y = 0 })
end
function Point.at(x, y)
  return Point.new({ x = x, y = y })
end
function Point.with_x(self, x)
  return Point.new((function()
    local __b = {}
    for k, v in pairs(self) do __b[k] = v end
    __b.x = x
    return __b
  end)())
end
function Point.diagonal(n)
  local make = function(k) return Point.new({ x = k, y = k }) end
  return make((n * 2))
end
function Point.make()
  return Point.new({ x = 42, y = 0 })
end
local Wrap = {}
Wrap.__index = Wrap
function Wrap.new(fields)
  fields.__struct = "Wrap"
  return __check_Wrap(setmetatable(fields, Wrap), "Wrap")
end
function Wrap.of(v)
  return Wrap.new({ v = v, n = 1 })
end
function Wrap.with(self, v)
  return Wrap.new((function()
    local __b = {}
    for k, v in pairs(self) do __b[k] = v end
    __b.v = v
    return __b
  end)())
end
local function main()
  local o = Point.origin()
  local p = Point.at(3, 4):with_x(9)
  println(o.x)
  println(p.x)
  println(p.y)
  println(Point.diagonal(4).y)
  println(Point.make().x)
  local w = Wrap.of(5):with(7)
  println(w.v)
  println(w.n)
end

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Circle, __check_Square
function __check_Circle(v, path)
  if type(v) ~= "table" or v.__struct ~= "Circle" then __type_error(path, "Circle", v) end
  __check_number(v.r, path .. ".r")
  return v
end
function __check_Square(v, path)
  if type(v) ~= "table" or v.__struct ~= "Square" then __type_error(path, "Square", v) end
  __check_number(v.side, path .. ".side")
  return v
end
local Circle = {}
Circle.__index = Circle
function Circle.new(fields)
  fields.__struct = "Circle"
  return __check_Circle(setmetatable(fields, Circle), "Circle")
end
Circle.__traits = { "Drawable" }
local Square = {}
Square.__index = Square
function Square.new(fields)
  fields.__struct = "Square"
  return __check_Square(setmetatable(fields, Square), "Square")
end
Square.__traits = { "Drawable" }
function Circle.draw(self)
  return "O"
end
function Square.draw(self)
  return "[]"
end
local function render(shape)
  return shape:draw()
end
local function main()
  println(render(Circle.new({ r = 1 })))
  println(render(Square.new({ side = 2 })))
end

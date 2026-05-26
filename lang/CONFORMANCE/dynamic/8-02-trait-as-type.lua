local Circle = {}
Circle.__index = Circle
function Circle.new(fields)
  fields.__struct = "Circle"
  return setmetatable(fields, Circle)
end
local Square = {}
Square.__index = Square
function Square.new(fields)
  fields.__struct = "Square"
  return setmetatable(fields, Square)
end
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

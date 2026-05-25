local Dog = {}
Dog.__index = Dog
function Dog.new(fields)
  fields._struct = "Dog"
  return setmetatable(fields, Dog)
end
function Dog.greet(self)
  return (self.name .. " says woof")
end
local function main()
  local d = Dog.new({ name = "Rex" })
  println(d:greet())
end

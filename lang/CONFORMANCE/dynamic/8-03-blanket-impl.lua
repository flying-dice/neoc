local Dog = {}
Dog.__index = Dog
function Dog.new(fields)
  fields.__struct = "Dog"
  return setmetatable(fields, Dog)
end
Dog.__traits = { "Describe", "Show" }
function Dog.show(self)
  return self.name
end
local function main()
  local d = Dog.new({ name = "Rex" })
  println(d:describe())
end
function Dog.describe(self)
  return self:show()
end

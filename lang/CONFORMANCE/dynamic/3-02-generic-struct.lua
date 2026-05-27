local Box = {}
Box.__index = Box
function Box.new(fields)
  fields.__struct = "Box"
  return setmetatable(fields, Box)
end
function Box.get(self)
  return self.value
end
local function main()
  local b = Box.new({ value = 99 })
  println(b:get())
end

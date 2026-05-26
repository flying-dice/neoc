local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return setmetatable(fields, Counter)
end
function Counter.clone(self)
  return setmetatable({ __struct = "Counter", value = self.value }, Counter)
end
local function main()
  local a = Counter.new({ value = 5 })
  local b = a:clone()
  println(a.value)
  println(b.value)
end

local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return setmetatable(fields, Counter)
end
function Counter.start(v)
  return Counter.new({ value = v })
end
function Counter.bump(self)
  return (self.value + 1)
end
local function main()
  local c = Counter.start(41)
  println(c.value)
  println(c:bump())
end

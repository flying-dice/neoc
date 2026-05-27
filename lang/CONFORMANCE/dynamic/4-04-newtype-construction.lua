local Wrapper = {}
Wrapper.__index = Wrapper
function Wrapper.new(value)
  return setmetatable({ __struct = "Wrapper", value }, Wrapper)
end
function Wrapper.get(self)
  local __d = self
  local n = __d[1]
  return n
end
local function main()
  local w = Wrapper.new(42)
  println(w:get())
  local __d = w
  local m = __d[1]
  println(m)
end

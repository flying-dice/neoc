local function adder(base)
  return function(x) return (x + base) end
end
local function main()
  local add10 = adder(10)
  println(add10(5))
  println(add10(32))
end

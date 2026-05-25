local Pair = {}
Pair.__index = Pair
function Pair.new(fields)
  fields._struct = "Pair"
  return setmetatable(fields, Pair)
end
function Pair.eq(a, b)
  return a._struct == b._struct and a.a == b.a and a.b == b.b
end
local function main()
  local p = Pair.new({ a = 1, b = 2 })
  local q = Pair.new({ a = 1, b = 2 })
  local r = Pair.new({ a = 9, b = 2 })
  if p:eq(q) then
    println("eq")
  end
  return (function()
    if (not p:eq(r)) then
      println("ne")
    end
  end)()
end

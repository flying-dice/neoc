local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Pair
function __check_Pair(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pair" then __type_error(path, "Pair", v) end
  __check_number(v.a, path .. ".a")
  __check_number(v.b, path .. ".b")
  return v
end
local Pair = {}
Pair.__index = Pair
function Pair.new(fields)
  fields.__struct = "Pair"
  return __check_Pair(setmetatable(fields, Pair), "Pair")
end
function Pair.eq(a, b)
  return a.__struct == b.__struct and a.a == b.a and a.b == b.b
end
Pair.__traits = { "Eq" }
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

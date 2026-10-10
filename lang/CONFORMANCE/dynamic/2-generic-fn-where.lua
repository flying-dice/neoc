local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Num
function __check_Num(v, path)
  if type(v) ~= "table" or v.__struct ~= "Num" then __type_error(path, "Num", v) end
  __check_number(v.v, path .. ".v")
  return v
end
local Num = {}
Num.__index = Num
function Num.new(fields)
  fields.__struct = "Num"
  return __check_Num(setmetatable(fields, Num), "Num")
end
Num.__traits = { "Doubler" }
function Num.double(self)
  return (self.v + self.v)
end
local function combine(a, b)
  return (a:double() + b:double())
end
local function main()
  println(combine(Num.new({ v = 21 }), Num.new({ v = 5 })))
end

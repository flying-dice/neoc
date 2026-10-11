local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Meters
function __check_Meters(v, path)
  if type(v) ~= "table" or v.__struct ~= "Meters" then __type_error(path, "Meters", v) end
  __check_number(v.value, path .. ".value")
  return v
end
local Meters = {}
Meters.__index = Meters
function Meters.new(fields)
  fields.__struct = "Meters"
  return __check_Meters(setmetatable(fields, Meters), "Meters")
end
function Meters.origin()
  return Meters.new({ value = 0 })
end
function Meters.plus(self, other)
  return Meters.new({ value = (self.value + other.value) })
end
local function main()
  local a = Meters.new({ value = 3 })
  local b = Meters.new({ value = 4 })
  println(a:plus(b).value)
  println(Meters.origin().value)
end

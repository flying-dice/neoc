local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Bin
function __check_Bin(v, path)
  if type(v) ~= "table" or v.__struct ~= "Bin" then __type_error(path, "Bin", v) end
  __check_number(v.value, path .. ".value")
  return v
end
local Bin = {}
Bin.__index = Bin
function Bin.new(fields)
  fields.__struct = "Bin"
  return __check_Bin(setmetatable(fields, Bin), "Bin")
end
Bin.__traits = { "Container" }
function Bin.peek(self)
  return self.value
end
local function main()
  local b = Bin.new({ value = 7 })
  println(b:peek())
end

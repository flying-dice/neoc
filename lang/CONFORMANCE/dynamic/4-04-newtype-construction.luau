type Wrapper = { __struct: "Wrapper", [number]: number }

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Wrapper
function __check_Wrapper(v, path)
  if type(v) ~= "table" or v.__struct ~= "Wrapper" then __type_error(path, "Wrapper", v) end
  __check_number(v[1], path .. ".0")
  return v
end
local Wrapper = {}
Wrapper.__index = Wrapper
function Wrapper.new(value)
  return __check_Wrapper(setmetatable({ __struct = "Wrapper", value }, Wrapper), "Wrapper")
end
function Wrapper.get(self): number
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

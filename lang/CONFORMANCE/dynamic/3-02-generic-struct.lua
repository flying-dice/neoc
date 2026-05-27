local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local __check_Box
function __check_Box(v, path)
  if type(v) ~= "table" or v.__struct ~= "Box" then __type_error(path, "Box", v) end
  return v
end
local Box = {}
Box.__index = Box
function Box.new(fields)
  fields.__struct = "Box"
  return __check_Box(setmetatable(fields, Box), "Box")
end
function Box.get(self)
  return self.value
end
local function main()
  local b = Box.new({ value = 99 })
  println(b:get())
end

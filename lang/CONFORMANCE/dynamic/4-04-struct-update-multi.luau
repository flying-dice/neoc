type Vec3 = { __struct: "Vec3", x: number, y: number, z: number }

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Vec3
function __check_Vec3(v, path)
  if type(v) ~= "table" or v.__struct ~= "Vec3" then __type_error(path, "Vec3", v) end
  __check_number(v.x, path .. ".x")
  __check_number(v.y, path .. ".y")
  __check_number(v.z, path .. ".z")
  return v
end
local Vec3 = {}
Vec3.__index = Vec3
function Vec3.new(fields)
  fields.__struct = "Vec3"
  return __check_Vec3(setmetatable(fields, Vec3), "Vec3")
end
local function main()
  local v = Vec3.new({ x = 1, y = 2, z = 3 })
  local w = Vec3.new((function()
    local __b = {}
    for k, v in pairs(v) do __b[k] = v end
    __b.x = 10
    __b.z = 30
    return __b
  end)())
  println(w.x)
  println(w.y)
  println(w.z)
end

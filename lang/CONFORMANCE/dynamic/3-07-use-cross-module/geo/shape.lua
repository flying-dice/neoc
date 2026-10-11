local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Circle
function __check_Circle(v, path)
  if type(v) ~= "table" or v.__struct ~= "Circle" then __type_error(path, "Circle", v) end
  __check_number(v.r, path .. ".r")
  return v
end
local Circle = {}
Circle.__index = Circle
function Circle.new(fields)
  fields.__struct = "Circle"
  return __check_Circle(setmetatable(fields, Circle), "Circle")
end
function Circle.area(self)
  return ((3 * self.r) * self.r)
end
function Circle.kind(self)
  return (function()
    if (self.r > 2) then
      return { __enum = "Kind", __tag = "Large" }
    else
      return { __enum = "Kind", __tag = "Small" }
    end
  end)()
end
return { Circle = Circle }

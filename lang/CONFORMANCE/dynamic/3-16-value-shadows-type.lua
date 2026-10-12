local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Box, __check_Hue, __check_Pt
function __check_Box(v, path)
  if type(v) ~= "table" or v.__struct ~= "Box" then __type_error(path, "Box", v) end
  __check_number(v.v, path .. ".v")
  return v
end
function __check_Hue(v, path)
  if type(v) ~= "table" or v.__struct ~= "Hue" then __type_error(path, "Hue", v) end
  __check_number(v.r, path .. ".r")
  return v
end
function __check_Pt(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pt" then __type_error(path, "Pt", v) end
  __check_number(v.x, path .. ".x")
  return v
end
local __type_Hue = {}
__type_Hue.__index = __type_Hue
function __type_Hue.new(fields)
  fields.__struct = "Hue"
  return __check_Hue(setmetatable(fields, __type_Hue), "Hue")
end
function __type_Hue.clone(self)
  return setmetatable({ __struct = "Hue", r = self.r }, __type_Hue)
end
__type_Hue.__traits = { "Clone" }
local __type_Pt = {}
__type_Pt.__index = __type_Pt
function __type_Pt.new(fields)
  fields.__struct = "Pt"
  return __check_Pt(setmetatable(fields, __type_Pt), "Pt")
end
__type_Pt.__traits = { "Score" }
local __type_Box = {}
__type_Box.__index = __type_Box
function __type_Box.new(fields)
  fields.__struct = "Box"
  return __check_Box(setmetatable(fields, __type_Box), "Box")
end
local __type_Mode = {}
__type_Mode.__index = __type_Mode
local Score = {}
function Score.bonus(self)
  return (self:base() + 100)
end
local Pt = 5
local function Box()
  return 6
end
function __type_Pt.make(Pt)
  return __type_Pt.new({ x = Pt })
end
function __type_Mode.code(self)
  return (function()
    local __s = self
    if __s.__tag == "On" then
      return 1
    end
    if __s.__tag == "Off" then
      return 0
    end
  end)()
end
function __type_Pt.base(self)
  return self.x
end
__type_Pt.bonus = Score.bonus
local function pick(Mode)
  local m = setmetatable({ __enum = "Mode", __tag = "On" }, __type_Mode)
  return (Mode + m:code())
end
local function main()
  local Hue = 7
  local h = __type_Hue.new({ r = Hue })
  println(h.r)
  local grow = function(Hue) return __type_Hue.new({ r = (Hue + 1) }) end
  println(grow(1).r)
  println(h:clone().r)
  println(Pt)
  println(__type_Pt.make(3).x)
  println(__type_Pt.make(3):bonus())
  println(Box())
  println(__type_Box.new({ v = Box() }).v)
  println(pick(10))
  for Hue = 0, (2) - 1 do
    println(Hue)
  end
  println(__type_Pt.make(2).x)
end

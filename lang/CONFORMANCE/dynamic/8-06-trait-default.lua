local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_Loud, __check_Quiet
function __check_Loud(v, path)
  if type(v) ~= "table" or v.__struct ~= "Loud" then __type_error(path, "Loud", v) end
  __check_string(v.n, path .. ".n")
  return v
end
function __check_Quiet(v, path)
  if type(v) ~= "table" or v.__struct ~= "Quiet" then __type_error(path, "Quiet", v) end
  __check_string(v.n, path .. ".n")
  return v
end
local Greet = {}
function Greet.hi(self)
  return ("hi " .. self:name())
end
local Quiet = {}
Quiet.__index = Quiet
function Quiet.new(fields)
  fields.__struct = "Quiet"
  return __check_Quiet(setmetatable(fields, Quiet), "Quiet")
end
Quiet.__traits = { "Greet" }
local Loud = {}
Loud.__index = Loud
function Loud.new(fields)
  fields.__struct = "Loud"
  return __check_Loud(setmetatable(fields, Loud), "Loud")
end
Loud.__traits = { "Greet" }
function Quiet.name(self)
  return self.n
end
Quiet.hi = Greet.hi
function Loud.name(self)
  return self.n
end
function Loud.hi(self)
  return ("HEY " .. self:name())
end
local function main()
  local q = Quiet.new({ n = "ann" })
  local l = Loud.new({ n = "bob" })
  println(q:hi())
  println(l:hi())
end

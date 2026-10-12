local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_Badger
function __check_Badger(v, path)
  if type(v) ~= "table" or v.__struct ~= "Badger" then __type_error(path, "Badger", v) end
  __check_string(v.name, path .. ".name")
  return v
end
local Show = {}
function Show.shout(self)
  return (self:show() .. "!")
end
local Greet = {}
function Greet.greet(self)
  return ("Hi, " .. self:shout())
end
local Badger = {}
Badger.__index = Badger
function Badger.new(fields)
  fields.__struct = "Badger"
  return __check_Badger(setmetatable(fields, Badger), "Badger")
end
Badger.__traits = { "Greet", "Show" }
function Badger.show(self)
  return self.name
end
Badger.shout = Show.shout
Badger.greet = Greet.greet
local function main()
  local b = Badger.new({ name = "Bram" })
  println(b:shout())
  println(b:greet())
end

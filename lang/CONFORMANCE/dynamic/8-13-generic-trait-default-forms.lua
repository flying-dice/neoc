local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_Loud, __check_Namer
function __check_Loud(v, path)
  if type(v) ~= "table" or v.__struct ~= "Loud" then __type_error(path, "Loud", v) end
  __check_string(v.s, path .. ".s")
  return v
end
function __check_Namer(v, path)
  if type(v) ~= "table" or v.__struct ~= "Namer" then __type_error(path, "Namer", v) end
  __check_string(v.s, path .. ".s")
  return v
end
local Base = {}
function Base.peek(self)
  return self:next()
end
function Base.make(x)
  return x
end
local Sub = {}
function Sub.both(self)
  return { self:peek(), self:next() }
end
local Pick = {}
function Pick.pick(self)
  return self:get()
end
local Namer = {}
Namer.__index = Namer
function Namer.new(fields)
  fields.__struct = "Namer"
  return __check_Namer(setmetatable(fields, Namer), "Namer")
end
Namer.__traits = { "Base", "Sub" }
local Loud = {}
Loud.__index = Loud
function Loud.new(fields)
  fields.__struct = "Loud"
  return __check_Loud(setmetatable(fields, Loud), "Loud")
end
Loud.__traits = { "Base" }
local Quiet = { __struct = "Quiet" }
Quiet.__traits = { "Pick", "Show" }
function Namer.next(self)
  return self.s
end
Namer.peek = Base.peek
Namer.make = Base.make
Namer.both = Sub.both
function Loud.next(self)
  return self.s
end
function Loud.peek(self)
  return "loud"
end
Loud.make = Base.make
function Quiet.show(self)
  return "quiet"
end
local function main()
  local n = Namer.new({ s = "n" })
  println(n:peek())
  println(n:both()[(0) + 1])
  println(Namer.make("m"))
  local l = Loud.new({ s = "l" })
  println(l:peek())
  println(l:next())
  println(Loud.make("k"))
  println(Quiet:pick())
end
function Quiet.get(self)
  return self:show()
end
Quiet.pick = Pick.pick

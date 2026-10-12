local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_Box, __check_Counter, __check_Namer
function __check_Box(v, path)
  if type(v) ~= "table" or v.__struct ~= "Box" then __type_error(path, "Box", v) end
  return v
end
function __check_Counter(v, path)
  if type(v) ~= "table" or v.__struct ~= "Counter" then __type_error(path, "Counter", v) end
  __check_number(v.n, path .. ".n")
  return v
end
function __check_Namer(v, path)
  if type(v) ~= "table" or v.__struct ~= "Namer" then __type_error(path, "Namer", v) end
  __check_string(v.s, path .. ".s")
  return v
end
local Source = {}
function Source.peek(self)
  return self:next()
end
function Source.twice(self, item)
  return { item, self:peek() }
end
local Counter = {}
Counter.__index = Counter
function Counter.new(fields)
  fields.__struct = "Counter"
  return __check_Counter(setmetatable(fields, Counter), "Counter")
end
Counter.__traits = { "Source" }
local Namer = {}
Namer.__index = Namer
function Namer.new(fields)
  fields.__struct = "Namer"
  return __check_Namer(setmetatable(fields, Namer), "Namer")
end
Namer.__traits = { "Source" }
function Counter.next(self)
  return self.n
end
Counter.peek = Source.peek
Counter.twice = Source.twice
function Namer.next(self)
  return self.s
end
Namer.peek = Source.peek
Namer.twice = Source.twice
local Box = {}
Box.__index = Box
function Box.new(fields)
  fields.__struct = "Box"
  return __check_Box(setmetatable(fields, Box), "Box")
end
Box.__traits = { "Source" }
function Box.next(self)
  return self.v
end
Box.peek = Source.peek
Box.twice = Source.twice
local function pull(s)
  return s:twice(s:peek())
end
local function main()
  local c = Counter.new({ n = 4 })
  println(c:peek())
  println(c:twice(9)[(1) + 1])
  local n = Namer.new({ s = "a" })
  local t = n:peek()
  local b = Box.new({ v = 7 })
  println(b:peek())
  println(pull(c)[(0) + 1])
  println(pull(b)[(1) + 1])
end

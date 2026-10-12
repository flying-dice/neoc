local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local __check_Foo, __check_Pair
function __check_Foo(v, path)
  if type(v) ~= "table" or v.__struct ~= "Foo" then __type_error(path, "Foo", v) end
  __check_number(v.a, path .. ".a")
  return v
end
function __check_Pair(v, path)
  if type(v) ~= "table" or v.__struct ~= "Pair" then __type_error(path, "Pair", v) end
  return v
end
local Foo = {}
Foo.__index = Foo
function Foo.new(fields)
  fields.__struct = "Foo"
  return __check_Foo(setmetatable(fields, Foo), "Foo")
end
local function pick(c)
  return (function()
    local __s = c
    if __s.__tag == "Red" then
      return 0
    end
    if __s.__tag == "Green" then
      local n = __s[1]
      return n
    end
    if __s.__tag == "Pt" then
      local a = __s.a
      return a
    end
  end)()
end
local Pair = {}
Pair.__index = Pair
function Pair.new(fields)
  fields.__struct = "Pair"
  return __check_Pair(setmetatable(fields, Pair), "Pair")
end
Pair.__traits = { "Sup" }
function Pair.s(self)
  return self.b
end
local function through(t)
  return t:s()
end
local function main()
  println(pick({ __enum = "Color", __tag = "Red" }))
  println(pick({ __enum = "Color", __tag = "Green", 4 }))
  println(pick({ __enum = "Color", __tag = "Pt", a = 5 }))
  println(through(Pair.new({ a = true, b = 6 })))
  println(Foo.new({ a = 7 }).a)
end

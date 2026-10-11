local Option = {}
Option.__index = Option
local Result = {}
Result.__index = Result
local Ok   = function(v) return setmetatable({ __enum = "Result", __tag = "Ok", v }, Result) end
local Err  = function(e) return setmetatable({ __enum = "Result", __tag = "Err", e }, Result) end
local Some = function(v) return setmetatable({ __enum = "Option", __tag = "Some", v }, Option) end
local None = setmetatable({ __enum = "Option", __tag = "None" }, Option)
function Option.is_some(self) return self.__tag == "Some" end
function Option.is_none(self) return self.__tag == "None" end
function Option.unwrap(self)
  if self.__tag == "Some" then return self[1] end
  error("called unwrap on a None value", 0)
end
function Option.unwrap_or(self, default)
  if self.__tag == "Some" then return self[1] else return default end
end
function Option.map(self, f)
  if self.__tag == "Some" then return Some(f(self[1])) else return self end
end
function Option.ok_or(self, err)
  if self.__tag == "Some" then return Ok(self[1]) else return Err(err) end
end
function Result.is_ok(self) return self.__tag == "Ok" end
function Result.is_err(self) return self.__tag == "Err" end
function Result.unwrap(self)
  if self.__tag == "Ok" then return self[1] end
  error("called unwrap on an Err value", 0)
end
function Result.unwrap_err(self)
  if self.__tag == "Err" then return self[1] end
  error("called unwrap_err on an Ok value", 0)
end
function Result.map(self, f)
  if self.__tag == "Ok" then return Ok(f(self[1])) else return self end
end
function Result.map_err(self, f)
  if self.__tag == "Err" then return Err(f(self[1])) else return self end
end

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local function __check_list(v, path, elem)
  if type(v) ~= "table" or v.__struct or v.__enum then __type_error(path, "list", v) end
  local n = 0
  for _ in pairs(v) do n = n + 1 end
  for i = 1, n do
    if v[i] == nil then __type_error(path, "list", v) end
    elem(v[i], path .. "[" .. (i - 1) .. "]")
  end
  return v
end
local function __check_option(v, path, some)
  if type(v) ~= "table" or v.__enum ~= "Option" then __type_error(path, "Option", v) end
  if v.__tag == "Some" then some(v[1], path .. ".0")
  elseif v.__tag ~= "None" then __type_error(path, "Option", v) end
  return v
end
local __check_Inner, __check_Outer, __check_Shape
function __check_Inner(v, path)
  if type(v) ~= "table" or v.__struct ~= "Inner" then __type_error(path, "Inner", v) end
  __check_number(v.v, path .. ".v")
  return v
end
function __check_Outer(v, path)
  if type(v) ~= "table" or v.__struct ~= "Outer" then __type_error(path, "Outer", v) end
  __check_Inner(v.i, path .. ".i")
  __check_option(v.o, path .. ".o", __check_number)
  __check_list(v.xs, path .. ".xs", __check_number)
  __check_Shape(v.s, path .. ".s")
  return v
end
function __check_Shape(v, path)
  if type(v) ~= "table" or v.__enum ~= "Shape" then __type_error(path, "Shape", v) end
  local t = v.__tag
  if t == "Dot" then
  elseif t == "Boxed" then
    __check_Inner(v[1], path .. ".0")
  else __type_error(path, "Shape", v) end
  return v
end
local Inner = {}
Inner.__index = Inner
function Inner.new(fields)
  fields.__struct = "Inner"
  return __check_Inner(setmetatable(fields, Inner), "Inner")
end
function Inner.clone(self)
  return setmetatable({ __struct = "Inner", v = self.v }, Inner)
end
Inner.__traits = { "Clone" }
function Inner.double(self)
  return (self.v * 2)
end
local Shape = {}
Shape.__index = Shape
function Shape.clone(self)
  if self.__tag == "Dot" then
    return setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape)
  end
  if self.__tag == "Boxed" then
    return setmetatable({ __enum = "Shape", __tag = "Boxed", self[1] }, Shape)
  end
end
Shape.__traits = { "Clone" }
function Shape.weight(self)
  return (function()
    local __s = self
    if __s.__tag == "Dot" then
      return 0
    end
    if __s.__tag == "Boxed" then
      local i = __s[1]
      return i.v
    end
  end)()
end
local Outer = {}
Outer.__index = Outer
function Outer.new(fields)
  fields.__struct = "Outer"
  return __check_Outer(setmetatable(fields, Outer), "Outer")
end
function Outer.clone(self)
  return setmetatable({ __struct = "Outer", i = self.i, o = self.o, xs = self.xs, s = self.s }, Outer)
end
Outer.__traits = { "Clone" }
local function main()
  local a = Outer.new({ i = Inner.new({ v = 4 }), o = Some(5), xs = { 6, 7 }, s = setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 8 }) }, Shape) })
  local b = a:clone()
  println(b.i:double())
  println(b.o:unwrap())
  println(b.xs[(1) + 1])
  println(b.s:weight())
  local d = setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape):clone()
  println(d:weight())
  local e = setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 9 }) }, Shape):clone()
  println(e:weight())
  local n = None
  local c = Outer.new({ i = Inner.new({ v = 1 }), o = n, xs = {  }, s = setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape) })
  local c2 = c:clone()
  return (function()
    if c2.o:is_none() then
      println(1)
    end
  end)()
end

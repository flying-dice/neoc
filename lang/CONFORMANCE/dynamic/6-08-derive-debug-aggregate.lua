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

local function __debug(v)
  if type(v) == "string" then
    local escaped = string.gsub(v, "[\\\"\n\t\r]", { ["\\"] = "\\\\", ["\""] = "\\\"", ["\n"] = "\\n", ["\t"] = "\\t", ["\r"] = "\\r" })
    return "\"" .. escaped .. "\""
  end
  if type(v) ~= "table" then return tostring(v) end
  local mt = getmetatable(v)
  if mt and mt.debug then return mt.debug(v) end
  if v.__struct then return v.__struct .. " { .. }" end
  local parts = {}
  for i = 1, #v do parts[i] = __debug(v[i]) end
  if v.__tag == nil then return "[" .. table.concat(parts, ", ") .. "]" end
  if #parts > 0 then return v.__tag .. "(" .. table.concat(parts, ", ") .. ")" end
  for k in pairs(v) do
    if k ~= "__enum" and k ~= "__tag" then return v.__tag .. " { .. }" end
  end
  return v.__tag
end

local function __type_error(path, expected, got)
  local actual = type(got) == "table" and (got.__struct or got.__enum or "table") or type(got)
  error(path .. ": expected " .. expected .. ", got " .. actual)
end
local function __check_number(v, path) if type(v) ~= "number" then __type_error(path, "number", v) end return v end
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
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
local function __check_result(v, path, ok, err)
  if type(v) ~= "table" or v.__enum ~= "Result" then __type_error(path, "Result", v) end
  if v.__tag == "Ok" then ok(v[1], path .. ".0")
  elseif v.__tag == "Err" then err(v[1], path .. ".0")
  else __type_error(path, "Result", v) end
  return v
end
local __check_All, __check_Inner, __check_Many, __check_Plain, __check_Shape, __check_Text, __check_WithInner, __check_WithList, __check_WithOption, __check_WithShape, __check_Wrap
function __check_All(v, path)
  if type(v) ~= "table" or v.__struct ~= "All" then __type_error(path, "All", v) end
  __check_Inner(v.i, path .. ".i")
  __check_option(v.o, path .. ".o", __check_number)
  __check_list(v.xs, path .. ".xs", __check_number)
  __check_Shape(v.s, path .. ".s")
  return v
end
function __check_Inner(v, path)
  if type(v) ~= "table" or v.__struct ~= "Inner" then __type_error(path, "Inner", v) end
  __check_number(v.v, path .. ".v")
  return v
end
function __check_Many(v, path)
  if type(v) ~= "table" or v.__struct ~= "Many" then __type_error(path, "Many", v) end
  __check_list(v.ys, path .. ".ys", __check_Inner)
  __check_result(v.r, path .. ".r", __check_number, __check_string)
  __check_option(v.oo, path .. ".oo", function(v, p) return __check_option(v, p, __check_Inner) end)
  return v
end
function __check_Plain(v, path)
  if type(v) ~= "table" or v.__struct ~= "Plain" then __type_error(path, "Plain", v) end
  __check_number(v.v, path .. ".v")
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
function __check_Text(v, path)
  if type(v) ~= "table" or v.__struct ~= "Text" then __type_error(path, "Text", v) end
  __check_string(v.s, path .. ".s")
  __check_option(v.o, path .. ".o", __check_string)
  __check_list(v.xs, path .. ".xs", __check_string)
  return v
end
function __check_WithInner(v, path)
  if type(v) ~= "table" or v.__struct ~= "WithInner" then __type_error(path, "WithInner", v) end
  __check_Inner(v.i, path .. ".i")
  return v
end
function __check_WithList(v, path)
  if type(v) ~= "table" or v.__struct ~= "WithList" then __type_error(path, "WithList", v) end
  __check_list(v.xs, path .. ".xs", __check_number)
  return v
end
function __check_WithOption(v, path)
  if type(v) ~= "table" or v.__struct ~= "WithOption" then __type_error(path, "WithOption", v) end
  __check_option(v.o, path .. ".o", __check_number)
  return v
end
function __check_WithShape(v, path)
  if type(v) ~= "table" or v.__struct ~= "WithShape" then __type_error(path, "WithShape", v) end
  __check_Shape(v.s, path .. ".s")
  return v
end
function __check_Wrap(v, path)
  if type(v) ~= "table" or v.__struct ~= "Wrap" then __type_error(path, "Wrap", v) end
  return v
end
local Inner = {}
Inner.__index = Inner
function Inner.new(fields)
  fields.__struct = "Inner"
  return __check_Inner(setmetatable(fields, Inner), "Inner")
end
function Inner.debug(self)
  return "Inner { " .. "v = " .. tostring(self.v) .. " }"
end
Inner.__traits = { "Debug" }
local Shape = {}
Shape.__index = Shape
function Shape.debug(self)
  if self.__tag == "Dot" then
    return "Dot"
  end
  if self.__tag == "Boxed" then
    return "Boxed(" .. __debug(self[1]) .. ")"
  end
end
Shape.__traits = { "Debug" }
local WithInner = {}
WithInner.__index = WithInner
function WithInner.new(fields)
  fields.__struct = "WithInner"
  return __check_WithInner(setmetatable(fields, WithInner), "WithInner")
end
function WithInner.debug(self)
  return "WithInner { " .. "i = " .. __debug(self.i) .. " }"
end
WithInner.__traits = { "Debug" }
local WithOption = {}
WithOption.__index = WithOption
function WithOption.new(fields)
  fields.__struct = "WithOption"
  return __check_WithOption(setmetatable(fields, WithOption), "WithOption")
end
function WithOption.debug(self)
  return "WithOption { " .. "o = " .. __debug(self.o) .. " }"
end
WithOption.__traits = { "Debug" }
local WithList = {}
WithList.__index = WithList
function WithList.new(fields)
  fields.__struct = "WithList"
  return __check_WithList(setmetatable(fields, WithList), "WithList")
end
function WithList.debug(self)
  return "WithList { " .. "xs = " .. __debug(self.xs) .. " }"
end
WithList.__traits = { "Debug" }
local WithShape = {}
WithShape.__index = WithShape
function WithShape.new(fields)
  fields.__struct = "WithShape"
  return __check_WithShape(setmetatable(fields, WithShape), "WithShape")
end
function WithShape.debug(self)
  return "WithShape { " .. "s = " .. __debug(self.s) .. " }"
end
WithShape.__traits = { "Debug" }
local All = {}
All.__index = All
function All.new(fields)
  fields.__struct = "All"
  return __check_All(setmetatable(fields, All), "All")
end
function All.debug(self)
  return "All { " .. "i = " .. __debug(self.i) .. ", " .. "o = " .. __debug(self.o) .. ", " .. "xs = " .. __debug(self.xs) .. ", " .. "s = " .. __debug(self.s) .. " }"
end
All.__traits = { "Debug" }
local Many = {}
Many.__index = Many
function Many.new(fields)
  fields.__struct = "Many"
  return __check_Many(setmetatable(fields, Many), "Many")
end
function Many.debug(self)
  return "Many { " .. "ys = " .. __debug(self.ys) .. ", " .. "r = " .. __debug(self.r) .. ", " .. "oo = " .. __debug(self.oo) .. " }"
end
Many.__traits = { "Debug" }
local Wrap = {}
Wrap.__index = Wrap
function Wrap.new(fields)
  fields.__struct = "Wrap"
  return __check_Wrap(setmetatable(fields, Wrap), "Wrap")
end
function Wrap.debug(self)
  return "Wrap { " .. "v = " .. __debug(self.v) .. " }"
end
Wrap.__traits = { "Debug" }
local Plain = {}
Plain.__index = Plain
function Plain.new(fields)
  fields.__struct = "Plain"
  return __check_Plain(setmetatable(fields, Plain), "Plain")
end
local Text = {}
Text.__index = Text
function Text.new(fields)
  fields.__struct = "Text"
  return __check_Text(setmetatable(fields, Text), "Text")
end
function Text.debug(self)
  return "Text { " .. "s = " .. __debug(self.s) .. ", " .. "o = " .. __debug(self.o) .. ", " .. "xs = " .. __debug(self.xs) .. " }"
end
Text.__traits = { "Debug" }
local function main()
  println(WithInner.new({ i = Inner.new({ v = 1 }) }):debug())
  println(WithOption.new({ o = Some(1) }):debug())
  println(WithOption.new({ o = None }):debug())
  println(WithList.new({ xs = { 1, 2 } }):debug())
  println(WithList.new({ xs = {  } }):debug())
  println(WithShape.new({ s = setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape) }):debug())
  println(WithShape.new({ s = setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape) }):debug())
  println(setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape):debug())
  local all = All.new({ i = Inner.new({ v = 1 }), o = Some(3), xs = { 4 }, s = setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape) })
  println(all:debug())
  println(Many.new({ ys = { Inner.new({ v = 1 }) }, r = Err("bad"), oo = Some(None) }):debug())
  println(Many.new({ ys = {  }, r = Ok(2), oo = Some(Some(Inner.new({ v = 3 }))) }):debug())
  println(Wrap.new({ v = Inner.new({ v = 1 }) }):debug())
  println(Wrap.new({ v = 5 }):debug())
  println(Wrap.new({ v = Plain.new({ v = 1 }) }):debug())
  println(Wrap.new({ v = { __enum = "NoDbg", __tag = "P", 2 } }):debug())
  println(Wrap.new({ v = { __enum = "NoDbg", __tag = "N", v = 3 } }):debug())
  println(Wrap.new({ v = { __enum = "NoDbg", __tag = "U" } }):debug())
  println(Text.new({ s = "a", o = Some("b"), xs = { "b", "c" } }):debug())
  println(Text.new({ s = "q\"x\\y", o = None, xs = { "b, c" } }):debug())
  println(Text.new({ s = "t\tn\nz", o = None, xs = {  } }):debug())
end

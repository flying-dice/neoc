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

local function __eq(a, b)
  if a == b then return true end
  if type(a) ~= "table" or type(b) ~= "table" then return false end
  local mt = getmetatable(a)
  if mt and mt.eq then return mt.eq(a, b) end
  for k, v in pairs(a) do
    if not __eq(v, b[k]) then return false end
  end
  for k in pairs(b) do
    if a[k] == nil then return false end
  end
  return true
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
local __check_HasMod, __check_Inner, __check_Many, __check_Mod, __check_Outer, __check_Shape, __check_Wrap
function __check_HasMod(v, path)
  if type(v) ~= "table" or v.__struct ~= "HasMod" then __type_error(path, "HasMod", v) end
  __check_Mod(v.m, path .. ".m")
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
function __check_Mod(v, path)
  if type(v) ~= "table" or v.__struct ~= "Mod" then __type_error(path, "Mod", v) end
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
function Inner.eq(a, b)
  return a.__struct == b.__struct and a.v == b.v
end
Inner.__traits = { "Eq" }
local Shape = {}
Shape.__index = Shape
function Shape.eq(a, b)
  if a.__enum ~= b.__enum or a.__tag ~= b.__tag then
    return false
  end
  if a.__tag == "Boxed" then
    return __eq(a[1], b[1])
  end
  return true
end
Shape.__traits = { "Eq" }
local Outer = {}
Outer.__index = Outer
function Outer.new(fields)
  fields.__struct = "Outer"
  return __check_Outer(setmetatable(fields, Outer), "Outer")
end
function Outer.eq(a, b)
  return a.__struct == b.__struct and __eq(a.i, b.i) and __eq(a.o, b.o) and __eq(a.xs, b.xs) and __eq(a.s, b.s)
end
Outer.__traits = { "Eq" }
local Many = {}
Many.__index = Many
function Many.new(fields)
  fields.__struct = "Many"
  return __check_Many(setmetatable(fields, Many), "Many")
end
function Many.eq(a, b)
  return a.__struct == b.__struct and __eq(a.ys, b.ys) and __eq(a.r, b.r) and __eq(a.oo, b.oo)
end
Many.__traits = { "Eq" }
local Wrap = {}
Wrap.__index = Wrap
function Wrap.new(fields)
  fields.__struct = "Wrap"
  return __check_Wrap(setmetatable(fields, Wrap), "Wrap")
end
function Wrap.eq(a, b)
  return a.__struct == b.__struct and __eq(a.v, b.v)
end
Wrap.__traits = { "Eq" }
local Mod = {}
Mod.__index = Mod
function Mod.new(fields)
  fields.__struct = "Mod"
  return __check_Mod(setmetatable(fields, Mod), "Mod")
end
function Mod.eq(self, o)
  return ((self.v % 10) == (o.v % 10))
end
local HasMod = {}
HasMod.__index = HasMod
function HasMod.new(fields)
  fields.__struct = "HasMod"
  return __check_HasMod(setmetatable(fields, HasMod), "HasMod")
end
function HasMod.eq(a, b)
  return a.__struct == b.__struct and __eq(a.m, b.m)
end
HasMod.__traits = { "Eq" }
local function many(y, r, oo)
  return Many.new({ ys = { Inner.new({ v = y }) }, r = r, oo = oo })
end
local function make(v, o, xs, s)
  return Outer.new({ i = Inner.new({ v = v }), o = o, xs = xs, s = s })
end
local function main()
  local base = make(1, Some(1), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape))
  local same = make(1, Some(1), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape))
  if base:eq(same) then
    println("equal")
  end
  if (not base:eq(make(9, Some(1), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("nested-ne")
  end
  if (not base:eq(make(1, None, { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("some-vs-none-ne")
  end
  if (not base:eq(make(1, Some(7), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("option-payload-ne")
  end
  if (not base:eq(make(1, Some(1), { 1, 3 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("element-ne")
  end
  if (not base:eq(make(1, Some(1), { 1 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("length-ne")
  end
  if (not base:eq(make(1, Some(1), {  }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 2 }) }, Shape)))) then
    println("empty-vs-nonempty-ne")
  end
  if (not base:eq(make(1, Some(1), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape)))) then
    println("tag-ne")
  end
  if (not base:eq(make(1, Some(1), { 1, 2 }, setmetatable({ __enum = "Shape", __tag = "Boxed", Inner.new({ v = 3 }) }, Shape)))) then
    println("enum-payload-ne")
  end
  local none = make(1, None, {  }, setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape))
  if none:eq(make(1, None, {  }, setmetatable({ __enum = "Shape", __tag = "Dot" }, Shape))) then
    println("none-empty-equal")
  end
  local m = many(1, Ok(1), Some(Some(Inner.new({ v = 1 }))))
  if m:eq(many(1, Ok(1), Some(Some(Inner.new({ v = 1 }))))) then
    println("many-equal")
  end
  if (not m:eq(many(2, Ok(1), Some(Some(Inner.new({ v = 1 })))))) then
    println("list-of-struct-ne")
  end
  if (not m:eq(many(1, Err("1"), Some(Some(Inner.new({ v = 1 })))))) then
    println("ok-vs-err-ne")
  end
  if (not m:eq(many(1, Ok(2), Some(Some(Inner.new({ v = 1 })))))) then
    println("ok-payload-ne")
  end
  if (not m:eq(many(1, Ok(1), Some(Some(Inner.new({ v = 2 })))))) then
    println("nested-option-payload-ne")
  end
  if (not m:eq(many(1, Ok(1), Some(None)))) then
    println("some-some-vs-some-none-ne")
  end
  local w = Wrap.new({ v = Inner.new({ v = 1 }) })
  local w_same = Wrap.new({ v = Inner.new({ v = 1 }) })
  local w_other = Wrap.new({ v = Inner.new({ v = 2 }) })
  if w:eq(w_same) then
    println("generic-struct-equal")
  end
  if (not w:eq(w_other)) then
    println("generic-struct-ne")
  end
  local n = Wrap.new({ v = 5 })
  local n_same = Wrap.new({ v = 5 })
  local n_other = Wrap.new({ v = 6 })
  if n:eq(n_same) then
    println("generic-number-equal")
  end
  if (not n:eq(n_other)) then
    println("generic-number-ne")
  end
  local h = HasMod.new({ m = Mod.new({ v = 1 }) })
  local h_mod = HasMod.new({ m = Mod.new({ v = 11 }) })
  local h_other = HasMod.new({ m = Mod.new({ v = 2 }) })
  if h:eq(h_mod) then
    println("hand-written-eq")
  end
  return (function()
    if (not h:eq(h_other)) then
      println("hand-written-ne")
    end
  end)()
end

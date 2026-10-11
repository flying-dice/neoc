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
local function __check_string(v, path) if type(v) ~= "string" then __type_error(path, "string", v) end return v end
local __check_N, __check_P
function __check_N(v, path)
  if type(v) ~= "table" or v.__struct ~= "N" then __type_error(path, "N", v) end
  __check_number(v[1], path .. ".0")
  return v
end
function __check_P(v, path)
  if type(v) ~= "table" or v.__struct ~= "P" then __type_error(path, "P", v) end
  __check_number(v.x, path .. ".x")
  __check_string(v.name, path .. ".name")
  return v
end
local P = {}
P.__index = P
function P.new(fields)
  fields.__struct = "P"
  return __check_P(setmetatable(fields, P), "P")
end
local N = {}
N.__index = N
function N.new(value)
  return __check_N(setmetatable({ __struct = "N", value }, N), "N")
end
local function tick(calls)
  calls[(0) + 1] = (calls[(0) + 1] .. "t")
  return Ok(0)
end
local function run(calls)
  local xs = { "a", "b" }
  do
    local __try_0 = tick(calls)
    if __try_0.__tag == "Err" or __try_0.__tag == "None" then return __try_0 end
    local __place_1 = __try_0[1]
    xs[(__place_1) + 1] = (xs[(__place_1) + 1] .. "!")
  end
  local ns = { 1, 2 }
  do
    local __try_1 = tick(calls)
    if __try_1.__tag == "Err" or __try_1.__tag == "None" then return __try_1 end
    local __place_2 = __try_1[1]
    ns[(__place_2) + 1] = ns[(__place_2) + 1] + 10
  end
  return (function()
    if (ns[(0) + 1] == 11) then
      return Ok((xs[(0) + 1] .. xs[(1) + 1]))
    else
      return Err("numeric place evaluated twice")
    end
  end)()
end
local function pick(ps, calls)
  calls[(0) + 1] = (calls[(0) + 1] .. "p")
  return ps[(0) + 1]
end
local function pick_ok(ns, calls)
  calls[(0) + 1] = (calls[(0) + 1] .. "n")
  return Ok(ns[(0) + 1])
end
local function fields(calls)
  local ps = { P.new({ x = 1, name = "a" }) }
  do
    local __place_3 = pick(ps, calls)
    __place_3.x = __place_3.x + 5
  end
  do
    local __place_4 = pick(ps, calls)
    __place_4.name = (__place_4.name .. "!")
  end
  local ns = { N.new(1) }
  do
    local __try_2 = pick_ok(ns, calls)
    if __try_2.__tag == "Err" or __try_2.__tag == "None" then return __try_2 end
    local __place_5 = __try_2[1]
    __place_5[1] = __place_5[1] + 10
  end
  return (function()
    if ((ps[(0) + 1].x == 6) and (ns[(0) + 1][1] == 11)) then
      return Ok(ps[(0) + 1].name)
    else
      return Err("field place evaluated twice")
    end
  end)()
end
local function show(r)
  return (function()
    local __s = r
    if __s.__tag == "Ok" then
      local v = __s[1]
      return v
    end
    if __s.__tag == "Err" then
      local e = __s[1]
      return e
    end
  end)()
end
local function main()
  local calls = { "" }
  println(show(run(calls)))
  println(show(fields(calls)))
  println(calls[(0) + 1])
end

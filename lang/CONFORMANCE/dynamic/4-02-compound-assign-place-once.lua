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
local function main()
  local calls = { "" }
  do
    local __s = run(calls)
    if __s.__tag == "Ok" then
      local v = __s[1]
      println(v)
    end
    if __s.__tag == "Err" then
      local e = __s[1]
      println(e)
    end
  end
  println(calls[(0) + 1])
end
